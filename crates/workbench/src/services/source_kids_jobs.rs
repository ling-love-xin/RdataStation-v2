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

use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};

use crate::components::data_objects::{self, Object, SourceRef};

/// 一次源行列举的结果。
///
/// 联邦源给带限定名规则的 `MountedTable`（叶子只需复制名）；
/// **连接行**给翻好的 [`Object`]（叶子可选用、可取样）——连接行的叶子同样是分析实例视角，
/// 只是这几类叶子要能进洞察取样，得带 `srctable:` 身份（铁律 H 组第 37 条）。
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
    /// 文件库（`.duckdb` / `.sqlite`）**已挂载**的本体路径：连接行的叶子要靠它算 ATTACH 别名。
    /// `None` = 不是文件库，或这一刻没挂（没挂就没有叶子可列，见 [`connection_kids`]）。
    mounted_file: Option<String>,
    /// 行上显示的名字：失败原话里用它，别把 `conn:C_xxx` 这种内部 id 甩给用户
    display: String,
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
                    let result = load_kids(
                        &request.source,
                        request.mounted_file.as_deref(),
                        &request.display,
                    );
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
    mounted_file: Option<String>,
    display: String,
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
            mounted_file,
            display,
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
///
/// 所有分支都取**分析实例视角**（铁律 H 组第 37 条）：列的是「分析实例此刻能查到什么」，
/// 不是「源库里有什么」。`NavigatorService` 直连内省不作叶子来源——它只在归档前选表时用。
pub fn load_kids(
    source: &SourceRef,
    mounted_file: Option<&str>,
    display: &str,
) -> Result<Kids, String> {
    match source {
        SourceRef::Federated { owner, alias } => {
            engine::duckdb::federation::session::list_tables_for(owner, alias).map(Kids::Mounted)
        }
        SourceRef::FileView { alias } => {
            engine::duckdb::file_view::list_tables_global(alias).map(Kids::Mounted)
        }
        SourceRef::Connection { conn_id } => connection_kids(conn_id, mounted_file, display),
    }
}

/// 连接行的叶子：**只从分析实例里取**（第 37 条），认不出挂载就如实说，不给空清单。
///
/// 一个连接能挂进实例的路只有两条：本地加速（实例按 `conn_id` 记实际别名，冲突时
/// 可能是 `rds_src_2`）、文件库 ATTACH（`.duckdb` / `.sqlite`，按本体路径算别名）。
/// 两条都没有 = 这一刻实例里没有它的表，展开只会得到空列表——所以这里报错并教怎么挂，
/// 而不是假装「库里没表」。
fn connection_kids(
    conn_id: &str,
    mounted_file: Option<&str>,
    display: &str,
) -> Result<Kids, String> {
    if let Some(instance) = engine::duckdb::instance::current() {
        if instance.alias_for_conn(conn_id).is_some() {
            return instance
                .list_mounted_tables_for_conn(conn_id)
                .map(|tables| Kids::Connection(connection_leaves(conn_id, &tables)));
        }
    }
    if let Some(path) = mounted_file.filter(|path| engine::duckdb::file_view::is_database_file(path))
    {
        let alias = engine::duckdb::file_view::alias_for(path);
        return engine::duckdb::file_view::list_tables_global(&alias)
            .map(|tables| Kids::Connection(connection_leaves(conn_id, &tables)));
    }
    Err(format!(
        "「{display}」这一刻没挂进分析实例，没有表可列：先在这条连接上开「本地加速」；\
         走联邦挂的源，请展开那条联邦源行。"
    ))
}

/// 挂载清单 → 连接叶子对象。
///
/// catalog 一律留空：`MountedTable` 的 schema 已经是**源库自己的**一层（MySQL 是库名、
/// PG 是 schema、本机库文件是 `main`），拼 `schema.表` 就是这条连接上的合法取样名；
/// 硬塞 DuckDB 侧别名反而会让洞察取样 SQL 在源连接上跑不通。
fn connection_leaves(
    conn_id: &str,
    tables: &[engine::duckdb::accel::MountedTable],
) -> Vec<Object> {
    tables
        .iter()
        .map(|table| data_objects::source_table_object(conn_id, "", &table.schema, &table.name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Kids, load_kids};
    use crate::components::data_objects::SourceRef;

    /// 不存在的连接：列举必须报错，而不是返回空清单冒充「库里没表」。
    /// 报错原话还要把**怎么挂**说出来（第 37 条：实例里查不到就不装「没表」）。
    #[test]
    fn unknown_sources_yield_errors_not_empty_lists() {
        let outcome = load_kids(
            &SourceRef::Connection {
                conn_id: "C_not_exists_9".to_string(),
            },
            None,
            "演示连接",
        );
        assert!(outcome.is_err(), "不存在的连接应报错：{outcome:?}");
        match outcome {
            Err(reason) => {
                assert!(!reason.trim().is_empty(), "错误原因不能为空");
                assert!(
                    reason.contains("本地加速"),
                    "错误原话要指出去哪挂：{reason}"
                );
                assert!(reason.contains("演示连接"), "报错要用行上的名字：{reason}");
            }
            Ok(Kids::Mounted(_)) | Ok(Kids::Connection(_)) => unreachable!(),
        }
    }
}
