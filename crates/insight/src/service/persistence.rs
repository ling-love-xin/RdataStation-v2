use crate::model::types::ColumnInsightFull;
use crate::model::{ColumnProfileView, SampleSource, TableProfileView};
use crate::store::{InsightMetaStore, InsightStorage};
use crate::{insight_engine, quality_scorer, store};
use shared::error::{CommonError, CoreError};

/// 源取样的默认行数上限（`ProbeConfig::sample_default` 的 `RowCount` 基数）。
///
/// 保留为常量供既有调用方/测试引用；新代码一律走 `ProbeConfig`（确认窗口产物）。
pub const SOURCE_SAMPLE_LIMIT: usize = 500;

/// 源取样统一口径：入口给的只读查询 → `tmp_i_` 分析临时表（建表即登记）。
///
/// 这是「凡能喂给 DuckDB 的数据都能洞察」的落点：导航树表 / 分析存档 / 草稿箱 /
/// 编辑器结果集都只提供「连接 + 一段只读查询 + 标签」，样本与后续分析在洞察这边完成。
///
/// 取数口径只在**这里**（`ProbeConfig` 驱动，见
/// `docs/architecture/insight/insight-probe-config-decision.md` §4/§7）：
/// 数据量形态（全量 / 前 N 行 / 百分比）+ 采样方式 + 过滤 + 种子；
/// 入口的查询可以不带限，也不能靠入口自觉。
///
/// 两条路径：
/// - **源库连接**：数据过 Rust（引擎的执行结果是 JSON，要重新打型建表）。
///   `Percent` 在这里退化为「`COUNT(*)` 估算行数 → `LIMIT N'`」近似比例取样
///   （MySQL / PG 没有 `TABLESAMPLE reservoir`；差异在确认窗口 UI 说明）。
/// - **DuckDB 内存库**：数据**不过 Rust**——`CREATE TABLE … AS` 直接在库里落，
///   `Percent` 走 `USING SAMPLE reservoir`（均匀随机，可配 seed 复现）。
pub async fn sample_source_to_analysis_table(
    source: &SampleSource,
    config: &crate::model::ProbeConfig,
) -> Result<String, CoreError> {
    match source.conn_id() {
        Some(conn_id) => {
            // 源库路径：Full / RowCount / WHERE 直接拼进发给源库的 SQL；
            // Percent → 先 COUNT 估算行数，再 LIMIT（源库无 reservoir 采样）。
            let sql = match &config.amount {
                crate::model::ProbeAmount::Percent(pct) => {
                    let count = count_source_rows(conn_id, source).await?;
                    let n = ((count as f64) * pct.clamp(0.0, 100.0) / 100.0).ceil() as u64;
                    build_source_sample_sql(
                        source,
                        crate::model::ProbeAmount::RowCount(n.max(1) as u32),
                        config.filter.as_deref(),
                        config.column_subset(),
                    )
                }
                _ => build_source_sample_sql(
                    source,
                    config.amount.clone(),
                    config.filter.as_deref(),
                    config.column_subset(),
                ),
            };
            sample_from_connection(conn_id, &sql, &source.label).await
        }
        None => {
            let duckdb = insight_engine::get_or_create_duckdb()?;
            let conn = duckdb.lock().map_err(|e| {
                CoreError::common(CommonError::General(format!("DuckDB lock error: {e}")))
            })?;
            engine::duckdb::analysis::create_analysis_temp_table_as(
                &conn,
                &build_duckdb_sample_sql(source, config),
                "source_sample",
            )
        }
    }
}

/// 构造发给**源库连接**的取样 SQL（LIMIT / WHERE / 列投影由源库执行）。
///
/// `subset` = 确认窗口的列选择（C4：未选列不在 EDA 上存在——不投影就不进临时表）；
/// `None` / 空 = 全部列（确认窗口「全部列」默认口径）。
fn build_source_sample_sql(
    source: &SampleSource,
    amount: crate::model::ProbeAmount,
    filter: Option<&str>,
    subset: Option<&[String]>,
) -> String {
    let inner = source.sql.trim().trim_end_matches(';');
    // WHERE 过滤包进内层：先过滤再取数（R1，根源减资源的延伸）
    let inner = match filter {
        Some(f) if !f.trim().is_empty() => {
            format!("SELECT * FROM ({inner}) AS rds_where WHERE {f}")
        }
        _ => inner.to_string(),
    };
    let projection = column_projection(subset);
    match amount {
        crate::model::ProbeAmount::Full => format!("SELECT {projection} FROM ({inner}) AS rds_sample"),
        crate::model::ProbeAmount::RowCount(n) => {
            format!("SELECT {projection} FROM ({inner}) AS rds_sample LIMIT {n}")
        }
        // 源库路径的 Percent 在调用方已折算成 RowCount（此处不出现）
        crate::model::ProbeAmount::Percent(_) => {
            format!("SELECT {projection} FROM ({inner}) AS rds_sample")
        }
    }
}

/// 列投影片段（C4）：子集列转 `"a", "b"`（双引号内引号翻倍转义）；空 / None = `*`。
fn column_projection(subset: Option<&[String]>) -> String {
    match subset {
        Some(cols) if !cols.is_empty() => cols
            .iter()
            .map(|c| format!("\"{}\"", c.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(", "),
        _ => "*".to_string(),
    }
}

/// 构造 DuckDB 内存库路径的取样 SQL：支持 LIMIT / reservoir 百分比 / WHERE / seed / 列投影。
fn build_duckdb_sample_sql(source: &SampleSource, config: &crate::model::ProbeConfig) -> String {
    let inner = source.sql.trim().trim_end_matches(';');
    let inner = if config.has_filter() {
        let f = config.filter.as_deref().unwrap_or_default();
        format!("SELECT * FROM ({inner}) AS rds_where WHERE {f}")
    } else {
        inner.to_string()
    };
    let projection = column_projection(config.column_subset());
    match &config.amount {
        crate::model::ProbeAmount::Full => {
            format!("SELECT {projection} FROM ({inner}) AS rds_sample")
        }
        crate::model::ProbeAmount::RowCount(n) => {
            format!("SELECT {projection} FROM ({inner}) AS rds_sample LIMIT {n}")
        }
        crate::model::ProbeAmount::Percent(pct) => {
            let pct = pct.clamp(0.0, 100.0);
            match config.seed {
                Some(seed) => format!(
                    "SELECT {projection} FROM ({inner}) AS rds_sample USING SAMPLE reservoir({pct} PERCENT) (SEED {seed})"
                ),
                None => format!(
                    "SELECT {projection} FROM ({inner}) AS rds_sample USING SAMPLE reservoir({pct} PERCENT)"
                ),
            }
        }
    }
}

/// 源查询的列清单轻量内省（确认窗口「列」块的候选，C1）。
///
/// **不取数据**：源库连接走 `SELECT * FROM (…) LIMIT 0`（只拿列结构）；
/// DuckDB 内存库走 `DESCRIBE (…)`。失败不阻塞确认——列候选为空时默认「全部列」。
pub async fn probe_source_columns(source: &SampleSource) -> Result<Vec<String>, CoreError> {
    let inner = source.sql.trim().trim_end_matches(';');
    match source.conn_id() {
        Some(conn_id) => {
            use engine::SqlService;
            use engine::get_connection_manager;
            use engine::services::sql_service::SqlExecuteOptions;

            let service = SqlService::new(get_connection_manager().clone());
            let opts = SqlExecuteOptions {
                channel: None,
                record_history: false,
                use_transaction: false,
                timeout_ms: Some(15_000),
                use_cache: false,
                schema: None,
            };
            let sql = format!("SELECT * FROM ({inner}) AS rds_cols LIMIT 0");
            let result = service                .execute(Some(conn_id.to_string()), &sql, opts)
                .await?;
            // 源库 `LIMIT 0` 的列集合即列名（数据不过 Rust 打型，只读结构）
            Ok(crate::service::result_columns_and_rows(&result.result).0)
        }
        None => {
            let duckdb = insight_engine::get_or_create_duckdb()?;
            let conn = duckdb.lock().map_err(|e| {
                CoreError::common(CommonError::General(format!("DuckDB lock error: {e}")))
            })?;
            // DESCRIBE 子查询：列定义是 DuckDB 稳定契约，只取第 0 列（列名）
            let sql = format!("DESCRIBE (SELECT * FROM ({inner}) AS rds_cols)");
            let mut stmt = conn.prepare(&sql).map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "DuckDB describe prepare failed: {e}"
                )))
            })?;
            let names: Vec<String> = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| {
                    CoreError::common(CommonError::General(format!(
                        "DuckDB describe query failed: {e}"
                    )))
                })?
                .filter_map(|r| r.ok())
                .collect();
            if names.is_empty() {
                return Err(CoreError::common(CommonError::General(
                    "源查询没有可读的列结构".into(),
                )));
            }
            Ok(names)
        }
    }
}

/// 源库连接的行数估算（`Percent` 模式的取数前奏）：`COUNT(*)` 一次轻量查询。
async fn count_source_rows(conn_id: &str, source: &SampleSource) -> Result<u64, CoreError> {
    use engine::SqlService;
    use engine::get_connection_manager;
    use engine::services::sql_service::SqlExecuteOptions;

    let service = SqlService::new(get_connection_manager().clone());
    let opts = SqlExecuteOptions {
        channel: None,
        record_history: false,
        use_transaction: false,
        timeout_ms: Some(15_000),
        use_cache: false,
        schema: None,
    };
    let inner = source.sql.trim().trim_end_matches(';');
    let count_sql = format!("SELECT COUNT(*) FROM ({inner}) AS rds_count");
    let result = service
        .execute(Some(conn_id.to_string()), &count_sql, opts)
        .await?;
    let (_columns, rows) = crate::service::result_columns_and_rows(&result.result);
    let first = rows
        .first()
        .and_then(|row| row.first())
        .and_then(|v| v.as_i64().or_else(|| v.as_u64().map(|u| u as i64)));
    first
        .map(|n| n.max(0) as u64)
        .ok_or_else(|| {
            CoreError::common(CommonError::General(
                "行数估算失败：COUNT 查询没有返回可读的数字".into(),
            ))
        })
}

/// 从一条**源库连接**取样本并落成分析临时表。
async fn sample_from_connection(
    conn_id: &str,
    sample_sql: &str,
    label: &str,
) -> Result<String, CoreError> {
    use engine::SqlService;
    use engine::get_connection_manager;
    use engine::services::sql_service::SqlExecuteOptions;

    let service = SqlService::new(get_connection_manager().clone());
    let opts = SqlExecuteOptions {
        channel: None,
        record_history: false,
        use_transaction: false,
        timeout_ms: Some(15_000),
        use_cache: false,
        // 取样用三段限定名（`db`.`schema`.`table`），无需会话上下文
        schema: None,
    };
    let result = service
        .execute(Some(conn_id.to_string()), sample_sql, opts)
        .await?;
    // 进程内直读（不能走 JSON 契约序列化：它不含 `batches`，永远是空行）
    let (columns, rows) = crate::service::result_columns_and_rows(&result.result);
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

/// 源目标的列画像：取样（或全量）→ 画像，返回**（样本表名, 视图）**。
///
/// 返回样本表名是必需的：面板保存快照 / 开多列 / 下钻都要接着用同一份样本
/// （否则会再抽一次，两次结果还不一样）。
pub async fn profile_source_column(
    project_root: Option<&std::path::Path>,
    source: &SampleSource,
    column_name: &str,
    config: &crate::model::ProbeConfig,
) -> Result<(String, ColumnProfileView), CoreError> {
    let temp_table = sample_source_to_analysis_table(source, config).await?;

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

/// 源目标的表探查：取样（或全量）→ 内省，返回**（样本表名, 视图）**。
///
/// 表探查不跑规则（与临时表那一路同一口径）：逐列统计是「评估全表」的事。
pub async fn profile_source_table(
    source: &SampleSource,
    table_name: &str,
    config: &crate::model::ProbeConfig,
) -> Result<(String, TableProfileView), CoreError> {
    let temp_table = sample_source_to_analysis_table(source, config).await?;
    let profile = insight_engine::get_temp_table_profile(&temp_table)?;
    let mut view = TableProfileView::from_profile(&profile, table_name);
    // EDA 列统计摘要（缺失率 / 唯一率）：失败不阻塞画像（保留 None）
    let column_names: Vec<String> = profile
        .columns
        .iter()
        .map(|c| c.column_name.clone())
        .collect();
    if let Ok(stats) = insight_engine::compute_table_column_summary(&temp_table, &column_names) {
        view.fill_column_stats(&stats);
    }
    // 整行重复统计（概览五卡「重复行」）：失败不阻塞画像，卡位显示「—」
    if let Ok(dup) = insight_engine::compute_dup_rows(&temp_table) {
        view.dup_rows = Some(dup);
    }
    // EDA 样本（前 5 行）：失败不阻塞画像（样本区给空态）
    if let Ok(sample) = insight_engine::get_temp_table_sample(&temp_table) {
        view.sample = Some(sample);
    }
    Ok((temp_table, view))
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
    use engine::SqlService;
    use engine::get_connection_manager;
    use engine::services::sql_service::SqlExecuteOptions;

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
        channel: None,
        record_history: false,
        use_transaction: false,
        timeout_ms: Some(15000),
        use_cache: false,
        // 取样用三段限定名（`db`.`schema`.`table`），无需会话上下文
        schema: None,
    };

    let result = service
        .execute(Some(conn_id.clone()), &sample_sql, opts)
        .await?;
    // 同上：进程内直读
    let (columns, rows) = crate::service::result_columns_and_rows(&result.result);

    if columns.is_empty() {
        return Err(CoreError::common(CommonError::General(
            "无法从表中读取数据".to_string(),
        )));
    }

    // 样本表用**分析临时表**的姿势建：带 `tmp_i_` 前缀（TTL / 上限 / 按来源清理才认它），
    // 并且算完就收掉——它是纯中间产物，留在内存库里只会越积越多（架构 K16）。
    let duckdb = insight_engine::get_or_create_duckdb()?;
    let conn = duckdb
        .lock()
        .map_err(|e| CoreError::common(CommonError::General(format!("DuckDB lock error: {e}"))))?;
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
    use engine::SqlService;
    use engine::get_connection_manager;
    use engine::services::sql_service::SqlExecuteOptions;

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
        channel: None,
        record_history: false,
        use_transaction: false,
        timeout_ms: Some(15000),
        use_cache: false,
        // 取样用三段限定名（`db`.`schema`.`table`），无需会话上下文
        schema: None,
    };

    let result = service.execute(Some(conn_id), &sample_sql, opts).await?;
    let (col_names, rows_data) = crate::service::result_columns_and_rows(&result.result);

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
    let conn = duckdb
        .lock()
        .map_err(|e| CoreError::common(CommonError::General(format!("DuckDB lock error: {e}"))))?;
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
                        registry, &conn, temp_table, col_name,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ProbeAmount, ProbeColumns, ProbeConfig, ProbeSampleMode};

    fn config(
        amount: ProbeAmount,
        columns: ProbeColumns,
        filter: Option<&str>,
        seed: Option<u64>,
    ) -> ProbeConfig {
        ProbeConfig {
            amount,
            sample_mode: ProbeSampleMode::Head,
            columns,
            filter: filter.map(str::to_string),
            seed,
        }
    }

    fn duckdb_source(sql: &str) -> SampleSource {
        SampleSource::on_duckdb(sql, "t_probe")
    }

    /// C4：未选列不进建表 SQL——列子集投影到 SELECT 列表（双引号内引号翻倍转义）。
    #[test]
    fn source_sample_sql_projects_selected_columns() {
        let source = duckdb_source("SELECT * FROM orders");
        let sql = build_source_sample_sql(
            &source,
            ProbeAmount::RowCount(10),
            None,
            Some(&["id".into(), "note".into()]),
        );
        assert!(
            sql.starts_with("SELECT \"id\", \"note\" FROM (SELECT * FROM orders) AS rds_sample LIMIT 10"),
            "子集列应投影进 SELECT：{sql}"
        );

        // 全列（默认口径）= SELECT *
        let sql = build_source_sample_sql(&source, ProbeAmount::RowCount(10), None, None);
        assert!(sql.starts_with("SELECT * FROM"), "无子集应保持 SELECT *：{sql}");
    }

    /// C4：WHERE 过滤包内层先过滤，再投影 + LIMIT（R1 的延伸）。
    #[test]
    fn source_sample_sql_keeps_where_inner_then_projects() {
        let source = duckdb_source("SELECT * FROM orders");
        let sql = build_source_sample_sql(
            &source,
            ProbeAmount::Full,
            Some("status = 'ok'"),
            Some(&["id".into(), "note".into()]),
        );
        assert!(
            sql.starts_with("SELECT \"id\", \"note\" FROM (SELECT * FROM (SELECT * FROM orders) AS rds_where WHERE status = 'ok') AS rds_sample"),
            "WHERE 应先过滤再投影：{sql}"
        );
    }

    /// C4：DuckDB 百分比采样同样投影列（Percent + seed 组合）。
    #[test]
    fn duckdb_sample_sql_projects_columns_with_reservoir() {
        let source = duckdb_source("SELECT * FROM orders");
        let sql = build_duckdb_sample_sql(
            &source,
            &config(
                ProbeAmount::Percent(10.0),
                ProbeColumns::Subset(vec!["a b".into(), "q\"uote".into()]),
                None,
                Some(42),
            ),
        );
        assert!(
            sql.starts_with("SELECT \"a b\", \"q\"\"uote\" FROM (SELECT * FROM orders) AS rds_sample USING SAMPLE reservoir(10 PERCENT) (SEED 42)"),
            "DuckDB 百分比采样应投影列 + seed：{sql}"
        );
    }

    /// C4：DuckDB 全量档列子集投影（RowCount 路径的对应）。
    #[test]
    fn duckdb_sample_sql_projects_columns_for_limit() {
        let source = duckdb_source("SELECT * FROM orders");
        let sql = build_duckdb_sample_sql(
            &source,
            &config(
                ProbeAmount::RowCount(5),
                ProbeColumns::Subset(vec!["id".into()]),
                None,
                None,
            ),
        );
        assert!(
            sql.starts_with("SELECT \"id\" FROM (SELECT * FROM orders) AS rds_sample LIMIT 5"),
            "DuckDB LIMIT 路径应投影列：{sql}"
        );
    }
}
