use crate::core::error::{CommonError, CoreError};
use crate::core::get_connection_manager;
use crate::core::services::duckdb_service::{self, extract_rows_from_serialized};
use crate::core::services::result_service::ResultSet;
use crate::core::services::sql_service::SqlExecuteOptions;
use crate::core::services::SqlService;
use crate::core::DuckDBManager;

pub(crate) async fn re_execute_with_filter(
    conn_id: String,
    original_sql: &str,
    where_clause: &str,
    order_clause: &str,
) -> Result<ResultSet, CoreError> {
    let start = std::time::Instant::now();
    let manager = get_connection_manager().clone();
    let service = SqlService::new(manager);

    let base_sql = original_sql.trim().trim_end_matches(';');
    let mut filtered_sql = format!("SELECT * FROM ({}) AS _result", base_sql);
    if !where_clause.trim().is_empty() {
        filtered_sql.push_str(&format!(" WHERE {}", where_clause));
    }
    if !order_clause.trim().is_empty() {
        filtered_sql.push_str(&format!(" ORDER BY {}", order_clause));
    }

    let options = SqlExecuteOptions {
        record_history: false,
        use_transaction: false,
        timeout_ms: None,
        use_cache: false,
    };

    let result = service
        .execute(Some(conn_id), &filtered_sql, options)
        .await?;
    let elapsed = start.elapsed().as_millis() as u64;

    let json_value = serde_json::to_value(&result.result)
        .map_err(|e| CoreError::common(CommonError::General(format!("Serialize error: {}", e))))?;

    let columns = json_value["columns"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();

    let rows = extract_rows_from_serialized(&json_value);
    let temp_table = duckdb_service::DuckDbService::create_duckdb_temp_table(&columns, &rows)?;

    Ok(ResultSet {
        row_count: rows.len() as u32,
        columns,
        rows,
        elapsed_ms: elapsed as u32,
        temp_table,
    })
}

pub(crate) fn execute_duckdb_analysis(
    temp_table: &str,
    sql: &str,
    columns: Option<Vec<String>>,
    rows: Option<Vec<Vec<serde_json::Value>>>,
) -> Result<ResultSet, CoreError> {
    let start = std::time::Instant::now();
    let duckdb = duckdb_service::DuckDbService::get_or_create_duckdb()?;
    let mut conn = duckdb.lock().map_err(|e| {
        CoreError::common(CommonError::General(format!("DuckDB lock error: {}", e)))
    })?;

    let actual_table = if temp_table.is_empty() {
        if let (Some(cols), Some(rws)) = (columns, rows) {
            duckdb_service::DuckDbService::create_temp_table_internal(&mut conn, &cols, &rws)?
        } else {
            return Err(CoreError::common(CommonError::General(
                "No temp table or data provided".to_string(),
            )));
        }
    } else {
        temp_table.to_string()
    };

    let analysis_sql = sql
        .replace("{table}", &actual_table)
        .replace("result_temp", &actual_table);

    DuckDBManager::validate_analysis_sql(&analysis_sql)?;

    let (cols_out, rws_out) =
        duckdb_service::DuckDbService::query_duckdb(&mut conn, &analysis_sql)?;
    let elapsed = start.elapsed().as_millis() as u64;
    let row_count = rws_out.len();

    Ok(ResultSet {
        columns: cols_out,
        rows: rws_out,
        row_count: row_count as u32,
        elapsed_ms: elapsed as u32,
        temp_table: actual_table,
    })
}
