use crate::model::types::ColumnInsightFull;
use crate::model::{ColumnProfileView, SampleSource, TableProfileView};
use crate::store::{InsightMetaStore, InsightStorage};
use crate::{insight_engine, quality_scorer, store};
use shared::error::{CommonError, CoreError};

/// 源取样的行数上限（与源表列画像的 `LIMIT 500` 同一口径）。
///
/// 不放进 `SampleSource`：抽样多少行是**洞察的口径**，不该由每个入口各写一次。
pub const SOURCE_SAMPLE_LIMIT: usize = 500;

/// 源取样统一口径：入口给的只读查询 → `tmp_i_` 分析临时表（建表即登记）。
///
/// 这是「凡能喂给 DuckDB 的数据都能洞察」的落点：导航树表 / 分析存档 / 草稿箱 /
/// 编辑器结果集都只提供「连接 + 一段只读查询 + 标签」，样本与后续分析在洞察这边完成。
///
/// 外层**再包一层 `LIMIT`**：入口的查询可以不带限（源表全量查询），
/// 也不能靠入口自觉——口径只在这里。
pub async fn sample_source_to_analysis_table(source: &SampleSource) -> Result<String, CoreError> {
    let sample_sql = format!(
        "SELECT * FROM ({}) AS rds_sample LIMIT {SOURCE_SAMPLE_LIMIT}",
        source.sql.trim().trim_end_matches(';')
    );

    match source.conn_id() {
        // 源库连接：数据过 Rust（引擎的执行结果是 JSON，要重新打型建表）
        Some(conn_id) => sample_from_connection(conn_id, &sample_sql, &source.label).await,
        // DuckDB 内存库：**数据不过 Rust**——`CREATE TABLE … AS` 直接在库里落，
        // 文件类（CSV / Parquet / Excel，靠扩展读）与已 `ATTACH` 的库表走这条
        None => {
            let duckdb = insight_engine::get_or_create_duckdb()?;
            let conn = duckdb.lock().map_err(|e| {
                CoreError::common(CommonError::General(format!("DuckDB lock error: {e}")))
            })?;
            engine::duckdb::analysis::create_analysis_temp_table_as(
                &conn,
                &sample_sql,
                "source_sample",
            )
        }
    }
}

/// 从一条**源库连接**取样本并落成分析临时表。
async fn sample_from_connection(
    conn_id: &str,
    sample_sql: &str,
    label: &str,
) -> Result<String, CoreError> {
    use engine::get_connection_manager;
    use engine::services::sql_service::SqlExecuteOptions;
    use engine::SqlService;

    let service = SqlService::new(get_connection_manager().clone());
    let opts = SqlExecuteOptions {
        record_history: false,
        use_transaction: false,
        timeout_ms: Some(15_000),
        use_cache: false,
    };
    let result = service
        .execute(Some(conn_id.to_string()), sample_sql, opts)
        .await?;
    let json = serde_json::to_value(&result.result)
        .map_err(|e| CoreError::common(CommonError::General(format!("Serialize error: {e}"))))?;

    let (columns, rows) = batch_columns_and_rows(&json);
    if columns.is_empty() {
        return Err(CoreError::common(CommonError::General(format!(
            "取样没有拿到列（来源：{label}）——查询可能没有结果集"
        ))));
    }

    let duckdb = insight_engine::get_or_create_duckdb()?;
    let conn = duckdb
        .lock()
        .map_err(|e| CoreError::common(CommonError::General(format!("DuckDB lock error: {e}"))))?;
    engine::duckdb::analysis::create_analysis_temp_table(&conn, &columns, &rows, "source_sample")
}

/// 首个 batch 的列与行（引擎的执行结果 JSON → 可建表的两段）。
fn batch_columns_and_rows(
    json: &serde_json::Value,
) -> (Vec<String>, Vec<Vec<serde_json::Value>>) {
    match json["batches"]
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
    }
}

/// 源目标的列画像：取样 → 画像，返回**（样本表名, 视图）**。
///
/// 返回样本表名是必需的：面板保存快照 / 开多列 / 下钻都要接着用同一份样本
/// （否则会再抽一次，两次结果还不一样）。
pub async fn profile_source_column(
    project_root: Option<&std::path::Path>,
    source: &SampleSource,
    column_name: &str,
) -> Result<(String, ColumnProfileView), CoreError> {
    let temp_table = sample_source_to_analysis_table(source).await?;

    let duckdb = insight_engine::get_or_create_duckdb()?;
    let conn = duckdb
        .lock()
        .map_err(|e| CoreError::common(CommonError::General(format!("DuckDB lock error: {e}"))))?;
    // 与样表同一把锁：用 `*_on`（已持连接版本），避开 std `Mutex` 自重入
    let full = crate::with_rules(project_root, |registry| {
        insight_engine::get_column_insight_full_on(registry, &conn, &temp_table, column_name)
    })?;
    Ok((temp_table, ColumnProfileView::from_domain(&full)))
}

/// 源目标的表探查：取样 → 内省，返回**（样本表名, 视图）**。
///
/// 表探查不跑规则（与临时表那一路同一口径）：逐列统计是「评估全表」的事。
pub async fn profile_source_table(
    source: &SampleSource,
    table_name: &str,
) -> Result<(String, TableProfileView), CoreError> {
    let temp_table = sample_source_to_analysis_table(source).await?;
    let profile = insight_engine::get_temp_table_profile(&temp_table)?;
    Ok((temp_table, TableProfileView::from_profile(&profile, table_name)))
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

    // 采样取自**用户源库**：反引号三段名是 MySQL 语法（PostgreSQL / SQLite 会直接报错）。
    // 本函数目前无宿主侧调用者（表入口与导航右键欠账，见
    // `docs/architecture/insight/insight-dev-plan.md`），接线前须先按 `db_type` 分派方言。
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

    let (columns, rows) = batch_columns_and_rows(&json);

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

    // 采样取自**用户源库**：反引号三段名是 MySQL 语法（PostgreSQL / SQLite 会直接报错）。
    // 本函数目前无宿主侧调用者（表入口与导航右键欠账，见
    // `docs/architecture/insight/insight-dev-plan.md`），接线前须先按 `db_type` 分派方言。
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
