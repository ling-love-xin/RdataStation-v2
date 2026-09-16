//! 草稿箱后台任务（加载 / 导入与粘贴 / 清空回收站 / 内容搜索与替换）。
//!
//! 为什么需要：草稿箱有两类操作会明显拖慢 UI——① render 要读盘（首次进入、操作后重载、
//! 展开文件夹）；② 搬运字节或遍历全树的操作（导入、复制粘贴、清空回收站、全文搜索、
//! 批量替换）。直接在 UI 线程 `block_on` 都会冻结界面。
//! 本模块沿用 `nav_jobs` 的成熟模式：**单一工作线程 + tokio 运行时**串行执行任务，
//! 视图只提交任务、轮询原子量并取回结果队列，不参与阻塞。
//!
//! 任务类型：
//! - `LoadRoot`：模块根 + 外部引用可用性 + 回收站 + （已展开过的）子目录缓存刷新；
//! - `LoadDir`：展开文件夹时的单目录懒加载；
//! - `Import` / `Paste`：复制字节（导入外部文件、复制粘贴）；
//! - `EmptyTrash`：删除回收站内可能很大的 payload；
//! - `Search` / `ReplaceAll`：遍历全树搜索，以及「匹配 → 逐文件写回 → 重新搜索」。
//!
//! 不同步的设计原则：**只改元数据或只做 rename 的操作**（新建/重命名/剪切移动/删除入回收站/
//! 回收站还原/引用增删改）仍留在事件路径同步执行——它们是单次系统调用（微秒~毫秒级），
//! 迁到后台反而增加状态同步成本。
//!
//! 过期结果防护：`LoadRoot` 携带自增 `seq`，视图只接受不小于已应用序号的结果，
//! 避免「连续两次重载，先发的后到」把旧数据盖回去。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};

use crate::{ExternalReferenceStatus, ScratchpadEntry, ScratchpadStore, SearchMatch, TrashEntry};

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

/// 搜索 / 替换任务的结果载荷（回传主线程）。
pub struct SearchPayload {
    pub scanned: usize,
    pub truncated: bool,
    pub matches: Vec<SearchMatch>,
    /// 替换任务才有值：`(替换总处数, 受影响文件数)`。
    pub replaced: Option<(usize, usize)>,
}

/// 写操作结果（回传主线程；前端按变体决定文案、刷新与通知）。
pub enum OpResult {
    /// 导入外部文件（复制进模块根）。
    Import { outcome: Result<(), String> },
    /// 粘贴（剪切 = 移动，复制 = 递归复制）。
    Paste { cut: bool, outcome: Result<(), String> },
    /// 清空回收站。
    EmptyTrash { outcome: Result<(), String> },
    /// 内容搜索 / 替换后刷新（`replaced` 非空表示本次由替换发起）。
    Search {
        query: String,
        is_regex: bool,
        case_sensitive: bool,
        replaced: Option<(usize, usize)>,
        outcome: Result<SearchPayload, String>,
    },
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
    /// 导入外部文件（逐个复制进模块根）。
    Import { project_root: PathBuf, paths: Vec<PathBuf> },
    /// 粘贴（`cut = true` 走 `move_entry`，否则走递归 `copy_entry`）。
    Paste {
        project_root: PathBuf,
        cut: bool,
        paths: Vec<String>,
        target: String,
    },
    /// 清空项目级回收站。
    EmptyTrash { project_root: PathBuf },
    /// 内容搜索（结果回填到中央编辑区）。
    Search {
        project_root: PathBuf,
        query: String,
        case_sensitive: bool,
        is_regex: bool,
    },
    /// 批量替换：匹配 → 逐文件写回 → 重新搜索（一次任务里完成，避免中间态）。
    ReplaceAll {
        project_root: PathBuf,
        query: String,
        replacement: String,
        case_sensitive: bool,
        is_regex: bool,
    },
}

/// 共享状态：任务队列 + 未完成计数 + 结果队列。
struct Shared {
    tx: Sender<Job>,
    /// 未完成的模块根加载（排队 + 执行中）。
    pending_roots: AtomicUsize,
    /// 未完成的子目录加载。
    pending_dirs: AtomicUsize,
    /// 未完成的写操作 / 搜索替换任务。
    pending_ops: AtomicUsize,
    load_results: Mutex<Vec<LoadResult>>,
    dir_results: Mutex<Vec<DirResult>>,
    op_results: Mutex<Vec<OpResult>>,
}

/// 搜索上下文行数（与面板展示一致：命中行前后各 2 行）。
const SEARCH_CONTEXT_LINES: usize = 2;

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
            pending_ops: AtomicUsize::new(0),
            load_results: Mutex::new(Vec::new()),
            dir_results: Mutex::new(Vec::new()),
            op_results: Mutex::new(Vec::new()),
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
                let unavailable = format!("后台运行时不可用: {e}");
                match job {
                    Job::LoadRoot { seq, .. } => {
                        lock(&shared().load_results).push(LoadResult {
                            seq,
                            entries: Err(unavailable),
                            refs: Vec::new(),
                            trash: Vec::new(),
                            children: Vec::new(),
                        });
                        shared().pending_roots.fetch_sub(1, Ordering::SeqCst);
                    }
                    Job::LoadDir { parent, .. } => {
                        lock(&shared().dir_results).push(DirResult {
                            parent,
                            result: Err(unavailable),
                        });
                        shared().pending_dirs.fetch_sub(1, Ordering::SeqCst);
                    }
                    Job::Import { .. } => {
                        lock(&shared().op_results).push(OpResult::Import {
                            outcome: Err(unavailable),
                        });
                        shared().pending_ops.fetch_sub(1, Ordering::SeqCst);
                    }
                    Job::Paste { .. } => {
                        lock(&shared().op_results).push(OpResult::Paste {
                            cut: false,
                            outcome: Err(unavailable),
                        });
                        shared().pending_ops.fetch_sub(1, Ordering::SeqCst);
                    }
                    Job::EmptyTrash { .. } => {
                        lock(&shared().op_results).push(OpResult::EmptyTrash {
                            outcome: Err(unavailable),
                        });
                        shared().pending_ops.fetch_sub(1, Ordering::SeqCst);
                    }
                    Job::Search {
                        query,
                        case_sensitive,
                        is_regex,
                        ..
                    } => {
                        lock(&shared().op_results).push(OpResult::Search {
                            query,
                            is_regex,
                            case_sensitive,
                            replaced: None,
                            outcome: Err(unavailable),
                        });
                        shared().pending_ops.fetch_sub(1, Ordering::SeqCst);
                    }
                    Job::ReplaceAll {
                        query,
                        case_sensitive,
                        is_regex,
                        ..
                    } => {
                        lock(&shared().op_results).push(OpResult::Search {
                            query,
                            is_regex,
                            case_sensitive,
                            replaced: None,
                            outcome: Err(unavailable),
                        });
                        shared().pending_ops.fetch_sub(1, Ordering::SeqCst);
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
            Job::Import {
                project_root,
                paths,
            } => {
                let store = ScratchpadStore::new(project_root);
                let outcome = rt.block_on(async {
                    // 与同步版同口径：逐个导入，任一失败则报错（已成功的保留）。
                    for path in &paths {
                        store
                            .import_external_file(path)
                            .await
                            .map_err(|e| e.to_string())?;
                    }
                    Ok(())
                });
                lock(&shared().op_results).push(OpResult::Import { outcome });
                shared().pending_ops.fetch_sub(1, Ordering::SeqCst);
            }
            Job::Paste {
                project_root,
                cut,
                paths,
                target,
            } => {
                let store = ScratchpadStore::new(project_root);
                let outcome = rt.block_on(async {
                    for path in &paths {
                        if cut {
                            store
                                .move_entry(path, &target)
                                .await
                                .map_err(|e| e.to_string())?;
                        } else {
                            store
                                .copy_entry(path, &target)
                                .await
                                .map_err(|e| e.to_string())?;
                        }
                    }
                    Ok(())
                });
                lock(&shared()
                    .op_results)
                    .push(OpResult::Paste { cut, outcome });
                shared().pending_ops.fetch_sub(1, Ordering::SeqCst);
            }
            Job::EmptyTrash { project_root } => {
                let store = ScratchpadStore::new(project_root);
                let outcome = rt
                    .block_on(store.empty_trash())
                    .map_err(|e| e.to_string());
                lock(&shared().op_results).push(OpResult::EmptyTrash { outcome });
                shared().pending_ops.fetch_sub(1, Ordering::SeqCst);
            }
            Job::Search {
                project_root,
                query,
                case_sensitive,
                is_regex,
            } => {
                let store = ScratchpadStore::new(project_root);
                let outcome = rt.block_on(async {
                    let res = store
                        .search_file_content(&query, case_sensitive, SEARCH_CONTEXT_LINES, is_regex)
                        .await
                        .map_err(|e| e.to_string())?;
                    Ok(SearchPayload {
                        scanned: res.total_files_scanned,
                        truncated: res.truncated,
                        matches: res.matches,
                        replaced: None,
                    })
                });
                lock(&shared().op_results).push(OpResult::Search {
                    query,
                    is_regex,
                    case_sensitive,
                    replaced: None,
                    outcome,
                });
                shared().pending_ops.fetch_sub(1, Ordering::SeqCst);
            }
            Job::ReplaceAll {
                project_root,
                query,
                replacement,
                case_sensitive,
                is_regex,
            } => {
                let store = ScratchpadStore::new(project_root);
                let outcome = rt.block_on(async {
                    // 先搜（拿到去重文件列表），再逐文件写回，最后重搜返回新结果：
                    // 一次任务里完成，避免“已替换但仍显示旧命中”的中间态。
                    let before = store
                        .search_file_content(&query, case_sensitive, SEARCH_CONTEXT_LINES, is_regex)
                        .await
                        .map_err(|e| e.to_string())?;
                    let mut files: Vec<String> =
                        before.matches.iter().map(|m| m.file.clone()).collect();
                    files.sort();
                    files.dedup();
                    let mut total = 0usize;
                    let mut changed = 0usize;
                    for file in &files {
                        let r = store
                            .replace_in_file(
                                file,
                                &query,
                                &replacement,
                                is_regex,
                                case_sensitive,
                            )
                            .await
                            .map_err(|e| format!("{file}: {e}"))?;
                        if r.replaced > 0 {
                            changed += 1;
                            total += r.replaced;
                        }
                    }
                    let after = store
                        .search_file_content(&query, case_sensitive, SEARCH_CONTEXT_LINES, is_regex)
                        .await
                        .map_err(|e| e.to_string())?;
                    Ok(SearchPayload {
                        scanned: after.total_files_scanned,
                        truncated: after.truncated,
                        matches: after.matches,
                        replaced: Some((total, changed)),
                    })
                });
                let replaced = match &outcome {
                    Ok(p) => p.replaced,
                    Err(_) => None,
                };
                lock(&shared().op_results).push(OpResult::Search {
                    query,
                    is_regex,
                    case_sensitive,
                    replaced,
                    outcome,
                });
                shared().pending_ops.fetch_sub(1, Ordering::SeqCst);
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

/// 是否仍有未完成的草稿箱任务（加载 / 写操作 / 搜索替换）。
pub fn has_pending() -> bool {
    let s = shared();
    s.pending_roots.load(Ordering::SeqCst) > 0
        || s.pending_dirs.load(Ordering::SeqCst) > 0
        || s.pending_ops.load(Ordering::SeqCst) > 0
}

/// 取走已完成的模块根加载结果。
pub fn drain_loads() -> Vec<LoadResult> {
    std::mem::take(&mut *lock(&shared().load_results))
}

/// 取走已完成的子目录加载结果。
pub fn drain_dirs() -> Vec<DirResult> {
    std::mem::take(&mut *lock(&shared().dir_results))
}

/// 取走已完成的写操作 / 搜索替换结果。
pub fn drain_ops() -> Vec<OpResult> {
    std::mem::take(&mut *lock(&shared().op_results))
}

/// 提交导入（逐个复制外部文件进模块根）。
pub fn enqueue_import(project_root: &Path, paths: Vec<PathBuf>) {
    shared().pending_ops.fetch_add(1, Ordering::SeqCst);
    let _ = shared().tx.send(Job::Import {
        project_root: project_root.to_path_buf(),
        paths,
    });
}

/// 提交粘贴（`cut` 为剪切移动，否则为递归复制）。
pub fn enqueue_paste(project_root: &Path, cut: bool, paths: Vec<String>, target: &str) {
    shared().pending_ops.fetch_add(1, Ordering::SeqCst);
    let _ = shared().tx.send(Job::Paste {
        project_root: project_root.to_path_buf(),
        cut,
        paths,
        target: target.to_string(),
    });
}

/// 提交清空回收站。
pub fn enqueue_empty_trash(project_root: &Path) {
    shared().pending_ops.fetch_add(1, Ordering::SeqCst);
    let _ = shared().tx.send(Job::EmptyTrash {
        project_root: project_root.to_path_buf(),
    });
}

/// 提交内容搜索。
pub fn enqueue_search(project_root: &Path, query: &str, case_sensitive: bool, is_regex: bool) {
    shared().pending_ops.fetch_add(1, Ordering::SeqCst);
    let _ = shared().tx.send(Job::Search {
        project_root: project_root.to_path_buf(),
        query: query.to_string(),
        case_sensitive,
        is_regex,
    });
}

/// 提交批量替换（内含替换后的重新搜索）。
pub fn enqueue_replace_all(
    project_root: &Path,
    query: &str,
    replacement: &str,
    case_sensitive: bool,
    is_regex: bool,
) {
    shared().pending_ops.fetch_add(1, Ordering::SeqCst);
    let _ = shared().tx.send(Job::ReplaceAll {
        project_root: project_root.to_path_buf(),
        query: query.to_string(),
        replacement: replacement.to_string(),
        case_sensitive,
        is_regex,
    });
}

#[cfg(test)]
mod tests {
    // 注意：不通配导入（`use gpui_kit::*` 会把 gpui 的 `test` 宏带入作用域）。
    use super::*;
    use std::time::{Duration, Instant};

    /// 结果队列是**进程级共享**的，测试之间会互相取走结果 —— 用锁串行执行。
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn lock_tests() -> std::sync::MutexGuard<'static, ()> {
        TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 临时项目目录（每个用例一个，避免互扰）。
    fn temp_project(tag: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "rds_sp_jobs_{tag}_{}_{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("temp project");
        dir
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Runtime::new().expect("tokio runtime")
    }

    /// 轮询等待结果（真实工作线程 + 真实文件系统，给足 20 s）。
    fn wait_for<T>(what: &str, mut take: impl FnMut() -> Option<T>) -> T {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if let Some(value) = take() {
                return value;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("等待「{what}」超时（20 s）");
    }

    /// 按请求序号取出模块根加载结果（丢弃别人的）。
    fn wait_load(seq: u64) -> LoadResult {
        wait_for("模块根加载", || {
            let mut found = None;
            for r in drain_loads() {
                if r.seq == seq {
                    found = Some(r);
                }
            }
            found
        })
    }

    #[test]
    fn root_load_returns_entries_and_reports_failure() {
        let _guard = lock_tests();
        let project = temp_project("root");
        let store = ScratchpadStore::new(project.clone());
        let rt = rt();
        rt.block_on(store.create_entry("a.sql", None, false)).unwrap();
        rt.block_on(store.create_entry("data", None, true)).unwrap();
        // 先建文件：`save_file` 走 `resolve_path(必须已存在)`，它改的是既有文件的内容。
        rt.block_on(store.create_entry("b.csv", Some("data"), false))
            .unwrap();
        rt.block_on(store.save_file("data/b.csv", "x,y")).unwrap();

        // 带已展开子目录：重拉时一并回传子项（面板用它刷新缓存）。
        let seq = enqueue_root_load(&project, vec!["data".to_string()]);
        assert!(has_pending(), "入队后应有在途任务");
        let result = wait_load(seq);
        let entries = result.entries.expect("根列表应成功");
        assert_eq!(entries.len(), 2, "模块根应有 2 个条目");
        assert_eq!(result.children.len(), 1, "已展开子目录应被刷新");
        assert_eq!(result.children[0].1.len(), 1);

        // 轮询空闲后 `has_pending` 回落（面板靠它决定何时停掉「加载中…」）。
        wait_for("待办清零", || (!has_pending()).then_some(()));

        // 失败路径：把项目根指向一个**文件**，`create_dir_all` 必然失败。
        // 注意相对路径的基准是 `{项目}/scratchpad/`，所以那个文件在 `scratchpad/` 下：
        // 写成 `{项目}/a.sql` 是个不存在的路径，`create_dir_all` 反而会成功建出目录。
        let broken = project.join("scratchpad").join("a.sql");
        let seq_err = enqueue_root_load(&broken, Vec::new());
        let failed = wait_load(seq_err);
        assert!(failed.entries.is_err(), "非目录项目根应回传错误");

        std::fs::remove_dir_all(&project).ok();
    }

    #[test]
    fn paste_job_moves_and_copies() {
        let _guard = lock_tests();
        let project = temp_project("paste");
        let store = ScratchpadStore::new(project.clone());
        let rt = rt();
        rt.block_on(store.create_entry("dir", None, true)).unwrap();
        rt.block_on(store.create_entry("a.sql", None, false)).unwrap();
        rt.block_on(store.save_file("a.sql", "select 1")).unwrap();

        // 复制：
        enqueue_paste(&project, false, vec!["a.sql".to_string()], "dir");
        wait_for("复制结果", || {
            matches!(drain_ops().as_slice(), [OpResult::Paste { outcome: Ok(()), .. }]).then_some(())
        });
        assert!(project.join("scratchpad/dir/a_copy.sql").is_file());

        // 剪切：原位置应消失。
        enqueue_paste(&project, true, vec!["a.sql".to_string()], "dir");
        wait_for("剪切结果", || {
            matches!(drain_ops().as_slice(), [OpResult::Paste { cut: true, outcome: Ok(()), .. }])
                .then_some(())
        });
        assert!(!project.join("scratchpad/a.sql").exists(), "剪切后原文件应已移走");
        assert!(project.join("scratchpad/dir/a.sql").is_file());

        std::fs::remove_dir_all(&project).ok();
    }

    #[test]
    fn import_and_empty_trash_jobs() {
        let _guard = lock_tests();
        let project = temp_project("import");
        let outside = temp_project("outside");
        let source = outside.join("raw.csv");
        std::fs::write(&source, "a,b").unwrap();

        enqueue_import(&project, vec![source.clone()]);
        wait_for("导入结果", || {
            matches!(drain_ops().as_slice(), [OpResult::Import { outcome: Ok(()) }]).then_some(())
        });
        assert!(project.join("scratchpad/raw.csv").is_file(), "导入应复制进模块根");

        // 删一个文件入回收站，再清空。
        let store = ScratchpadStore::new(project.clone());
        let rt = rt();
        rt.block_on(store.delete_entry("raw.csv")).unwrap();
        assert_eq!(rt.block_on(store.list_trash()).unwrap().len(), 1);
        enqueue_empty_trash(&project);
        wait_for("清空回收站结果", || {
            matches!(drain_ops().as_slice(), [OpResult::EmptyTrash { outcome: Ok(()) }])
                .then_some(())
        });
        assert!(rt.block_on(store.list_trash()).unwrap().is_empty());

        std::fs::remove_dir_all(&project).ok();
        std::fs::remove_dir_all(&outside).ok();
    }

    #[test]
    fn search_and_replace_jobs_share_one_payload() {
        let _guard = lock_tests();
        let project = temp_project("replace");
        let store = ScratchpadStore::new(project.clone());
        let rt = rt();
        rt.block_on(store.create_entry("a.sql", None, false)).unwrap();
        rt.block_on(store.save_file("a.sql", "select ID from t\nselect id from t\n"))
            .unwrap();

        // 搜索：不区分大小写 → 两行命中。
        enqueue_search(&project, "id", false, false);
        let payload = wait_for("搜索结果", || {
            drain_ops().into_iter().find_map(|r| match r {
                OpResult::Search {
                    replaced: None,
                    outcome: Ok(payload),
                    ..
                } => Some(payload),
                _ => None,
            })
        });
        assert_eq!(payload.matches.len(), 2);
        assert!(payload.replaced.is_none(), "搜索任务不带替换汇总");

        // 替换：一次任务内完成写回 + 重搜（替换后不应再有命中）。
        enqueue_replace_all(&project, "id", "key", false, false);
        let payload = wait_for("替换结果", || {
            drain_ops().into_iter().find_map(|r| match r {
                OpResult::Search {
                    replaced: Some(_),
                    outcome: Ok(payload),
                    ..
                } => Some(payload),
                _ => None,
            })
        });
        assert_eq!(payload.replaced, Some((2, 1)), "2 处替换，1 个文件");
        assert!(payload.matches.is_empty(), "重搜结果应为空");
        assert_eq!(
            rt.block_on(store.read_file("a.sql")).unwrap(),
            "select key from t\nselect key from t\n"
        );

        std::fs::remove_dir_all(&project).ok();
    }

    #[test]
    fn dir_load_returns_children_only() {
        let _guard = lock_tests();
        let project = temp_project("dir");
        let store = ScratchpadStore::new(project.clone());
        let rt = rt();
        rt.block_on(store.create_entry("dir", None, true)).unwrap();
        rt.block_on(store.create_entry("x.sql", Some("dir"), false))
            .unwrap();
        rt.block_on(store.create_entry("sub", Some("dir"), true))
            .unwrap();

        enqueue_dir_load(&project, "dir");
        let result = wait_for("子目录加载", || drain_dirs().into_iter().next());
        assert_eq!(result.parent, "dir");
        let kids = result.result.expect("子目录列表应成功");
        assert_eq!(kids.len(), 2);
        assert!(
            kids.iter().any(|k| k.name == "sub")
                && kids.iter().any(|k| k.name == "x.sql"),
            "应回传该目录的直接子项"
        );

        std::fs::remove_dir_all(&project).ok();
    }
}
