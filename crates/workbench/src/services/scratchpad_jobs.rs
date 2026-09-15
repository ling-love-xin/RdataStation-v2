//! 草稿箱后台任务（模块根加载 / 子目录懒加载）。
//!
//! 为什么需要：草稿箱的 render 是**纯读路径**——首次进入、操作后重载、展开文件夹都要读盘，
//! 直接在 UI 线程 `block_on` 会冻结界面（大目录或网络盘上尤其明显）。
//! 本模块沿用 `nav_jobs` 的成熟模式：**单一工作线程 + tokio 运行时**串行执行任务，
//! 视图只提交任务、轮询原子量并取回结果队列，不参与阻塞。
//!
//! 任务类型：
//! - `LoadRoot`：模块根 + 外部引用可用性 + 回收站 + （已展开过的）子目录缓存刷新；
//! - `LoadDir`：展开文件夹时的单目录懒加载。
//!
//! 过期结果防护：`LoadRoot` 携带自增 `seq`，视图只接受不小于已应用序号的结果，
//! 避免「连续两次重载，先发的后到」把旧数据盖回去。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};

use scratchpad::{ExternalReferenceStatus, ScratchpadEntry, ScratchpadStore, TrashEntry};

/// 模块根加载结果（回传主线程）。
pub struct LoadResult {
    /// 请求序号（用于丢弃过期结果）。
    pub seq: u64,
    /// 模块根直接子条目（失败时为 `Err`）。
    pub entries: Result<Vec<ScratchpadEntry>, String>,
    /// 外部引用可用性（路径存在性在后台探测）。
    pub refs: Vec<ExternalReferenceStatus>,
    /// 项目级回收站条目。
    pub trash: Vec<TrashEntry>,
    /// 之前已展开过的子目录 → 重新拉取的内容（避免「操作后展开态看起来空了」）。
    pub children: Vec<(String, Vec<ScratchpadEntry>)>,
}

/// 子目录懒加载结果（回传主线程）。
pub struct DirResult {
    pub parent: String,
    pub result: Result<Vec<ScratchpadEntry>, String>,
}

enum Job {
    /// 加载模块根（`parents` = 需要刷新缓存的已展开子目录）。
    LoadRoot {
        seq: u64,
        project_root: PathBuf,
        parents: Vec<String>,
    },
    /// 懒加载单个子目录。
    LoadDir {
        project_root: PathBuf,
        parent: String,
    },
}

/// 共享状态：任务队列 + 未完成计数 + 结果队列。
struct Shared {
    tx: Sender<Job>,
    /// 未完成的模块根加载（排队 + 执行中）。
    pending_roots: AtomicUsize,
    /// 未完成的子目录加载。
    pending_dirs: AtomicUsize,
    load_results: Mutex<Vec<LoadResult>>,
    dir_results: Mutex<Vec<DirResult>>,
}

static JOBS: OnceLock<Shared> = OnceLock::new();
/// 模块根加载请求序号（单调递增）。
static LOAD_SEQ: AtomicU64 = AtomicU64::new(1);

fn shared() -> &'static Shared {
    JOBS.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("rds-scratchpad-jobs".to_string())
            .spawn(move || worker(rx))
            .expect("failed to spawn scratchpad jobs worker");
        Shared {
            tx,
            pending_roots: AtomicUsize::new(0),
            pending_dirs: AtomicUsize::new(0),
            load_results: Mutex::new(Vec::new()),
            dir_results: Mutex::new(Vec::new()),
        }
    })
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn worker(rx: mpsc::Receiver<Job>) {
    // tokio 运行时随工作线程存活；存储层（tokio::fs）依赖它。
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            // 运行时建不起来：把所有已入队任务以错误结果交回，并归零计数，
            // 否则待办计数永远 >0，轮询印会一直转。
            tracing::error!("[ScratchpadJobs] runtime init failed: {e}");
            while let Ok(job) = rx.recv() {
                match job {
                    Job::LoadRoot { seq, .. } => {
                        lock(&shared().load_results).push(LoadResult {
                            seq,
                            entries: Err(format!("后台运行时不可用: {e}")),
                            refs: Vec::new(),
                            trash: Vec::new(),
                            children: Vec::new(),
                        });
                        shared().pending_roots.fetch_sub(1, Ordering::SeqCst);
                    }
                    Job::LoadDir { parent, .. } => {
                        lock(&shared().dir_results).push(DirResult {
                            parent,
                            result: Err(format!("后台运行时不可用: {e}")),
                        });
                        shared().pending_dirs.fetch_sub(1, Ordering::SeqCst);
                    }
                }
            }
            return;
        }
    };
    while let Ok(job) = rx.recv() {
        match job {
            Job::LoadRoot {
                seq,
                project_root,
                parents,
            } => {
                let store = ScratchpadStore::new(project_root);
                let loaded = rt.block_on(async {
                    // 先确保目录/元数据就绪（含旧布局迁移，幂等）。
                    if let Err(e) = store.ensure_dir().await {
                        return Err(e.to_string());
                    }
                    let entries = store.list_local_entries(0).await.map_err(|e| e.to_string())?;
                    let refs = store
                        .external_reference_status()
                        .await
                        .unwrap_or_default();
                    let trash = store.list_trash().await.unwrap_or_default();
                    let mut children: Vec<(String, Vec<ScratchpadEntry>)> = Vec::new();
                    for parent in parents {
                        // 已被删除的目录忽略（下次展开时自然不可用）。
                        if let Ok(kids) = store.list_directory_entries(&parent).await {
                            children.push((parent, kids));
                        }
                    }
                    Ok::<_, String>((entries, refs, trash, children))
                });
                let result = match loaded {
                    Ok((entries, refs, trash, children)) => LoadResult {
                        seq,
                        entries: Ok(entries),
                        refs,
                        trash,
                        children,
                    },
                    Err(e) => LoadResult {
                        seq,
                        entries: Err(e),
                        refs: Vec::new(),
                        trash: Vec::new(),
                        children: Vec::new(),
                    },
                };
                lock(&shared().load_results).push(result);
                shared().pending_roots.fetch_sub(1, Ordering::SeqCst);
            }
            Job::LoadDir {
                project_root,
                parent,
            } => {
                let store = ScratchpadStore::new(project_root);
                let result = rt
                    .block_on(store.list_directory_entries(&parent))
                    .map_err(|e| e.to_string());
                lock(&shared().dir_results).push(DirResult { parent, result });
                shared().pending_dirs.fetch_sub(1, Ordering::SeqCst);
            }
        }
    }
}

/// 提交模块根加载，返回本次请求序号（视图据此丢弃过期结果）。
pub fn enqueue_root_load(project_root: &Path, parents: Vec<String>) -> u64 {
    let seq = LOAD_SEQ.fetch_add(1, Ordering::SeqCst);
    shared().pending_roots.fetch_add(1, Ordering::SeqCst);
    let _ = shared().tx.send(Job::LoadRoot {
        seq,
        project_root: project_root.to_path_buf(),
        parents,
    });
    seq
}

/// 失效所有在途加载：推进序号，使后续到达的旧结果被视图丢弃。
///
/// 场景：项目被关闭/切换时，先前项目目录的加载结果不得回填到新会话。
pub fn invalidate_loads() -> u64 {
    LOAD_SEQ.fetch_add(1, Ordering::SeqCst)
}

/// 提交子目录懒加载。
pub fn enqueue_dir_load(project_root: &Path, parent: &str) {
    shared().pending_dirs.fetch_add(1, Ordering::SeqCst);
    let _ = shared().tx.send(Job::LoadDir {
        project_root: project_root.to_path_buf(),
        parent: parent.to_string(),
    });
}

/// 是否仍有未完成的草稿箱加载（排队或执行中）。
pub fn has_pending() -> bool {
    let s = shared();
    s.pending_roots.load(Ordering::SeqCst) > 0 || s.pending_dirs.load(Ordering::SeqCst) > 0
}

/// 取走已完成的模块根加载结果。
pub fn drain_loads() -> Vec<LoadResult> {
    std::mem::take(&mut *lock(&shared().load_results))
}

/// 取走已完成的子目录加载结果。
pub fn drain_dirs() -> Vec<DirResult> {
    std::mem::take(&mut *lock(&shared().dir_results))
}
