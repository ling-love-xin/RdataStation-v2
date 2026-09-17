use crate::model::types::ColumnInsightFull;
use crate::store::{InsightMetaStore, InsightStorage};
use crate::{insight_engine, quality_scorer, store};
use shared::error::{CommonError, CoreError};

#[allow(clippy::too_many_arguments)]
pub async fn save_column_insight_snapshot(
    insight: &ColumnInsightFull,
    conn_id: Option<&str>,
    db_name: Option<&str>,
    schema_name: Option<&str>,
    table_name: Option<&str>,
    row_count: Option<i32>,
    elapsed_ms: Option<i32>,
    insight_store: &InsightStorage,
    meta_store: &InsightMetaStore,
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

    let checksum = store::snapshot_checksum(insight)?;

    // Q5 / D55：元数据写失败时**回滚刚写入的正文**（要么都成、要么都不留），
    // 不留下「界面上看不见、清理也配不上对」的孤儿。
    if let Err(e) = meta_store
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
        .await
    {
        return Err(
            crate::store::rollback_snapshot_body(&insight_store.columns, &snapshot_id, e).await,
        );
    }

    Ok((snapshot_id, version_id))
}

pub async fn get_column_insight_history(
    column_name: &str,
    insight_store: &InsightStorage,
) -> Result<Vec<crate::store::InsightVersionEntry>, CoreError> {
    insight_store
        .columns
        .get_history(column_name, Some(10))
        .await
}

pub async fn cleanup_old_insight_snapshots(
    days: i32,
    insight_store: &InsightStorage,
    meta_store: &InsightMetaStore,
) -> Result<(i32, usize), CoreError> {
    let duckdb_deleted = insight_store
        .columns
        .cleanup_older_than(days as i64)
        .await? as i32;
    let sqlite_deleted = meta_store.cleanup_older_than(days).await?;
    Ok((duckdb_deleted, sqlite_deleted))
}

pub async fn get_insight_storage_stats(
    insight_store: &InsightStorage,
) -> Result<crate::store::InsightStorageStats, CoreError> {
    insight_store.columns.get_storage_stats().await
}

pub async fn get_insight_version_detail(
    version_id: &str,
    insight_store: &InsightStorage,
) -> Result<Option<ColumnInsightFull>, CoreError> {
    insight_store
        .columns
        .get_snapshot_by_version(version_id)
        .await
}

pub async fn profile_column_from_table(
    project_root: Option<&std::path::Path>,
    conn_id: String,
    database: &str,
    schema: &str,
    table: &str,
    column_name: &str,
) -> Result<ColumnInsightFull, CoreError> {
    use engine::get_connection_manager;
    use engine::services::sql_service::SqlExecuteOptions;
    use engine::SqlService;

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

    // 样本表用**分析临时表**的姿势建：带 `tmp_i_` 前缀（TTL / 上限 / 按来源清理才认它），
    // 并且算完就收掉——它是纯中间产物，留在内存库里只会越积越多（架构 K16）。
    let duckdb = insight_engine::get_or_create_duckdb()?;
    let conn = duckdb.lock().map_err(|e| {
        CoreError::common(CommonError::General(format!("DuckDB lock error: {e}")))
    })?;
    engine::duckdb::analysis::with_analysis_temp_table(
        &conn,
        &columns,
        &rows,
        "col_sample",
        |temp_table| {
            // 基础统计由 TOML 规则驱动，故需按项目取规则集。
            // 用 `*_on`（已持有连接）：与建表同一把锁，避免 `std::sync::Mutex` 自重入。
            crate::with_rules(project_root, |registry| {
                insight_engine::get_column_insight_full_on(registry, &conn, temp_table, column_name)
            })
        },
    )
}

pub async fn batch_evaluate_columns(
    project_root: Option<&std::path::Path>,
    conn_id: String,
    database: &str,
    schema: &str,
    table: &str,
) -> Result<crate::model::types::TableQuality, CoreError> {
    use engine::get_connection_manager;
    use engine::services::sql_service::SqlExecuteOptions;
    use engine::SqlService;

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
        return Ok(crate::model::types::TableQuality {
            table_name: table.into(),
            overall_score: 0.0,
            level: "无数据".into(),
            column_scores: vec![],
            summary: "表为空或无数据".into(),
            scored_count: 0,
            total_columns: 0,
        });
    }

    // 同上：整表评估也是一次性的样本表（逐列串行跑完就收）
    let duckdb = insight_engine::get_or_create_duckdb()?;
    let conn = duckdb.lock().map_err(|e| {
        CoreError::common(CommonError::General(format!("DuckDB lock error: {e}")))
    })?;
    engine::duckdb::analysis::with_analysis_temp_table(
        &conn,
        &col_names,
        &rows_data,
        "table_sample",
        |temp_table| {
            // 整表评估：规则集只取一次（不在逐列循环里重复加读锁）。
            // 串行逐列是刻意的——洞察并发上限为 4 且快速失败，并行会把「部分列静默缺失」
            // 变成常态；串行起步虽慢，但「哪些列没评上」是确定的（单列失败仍跳过并继续）。
            crate::with_rules(project_root, |registry| {
                let mut stats_list: Vec<ColumnInsightFull> = Vec::new();
                for col_name in &col_names {
                    let stats = insight_engine::get_column_insight_full_on(
                        registry,
                        &conn,
                        temp_table,
                        col_name,
                    );
                    match stats {
                        Ok(stats) => stats_list.push(stats),
                        Err(e) => {
                            tracing::warn!(
                                "Skipping column '{}' during batch evaluation: {}",
                                col_name,
                                e
                            );
                            continue;
                        }
                    }
                }

                Ok(quality_scorer::compute_table_quality(table, &stats_list))
            })
        },
    )
}
