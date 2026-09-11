//! 结果集服务 + 洞察计算引擎
//!
//! 提供：
//! - SQL 过滤（拼接 WHERE 重新查询）
//! - DuckDB 深度分析（针对临时表）
//! - 列洞察全量统计（统计 + 样本 + 直方图）
//! - 规则引擎 API（统一入口，SQL 模板与 Rust 代码分离）

use shared::error::CoreError;

use engine::persistence::insight_types::{
    ColumnInsightFull, ColumnStats, QualityScore, TableProfile, TableQuality,
};
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

    pub fn get_column_insight_full(
        temp_table: &str,
        column_name: &str,
    ) -> Result<ColumnInsightFull, CoreError> {
        insight::insight_engine::get_column_insight_full(temp_table, column_name)
    }

    pub fn get_column_insights(
        temp_table: &str,
        column_name: &str,
    ) -> Result<ColumnStats, CoreError> {
        insight::insight_engine::get_column_insights(temp_table, column_name)
    }

    pub fn execute_insight_rule(
        rule_id: &str,
        conn: &duckdb::Connection,
        params: &std::collections::HashMap<String, String>,
    ) -> Result<insight::ExecutionResult, CoreError> {
        insight::insight_engine::execute_insight_rule(rule_id, conn, params)
    }

    pub fn list_insight_rules(category: Option<&str>) -> Result<Vec<serde_json::Value>, CoreError> {
        insight::insight_engine::list_insight_rules(category)
    }

    pub fn list_rules_for_column(column_type: &str) -> Result<Vec<serde_json::Value>, CoreError> {
        insight::insight_engine::list_rules_for_column(column_type)
    }

    pub fn compute_column_quality(stats: &ColumnInsightFull) -> QualityScore {
        insight::quality_scorer::compute_column_quality(stats)
    }

    pub fn compute_table_quality(
        table_name: &str,
        stats_list: &[ColumnInsightFull],
    ) -> TableQuality {
        insight::quality_scorer::compute_table_quality(table_name, stats_list)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn save_column_insight_snapshot(
        insight: &ColumnInsightFull,
        conn_id: Option<&str>,
        db_name: Option<&str>,
        schema_name: Option<&str>,
        table_name: Option<&str>,
        row_count: Option<i32>,
        elapsed_ms: Option<i32>,
        insight_store: &engine::persistence::InsightStorage,
        meta_store: &engine::persistence::InsightMetaStore,
    ) -> Result<(String, String), CoreError> {
        crate::services::persistence_service::save_column_insight_snapshot(
            insight,
            conn_id,
            db_name,
            schema_name,
            table_name,
            row_count,
            elapsed_ms,
            insight_store,
            meta_store,
        )
        .await
    }

    pub async fn get_column_insight_history(
        column_name: &str,
        insight_store: &engine::persistence::InsightStorage,
    ) -> Result<Vec<engine::persistence::InsightVersionEntry>, CoreError> {
        crate::services::persistence_service::get_column_insight_history(column_name, insight_store)
            .await
    }

    pub async fn cleanup_old_insight_snapshots(
        days: i32,
        insight_store: &engine::persistence::InsightStorage,
        meta_store: &engine::persistence::InsightMetaStore,
    ) -> Result<(i32, usize), CoreError> {
        crate::services::persistence_service::cleanup_old_insight_snapshots(
            days,
            insight_store,
            meta_store,
        )
        .await
    }

    pub async fn get_table_profile(
        conn_id: String,
        db_type: String,
        database: &str,
        schema: &str,
        table: &str,
    ) -> Result<TableProfile, CoreError> {
        insight::table_profile_service::get_table_profile(conn_id, db_type, database, schema, table)
            .await
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

    pub async fn get_insight_storage_stats(
        insight_store: &engine::persistence::InsightStorage,
    ) -> Result<engine::persistence::InsightStorageStats, CoreError> {
        crate::services::persistence_service::get_insight_storage_stats(insight_store).await
    }

    pub async fn get_insight_version_detail(
        version_id: &str,
        insight_store: &engine::persistence::InsightStorage,
    ) -> Result<Option<ColumnInsightFull>, CoreError> {
        crate::services::persistence_service::get_insight_version_detail(version_id, insight_store)
            .await
    }

    pub async fn profile_column_from_table(
        conn_id: String,
        database: &str,
        schema: &str,
        table: &str,
        column_name: &str,
    ) -> Result<ColumnInsightFull, CoreError> {
        crate::services::persistence_service::profile_column_from_table(
            conn_id,
            database,
            schema,
            table,
            column_name,
        )
        .await
    }

    pub async fn batch_evaluate_columns(
        conn_id: String,
        database: &str,
        schema: &str,
        table: &str,
    ) -> Result<TableQuality, CoreError> {
        crate::services::persistence_service::batch_evaluate_columns(
            conn_id, database, schema, table,
        )
        .await
    }
}
