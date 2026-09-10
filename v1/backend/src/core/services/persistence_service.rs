use crate::core::error::{CommonError, CoreError};
use crate::core::services::insight_engine;
use crate::core::services::result_service::ColumnInsightFull;

fn sha256_hex(input: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn save_column_insight_snapshot(
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
    let parent_version_id = meta_store
        .get_latest_meta("column", &insight.stats.column_name)
        .await
        .ok()
        .flatten()
        .map(|m| m.version_id);

    let (snapshot_id, version_id) = insight_store
        .columns
        .save_snapshot(insight, parent_version_id.as_deref())
        .await?;

    let entity_source = {
        let mut parts = Vec::new();
        if let Some(c) = conn_id {
            parts.push(format!("conn={}", c));
        }
        if let Some(d) = db_name {
            parts.push(format!("db={}", d));
        }
        if let Some(s) = schema_name {
            parts.push(format!("schema={}", s));
        }
        if let Some(t) = table_name {
            parts.push(format!("table={}", t));
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(","))
        }
    };

    let checksum = sha256_hex(&serde_json::to_string(insight).unwrap_or_default());

    meta_store
        .save_meta(
            "column",
            &insight.stats.column_name,
            entity_source.as_deref(),
            &snapshot_id,
            row_count,
            elapsed_ms,
            &version_id,
            parent_version_id.as_deref(),
            &checksum,
        )
        .await?;

    Ok((snapshot_id, version_id))
}

pub(crate) async fn get_column_insight_history(
    column_name: &str,
    insight_store: &crate::core::persistence::InsightStorage,
) -> Result<Vec<crate::core::persistence::InsightVersionEntry>, CoreError> {
    insight_store
        .columns
        .get_history(column_name, Some(10))
        .await
}

pub(crate) async fn cleanup_old_insight_snapshots(
    days: i32,
    insight_store: &crate::core::persistence::InsightStorage,
    meta_store: &crate::core::persistence::InsightMetaStore,
) -> Result<(i32, usize), CoreError> {
    let duckdb_deleted = insight_store
        .columns
        .cleanup_older_than(days as i64)
        .await? as i32;
    let sqlite_deleted = meta_store.cleanup_older_than(days).await?;
    Ok((duckdb_deleted, sqlite_deleted))
}

pub(crate) async fn get_insight_storage_stats(
    insight_store: &crate::core::persistence::InsightStorage,
) -> Result<crate::core::persistence::InsightStorageStats, CoreError> {
    insight_store.columns.get_storage_stats().await
}

pub(crate) async fn get_insight_version_detail(
    version_id: &str,
    insight_store: &crate::core::persistence::InsightStorage,
) -> Result<Option<ColumnInsightFull>, CoreError> {
    insight_store
        .columns
        .get_snapshot_by_version(version_id)
        .await
}

pub(crate) async fn profile_column_from_table(
    conn_id: String,
    database: &str,
    schema: &str,
    table: &str,
    column_name: &str,
) -> Result<ColumnInsightFull, CoreError> {
    use crate::core::get_connection_manager;
    use crate::core::services::sql_service::SqlExecuteOptions;
    use crate::core::services::SqlService;

    let manager = get_connection_manager().clone();
    let service = SqlService::new(manager);

    let sample_sql = format!(
        "SELECT * FROM `{}`.`{}`.`{}` LIMIT 500",
        database, schema, table
    );

    let opts = SqlExecuteOptions {
        record_history: false,
        use_transaction: false,
        timeout_ms: Some(15000),
        use_cache: false,
    };

    let result = service
        .execute(Some(conn_id.clone()), &sample_sql, opts)
        .await?;
    let json = serde_json::to_value(&result.result)
        .map_err(|e| CoreError::common(CommonError::General(format!("Serialize error: {}", e))))?;

    let (columns, rows) = match json["batches"]
        .as_array()
        .and_then(|batches| batches.first())
    {
        Some(batch) => {
            let cols: Vec<String> = batch["columns"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|c| c.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();

            let rows_data: Vec<Vec<serde_json::Value>> = batch["rows"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .map(|row| row.as_array().cloned().unwrap_or_default())
                        .collect()
                })
                .unwrap_or_default();

            (cols, rows_data)
        }
        None => (vec![], vec![]),
    };

    if columns.is_empty() {
        return Err(CoreError::common(CommonError::General(
            "无法从表中读取数据".to_string(),
        )));
    }

    let temp_table =
        crate::core::services::duckdb_service::DuckDbService::create_duckdb_temp_table(
            &columns, &rows,
        )?;

    let stats = insight_engine::get_column_insight_full(&temp_table, column_name)?;

    Ok(stats)
}

pub(crate) async fn batch_evaluate_columns(
    conn_id: String,
    database: &str,
    schema: &str,
    table: &str,
) -> Result<crate::core::services::result_service::TableQuality, CoreError> {
    use crate::core::get_connection_manager;
    use crate::core::services::sql_service::SqlExecuteOptions;
    use crate::core::services::SqlService;

    let manager = get_connection_manager().clone();
    let service = SqlService::new(manager);

    let sample_sql = format!(
        "SELECT * FROM `{}`.`{}`.`{}` LIMIT 500",
        database, schema, table
    );

    let opts = SqlExecuteOptions {
        record_history: false,
        use_transaction: false,
        timeout_ms: Some(15000),
        use_cache: false,
    };

    let result = service.execute(Some(conn_id), &sample_sql, opts).await?;
    let json = serde_json::to_value(&result.result)
        .map_err(|e| CoreError::common(CommonError::General(format!("Serialize error: {}", e))))?;

    let (col_names, rows_data) = match json["batches"].as_array().and_then(|b| b.first()) {
        Some(batch) => {
            let cols: Vec<String> = batch["columns"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|c| c.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();

            let rows: Vec<Vec<serde_json::Value>> = batch["rows"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .map(|row| row.as_array().cloned().unwrap_or_default())
                        .collect()
                })
                .unwrap_or_default();

            (cols, rows)
        }
        None => (vec![], vec![]),
    };

    if col_names.is_empty() {
        return Ok(crate::core::services::result_service::TableQuality {
            table_name: table.into(),
            overall_score: 0.0,
            level: "无数据".into(),
            column_scores: vec![],
            summary: "表为空或无数据".into(),
            scored_count: 0,
            total_columns: 0,
        });
    }

    let temp_table =
        crate::core::services::duckdb_service::DuckDbService::create_duckdb_temp_table(
            &col_names, &rows_data,
        )?;

    let mut stats_list: Vec<ColumnInsightFull> = Vec::new();
    for col_name in &col_names {
        match insight_engine::get_column_insight_full(&temp_table, col_name) {
            Ok(stats) => stats_list.push(stats),
            Err(_) => continue,
        }
    }

    Ok(crate::core::services::quality_scorer::compute_table_quality(table, &stats_list))
}
