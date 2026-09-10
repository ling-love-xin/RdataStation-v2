//! 结果集服务 + 洞察计算引擎
//!
//! 提供：
//! - SQL 过滤（拼接 WHERE 重新查询）
//! - DuckDB 深度分析（针对临时表）
//! - 列洞察全量统计（统计 + 样本 + 直方图）
//! - 规则引擎 API（统一入口，SQL 模板与 Rust 代码分离）

use crate::core::error::CoreError;
use specta::Type;

// ==================== 结果集响应 ====================

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone, Type)]
pub struct ResultSet {
    pub columns: Vec<String>,
    #[specta(skip)]
    pub rows: Vec<Vec<serde_json::Value>>,
    pub row_count: u32,
    pub elapsed_ms: u32,
    pub temp_table: String,
}

// ==================== 洞察体系 — 顶层结构 ====================

#[derive(Debug, serde::Serialize, serde::Deserialize, Type)]
pub struct ColumnInsightFull {
    pub stats: ColumnStats,
    #[specta(skip)]
    pub sample: Vec<serde_json::Value>,
    pub histogram: Option<Vec<DistributionBin>>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Type)]
pub struct ColumnStats {
    pub column_name: String,
    pub data_type: String,
    pub total_count: u32,
    pub null_count: u32,
    pub null_rate: f64,
    pub unique_count: Option<u32>,
    pub stats_detail: ColumnStatsDetail,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Type)]
#[serde(tag = "kind")]
pub enum ColumnStatsDetail {
    Numeric(NumericStats),
    Text(TextStats),
    DateTime(DateTimeStats),
    Boolean(BooleanStats),
    Unknown,
}

// ==================== 数值列统计 ====================

#[derive(Debug, serde::Serialize, serde::Deserialize, Type)]
pub struct NumericStats {
    pub min: f64,
    pub max: f64,
    pub avg: f64,
    pub median: f64,
    pub p25: f64,
    pub p75: f64,
    pub sum: f64,
    pub stddev: Option<f64>,
    pub skewness: Option<f64>,
    pub kurtosis: Option<f64>,
    pub is_extreme: Vec<ExtremeValue>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Type)]
pub struct ExtremeValue {
    pub value: f64,
    pub kind: String,
}

// ==================== 文本列统计 ====================

#[derive(Debug, serde::Serialize, serde::Deserialize, Type)]
pub struct TextStats {
    pub min_length: u32,
    pub max_length: u32,
    pub top_values: Vec<TextFrequency>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Type)]
pub struct TextFrequency {
    pub value: String,
    pub count: u32,
    pub ratio: f64,
}

// ==================== 日期时间列统计 ====================

#[derive(Debug, serde::Serialize, serde::Deserialize, Type)]
pub struct DateTimeStats {
    pub earliest: String,
    pub latest: String,
    pub span_days: i32,
    pub monthly_distribution: Vec<TextFrequency>,
}

// ==================== 布尔列统计 ====================

#[derive(Debug, serde::Serialize, serde::Deserialize, Type)]
pub struct BooleanStats {
    pub true_count: u32,
    pub false_count: u32,
    pub true_ratio: f64,
}

// ==================== 分箱直方图 ====================

#[derive(Debug, serde::Serialize, serde::Deserialize, Type)]
pub struct DistributionBin {
    pub label: String,
    pub count: u32,
    pub ratio: f64,
}

// ==================== 表探查 ====================

#[derive(Debug, serde::Serialize, serde::Deserialize, Type)]
pub struct TableProfile {
    pub table_name: String,
    pub db_type: String,
    pub columns: Vec<TableColumnMeta>,
    pub row_count: Option<i32>,
    pub schema_name: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Type)]
pub struct TableColumnMeta {
    pub column_name: String,
    pub data_type: String,
    pub is_nullable: bool,
    pub is_primary_key: bool,
    pub ordinal_position: i32,
}

// ==================== 质量评分 ====================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Type)]
pub struct QualityScore {
    pub column_name: String,
    pub overall_score: f64,
    pub level: String,
    pub dimensions: Vec<QualityDimension>,
    pub summary: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Type)]
pub struct QualityDimension {
    pub name: String,
    pub score: f64,
    pub weight: f64,
    pub detail: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Type)]
pub struct TableQuality {
    pub table_name: String,
    pub overall_score: f64,
    pub level: String,
    pub column_scores: Vec<ColumnQualityEntry>,
    pub summary: String,
    pub scored_count: u32,
    pub total_columns: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Type)]
pub struct ColumnQualityEntry {
    pub column_name: String,
    pub quality_score: f64,
    pub level: String,
    pub null_rate: f64,
}

// ==================== ResultService（外观 / Facade）====================

pub struct ResultService;

impl ResultService {
    pub async fn re_execute_with_filter(
        conn_id: String,
        original_sql: &str,
        where_clause: &str,
        order_clause: &str,
    ) -> Result<ResultSet, CoreError> {
        crate::core::services::execution_service::re_execute_with_filter(
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
        crate::core::services::execution_service::execute_duckdb_analysis(
            temp_table, sql, columns, rows,
        )
    }

    pub fn get_or_create_duckdb(
    ) -> Result<std::sync::Arc<std::sync::Mutex<duckdb::Connection>>, CoreError> {
        crate::core::services::duckdb_service::DuckDbService::get_or_create_duckdb()
    }

    pub fn create_duckdb_temp_table(
        columns: &[String],
        rows: &[Vec<serde_json::Value>],
    ) -> Result<String, CoreError> {
        crate::core::services::duckdb_service::DuckDbService::create_duckdb_temp_table(
            columns, rows,
        )
    }

    pub fn get_column_insight_full(
        temp_table: &str,
        column_name: &str,
    ) -> Result<ColumnInsightFull, CoreError> {
        crate::core::services::insight_engine::get_column_insight_full(temp_table, column_name)
    }

    pub fn get_column_insights(
        temp_table: &str,
        column_name: &str,
    ) -> Result<ColumnStats, CoreError> {
        crate::core::services::insight_engine::get_column_insights(temp_table, column_name)
    }

    pub fn execute_insight_rule(
        rule_id: &str,
        conn: &duckdb::Connection,
        params: &std::collections::HashMap<String, String>,
    ) -> Result<crate::core::insight::ExecutionResult, CoreError> {
        crate::core::services::insight_engine::execute_insight_rule(rule_id, conn, params)
    }

    pub fn list_insight_rules(category: Option<&str>) -> Result<Vec<serde_json::Value>, CoreError> {
        crate::core::services::insight_engine::list_insight_rules(category)
    }

    pub fn list_rules_for_column(column_type: &str) -> Result<Vec<serde_json::Value>, CoreError> {
        crate::core::services::insight_engine::list_rules_for_column(column_type)
    }

    pub fn compute_column_quality(stats: &ColumnInsightFull) -> QualityScore {
        crate::core::services::quality_scorer::compute_column_quality(stats)
    }

    pub fn compute_table_quality(
        table_name: &str,
        stats_list: &[ColumnInsightFull],
    ) -> TableQuality {
        crate::core::services::quality_scorer::compute_table_quality(table_name, stats_list)
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
        insight_store: &crate::core::persistence::InsightStorage,
        meta_store: &crate::core::persistence::InsightMetaStore,
    ) -> Result<(String, String), CoreError> {
        crate::core::services::persistence_service::save_column_insight_snapshot(
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
        insight_store: &crate::core::persistence::InsightStorage,
    ) -> Result<Vec<crate::core::persistence::InsightVersionEntry>, CoreError> {
        crate::core::services::persistence_service::get_column_insight_history(
            column_name,
            insight_store,
        )
        .await
    }

    pub async fn cleanup_old_insight_snapshots(
        days: i32,
        insight_store: &crate::core::persistence::InsightStorage,
        meta_store: &crate::core::persistence::InsightMetaStore,
    ) -> Result<(i32, usize), CoreError> {
        crate::core::services::persistence_service::cleanup_old_insight_snapshots(
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
        crate::core::services::table_profile_service::get_table_profile(
            conn_id, db_type, database, schema, table,
        )
        .await
    }

    pub async fn save_cell_update(
        conn_id: String,
        table_name: &str,
        column_name: &str,
        new_value: &serde_json::Value,
        row_identity: &std::collections::HashMap<String, serde_json::Value>,
    ) -> Result<(usize, String), CoreError> {
        use crate::core::services::connection_manager;
        use crate::core::services::sql_service::{value_to_sql, SqlExecuteOptions, SqlService};

        let set_clause = format!("`{}` = {}", column_name, value_to_sql(new_value));

        let where_parts: Vec<String> = row_identity
            .iter()
            .filter(|(k, _)| *k != column_name)
            .map(|(col, val)| format!("`{}` = {}", col, value_to_sql(val)))
            .collect();

        if where_parts.is_empty() {
            return Err(CoreError::common(crate::core::error::CommonError::General(
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
        use crate::core::services::duckdb_service::{DuckDbService, ExportFormat};

        let fmt = match format {
            "csv" => ExportFormat::Csv,
            "parquet" => ExportFormat::Parquet,
            "xlsx" => ExportFormat::Xlsx,
            other => {
                return Err(CoreError::common(crate::core::error::CommonError::General(
                    format!("不支持的导出格式: {}", other),
                )))
            }
        };
        DuckDbService::export_temp_table(temp_table, file_path, fmt)
    }

    pub async fn get_insight_storage_stats(
        insight_store: &crate::core::persistence::InsightStorage,
    ) -> Result<crate::core::persistence::InsightStorageStats, CoreError> {
        crate::core::services::persistence_service::get_insight_storage_stats(insight_store).await
    }

    pub async fn get_insight_version_detail(
        version_id: &str,
        insight_store: &crate::core::persistence::InsightStorage,
    ) -> Result<Option<ColumnInsightFull>, CoreError> {
        crate::core::services::persistence_service::get_insight_version_detail(
            version_id,
            insight_store,
        )
        .await
    }

    pub async fn profile_column_from_table(
        conn_id: String,
        database: &str,
        schema: &str,
        table: &str,
        column_name: &str,
    ) -> Result<ColumnInsightFull, CoreError> {
        crate::core::services::persistence_service::profile_column_from_table(
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
        crate::core::services::persistence_service::batch_evaluate_columns(
            conn_id, database, schema, table,
        )
        .await
    }
}
