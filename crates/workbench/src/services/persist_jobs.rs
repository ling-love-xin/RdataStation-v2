//! 结果集「持久化为项目表」的落库通道（worker 线程 + 单槽 + UI 泵取）。
//!
//! 为什么要有这一层，而不是在点击回调里直接写库：
//! - 项目分析库 `{项目}/.RSmeta/analytics.duckdb` 的写连接由 `ProjectDatabaseManager` 按次开关
//!   （`ProjectDatabaseManager::open` 是 async，运行期谁也不能把一条连接挂很久）；
//! - DuckDB 写是**阻塞**的，写在事件路径上会卡住 UI（gpui 规范 §状态与副作用）。
//!
//! 数据为什么要搬运（而不是 ATTACH）：`tmp_q_` 在**进程内存单例**（in-memory 库，别的实例挂不到），
//! 且项目库被别的连接持有时 ATTACH 必失败（实测见
//! `crates/engine/tests/analysis_db_exclusive_probe.rs`）。所以走
//! `engine::duckdb::temp_table::promote::write_table_across_connections`（Parquet 中转）。
//!
//! 口径：目标表名 = [`persistent_table_name`]（`res_<描述>_<时间戳>`），已存在则**报错**不覆盖
//! （覆盖别人的持久表不是「顺手做对」的事，要显式再来一次）。

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};

use engine::duckdb::temp_table::promote::{
    persistent_table_name, write_table_across_connections, PromoteMode,
};
use engine::persistence::project_db::ProjectDatabaseManager;
use engine::services::duckdb_service::DuckDbService;

/// 一次落库请求
#[derive(Debug, Clone)]
pub struct PersistRequest {
    /// 项目根（目标 = `{root}/.RSmeta/analytics.duckdb`）
    pub project_root: PathBuf,
    /// 源表（进程内存单例里的 `tmp_q_…`）
    pub source_table: String,
    /// 展示用来源（状态栏提示里认得出是哪份结果）
    pub label: String,
    /// 表名描述（拼进 `res_<描述>_<时间戳>`）
    pub description: String,
}

/// 落库结果（UI 泵取走后提示）
#[derive(Debug, Clone)]
pub enum PersistOutcome {
    Done { table: String, rows: u64, label: String },
    Failed { label: String, reason: String },
}

struct Shared {
    tx: Sender<PersistRequest>,
    /// 已提交、尚未取走结果的条数（0 = 空闲）
    pending: Mutex<usize>,
    /// 结果槽：一次只放一条（落库是用户动作，不做批量）
    outcome: Mutex<Option<PersistOutcome>>,
}

static JOBS: OnceLock<Shared> = OnceLock::new();

fn shared() -> &'static Shared {
    JOBS.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<PersistRequest>();
        std::thread::Builder::new()
            .name("rds-persist-jobs".to_string())
            // DuckDB 的写入 + tokio runtime 初始化都不浅，给足栈
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                for request in rx {
                    let outcome = match persist_result_table(
                        &request.project_root,
                        &request.source_table,
                        &request.description,
                    ) {
                        Ok((table, rows)) => PersistOutcome::Done {
                            table,
                            rows,
                            label: request.label.clone(),
                        },
                        Err(reason) => PersistOutcome::Failed {
                            label: request.label.clone(),
                            reason,
                        },
                    };
                    if let Ok(mut slot) = shared().outcome.lock() {
                        *slot = Some(outcome);
                    }
                    if let Ok(mut pending) = shared().pending.lock() {
                        *pending = pending.saturating_sub(1);
                    }
                }
            })
            .expect("failed to spawn persist jobs worker");
        Shared {
            tx,
            pending: Mutex::new(0),
            outcome: Mutex::new(None),
        }
    })
}

/// 提交一次落库（立即返回；结果由 UI 泵取 [`take_outcome`] 取走）
pub fn submit(request: PersistRequest) -> Result<(), String> {
    if request.source_table.trim().is_empty() {
        return Err("源表名为空：这份结果还没有物化成临时表".to_string());
    }
    if let Ok(mut pending) = shared().pending.lock() {
        *pending += 1;
    }
    shared()
        .tx
        .send(request)
        .map_err(|e| format!("落库任务入队失败: {e}"))
}

/// UI 泵取结果（取走即清空）
pub fn take_outcome() -> Option<PersistOutcome> {
    shared()
        .outcome
        .lock()
        .ok()
        .and_then(|mut slot| slot.take())
}

/// 还有没有在跑的落库（泵据此决定要不要继续轮询）
pub fn has_pending() -> bool {
    shared()
        .pending
        .lock()
        .map(|pending| *pending > 0)
        .unwrap_or(false)
}

/// 同步落库（worker 与用例共用）：源表 → 项目分析库的持久表，返回（表名，行数）。
///
/// **边界：调用方不得在 async 上下文中调它**——本函数自建 tokio runtime 并 `block_on`
/// （tokio 会直接 panic `Cannot start a runtime from within a runtime`，实测于 2026-11-15）。
/// 生产调用点是本模块的 worker 线程（普通 std 线程），天然满足；用例必须用 `#[test]` 而非
/// `#[tokio::test]`。
///
/// 失败原因一律**可读**：撞名 → 引导改名；锁冲突 → 提示稍后重试。
pub fn persist_result_table(
    project_root: &Path,
    source_table: &str,
    description: &str,
) -> Result<(String, u64), String> {
    let table = persistent_table_name(description);
    let runtime = tokio::runtime::Runtime::new().map_err(|e| format!("无法启动异步运行时: {e}"))?;
    let manager = open_project_manager(&runtime, project_root)?;

    // 顺序固定：先取**源**（内存单例），再锁项目连接——反过来的实现在别处若也先拿项目再拿
    // 单例，就会凑出死锁。这里只此一处同时持两把锁。
    let singleton = DuckDbService::get_or_create_duckdb()
        .map_err(|e| format!("取内存分析库失败: {e}"))?;
    let source = singleton
        .lock()
        .map_err(|e| format!("内存分析库锁中毒: {e}"))?;

    let project_conn = manager
        .duckdb_conn()
        .ok_or_else(|| "项目库读模式打开（无写连接）：请重新打开项目后重试".to_string())?;
    let guard = runtime
        .block_on(project_conn.acquire())
        .map_err(|e| format!("取项目写连接失败: {e}"))?;
    let mut slot = runtime.block_on(guard.lock());
    let dest = slot
        .as_mut()
        .ok_or_else(|| "项目写连接已关闭：请重新打开项目后重试".to_string())?;

    let result = write_table_across_connections(
        &source,
        dest,
        source_table,
        &table,
        PromoteMode::Create,
    )
    .map_err(|e| {
        let text = e.to_string();
        if text.contains("already exists") || text.contains("已存在") {
            format!("项目分析库已存在表 {table}：换一个结果名（再分析一次）或先删掉旧表")
        } else if text.contains("另一个程序正在使用此文件") || text.contains("already open") {
            format!("项目分析库正被别的操作占用，请稍后重试（{text}）")
        } else {
            format!("写入项目分析库失败: {text}")
        }
    })?;

    drop(slot);
    drop(guard);
    drop(source);
    drop(manager);
    Ok((table, result))
}

/// 打开项目库，**对并发窗口做短重试**。
///
/// 依据（实测）：项目分析库是单连接句柄，运行期每一次项目库操作（连接保存 / 洞察索引 /
/// 资产动作）都会短暂持有它；恰好撞上时 `ProjectDatabaseManager::open` 会报
/// 「另一个程序正在使用此文件」。这类冲突是**毫秒级**的，重试两三次即可过去；
/// 仍失败才把可读原因交给用户（而不是把 DuckDB 原文抛出去）。
fn open_project_manager(
    runtime: &tokio::runtime::Runtime,
    project_root: &Path,
) -> Result<ProjectDatabaseManager, String> {
    const ATTEMPTS: usize = 3;
    let mut last = String::new();
    for attempt in 0..ATTEMPTS {
        match runtime.block_on(ProjectDatabaseManager::open(project_root, 4)) {
            Ok(manager) => return Ok(manager),
            Err(error) => {
                let text = error.to_string();
                let busy = text.contains("另一个程序正在使用此文件") || text.contains("already open");
                last = if busy {
                    format!("项目分析库正被别的操作占用（{text}）")
                } else {
                    format!("打开项目库失败: {text}")
                };
                if !busy {
                    return Err(last);
                }
                if attempt + 1 < ATTEMPTS {
                    std::thread::sleep(std::time::Duration::from_millis(150));
                }
            }
        }
    }
    Err(format!("{last}；请稍后重试（同一时间只允许一个项目库写连接）"))
}
#[cfg(test)]
mod tests {
    use super::PersistRequest;

    #[test]
    fn empty_source_table_is_rejected_before_queueing() {
        let request = PersistRequest {
            project_root: std::path::PathBuf::from("."),
            source_table: "   ".to_string(),
            label: "结果 1".to_string(),
            description: "x".to_string(),
        };
        assert!(super::submit(request).is_err());
    }
}
