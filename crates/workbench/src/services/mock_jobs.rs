//! Mock 生成后台任务（M7）：把生成 / 追加的**阻塞段**放到工作线程。
//!
//! 为什么需要：生成是纯 CPU + DuckDB 往返的重活，在 UI 线程 `block_on` 会冻结界面
//! （十万行级尤其明显，且没有任何中断手段）。本模块沿用 `nav_jobs` / `scratchpad_jobs`
//! 的成熟模式：**单一工作线程 + 结果槽**，视图只提交任务、轮询进度并取回结果，不参与阻塞。
//!
//! 任务类型（[`MockJobKind`]）：
//! - `Generate`：只产内存临时表 + 预览（**不写库**）；
//! - `AppendTo(表名)`：生成后追加到分析库既有表（同一次任务里完成，自增起点按表内行数接续）。
//!
//! 取消：`cancel()` 置引擎的进程级取消标志，引擎在**批次边界**响应（10k 行/批），
//! 结果以 `Err("生成已取消")` 回传；取消后临时表可能残留已写入的批次，下次生成会重建它。
//!
//! 状态一致性：进度与结果放在**同一把锁下的同一个槽**里——worker 收尾时一次性
//! 「清进度 + 写结果」，UI 侧因此不会观察到「既无进度也无结果」的空洞。

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};

use mock::mock_view::{MockDraft, MockJobDone, MockJobKind, MockJobProgress, MockJobState};

use crate::services::mock_generator;

/// 槽：进行中的进度 + 已完成未被取走的结果。
struct Slot {
    /// `None` = 无进行中任务
    progress: Option<MockJobProgress>,
    /// `Some` = 已完成、等待 UI 取走
    done: Option<Result<MockJobDone, String>>,
}

/// 共享状态：任务队列（串行语义由槽的 `progress` 保证）+ 结果槽。
struct Shared {
    tx: Sender<Job>,
    slot: Mutex<Slot>,
}

static JOBS: OnceLock<Shared> = OnceLock::new();

fn shared() -> &'static Shared {
    JOBS.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("rds-mock-jobs".to_string())
            // 生成在 debug 下递归较深，给足栈避免溢出（与 nav_jobs 同口径）。
            .stack_size(16 * 1024 * 1024)
            .spawn(move || worker(rx))
            .expect("failed to spawn mock jobs worker");
        Shared {
            tx,
            slot: Mutex::new(Slot {
                progress: None,
                done: None,
            }),
        }
    })
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// 任务（只能通过 [`start`] 提交）。
struct Job {
    draft: MockDraft,
    kind: MockJobKind,
    /// 目标分析库（追加时用于读目标表行数；全局库路径由装配层给）
    db_path: PathBuf,
}

fn worker(rx: mpsc::Receiver<Job>) {
    while let Ok(job) = rx.recv() {
        let result = run_job(&job);
        let mut slot = lock(&shared().slot);
        // 一次性收尾：清进度 + 写结果（UI 不会看到中间态）
        slot.progress = None;
        slot.done = Some(result);
    }
}

/// 执行一次任务（生成 + 可选的追加），进度写进全局槽。
fn run_job(job: &Job) -> Result<MockJobDone, String> {
    let append_to = match &job.kind {
        MockJobKind::Generate => None,
        MockJobKind::AppendTo(table) => Some(table.as_str()),
    };

    // 进度回调是 `Fn(usize, usize) + Send + 'static`：只能经进程级单例回写（与 worker 同源）
    let info = mock_generator::generate_at_with_progress(
        &job.db_path,
        &job.draft,
        append_to,
        move |batches_done, batches_total| {
            if let Some(progress) = lock(&shared().slot).progress.as_mut() {
                progress.batches_done = batches_done;
                progress.batches_total = batches_total;
            }
        },
    )?;

    match &job.kind {
        MockJobKind::Generate => Ok(MockJobDone::Generated(info)),
        MockJobKind::AppendTo(table) => {
            let total_rows = mock_generator::append_table_at(&job.db_path, &job.draft, &info, table)?;
            Ok(MockJobDone::Appended {
                table: table.clone(),
                total_rows,
            })
        }
    }
}

/// 提交后台任务；已有任务在进行中时返回 `Err`。
pub fn start(draft: &MockDraft, kind: MockJobKind, db_path: &Path) -> Result<(), String> {
    {
        let mut slot = lock(&shared().slot);
        if slot.progress.is_some() {
            return Err("已有生成任务在进行中".to_string());
        }
        // 上一轮结果若未被取走（正常轮询下不会发生），丢弃以免陈旧结果混入本次
        slot.done = None;
        slot.progress = Some(MockJobProgress {
            batches_done: 0,
            batches_total: 0,
            rows_total: draft.options.rows,
        });
    }

    let sent = shared()
        .tx
        .send(Job {
            draft: draft.clone(),
            kind,
            db_path: db_path.to_path_buf(),
        })
        .is_ok();
    if !sent {
        // 工作线程已退出：回滚槽状态，让视图立刻拿到可读错误
        let mut slot = lock(&shared().slot);
        slot.progress = None;
        slot.done = Some(Err("后台工作线程不可用".to_string()));
        return Err("后台工作线程不可用".to_string());
    }
    Ok(())
}

/// 当前任务状态（UI 轮询；轻量）。
pub fn state() -> MockJobState {
    let slot = lock(&shared().slot);
    match slot.progress {
        Some(progress) => MockJobState::Running(progress),
        None => MockJobState::Idle,
    }
}

/// 取走已完成任务的结果（**一次性**：取走后归位 Idle）。
pub fn take_done() -> Option<Result<MockJobDone, String>> {
    lock(&shared().slot).done.take()
}

/// 请求取消进行中的任务：置引擎的进程级取消标志（引擎在批次边界响应）。
///
/// 无任务时调用无害：下一次生成开始会重置该标志。
pub fn cancel() {
    mock::MockEngine::cancel();
}
