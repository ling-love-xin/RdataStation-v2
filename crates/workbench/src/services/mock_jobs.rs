//! Mock 后台任务（M7）：把生成 / 追加 / **出口**的阻塞段放到工作线程。
//!
//! 为什么需要：生成是纯 CPU + DuckDB 往返的重活，落库与导出同样要跑完整个临时表——
//! 在 UI 线程 `block_on` 会冻结界面（十万行级尤其明显，且没有任何中断手段）。本模块沿用
//! `nav_jobs` / `scratchpad_jobs` 的成熟模式：**单一工作线程 + 结果槽**，视图只提交任务、
//! 轮询进度并取回结果，不参与阻塞。
//!
//! 任务类型（[`MockJobKind`]）：
//! - `Generate`：只产内存临时表 + 预览（**不写库**）；
//! - `Scenario(模板 id)`：按内置场景模板一次生成多张临时表（**同样不写库**；进度按「张表」计）；
//! - `AppendTo(表名)`：生成后追加到分析库既有表（同一次任务里完成，自增起点按表内行数接续）；
//! - `Persist` / `Export` / `Scratchpad`：三个**出口**，只读已生成的内存临时表，分别写分析库新表 /
//!   指定文件 / 草稿箱目录；目标表名与列定义全部取自**结果自己**（`MockGenInfo`）——
//!   场景模板的多张表因此能各自落到自己的表名下，**出口不读草稿**。
//!
//! 阶段（[`MockJobPhase`]）：生成有批次粒度进度；写入分析库与写文件跑在 DuckDB / 文件系统内部，
//! 探不到中间点，只能报「进行中」——面板据此切成不定量进度条。
//!
//! 取消：`cancel()` 置引擎的进程级取消标志，引擎在**批次边界**响应（10k 行/批），
//! 结果以 `Err("生成已取消")` 回传。只有含生成阶段的任务能取消；出口类不给取消
//! （强中断会留下半张表 / 半个文件），视图也不渲染取消按钮。
//!
//! 状态一致性：进度与结果放在**同一把锁下的同一个槽**里——worker 收尾时一次性
//! 「清进度 + 写结果」，UI 侧因此不会观察到「既无进度也无结果」的空洞。

use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};

use mock::mock_view::{
    MockDraft, MockGenInfo, MockJobDone, MockJobKind, MockJobPhase, MockJobProgress, MockJobState,
};

use crate::services::mock_generator;

/// 任务需要的外部路径（UI 线程解析好再交给工作线程）。
///
/// 工作线程不碰宿主状态（`Shared` 里的 `Rc<RefCell<…>>` 不可跨线程），
/// 所以「分析库在哪、项目根在哪」在提交任务时就定下来，出口类任务据此落地。
///
/// `db_path` 是**项目分析库**（`{项目}/.RSmeta/analytics.duckdb`）且可为 `None`：
/// Mock 不写全局库，而未打开项目时生成仍然可用（只产内存临时表）。
#[derive(Debug, Clone)]
pub struct JobPaths {
    /// 项目分析库路径（落库 / 追加 / 需要读既有表的生成）；未打开项目为 `None`
    pub db_path: Option<PathBuf>,
    /// 项目根（草稿箱出口与历史落点；未打开项目为 `None`）
    pub project_root: Option<PathBuf>,
}

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
    /// 单表生成用的草稿（出口类任务只用 `kind` 里的结果，不读它）
    draft: MockDraft,
    kind: MockJobKind,
    /// 任务需要的路径（提交时由 UI 线程解析）
    paths: JobPaths,
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

/// 执行一次任务，进度写进全局槽。
fn run_job(job: &Job) -> Result<MockJobDone, String> {
    // 需要碰库的任务（追加 / 落库）：项目分析库缺失时给可读原因，不让 DuckDB 报文件错
    let db_path = || {
        job.paths.db_path.as_deref().ok_or_else(|| {
            "未打开项目：Mock 只写项目分析库（{项目}/.RSmeta/analytics.duckdb）——请先打开项目；\
             要进全局分析库，用资产库存档（M6）或草稿箱（M5）升级"
                .to_string()
        })
    };
    match &job.kind {
        MockJobKind::Generate => Ok(MockJobDone::Generated(generate(job, None)?)),
        MockJobKind::Scenario(template) => {
            // 场景模板不写库：不需要 `db_path`（未打开项目也能生成）
            let (template_name, tables) = mock_generator::generate_scenario_at(
                template,
                move |tables_done, tables_total| {
                    let mut slot = lock(&shared().slot);
                    if let Some(progress) = slot.progress.as_mut() {
                        progress.phase = MockJobPhase::Generating;
                        // 场景任务的量纲是「张表」，不是行/批次（见 `MockJobKind::rows_total`）
                        progress.batches_done = tables_done;
                        progress.batches_total = tables_total;
                    }
                },
            )?;
            Ok(MockJobDone::ScenarioGenerated {
                template_name,
                tables,
            })
        }
        MockJobKind::AppendTo(table) => {
            let info = generate(job, Some(table))?;
            // 进入写入阶段：引擎侧写入没有批次回调，只能报「进行中」
            set_phase(MockJobPhase::Writing);
            let total_rows = mock_generator::append_table_at(db_path()?, &info, table)?;
            Ok(MockJobDone::Appended {
                table: table.clone(),
                total_rows,
            })
        }
        MockJobKind::Persist(info) => {
            set_phase(MockJobPhase::Writing);
            let rows = mock_generator::persist_table_at(db_path()?, info)?;
            // 表名与落库时使用的是同一取值口径（`persist_table_at` 用 trim 后的结果表名建表）
            Ok(MockJobDone::Persisted {
                table: info.table_name.trim().to_string(),
                rows,
            })
        }
        MockJobKind::Export { info, format, path } => {
            set_phase(MockJobPhase::Exporting);
            let message = mock_generator::export_file(info, format, path)?;
            Ok(MockJobDone::Exported { message })
        }
        MockJobKind::Scratchpad { info, format } => {
            set_phase(MockJobPhase::Exporting);
            let message =
                mock_generator::save_scratchpad(info, format, job.paths.project_root.as_deref())?;
            Ok(MockJobDone::Exported { message })
        }
    }
}

/// 生成阶段（进度回调是 `Fn(usize, usize) + Send + 'static`：只能经进程级单例回写，
/// 与 worker 同源）。
fn generate(job: &Job, append_to: Option<&str>) -> Result<MockGenInfo, String> {
    // 只有追加要在生成期读目标表（自增接续）：纯生成不碰任何库
    let db_path = match append_to {
        Some(_) => Some(job.paths.db_path.as_deref().ok_or_else(|| {
            "未打开项目：Mock 只写项目分析库（{项目}/.RSmeta/analytics.duckdb）——请先打开项目；\
             要进全局分析库，用资产库存档（M6）或草稿箱（M5）升级"
                .to_string()
        })?),
        None => None,
    };
    mock_generator::generate_at_with_progress(
        db_path,
        &job.draft,
        append_to,
        move |batches_done, batches_total| {
            let mut slot = lock(&shared().slot);
            if let Some(progress) = slot.progress.as_mut() {
                progress.phase = MockJobPhase::Generating;
                progress.batches_done = batches_done;
                progress.batches_total = batches_total;
            }
        },
    )
}

/// 切换当前阶段（批次与行数不动：写入 / 导出阶段的量纲是同一批行）。
fn set_phase(phase: MockJobPhase) {
    let mut slot = lock(&shared().slot);
    if let Some(progress) = slot.progress.as_mut() {
        progress.phase = phase;
    }
}

/// 提交后台任务；已有任务在进行中时返回 `Err`。
pub fn start(draft: &MockDraft, kind: MockJobKind, paths: &JobPaths) -> Result<(), String> {
    {
        let mut slot = lock(&shared().slot);
        if slot.progress.is_some() {
            return Err("已有任务在进行中".to_string());
        }
        // 上一轮结果若未被取走（正常轮询下不会发生），丢弃以免陈旧结果混入本次
        slot.done = None;
        slot.progress = Some(MockJobProgress {
            phase: kind.phase(),
            batches_done: 0,
            batches_total: 0,
            rows_total: kind.rows_total(draft),
        });
    }

    let sent = shared()
        .tx
        .send(Job {
            draft: draft.clone(),
            kind,
            paths: paths.clone(),
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
