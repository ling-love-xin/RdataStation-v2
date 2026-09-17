//! 表探查（源库内省）
//!
//! ⚠️ **方言假设尚未统一**：本模块的 SQL 面向**用户源库**（经 `SqlService`），写法是
//! MySQL 形状——`information_schema.columns.column_key` 是 MySQL 扩展列（PostgreSQL 无此列）；
//! 而 `fetch_all_tables`（在 `schema_analyzer`，同一批源库内省）的 `table_catalog` 过滤
//! 则只在 PostgreSQL 语义下成立（MySQL 的 `table_catalog` 恒为 'def'），SQLite 连
//! `information_schema` 都没有。多种假设并存 = 这条路径尚未在真实源库上跑通。
//!
//! ⚠️ **当前零调用**：面板的表目标走 DuckDB 临时表（见 `insight_engine::get_temp_table_profile`，
//! 架构决策 D29「不走源库」），本模块属登记在册的欠账
//! （`docs/architecture/insight/insight-dev-plan.md`）。接线前必须先按 `db_type` 分派方言，
//! 或改走 `database::MetadataService`（其 `MetadataBrowser` 已按驱动内省）。

use shared::error::{CommonError, CoreError};
use engine::get_connection_manager;
use crate::model::types::{TableColumnMeta, TableProfile};
use engine::services::sql_service::SqlExecuteOptions;
use engine::SqlService;

pub async fn get_table_profile(
    conn_id: String,
    db_type: String,
    database: &str,
    schema: &str,
    table: &str,
) -> Result<TableProfile, CoreError> {
    let manager = get_connection_manager().clone();
    let service = SqlService::new(manager);
    let conn_id_opt = Some(conn_id.clone());

    let columns =
        fetch_table_columns(&service, conn_id_opt.clone(), database, schema, table).await?;

    let row_count = fetch_row_count(&service, conn_id_opt, database, schema, table)
        .await
        .ok()
        .map(|v| v as i32);

    Ok(TableProfile {
        table_name: table.to_string(),
        db_type,
        columns,
        row_count,
        schema_name: Some(schema.to_string()),
    })
}

async fn fetch_table_columns(
    service: &SqlService,
    conn_id: Option<String>,
    _database: &str,
    schema: &str,
    table: &str,
) -> Result<Vec<TableColumnMeta>, CoreError> {
    let sql = format!(
        "SELECT column_name, data_type, is_nullable, ordinal_position, column_key \
         FROM information_schema.columns \
         WHERE table_schema = '{}' AND table_name = '{}' \
         ORDER BY ordinal_position",
        schema, table
    );

    let opts = SqlExecuteOptions {
        channel: None,
        record_history: false,
        use_transaction: false,
        timeout_ms: Some(15000),
        use_cache: false,
    };

    let result = service.execute(conn_id, &sql, opts).await?;
    let json = serde_json::to_value(&result.result)
        .map_err(|e| CoreError::common(CommonError::General(format!("Serialize error: {}", e))))?;

    let rows = json["batches"].as_array().and_then(|batches| {
        batches
            .first()?
            .get("columns")
            .and_then(|c| c.as_array())
            .and_then(|columns_arr| {
                batches
                    .first()?
                    .get("rows")
                    .and_then(|r| r.as_array())
                    .map(|rows_arr| (columns_arr.clone(), rows_arr.clone()))
            })
    });

    let columns: Vec<TableColumnMeta> = match rows {
        Some((col_names, row_data)) => {
            let col_idx = |name: &str| -> Option<usize> {
                col_names.iter().position(|c| c.as_str() == Some(name))
            };
            row_data
                .iter()
                .filter_map(|row| {
                    let arr = row.as_array()?;
                    let name = arr.get(col_idx("column_name")?)?.as_str()?.to_string();
                    let dtype = arr.get(col_idx("data_type")?)?.as_str()?.to_string();
                    let nullable = arr
                        .get(col_idx("is_nullable")?)
                        .and_then(|v| v.as_str())
                        .map(|s| s == "YES")
                        .unwrap_or(false);
                    let pos = arr
                        .get(col_idx("ordinal_position")?)
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0) as i32;
                    let is_pk = arr
                        .get(col_idx("column_key")?)
                        .and_then(|v| v.as_str())
                        .map(|s| s == "PRI")
                        .unwrap_or(false);
                    Some(TableColumnMeta {
                        column_name: name,
                        data_type: dtype,
                        is_nullable: nullable,
                        is_primary_key: is_pk,
                        ordinal_position: pos,
                    })
                })
                .collect()
        }
        None => vec![],
    };

    Ok(columns)
}

async fn fetch_row_count(
    service: &SqlService,
    conn_id: Option<String>,
    database: &str,
    schema: &str,
    table: &str,
) -> Result<i64, CoreError> {
    let sql = format!(
        "SELECT COUNT(*) AS cnt FROM `{}`.`{}`.`{}`",
        database, schema, table
    );

    let opts = SqlExecuteOptions {
        channel: None,
        record_history: false,
        use_transaction: false,
        timeout_ms: Some(30000),
        use_cache: false,
    };

    let result = service.execute(conn_id, &sql, opts).await?;
    let json = serde_json::to_value(&result.result)
        .map_err(|e| CoreError::common(CommonError::General(format!("Serialize error: {}", e))))?;

    let count = json["batches"]
        .as_array()
        .and_then(|batches| batches.first())
        .and_then(|batch| batch["rows"].as_array())
        .and_then(|rows| rows.first())
        .and_then(|row| row.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    Ok(count)
}
