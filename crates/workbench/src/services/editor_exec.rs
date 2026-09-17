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

use std::sync::Arc;

use editor::execution::{QueryData, QueryRunner};
use editor::shared::EditorShared;
use engine::services::sql_service::{SqlExecuteOptions, SqlService};
use shared::models::{QueryResult, Value};

/// 引擎执行器（编辑器执行端口的工作台实现）
struct EngineQueryRunner {
    /// 执行器的 tokio runtime（**多线程**，随执行器常驻）
    ///
    /// 为什么不是 `current_thread`：sqlx 的连接在**首次使用时**把驱动任务（读写协议的
    /// 那个后台任务）挂到当时的 runtime 上；单线程 runtime 只在 `block_on` 期间被驱动，
    /// 一旦某条语句死在另一个 runtime 上（事务提交就是这么一条：它走旁路调用），
    /// 驱动任务就没人跑了——真机现象是 `COMMIT` 永远等不到响应。多线程 runtime 留出
    /// 工作线程，让这些后台任务在任何调用路径下都能推进。
    runtime: tokio::runtime::Runtime,
    service: SqlService,
}

impl EngineQueryRunner {
    fn new() -> Option<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("rds-editor-exec-rt")
            .build()
            .ok()?;
        let manager = engine::connection_manager::get_connection_manager();
        Some(Self {
            runtime,
            service: SqlService::new(manager.clone()),
        })
    }

    /// 本次执行该用多长超时（毫秒）：按**连接**的 `query_timeout`（B3）
    ///
    /// 未绑定的文档看当前活动连接（与 `execute(None, …)` 同口径）；连接没配就是
    /// **没有超时**——不在这里造一个默认值（超时是连接属性，不是编辑器属性）。
    async fn query_timeout_ms(&self, connection: Option<&str>) -> Option<u64> {
        let manager = engine::connection_manager::get_connection_manager();
        let conn_id = match connection {
            Some(id) => Some(id.to_string()),
            None => manager.get_active_connection_id().await,
        }?;
        let config = manager.get_connection_config(&conn_id).await?;
        config.query_timeout.map(|secs| u64::from(secs) * 1000)
    }
}

impl QueryRunner for EngineQueryRunner {
    fn run(
        &self,
        connection: Option<&str>,
        sql: &str,
        run_options: editor::execution::RunOptions,
    ) -> Result<QueryData, String> {
        let timeout_ms = self.runtime.block_on(self.query_timeout_ms(connection));
        let options = SqlExecuteOptions {
            // 历史由引擎侧统一记录（含耗时/行数，真实值）
            record_history: true,
            // 超时：引擎在到点后**自己取消**并返回“Query timed out after Nms”
            timeout_ms,
            // B4：自动提交关 → 引擎在没有事务时先自动开一个（DDL 除外）
            use_transaction: run_options.use_transaction,
            ..Default::default()
        };
        // 连接：文档绑定了就用它（B1）；未绑定回退到“当前活动连接”（1a 口径）
        //
        // 【B5b】查询走 `execute_first_segment`：只取第一段（1000 行），后续段由「取下一段」来要；
        // 写语句没有分段可言，引擎内部会走原路（历史记的都是**用户执行的原 SQL**）。
        let executed = self
            .runtime
            .block_on(self.service.execute_first_segment(
                connection.map(str::to_string),
                sql,
                editor::execution::SEGMENT_ROWS,
                options,
            ));
        // 【B8】不管成败，引擎都记了一条历史（含耗时 / 行数 / 失败原因）——
        // 摇一下版本号：右 Dock 的历史面板据此重新加载（面板不读盘，也不每帧问文件）
        editor::history::bump();
        let executed = executed.map_err(|error| error.to_string())?;
        let mut data = to_data(&executed.result, executed.elapsed_ms, executed.truncated);
        // 【B5b】首段拿满了就标“可能还有下一段”（写语句没有分段可言）：
        // 拿不满 = 这一份结果就这么多，界面不摆「取下一段」。
        data.has_more =
            !data.columns.is_empty() && data.rows.len() == editor::execution::SEGMENT_ROWS;
        Ok(data)
    }

    /// 【B5b】取下一段：同一条原 SQL 的后一段（引擎套窗口取，见 `SqlService::execute_segment`）
    fn fetch_next(
        &self,
        connection: Option<&str>,
        sql: &str,
        offset: usize,
        limit: usize,
    ) -> Result<QueryData, String> {
        let timeout_ms = self.runtime.block_on(self.query_timeout_ms(connection));
        let executed = self
            .runtime
            .block_on(self.service.execute_segment(
                connection.map(str::to_string),
                sql,
                limit,
                offset,
                timeout_ms,
            ))
            .map_err(|error| error.to_string())?;
        let mut data = to_data(&executed.result, executed.elapsed_ms, executed.truncated);
        // “还有没有下一段”由**拿没拿满**判（引擎不问总数）
        data.has_more = data.rows.len() == limit;
        Ok(data)
    }

    /// 事务状态：读引擎的**真值**（活动事务挂在连接管理器上，不是猜的）
    fn transaction_snapshot(&self, connection: Option<&str>) -> editor::execution::TxSnapshot {
        let status = self
            .runtime
            .block_on(self.service.get_transaction_status(connection.map(str::to_string)));
        match status {
            Ok(status) => editor::execution::TxSnapshot {
                in_transaction: status.is_in_transaction,
            },
            // 读不到就当作“不在事务里”：界面会因此少一个 TX 标记，不会凭空多一个
            Err(_) => editor::execution::TxSnapshot::default(),
        }
    }

    /// 事务动作：走引擎的驱动级事务接口（不是拼 `BEGIN` 文本——MySQL 会报 1295）
    fn transaction(
        &self,
        connection: Option<&str>,
        action: editor::execution::TxAction,
    ) -> Result<(), String> {
        use editor::execution::TxAction;
        let conn = connection.map(str::to_string);
        let result = match action {
            TxAction::Begin => self.runtime.block_on(self.service.begin_transaction(conn)),
            TxAction::Commit => self.runtime.block_on(self.service.commit_transaction(conn)),
            TxAction::Rollback => self.runtime.block_on(self.service.rollback_transaction(conn)),
        };
        result.map(|_status| ()).map_err(|error| error.to_string())
    }

    /// 引擎侧四个原生驱动都支持事务（P0.2 实测：含 MySQL 的驱动级事务）
    fn supports_transactions(&self) -> bool {
        true
    }

    /// 中断：走引擎的取消令牌（`Ok(false)` = 令牌不在 → 已在两句之间）
    fn cancel(&self, connection: Option<&str>) -> Result<bool, String> {
        self.runtime
            .block_on(self.service.cancel_query(connection.map(str::to_string)))
            .map_err(|error| error.to_string())
    }
}

/// 把执行器接到编辑器共享状态上（**启动装配调用一次**）
///
/// 建 runtime 失败时明确说一句：编辑器会以"未接入执行"运行——执行动作会给出
/// 可见的原因，而不是静默什么都不做。
pub fn attach(shared: &EditorShared) {
    match EngineQueryRunner::new() {
        Some(runner) => shared.attach_runner(Arc::new(runner)),
        None => eprintln!("[editor] 执行器未能建立 tokio runtime，编辑器将以“未接入执行”运行"),
    }
}

/// 引擎结果 → 编辑器结果（列 + 字符串化行；不读 `rows` 字段）
fn to_data(result: &QueryResult, elapsed_ms: u64, truncated: bool) -> QueryData {
    QueryData {
        columns: column_names(result),
        rows: result
            .to_rows()
            .iter()
            .map(|row| row.iter().map(cell_text).collect())
            .collect(),
        elapsed_ms,
        truncated,
        // B5：写语句的影响行数由驱动报（拿不到就是 `None`，不编造 0）
        affected_rows: result.affected_rows,
        // 非分段路径没有“下一段”可言；分段抓取由调用方按“拿没拿满”标
        has_more: false,
    }
}

/// 列名：优先驱动填的 `columns`，否则从 Arrow schema 取
///
/// 驱动并不保证填 `columns`（架构 §12 #22），所以这里要有兜底，否则网格会出现无名列。
fn column_names(result: &QueryResult) -> Vec<String> {
    if !result.columns.is_empty() {
        return result.columns.clone();
    }
    result
        .batches
        .first()
        .map(|batch| {
            batch
                .schema()
                .fields()
                .iter()
                .map(|field| field.name().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// 单元格文案
///
/// 直接走 `Value` 的 `Display`（`NULL` / 数字 / 文本都已定义好）。将来若要按类型区分
/// “NULL”与字符串 “NULL”（1b 的类型化网格），改这里一处并补测试。
fn cell_text(value: &Value) -> String {
    value.to_string()
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{cell_text, column_names};
    use shared::models::{QueryResult, Value};

    #[test]
    fn driver_columns_win_over_the_schema() {
        let mut result = QueryResult::empty();
        result.columns = vec!["n".to_string(), "m".to_string()];
        assert_eq!(column_names(&result), ["n".to_string(), "m".to_string()]);
    }

    #[test]
    fn a_result_without_columns_or_batches_has_no_names() {
        let result = QueryResult::empty();
        assert!(column_names(&result).is_empty());
    }

    #[test]
    fn cells_show_the_value_not_a_debug_dump() {
        assert_eq!(cell_text(&Value::Null), "NULL");
        assert_eq!(cell_text(&Value::Int(42)), "42");
        assert_eq!(cell_text(&Value::Float(1.5)), "1.5");
        assert_eq!(cell_text(&Value::Bool(true)), "true");
        assert_eq!(cell_text(&Value::Text("中文".to_string())), "中文");
    }
}
