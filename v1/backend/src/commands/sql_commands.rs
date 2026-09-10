//! SQL 执行相关命令
//!
//! 处理 SQL 查询、事务执行、历史记录等操作

use crate::adapters::tauri::state::AppState;
use crate::api::dto::QueryResult;
use crate::core::error::CoreError;
use crate::core::get_connection_manager;
use crate::core::persistence::history_store::SqlHistoryRecord;
use crate::core::services::duckdb_service::DuckDbService;
use crate::core::services::{
    sql_service::{SqlExecuteOptions, SqlExecuteResult, TransactionStatusResult},
    SqlService,
};

fn new_sql_service() -> SqlService {
    SqlService::new(get_connection_manager().clone())
}

// ==================== SQL Query Commands ====================

/// 执行 SQL 请求参数
#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct ExecuteSqlInput {
    pub conn_id: Option<String>,
    pub sql: String,
    pub timeout_ms: Option<u32>,
}

/// 执行 SQL 响应
#[derive(serde::Serialize, Debug, specta::Type)]
pub struct ExecuteSqlResponse {
    pub result: QueryResult,
    pub elapsed_ms: u32,
    pub affected_rows: Option<u32>,
    pub truncated: bool,
}

impl From<SqlExecuteResult> for ExecuteSqlResponse {
    fn from(result: SqlExecuteResult) -> Self {
        let affected_rows = result.result.affected_rows;
        Self {
            result: result.result,
            elapsed_ms: result.elapsed_ms as u32,
            affected_rows,
            truncated: result.truncated,
        }
    }
}

/// 执行 SQL 查询
#[tauri::command]
#[specta::specta]
pub async fn execute_sql(input: ExecuteSqlInput) -> Result<ExecuteSqlResponse, CoreError> {
    if input.sql.trim().is_empty() {
        return Err(CoreError::common(
            crate::core::error::CommonError::InvalidArgument {
                param: "sql".to_string(),
                reason: "SQL statement cannot be empty".to_string(),
            },
        ));
    }

    let manager = get_connection_manager().clone();
    let service = SqlService::new(manager);

    let options = SqlExecuteOptions {
        record_history: true,
        use_transaction: false,
        timeout_ms: input.timeout_ms.map(|v| v as u64),
        use_cache: true,
    };

    let result = service.execute(input.conn_id, &input.sql, options).await?;

    Ok(result.into())
}

/// 在事务中执行多个 SQL
#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct ExecuteTransactionInput {
    pub conn_id: Option<String>,
    pub sqls: Vec<String>,
}

#[derive(serde::Serialize, Debug, specta::Type)]
pub struct ExecuteTransactionResponse {
    pub results: Vec<ExecuteSqlResponse>,
}

#[tauri::command]
#[specta::specta]
pub async fn execute_transaction(
    input: ExecuteTransactionInput,
) -> Result<ExecuteTransactionResponse, CoreError> {
    if input.sqls.is_empty() {
        return Err(CoreError::common(
            crate::core::error::CommonError::InvalidArgument {
                param: "sqls".to_string(),
                reason: "SQL statements list cannot be empty".to_string(),
            },
        ));
    }

    let service = new_sql_service();

    let results = service
        .execute_in_transaction(input.conn_id, input.sqls)
        .await?;

    Ok(ExecuteTransactionResponse {
        results: results.into_iter().map(|r| r.into()).collect(),
    })
}

// ==================== Transaction Commands ====================

/// 事务状态响应
#[derive(serde::Serialize, Debug, specta::Type)]
pub struct TransactionStatusResponse {
    pub conn_id: String,
    pub is_in_transaction: bool,
    pub transaction_start_time_ms: Option<u32>,
    pub transaction_duration_ms: Option<u32>,
}

impl From<TransactionStatusResult> for TransactionStatusResponse {
    fn from(result: TransactionStatusResult) -> Self {
        Self {
            conn_id: result.conn_id,
            is_in_transaction: result.is_in_transaction,
            transaction_start_time_ms: result.transaction_start_time_ms.map(|v| v as u32),
            transaction_duration_ms: result.transaction_duration_ms.map(|v| v as u32),
        }
    }
}

/// 开始事务
#[tauri::command]
#[specta::specta]
pub async fn begin_transaction(
    conn_id: Option<String>,
) -> Result<TransactionStatusResponse, CoreError> {
    let service = new_sql_service();
    service.begin_transaction(conn_id).await.map(Into::into)
}

/// 提交事务
#[tauri::command]
#[specta::specta]
pub async fn commit_transaction(
    conn_id: Option<String>,
) -> Result<TransactionStatusResponse, CoreError> {
    let service = new_sql_service();
    service.commit_transaction(conn_id).await.map(Into::into)
}

/// 回滚事务
#[tauri::command]
#[specta::specta]
pub async fn rollback_transaction(
    conn_id: Option<String>,
) -> Result<TransactionStatusResponse, CoreError> {
    let service = new_sql_service();
    service.rollback_transaction(conn_id).await.map(Into::into)
}

/// 取消正在执行的 SQL 查询
#[tauri::command]
#[specta::specta]
pub async fn cancel_sql_query(conn_id: Option<String>) -> Result<bool, CoreError> {
    new_sql_service().cancel_query(conn_id).await
}

/// 连接健康检查（ping）
#[tauri::command]
#[specta::specta]
pub async fn ping_connection(conn_id: Option<String>) -> Result<bool, CoreError> {
    let manager = get_connection_manager().clone();
    let conn_id = match conn_id {
        Some(id) => id,
        None => manager.get_active_conn_id().await.ok_or_else(|| {
            CoreError::connection(crate::core::error::ConnectionError::NoActiveConnection)
        })?,
    };

    let db = manager.get_connection(&conn_id).await.ok_or_else(|| {
        CoreError::connection(crate::core::error::ConnectionError::NotFound(
            conn_id.clone(),
        ))
    })?;

    db.ping().await.map(|()| true)
}

/// 获取事务状态
#[tauri::command]
#[specta::specta]
pub async fn get_transaction_status(
    conn_id: Option<String>,
) -> Result<TransactionStatusResponse, CoreError> {
    let service = new_sql_service();
    service
        .get_transaction_status(conn_id)
        .await
        .map(Into::into)
}

/// SQL 历史记录响应（含审计信息）
#[derive(serde::Serialize, Debug, specta::Type)]
pub struct SqlHistoryResponse {
    pub id: String,
    pub sql: String,
    pub conn_id: Option<String>,
    pub db_type: Option<String>,
    pub executed_at: String,
    pub duration_ms: Option<u32>,
    pub success: Option<bool>,
    pub error_message: Option<String>,
    pub rows_affected: Option<u32>,
    pub rows_returned: Option<u32>,
}

impl From<SqlHistoryRecord> for SqlHistoryResponse {
    fn from(h: SqlHistoryRecord) -> Self {
        Self {
            id: h.id,
            sql: h.sql,
            conn_id: h.conn_id,
            db_type: h.db_type,
            executed_at: h.executed_at.to_rfc3339(),
            duration_ms: h.duration_ms.map(|v| v as u32),
            success: h.success,
            error_message: h.error_message,
            rows_affected: h.rows_affected.map(|v| v as u32),
            rows_returned: h.rows_returned.map(|v| v as u32),
        }
    }
}

/// 获取 SQL 执行历史（含审计日志）
#[tauri::command]
#[specta::specta]
pub async fn get_sql_history(limit: Option<u32>) -> Result<Vec<SqlHistoryResponse>, CoreError> {
    let service = new_sql_service();
    let history = service.get_sql_history(limit.unwrap_or(100) as usize)?;
    Ok(history.into_iter().map(Into::into).collect())
}

/// 搜索 SQL 历史
#[tauri::command]
#[specta::specta]
pub async fn search_sql_history(
    keyword: String,
    limit: Option<u32>,
) -> Result<Vec<SqlHistoryResponse>, CoreError> {
    let service = new_sql_service();
    let history = service.search_sql_history(&keyword, limit.unwrap_or(100) as usize)?;
    Ok(history.into_iter().map(Into::into).collect())
}

/// 清空 SQL 历史
#[tauri::command]
#[specta::specta]
pub async fn clear_sql_history() -> Result<(), CoreError> {
    new_sql_service().clear_sql_history()
}

/// 删除单条 SQL 历史
#[tauri::command]
#[specta::specta]
pub async fn remove_sql_history(id: String) -> Result<(), CoreError> {
    new_sql_service().remove_sql_history(&id)
}

// ==================== Federated Query Commands ====================

/// 注册外部数据库连接请求参数
#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct RegisterExternalDatabaseInput {
    pub conn_id: Option<String>,
    pub name: String,
    pub driver: String,
    pub connection_string: String,
}

/// 注册外部数据库连接
#[tauri::command]
#[specta::specta]
pub async fn register_external_database(
    input: RegisterExternalDatabaseInput,
) -> Result<(), CoreError> {
    new_sql_service()
        .register_external_database(
            input.conn_id,
            &input.name,
            &input.driver,
            &input.connection_string,
        )
        .await
}

/// DuckDB 加速执行输入
#[derive(Debug, serde::Deserialize, specta::Type)]
pub struct DuckDBAcceleratedInput {
    /// SQL 查询
    pub sql: String,
    /// 源数据库连接 ID
    pub conn_id: String,
    /// 数据库类型
    pub db_type: Option<String>,
    /// APP 数据目录（扩展存储）
    pub data_dir: Option<String>,
}

/// DuckDB 加速执行响应
#[derive(Debug, serde::Serialize, specta::Type)]
pub struct DuckDBAcceleratedResponse {
    pub success: bool,
    pub columns: Option<Vec<String>>,
    pub rows: Option<Vec<Vec<serde_json::Value>>>,
    pub elapsed_ms: u32,
    pub error: Option<String>,
}

/// 使用 DuckDB 加速引擎执行 SQL（含自动 ATTACH / DETACH）
#[tauri::command]
#[specta::specta]
pub async fn execute_duckdb_accelerated(
    state: tauri::State<'_, AppState>,
    input: DuckDBAcceleratedInput,
) -> Result<DuckDBAcceleratedResponse, CoreError> {
    use crate::core::error::CommonError;

    let manager = get_connection_manager().clone();
    let conn_info = manager
        .get_connection_info(&input.conn_id)
        .await
        .ok_or_else(|| {
            CoreError::common(CommonError::General(format!(
                "Connection not found: {}",
                input.conn_id
            )))
        })?;

    let engine = state.duckdb_engine.lock().await;
    let result = DuckDbService::accelerate_query(
        &conn_info.db_type,
        &conn_info.url,
        &conn_info.name,
        &input.sql,
        &engine,
        input.data_dir.as_deref(),
    )
    .await?;
    drop(engine);

    let columns: Vec<String> = result
        .batches
        .first()
        .map(|b| {
            b.schema()
                .fields()
                .iter()
                .map(|f| f.name().clone())
                .collect()
        })
        .unwrap_or_default();

    let rows: Vec<Vec<serde_json::Value>> = result
        .batches
        .iter()
        .flat_map(|batch| {
            let mut batch_rows = Vec::new();
            for row_idx in 0..batch.num_rows() {
                let row: Vec<serde_json::Value> = (0..batch.num_columns())
                    .map(|col_idx| {
                        let col = batch.column(col_idx);
                        format_arrow_value(col, row_idx)
                    })
                    .collect();
                batch_rows.push(row);
            }
            batch_rows
        })
        .collect();

    Ok(DuckDBAcceleratedResponse {
        success: true,
        columns: if columns.is_empty() {
            None
        } else {
            Some(columns)
        },
        rows: if rows.is_empty() { None } else { Some(rows) },
        elapsed_ms: 0u32,
        error: None,
    })
}

fn format_arrow_value(col: &dyn arrow::array::Array, row_idx: usize) -> serde_json::Value {
    use arrow::array::*;
    use serde_json::Value;

    if col.is_null(row_idx) {
        return Value::Null;
    }

    macro_rules! as_array {
        ($col:expr, $ty:ty) => {
            $col.as_any()
                .downcast_ref::<$ty>()
                .map(|a| a.value(row_idx))
        };
    }

    if let Some(v) = as_array!(col, Int8Array) {
        return Value::Number(serde_json::Number::from(v));
    }
    if let Some(v) = as_array!(col, Int16Array) {
        return Value::Number(serde_json::Number::from(v));
    }
    if let Some(v) = as_array!(col, Int32Array) {
        return Value::Number(serde_json::Number::from(v));
    }
    if let Some(v) = as_array!(col, Int64Array) {
        return Value::Number(serde_json::Number::from(v));
    }
    if let Some(v) = as_array!(col, UInt8Array) {
        return Value::Number(serde_json::Number::from(v));
    }
    if let Some(v) = as_array!(col, UInt16Array) {
        return Value::Number(serde_json::Number::from(v));
    }
    if let Some(v) = as_array!(col, UInt32Array) {
        return Value::Number(serde_json::Number::from(v));
    }
    if let Some(v) = as_array!(col, UInt64Array) {
        return Value::Number(serde_json::Number::from(v));
    }
    if let Some(v) = as_array!(col, Float32Array) {
        return serde_json::Number::from_f64(v as f64).map_or(Value::Null, Value::Number);
    }
    if let Some(v) = as_array!(col, Float64Array) {
        return serde_json::Number::from_f64(v).map_or(Value::Null, Value::Number);
    }

    if let Some(arr) = col.as_any().downcast_ref::<StringArray>() {
        return Value::String(arr.value(row_idx).to_string());
    }

    Value::String(
        arrow::util::display::array_value_to_string(col, row_idx)
            .unwrap_or_else(|_| "?".to_string())
            .to_string(),
    )
}

// ==================== Paginated Query Commands ====================

/// 分页执行 SQL 请求参数
#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct ExecuteSqlPaginatedInput {
    pub conn_id: Option<String>,
    pub sql: String,
    pub offset: u32,
    pub limit: u32,
    pub timeout_ms: Option<u32>,
}

/// 分页执行 SQL 查询
///
/// 与 `execute_sql` 的区别：
/// - 内部仍然执行完整 SQL 查询
/// - 序列化时只返回 [offset, offset+limit) 范围的行
/// - `total_rows` 始终返回原始总行数（非分页后行数）
#[tauri::command]
#[specta::specta]
pub async fn execute_sql_paginated(
    input: ExecuteSqlPaginatedInput,
) -> Result<ExecuteSqlResponse, CoreError> {
    if input.sql.trim().is_empty() {
        return Err(CoreError::common(
            crate::core::error::CommonError::InvalidArgument {
                param: "sql".to_string(),
                reason: "SQL statement cannot be empty".to_string(),
            },
        ));
    }

    let manager = get_connection_manager().clone();
    let service = SqlService::new(manager);

    let options = SqlExecuteOptions {
        record_history: true,
        use_transaction: false,
        timeout_ms: input.timeout_ms.map(|v| v as u64),
        use_cache: true,
    };

    let result = service.execute(input.conn_id, &input.sql, options).await?;

    let total_rows = result.result.total_rows() as u32;
    let offset = input.offset.min(total_rows);
    let limit = input.limit.min(total_rows.saturating_sub(offset));

    let mut paginated_result = result.result;
    if offset > 0 || (limit as usize) < paginated_result.total_rows() {
        paginated_result.slice(offset as usize, limit as usize);
    }

    let response: ExecuteSqlResponse = SqlExecuteResult {
        result: paginated_result,
        elapsed_ms: result.elapsed_ms,
        truncated: result.truncated,
    }
    .into();

    Ok(response)
}

/// 创建外部表请求参数
#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct CreateExternalTableInput {
    pub conn_id: Option<String>,
    pub external_db_name: String,
    pub schema_name: String,
    pub table_name: String,
    pub external_table_name: String,
}

/// 创建外部表
#[tauri::command]
#[specta::specta]
pub async fn create_external_table(input: CreateExternalTableInput) -> Result<(), CoreError> {
    new_sql_service()
        .create_external_table(
            input.conn_id,
            &input.external_db_name,
            &input.schema_name,
            &input.table_name,
            &input.external_table_name,
        )
        .await
}
