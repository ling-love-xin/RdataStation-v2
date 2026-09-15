//! 编辑器执行器（A14）：把编辑器的 SQL 交给引擎的 `SqlService`
//!
//! 编辑器（`crates/editor`）只定义执行端口（`QueryRunner`），不知道连接从哪来；这里回答
//! 那个问题：**当前活动连接**（`SqlService::execute(None, …)` 走连接管理器里的活动连接）。
//! 连接绑定到具体数据源是 1b 的事，那之前先"用当前连接跑"，跑不成会明确报错。
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
    runtime: tokio::runtime::Runtime,
    service: SqlService,
}

impl EngineQueryRunner {
    fn new() -> Option<Self> {
        // 单线程 runtime 足够：执行是串行的（编辑器通道同时只跑一次执行）
        let runtime = tokio::runtime::Builder::new_current_thread()
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
}

impl QueryRunner for EngineQueryRunner {
    fn run(&self, connection: Option<&str>, sql: &str) -> Result<QueryData, String> {
        let options = SqlExecuteOptions {
            // 历史由引擎侧统一记录（含耗时/行数，真实值）
            record_history: true,
            ..Default::default()
        };
        // 连接：文档绑定了就用它（B1）；未绑定回退到“当前活动连接”（1a 口径）
        let executed = self
            .runtime
            .block_on(self.service.execute(connection.map(str::to_string), sql, options))
            .map_err(|error| error.to_string())?;
        Ok(to_data(&executed.result, executed.elapsed_ms, executed.truncated))
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
