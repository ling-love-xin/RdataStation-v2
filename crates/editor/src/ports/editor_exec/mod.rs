//! 编辑器执行器（A14）：把编辑器的 SQL 交给引擎的 `SqlService`
//!
//! 编辑器（`crates/editor`）只定义执行端口（`QueryRunner`），不知道连接从哪来；这里回答
//! 那个问题：**当前活动连接**（`SqlService::execute(None, …)` 走连接管理器里的活动连接）。
//! 连接绑定到具体数据源是 1b 的事，那之前先"用当前连接跑"，跑不成会明确报错。
//!
//! ## 中断与超时（B3）
//!
//! - **中断**：`SqlService::cancel_query(conn_id)` 翻转该连接的取消令牌（引擎侧
//!   `query_with_cancel` 一直 select 着它），驱动因此回一条 "Query cancelled"。
//! - **超时**：按**连接**的 `query_timeout`（连接对话框的高级选项）交给引擎的
//!   `SqlExecuteOptions::timeout_ms`——到点后引擎自己取消并报 "Query timed out after Nms"。
//!   连接没配 = 没有超时，不在这里编一个默认值。
//!
//! ## 线程模型
//!
//! 编辑器的执行通道在自己的**工作线程**上调用 `QueryRunner::run`，所以这里可以安全地
//! `block_on` 一个专用 tokio runtime（驱动是异步的，而 UI 线程不能等）。
//!
//! ## 取数口径
//!
//! 结果一律经 `QueryResult::batches` / `to_rows()` 取（架构 §12 #21：驱动只填 `batches`，
//! 读 `rows` / `total_rows` **字段**会得到"大结果有数、小结果无数"）。

pub(crate) use std::sync::Arc;

pub(crate) use crate::analysis::AnalysisRequest;
pub(crate) use crate::channel::ExecChannel;
pub(crate) use crate::execution::{DuckDbExportRequest, QueryData, QueryRunner};
pub(crate) use crate::export::ExportFormat;
pub(crate) use crate::host::{FederatedSourcePort, FederatedSourceRecord};
pub(crate) use crate::shared::EditorShared;
pub(crate) use engine::duckdb::accel;
pub(crate) use engine::duckdb::federation::registry::{self as fed_registry, FederatedSource, MountState};
pub(crate) use engine::duckdb::federation::session as fed_session;
pub(crate) use engine::persistence::history_store::{self, SqlHistoryEntry};
pub(crate) use engine::services::execution_service::DuckDbExportFormat;
pub(crate) use engine::services::sql_service::{SqlExecuteOptions, SqlService, window_sql};
pub(crate) use shared::models::{QueryResult, Value};
pub(crate) use shared::{
    CurrentTier, FactExportMode, FactFrame, FactTier, dual_estimate, fact_export_mode,
    storage_choice,
};

mod accel_exec;
mod conn;
mod convert;
mod federation;
mod history;
mod runner;
#[cfg(test)]
mod tests;

/// 引擎执行器（编辑器执行端口的工作台实现）
pub(crate) struct EngineQueryRunner {
    /// 执行器的 tokio runtime（**多线程**，随执行器常驻）
    ///
    /// 为什么不是 `current_thread`：sqlx 的连接在**首次使用时**把驱动任务（读写协议的
    /// 那个后台任务）挂到当时的 runtime 上；单线程 runtime 只在 `block_on` 期间被驱动，
    /// 一旦某条语句死在另一个 runtime 上（事务提交就是这么一条：它走旁路调用），
    /// 驱动任务就没人跑了——真机现象是 `COMMIT` 永远等不到响应。多线程 runtime 留出
    /// 工作线程，让这些后台任务在任何调用路径下都能推进。
    runtime: tokio::runtime::Runtime,
    service: SqlService,
    /// 联邦源清单端口（工作线程专用：`QueryRunner` 是 `Send + Sync`，
    /// 端口实现不持 Shared，只调全局 DataSourceService）
    fed: Arc<dyn FederatedSourcePort>,
}

/// 联邦执行的一次组装结果（源清单 + 主源请求 + 组装时的实话）
///
/// 为什么要留住 `notes`：源清单浮层（T1.6）之前，用户看不到“为什么某个连接没进来”，
/// 不能让它静默——结果区那行小字得能说出来。
#[derive(Debug)]
pub(crate) struct FederatedPlan {
    /// 会话主人（按它缓存会话；= 本文档绑定的连接 id）
    owner: String,
    sources: Vec<FederatedSource>,
    /// 请求的主源别名（`None` = 本文档的连接没参与，由引擎选第一个可用源）
    primary: Option<String>,
    /// 组装时的说明（跳过的连接 / 本文档连接未参与）
    notes: Vec<String>,
}

/// 组装用的源记录（宿主从连接库 / 运行态读来；`url` 已带凭据）
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FedSourceRecord {
    conn_id: String,
    name: String,
    db_type: String,
    /// 运行时连接串（`accel::AccelSource::new` 认的那份）
    url: String,
}

/// 把执行器接到编辑器共享状态上（**启动装配调用一次**）
///
/// 建 runtime 失败时明确说一句：编辑器会以"未接入执行"运行——执行动作会给出
/// 可见的原因，而不是静默什么都不做。
pub fn attach(shared: &EditorShared, fed: Arc<dyn FederatedSourcePort>) {
    match EngineQueryRunner::new(fed) {
        Some(runner) => shared.attach_runner(Arc::new(runner)),
        None => {
            // 治理（2026-10-30）：启动装配边界双写——日志系统此时可能已就绪，
            // tracing 承接；未就绪时 eprintln 兜底（原行为不变）。
            tracing::warn!("执行器未能建立 tokio runtime，编辑器将以“未接入执行”运行");
            eprintln!("[editor] 执行器未能建立 tokio runtime，编辑器将以“未接入执行”运行");
        }
    }
}

/// 直接建一个执行器（**给集成测试用**）
///
/// 为什么留这个口子：联邦 / 加速的**源清单组装**发生在执行器里（读连接库、解口令、拼别名），
/// 那是窗口层之下的真逻辑；走窗口跑一遍代价太大，而不验又等于放着不测。
/// 返回 `None` = runtime 建不起来（与 [`attach`] 同口径）。
pub fn runner_for_test() -> Option<Arc<dyn QueryRunner>> {
    EngineQueryRunner::new(Arc::new(NoopFederatedSource))
        .map(|runner| Arc::new(runner) as Arc<dyn QueryRunner>)
}

/// 测试用空端口：不接任何连接库（集成测试只验执行链，不验宿主接线）。
pub(crate) struct NoopFederatedSource;

impl FederatedSourcePort for NoopFederatedSource {
    fn records(&self) -> Vec<FederatedSourceRecord> {
        Vec::new()
    }
}
