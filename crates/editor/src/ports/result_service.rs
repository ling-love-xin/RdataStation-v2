//! 结果集服务（SQL 执行侧的外观）
//!
//! 提供：
//! - SQL 过滤（拼接 WHERE 重新查询）
//! - DuckDB 深度分析（针对临时表）
//! - 临时表创建与句柄获取
//! - 结果导出
//!
//! **单格回写不在这里**：曾有一条 `save_cell_update`（拼 UPDATE 语句逐格改），
//! T2a 批量提交落地后已无调用方，2026-11-09 删除。要恢复单格编辑请接 T2a 的
//! 批量提交链，不要再拼单条 UPDATE。
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

    /// 【B4】事实档 batches 直灌建表后跑分析（保真档；`Some(table)` = 复用已有常驻表）。
    pub fn execute_duckdb_analysis_from_batches(
        temp_table: Option<&str>,
        sql: &str,
        batches: &[shared::models::ArrowBatch],
    ) -> Result<ResultSet, CoreError> {
        engine::services::execution_service::execute_duckdb_analysis_from_batches(
            temp_table, sql, batches,
        )
    }

    /// 【B7 切片二】结果行 → Parquet / XLSX（DuckDB `COPY … TO …`）
    ///
    /// 同步阻塞（XLSX 首次装扩展要联网几秒）：调用方在**一次性线程**上跑它。
    pub fn export_rows_via_duckdb(
        columns: &[String],
        rows: &[Vec<serde_json::Value>],
        path: &std::path::Path,
        format: engine::services::execution_service::DuckDbExportFormat,
    ) -> Result<engine::services::execution_service::ExportStats, CoreError> {
        engine::services::execution_service::export_rows_via_duckdb(columns, rows, path, format)
    }

    /// 【D1】事实档 batches 直灌建表后 `COPY` 落盘（保真档；临时表即用即收）。
    pub fn export_batches_via_duckdb(
        batches: &[shared::models::ArrowBatch],
        path: &std::path::Path,
        format: engine::services::execution_service::DuckDbExportFormat,
    ) -> Result<engine::services::execution_service::ExportStats, CoreError> {
        engine::services::execution_service::export_batches_via_duckdb(batches, path, format)
    }

    /// 【D1】表档常驻表直接 `COPY` 落盘（保真档；复用常驻表，不新建、不回收）。
    pub fn export_existing_table_via_duckdb(
        table: &str,
        row_count: usize,
        path: &std::path::Path,
        format: engine::services::execution_service::DuckDbExportFormat,
    ) -> Result<engine::services::execution_service::ExportStats, CoreError> {
        engine::services::execution_service::export_existing_table_via_duckdb(
            table, row_count, path, format,
        )
    }

    pub fn get_or_create_duckdb()
    -> Result<std::sync::Arc<std::sync::Mutex<duckdb::Connection>>, CoreError> {
        engine::services::duckdb_service::DuckDbService::get_or_create_duckdb()
    }

    pub fn create_duckdb_temp_table(
        columns: &[String],
        rows: &[Vec<serde_json::Value>],
    ) -> Result<String, CoreError> {
        engine::services::duckdb_service::DuckDbService::create_duckdb_temp_table(columns, rows)
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
                )));
            }
        };
        DuckDbService::export_temp_table(temp_table, file_path, fmt)
    }
}
