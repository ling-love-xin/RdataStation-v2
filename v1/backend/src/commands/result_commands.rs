//! 结果集分析命令
//!
//! 提供结果的二次分析：SQL 过滤、DuckDB 分析、列洞察、持久化存储

use crate::commands::project_commands::ProjectState;
use crate::core::error::{CommonError, CoreError};
use crate::core::insight;
use crate::core::insight::schema_analyzer::{SchemaAnalyzer, SchemaInsightReport};
use crate::core::persistence::{InsightMetaStore, InsightStorage};
use crate::core::services::result_service::{
    ColumnInsightFull, ColumnStats, QualityScore, ResultService, ResultSet, TableProfile,
    TableQuality,
};

/// 重新执行带过滤条件的 SQL
#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct ReExecuteFilterInput {
    pub conn_id: String,
    pub original_sql: String,
    pub where_clause: Option<String>,
    pub order_clause: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub async fn re_execute_with_filter(input: ReExecuteFilterInput) -> Result<ResultSet, CoreError> {
    let where_clause = input.where_clause.unwrap_or_default();
    let order_clause = input.order_clause.unwrap_or_default();

    ResultService::re_execute_with_filter(
        input.conn_id,
        &input.original_sql,
        &where_clause,
        &order_clause,
    )
    .await
}

// ═══════════════ 单元格编辑持久化 ═══════════════

#[derive(serde::Serialize, serde::Deserialize, Debug, specta::Type)]
pub struct CellUpdateResult {
    pub success: bool,
    pub affected_rows: u32,
    pub message: String,
}

#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct CellUpdateInput {
    pub conn_id: String,
    pub table_name: String,
    pub column_name: String,
    #[specta(skip)]
    pub new_value: serde_json::Value,
    #[specta(skip)]
    pub row_identity: std::collections::HashMap<String, serde_json::Value>,
}

#[tauri::command]
#[specta::specta]
pub async fn save_cell_update(input: CellUpdateInput) -> Result<CellUpdateResult, CoreError> {
    match ResultService::save_cell_update(
        input.conn_id,
        &input.table_name,
        &input.column_name,
        &input.new_value,
        &input.row_identity,
    )
    .await
    {
        Ok((affected, message)) => Ok(CellUpdateResult {
            success: true,
            affected_rows: affected as u32,
            message,
        }),
        Err(e) => Err(format!("更新失败: {}", e).into()),
    }
}

/// Schema 洞察输入
#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct SchemaInsightInput {
    pub conn_id: String,
    pub database: String,
    pub schema: String,
}

/// Schema 级洞察分析（外键推断、类型一致性、孤立表检测）
#[tauri::command]
#[specta::specta]
pub async fn get_schema_insight(
    input: SchemaInsightInput,
) -> Result<SchemaInsightReport, CoreError> {
    SchemaAnalyzer::analyze(input.conn_id, &input.database, &input.schema).await
}

/// 列质量评分输入
#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct ColumnQualityInput {
    pub column_name: String,
    pub temp_table: String,
}

/// 计算列数据质量评分 (0-100)
#[tauri::command]
#[specta::specta]
pub async fn get_column_quality(input: ColumnQualityInput) -> Result<QualityScore, CoreError> {
    let stats = ResultService::get_column_insight_full(&input.temp_table, &input.column_name)?;
    Ok(ResultService::compute_column_quality(&stats))
}

/// 批量评估表质量 — 一次调用完成全表所有列的质量评分
#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct BatchEvaluateInput {
    pub conn_id: String,
    pub database: String,
    pub schema: String,
    pub table: String,
}

#[tauri::command]
#[specta::specta]
pub async fn batch_evaluate_columns(input: BatchEvaluateInput) -> Result<TableQuality, CoreError> {
    ResultService::batch_evaluate_columns(
        input.conn_id,
        &input.database,
        &input.schema,
        &input.table,
    )
    .await
}

// ═══════════════ 从表直接探查列命令 ═══════════════

#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct ProfileColumnFromTableInput {
    pub conn_id: String,
    pub database: String,
    pub schema: String,
    pub table: String,
    pub column_name: String,
}

#[tauri::command]
#[specta::specta]
pub async fn profile_column_from_table(
    input: ProfileColumnFromTableInput,
) -> Result<ColumnInsightFull, CoreError> {
    ResultService::profile_column_from_table(
        input.conn_id,
        &input.database,
        &input.schema,
        &input.table,
        &input.column_name,
    )
    .await
}

// ═══════════════ 版本详情命令 ═══════════════

#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct InsightVersionDetailInput {
    pub version_id: String,
}

#[tauri::command]
pub async fn get_insight_version_detail(
    input: InsightVersionDetailInput,
    state: tauri::State<'_, ProjectState>,
) -> Result<Option<crate::core::services::result_service::ColumnInsightFull>, CoreError> {
    let store_guard = state.store.lock().await;
    let store = store_guard.as_ref().ok_or("No project store available")?;
    let db = &store.db_manager;

    let insight_store = InsightStorage::new(db.duckdb_conn());

    ResultService::get_insight_version_detail(&input.version_id, &insight_store).await
}

/// DuckDB 分析输入
#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct DuckDbAnalysisInput {
    pub temp_table: String,
    pub sql: String,
    pub columns: Option<Vec<String>>,
    #[specta(skip)]
    pub rows: Option<Vec<Vec<serde_json::Value>>>,
}

#[tauri::command]
#[specta::specta]
pub async fn execute_duckdb_analysis(input: DuckDbAnalysisInput) -> Result<ResultSet, CoreError> {
    ResultService::execute_duckdb_analysis(&input.temp_table, &input.sql, input.columns, input.rows)
}

/// 列洞察输入
#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct ColumnInsightInput {
    pub temp_table: String,
    pub column_name: String,
}

/// 获取列统计信息（旧版兼容，返回 ColumnStats）
#[tauri::command]
#[specta::specta]
pub async fn get_column_insights(input: ColumnInsightInput) -> Result<ColumnStats, CoreError> {
    ResultService::get_column_insights(&input.temp_table, &input.column_name)
}

/// 获取列全量洞察（统计 + 样本 + 直方图）
#[tauri::command]
#[specta::specta]
pub async fn get_column_insight_full(
    input: ColumnInsightInput,
) -> Result<ColumnInsightFull, CoreError> {
    ResultService::get_column_insight_full(&input.temp_table, &input.column_name)
}

/// 创建 DuckDB 临时表输入
#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct CreateTempTableInput {
    pub columns: Vec<String>,
    #[specta(skip)]
    pub rows: Vec<Vec<serde_json::Value>>,
}

/// 从已有数据创建 DuckDB 临时表，返回表名
#[tauri::command]
#[specta::specta]
pub async fn create_duckdb_temp_table(input: CreateTempTableInput) -> Result<String, CoreError> {
    ResultService::create_duckdb_temp_table(&input.columns, &input.rows)
}

// ═══════════════════ 持久化命令 ═══════════════════

/// 保存列洞察快照输入
#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct SaveInsightSnapshotInput {
    pub temp_table: String,
    pub column_name: String,
    pub conn_id: Option<String>,
    pub db_name: Option<String>,
    pub schema_name: Option<String>,
    pub table_name: Option<String>,
}

/// 保存列洞察快照到项目持久化存储
#[tauri::command]
pub async fn save_column_insight_snapshot(
    input: SaveInsightSnapshotInput,
    state: tauri::State<'_, ProjectState>,
) -> Result<String, CoreError> {
    let store_guard = state.store.lock().await;
    let store = store_guard.as_ref().ok_or("No project store available")?;
    let db = &store.db_manager;

    let insight_store = InsightStorage::new(db.duckdb_conn());
    let meta_store = InsightMetaStore::new(db.sqlite_pool());

    let insight = ResultService::get_column_insight_full(&input.temp_table, &input.column_name)?;

    let row_count = insight.stats.total_count as i32;
    let start = std::time::Instant::now();
    let elapsed = start.elapsed().as_millis() as i32;

    let (_snapshot_id, version_id) = ResultService::save_column_insight_snapshot(
        &insight,
        input.conn_id.as_deref(),
        input.db_name.as_deref(),
        input.schema_name.as_deref(),
        input.table_name.as_deref(),
        Some(row_count),
        Some(elapsed),
        &insight_store,
        &meta_store,
    )
    .await?;

    Ok(version_id)
}

/// 获取列洞察历史版本输入
#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct InsightHistoryInput {
    pub column_name: String,
}

/// 获取列洞察所有历史版本
#[tauri::command]
pub async fn get_column_insight_history(
    input: InsightHistoryInput,
    state: tauri::State<'_, ProjectState>,
) -> Result<Vec<crate::core::persistence::InsightVersionEntry>, CoreError> {
    let store_guard = state.store.lock().await;
    let store = store_guard.as_ref().ok_or("No project store available")?;
    let db = &store.db_manager;

    let insight_store = InsightStorage::new(db.duckdb_conn());

    ResultService::get_column_insight_history(&input.column_name, &insight_store).await
}

/// 清理洞察快照输入
#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct CleanupInsightInput {
    pub days: i32,
}

/// 清理 N 天前的洞察快照（DuckDB + SQLite 双写清理）
#[tauri::command]
pub async fn cleanup_insight_snapshots(
    input: CleanupInsightInput,
    state: tauri::State<'_, ProjectState>,
) -> Result<CleanupResult, CoreError> {
    let store_guard = state.store.lock().await;
    let store = store_guard.as_ref().ok_or("No project store available")?;
    let db = &store.db_manager;

    let insight_store = InsightStorage::new(db.duckdb_conn());
    let meta_store = InsightMetaStore::new(db.sqlite_pool());

    let (duckdb_deleted, sqlite_deleted) =
        ResultService::cleanup_old_insight_snapshots(input.days, &insight_store, &meta_store)
            .await?;

    Ok(CleanupResult {
        duckdb_deleted,
        sqlite_deleted: sqlite_deleted as i32,
    })
}

#[derive(serde::Serialize, specta::Type)]
pub struct CleanupResult {
    pub duckdb_deleted: i32,
    pub sqlite_deleted: i32,
}

/// 获取洞察存储用量统计
#[tauri::command]
pub async fn get_insight_storage_stats(
    state: tauri::State<'_, ProjectState>,
) -> Result<crate::core::persistence::InsightStorageStats, CoreError> {
    let store_guard = state.store.lock().await;
    let store = store_guard.as_ref().ok_or("No project store available")?;
    let db = &store.db_manager;

    let insight_store = InsightStorage::new(db.duckdb_conn());

    ResultService::get_insight_storage_stats(&insight_store).await
}

// ═══════════════ 规则引擎公开命令 ═══════════════

#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct ExecuteRuleInput {
    pub rule_id: String,
    pub params: std::collections::HashMap<String, String>,
    pub temp_table: String,
}

#[tauri::command]
#[specta::specta]
pub async fn execute_insight_rule(input: ExecuteRuleInput) -> Result<serde_json::Value, CoreError> {
    let duckdb = ResultService::get_or_create_duckdb()?;
    let conn = duckdb
        .lock()
        .map_err(|e| CoreError::common(CommonError::General(e.to_string())))?;

    let exec_result = ResultService::execute_insight_rule(&input.rule_id, &conn, &input.params)?;
    serde_json::to_value(exec_result)
        .map_err(|e| CoreError::common(CommonError::General(e.to_string())))
}

#[tauri::command]
#[specta::specta]
pub async fn list_insight_rules(
    category: Option<String>,
) -> Result<Vec<serde_json::Value>, CoreError> {
    ResultService::list_insight_rules(category.as_deref())
}

#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct RulesForColumnInput {
    pub column_type: String,
}

#[tauri::command]
#[specta::specta]
pub async fn list_rules_for_column(
    input: RulesForColumnInput,
) -> Result<Vec<serde_json::Value>, CoreError> {
    ResultService::list_rules_for_column(&input.column_type)
}

/// 规则热加载输入
#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct ReloadInsightRulesInput {
    pub project_path: String,
}

/// 热加载洞察规则（重新扫描 embedded + user 目录）
#[tauri::command]
#[specta::specta]
pub fn reload_insight_rules(input: ReloadInsightRulesInput) -> Result<u32, CoreError> {
    let path = std::path::PathBuf::from(&input.project_path);
    insight::reload_insight_rules(&path);
    let reg = insight::global_registry()
        .read()
        .map_err(|e| CoreError::common(CommonError::General(e.to_string())))?;
    Ok(reg.rule_count() as u32)
}

// ═══════════════ 表探查命令 ═══════════════

#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct TableProfileInput {
    pub conn_id: String,
    pub db_type: String,
    pub database: String,
    pub schema: String,
    pub table: String,
}

#[tauri::command]
#[specta::specta]
pub async fn get_table_profile(input: TableProfileInput) -> Result<TableProfile, CoreError> {
    ResultService::get_table_profile(
        input.conn_id,
        input.db_type,
        &input.database,
        &input.schema,
        &input.table,
    )
    .await
}

// ═══════════════ 数据导出命令 ═══════════════

#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct ExportInput {
    pub temp_table: String,
    pub file_path: String,
    pub format: String,
}

#[tauri::command]
#[specta::specta]
pub fn export_result_to_file(input: ExportInput) -> Result<String, CoreError> {
    ResultService::export_result(&input.temp_table, &input.file_path, &input.format)
}
