//! 源行展开的**后台列举通道**（worker 线程 + 结果槽 + UI 泵取）。
//!
//! 为什么要有这一层：展开一个源要问远端库（catalog → schema → 表），MySQL / Oracle 上
//! 就是一次网络往返；这段同步跑在事件路径上会卡住 UI，而清单的每次刷新（1s 指纹泵）
//! 又会把所有仍展开的源重列一遍——不挪到后台线程，面板一有展开行就每秒顿一下。
//!
//! 形态照 `persist_jobs`：一次性 worker 线程 + `mpsc` 队列 + 计数 + 结果槽。
//! 与它不同的只有一处：**结果按 key 排队**（可以同时展开多个源），所以槽是 `Vec`。
//!
//! UI 侧取走结果的泵在 `view/workspace.rs::ensure_source_kids_pump`。

use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};

use crate::components::data_objects::{self, Object, SourceRef};

/// 一次源行列举的结果。
///
/// 两类源的形状不同：已挂载的源（联邦 / 加速 / 文件库）给带限定名规则的
/// `MountedTable`（叶子只需复制名），源库连接给翻好的 [`Object`]（叶子可选用、可取样）。
#[derive(Debug, Clone)]
pub enum Kids {
    Mounted(Vec<engine::duckdb::accel::MountedTable>),
    Connection(Vec<Object>),
}

/// 一次列举的回执（成功 / 失败都带 key，UI 侧据此写回该行缓存）
#[derive(Debug, Clone)]
pub struct Outcome {
    pub key: String,
    /// 用户点击展开那次要弹错；例行刷新重取则静默（否则每秒弹一遍旧错误）
    pub announce: bool,
    pub result: Result<Kids, String>,
}

struct Request {
    key: String,
    source: SourceRef,
    /// 连接列举需要项目根（`NavigatorService` 的上下文；空 = 没开项目）
    project_root: Option<PathBuf>,
    announce: bool,
}

struct Shared {
    tx: Sender<Request>,
    /// 已提交、尚未取走的条数（0 = 空闲）
    pending: Mutex<usize>,
    /// 回执队列：同一拍可能完成多条（多个源一起展开）
    outcomes: Mutex<Vec<Outcome>>,
}

static JOBS: OnceLock<Shared> = OnceLock::new();

fn shared() -> &'static Shared {
    JOBS.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Request>();
        std::thread::Builder::new()
            .name("rds-source-kids".to_string())
            // 列举里会自建 tokio runtime（连接内省）+ 驱动栈，给足栈
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                for request in rx {
                    let result = load_kids(&request.source, request.project_root.clone());
                    if let Ok(mut outcomes) = shared().outcomes.lock() {
                        outcomes.push(Outcome {
                            key: request.key,
                            announce: request.announce,
                            result,
                        });
                    }
                    if let Ok(mut pending) = shared().pending.lock() {
                        *pending = pending.saturating_sub(1);
                    }
                }
            })
            .expect("failed to spawn source kids worker");
        Shared {
            tx,
            pending: Mutex::new(0),
            outcomes: Mutex::new(Vec::new()),
        }
    })
}

/// 提交一次列举（立即返回；结果由 UI 泵 [`take_outcomes`] 取走）。
pub fn submit(
    key: String,
    source: SourceRef,
    project_root: Option<PathBuf>,
    announce: bool,
) -> Result<(), String> {
    if let Ok(mut pending) = shared().pending.lock() {
        *pending += 1;
    }
    shared()
        .tx
        .send(Request {
            key,
            source,
            project_root,
            announce,
        })
        .map_err(|e| format!("源行列举任务入队失败: {e}"))
}

/// UI 泵取回执（取走即清空）。
pub fn take_outcomes() -> Vec<Outcome> {
    shared()
        .outcomes
        .lock()
        .map(|mut slot| std::mem::take(&mut *slot))
        .unwrap_or_default()
}

/// 还有没有在跑的列举（泵据此可以只在有任务时轮询，或按固定节奏取）。
pub fn has_pending() -> bool {
    shared()
        .pending
        .lock()
        .map(|pending| *pending > 0)
        .unwrap_or(false)
}

/// 同步列举（worker 与用例共用）：按源类型分派到引擎对应入口。
///
/// 失败一律翻成**可读原话**：调用方会把它们写进面板提示条。
pub fn load_kids(source: &SourceRef, project_root: Option<PathBuf>) -> Result<Kids, String> {
    match source {
        SourceRef::Federated { owner, alias } => {
            engine::duckdb::federation::session::list_tables_for(owner, alias).map(Kids::Mounted)
        }
        SourceRef::Accel { conn_id } => {
            engine::duckdb::accel::list_tables_for(conn_id).map(Kids::Mounted)
        }
        SourceRef::FileView { alias } => {
            engine::duckdb::file_view::list_tables_global(alias).map(Kids::Mounted)
        }
        SourceRef::Connection { conn_id } => {
            crate::components::source_picker::list_connection_tables_in(project_root, conn_id).map(
                |tables| {
                    Kids::Connection(
                        tables
                            .iter()
                            .map(|table| {
                                data_objects::source_table_object(
                                    conn_id,
                                    &table.catalog,
                                    &table.schema,
                                    &table.name,
                                )
                            })
                            .collect(),
                    )
                },
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Kids, load_kids};
    use crate::components::data_objects::SourceRef;

    /// 空 id 不该被提交出来（上游 `source_ref` 已挡一层，这里再钉一道）。
    #[test]
    fn unknown_sources_yield_errors_not_empty_lists() {
        // 不存在的连接：列举必须报错，而不是返回空清单冒充「库里没表」
        let outcome = load_kids(
            &SourceRef::Connection {
                conn_id: "C_not_exists_9".to_string(),
            },
            None,
        );
        assert!(outcome.is_err(), "不存在的连接应报错：{outcome:?}");
        match outcome {
            Err(reason) => assert!(!reason.trim().is_empty(), "错误原因不能为空"),
            Ok(Kids::Mounted(_)) | Ok(Kids::Connection(_)) => unreachable!(),
        }
    }
}
