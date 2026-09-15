//! 结果集服务（SQL 执行侧的外观）
//!
//! 提供：
//! - SQL 过滤（拼接 WHERE 重新查询）
//! - DuckDB 深度分析（针对临时表）
//! - 临时表创建与句柄获取
//! - 单元格回写与结果导出
//!
//! **洞察不属于本模块**。列画像 / 质量评分 / 规则 / 表探查 / Schema 洞察
//! 归 `crates/insight`，统一从 `insight::InsightService` 进入
//! （归属变更见 `docs/architecture/insight/insight-architecture.md` §3、开发方案 §3）。
//!
//! 两者的交界：结果集为洞察提供 **DuckDB 临时表**（`create_duckdb_temp_table`）
//! 与**连接句柄**（`get_or_create_duckdb`），洞察在其上做分析。

use shared::error::CoreError;

use engine::services::result_types::ResultSet;

// ==================== ResultService（外观 / Facade）====================

pub struct ResultService;

impl ResultService {
    pub async fn re_execute_with_filter(
        conn_id: String,
        original_sql: &str,
        where_clause: &str,
        order_clause: &str,
    ) -> Result<ResultSet, CoreError> {
        engine::services::execution_service::re_execute_with_filter(
            conn_id,
            original_sql,
            where_clause,
            order_clause,
        )
        .await
    }

    pub fn execute_duckdb_analysis(
        temp_table: &str,
        sql: &str,
        columns: Option<Vec<String>>,
        rows: Option<Vec<Vec<serde_json::Value>>>,
    ) -> Result<ResultSet, CoreError> {
        engine::services::execution_service::execute_duckdb_analysis(temp_table, sql, columns, rows)
    }

    pub fn get_or_create_duckdb(
    ) -> Result<std::sync::Arc<std::sync::Mutex<duckdb::Connection>>, CoreError> {
        engine::services::duckdb_service::DuckDbService::get_or_create_duckdb()
    }

    pub fn create_duckdb_temp_table(
        columns: &[String],
        rows: &[Vec<serde_json::Value>],
    ) -> Result<String, CoreError> {
        engine::services::duckdb_service::DuckDbService::create_duckdb_temp_table(columns, rows)
    }

    pub async fn save_cell_update(
        conn_id: String,
        table_name: &str,
        column_name: &str,
        new_value: &serde_json::Value,
        row_identity: &std::collections::HashMap<String, serde_json::Value>,
    ) -> Result<(usize, String), CoreError> {
        use engine::connection_manager;
        use engine::services::sql_service::{value_to_sql, SqlExecuteOptions, SqlService};

        let set_clause = format!("`{}` = {}", column_name, value_to_sql(new_value));

        let where_parts: Vec<String> = row_identity
            .iter()
            .filter(|(k, _)| *k != column_name)
            .map(|(col, val)| format!("`{}` = {}", col, value_to_sql(val)))
            .collect();

        if where_parts.is_empty() {
            return Err(CoreError::common(shared::error::CommonError::General(
                "无法构建 WHERE 条件：行标识数据为空".to_string(),
            )));
        }

        let sql = format!(
            "UPDATE `{}` SET {} WHERE {}",
            table_name,
            set_clause,
            where_parts.join(" AND ")
        );

        let manager = connection_manager::get_connection_manager();
        let service = SqlService::new(manager.clone());
        let opts = SqlExecuteOptions {
            record_history: false,
            use_transaction: true,
            timeout_ms: Some(10000),
            use_cache: false,
        };

        let result = service.execute(Some(conn_id), &sql, opts).await?;
        let affected = match result.result.affected_rows {
            Some(n) => n,
            None => {
                tracing::warn!("数据库未返回 affected_rows，UPDATE 影响行数未知");
                0
            }
        };
        Ok((affected as usize, format!("成功更新 {} 行", affected)))
    }

    pub fn export_result(
        temp_table: &str,
        file_path: &str,
        format: &str,
    ) -> Result<String, CoreError> {
        use engine::services::duckdb_service::{DuckDbService, ExportFormat};

        let fmt = match format {
            "csv" => ExportFormat::Csv,
            "parquet" => ExportFormat::Parquet,
            "xlsx" => ExportFormat::Xlsx,
            other => {
                return Err(CoreError::common(shared::error::CommonError::General(
                    format!("不支持的导出格式: {}", other),
                )))
            }
        };
        DuckDbService::export_temp_table(temp_table, file_path, fmt)
    }
}
