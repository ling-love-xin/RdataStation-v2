use crate::model::types::ColumnInsightFull;
use crate::model::{ColumnProfileView, SampleSource, TableProfileView};
use crate::store::{InsightMetaStore, InsightStorage};
use crate::{insight_engine, store};
use shared::error::{CommonError, CoreError};

/// 源取样的默认行数上限（`ProbeConfig::sample_default` 的 `RowCount` 基数）。
///
/// 保留为常量供既有调用方/测试引用；新代码一律走 `ProbeConfig`（确认窗口产物）。
pub const SOURCE_SAMPLE_LIMIT: usize = 500;

/// 源库连接路径「全量档」的行数护栏（P1-8a）。
///
/// 为什么只在源库路径设闸：这一路数据要**整表过 Rust**（`Vec<Vec<Value>>` JSON 值
/// 往返），千万行表是内存尖峰 + 取数超时（15s）的必然失败，且失败点是一句模糊的
/// timeout；DuckDB 内存库路径（文件类）CTAS 在库内完成，有内存闸（D51）与溢写
/// 兜底，无需此闸。超限**拒拉并给引导**（改取样档 / 先过滤），不做静默截断——
/// 截断的「全量」是假全量。
pub(crate) const SOURCE_FULL_GUARD_ROWS: u64 = 1_000_000;

/// 全量档护栏判定（纯函数，供接线与测试分离）。
pub(crate) fn check_full_guard(rows: u64) -> Result<(), CoreError> {
    if rows > SOURCE_FULL_GUARD_ROWS {
        return Err(CoreError::common(CommonError::General(format!(
            "源表约 {rows} 行，超过源库全量拉取上限 {SOURCE_FULL_GUARD_ROWS} 行（全量会整表载入内存）——\
             请改用「百分比 / 行数」取样档，或在过滤条件里先缩小范围"
        ))));
    }
    Ok(())
}

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
    // 全局库表（刀 7 · B）：全局库是 SQLite 文件、有自己的写入方，不能 ATTACH 进单例。
    // 所以这条走「短命会话只读挂载 → 会话内取样 → 搬进单例登记版 tmp_i_」：
    // 下游（EDA / 图表 / 导出）读的仍是进程单例，行为与其它来源一致。
    if source.global {
        let session = engine::analysis::global::open_global_session(&source.label)?;
        let in_session = engine::analysis::materialize::materialize_sample(
            &session,
            &source.label,
            &build_duckdb_sample_sql(source, config),
        )?;
        let landed = engine::analysis::materialize::move_sample_to_process_db(
            &session,
            &in_session,
            &source.label,
        )?;
        // 会话内的那张随会话丢弃（move 已经把数据搬走）；登记表跟 TTL 走
        return Ok(landed);
    }
    match source.conn_id() {
        Some(conn_id) => {
            // 源库路径：Full / RowCount / WHERE 直接拼进发给源库的 SQL；
            // Percent → 先 COUNT 估算行数，再 LIMIT（源库无 reservoir 采样）；
            // Full → 先 COUNT 过护栏（见 check_full_guard），超限拒拉并给取样引导。
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
                crate::model::ProbeAmount::Full => {
                    let count = count_source_rows(conn_id, source).await?;
                    check_full_guard(count)?;
                    build_source_sample_sql(
                        source,
                        config.amount.clone(),
                        config.filter.as_deref(),
                        config.column_subset(),
                    )
                }
                crate::model::ProbeAmount::RowCount(_) => build_source_sample_sql(
                    source,
                    config.amount.clone(),
                    config.filter.as_deref(),
                    config.column_subset(),
                ),
            };
            sample_from_connection(conn_id, &sql, &source.label).await
        }
        None => {
            // 走分析内核的「执行面 + 物化」：今天落 `ProcessDuckDb`（进程内存单例），
            // 与手写 `get_or_create_duckdb()` 完全同一形态，但通道判定收进一处——
            // 将来洞察开联邦取样时，只要这里换 `from_toggle` 的通道即可，
            // 取数口径（ProbeConfig）与登记（`tmp_i_`）都不用动。
            let plan = engine::analysis::mount::plan_for(
                &engine::analysis::channel::AnalysisChannel::ProcessDuckDb,
                &[],
                None,
                &source.label,
            )?;
            let handle = engine::analysis::kernel::open(
                &plan,
                &engine::analysis::kernel::PreparedMounts::none(),
            )?;
            engine::analysis::materialize::materialize_sample(
                &handle,
                "source_sample",
                &build_duckdb_sample_sql(source, config),
            )
        }
    }
}

/// 一组来源的取样：**每源各取一份样本，各出一张表**（多源 / 联邦场景的落点）。
///
/// 口径（已定）：取样是「取数据」，关联是「分析」。所以这里**不做跨源 join**，
/// 也不把多源塞进一条会话——每个源走它自己该走的那条路（源库连接 → 驱动取数；
/// DuckDB 可读 → 进程内存单例），各自建出登记版 `tmp_i_` 表。
///
/// 返回值按**输入顺序**逐项对应：`(label, table)`。中途失败整体失败（前面已经建好的表
/// 留给 TTL 去收——回滚多张已登记表会让错误路径更难懂，而 TTL 本来就是它们的归宿）。
///
/// 通道判定走 [`crate::model::SampleSet::channel`]（`from_toggle` 只升不降）：
/// 开关关 / 单源 / 无 owner 三种情况都仍是今天的行为。
pub async fn sample_set_to_analysis_tables(
    set: &crate::model::SampleSet,
    config: &crate::model::ProbeConfig,
) -> Result<Vec<(String, String)>, CoreError> {
    if set.sources.is_empty() {
        return Err(CoreError::common(CommonError::General(
            "这次分析没有来源：至少要有一个可取样来源".to_string(),
        )));
    }
    // 通道现在判定。联邦档需要**宿主**把连接记录组装成 `FederatedSource`（凭据只在宿主
    // 那一步出现，引擎不解密），所以这条通用入口无法自行建会话：明确指向
    // [`sample_federated_sources`]，而不是悄悄退回逐源——用户打开了开关，就不该看到
    // 他不知道的降级。
    let channel = set.channel();
    if matches!(channel, engine::analysis::channel::AnalysisChannel::Federated { .. }) {
        return Err(CoreError::common(CommonError::General(format!(
            "联邦档取样要先由宿主提供挂载清单（{} 个来源）：请走 sample_federated_sources；\
             这条逐源入口只适用于源库 / 内存库档",
            set.sources.len()
        ))));
    }
    let mut landed = Vec::with_capacity(set.sources.len());
    for source in &set.sources {
        let table = sample_source_to_analysis_table(source, config).await?;
        landed.push((source.label.clone(), table));
    }
    Ok(landed)
}

/// 联邦档取样的一条来源：宿主已组装的源 + 它在会话 SQL 里该用的名字。
///
/// 凭据与连接串只在这一层出现（引擎 `federation::registry` 的既有约定是**引擎不读连接库、
/// 不解密口令**），所以「连接 → 源」这一步永远留在宿主；本模块只负责开会话、取样、搬运。
pub struct FederatedSampleEntry {
    /// 计划里的挂载键（连接源要带别名：别名就是挂上后的 catalog 名）
    pub mount: engine::analysis::mount::MountSpec,
    /// 宿主组装出来的源
    pub source: engine::duckdb::federation::registry::FederatedSource,
    /// 会话内取样 SQL：必须按**别名**限定（`<别名>.<schema>.<表>`）
    pub sql: String,
    /// 人类可读标签（面板副标题 / 返回顺序对应）
    pub label: String,
}

/// 联邦档取样：一条会话里逐源取样，每源各出一张登记版 `tmp_i_`。
///
/// 为什么仍然「每源各取一份」而不是一条跨源 join：取样是取数据，关联是分析（用户口径）。
/// 联邦会话在这里的价值是**少开 N 条源库连接、同一批源一次挂上**，而不是替用户写 join。
///
/// 与会话内取样（加速 / 联邦）的既有口径一致：会话里建 `memory.tmp_i_*`（不登记），
/// 再 `move_sample_to_process_db` 搬进进程单例的登记版 `tmp_i_*`——下游（EDA / 图表 /
/// 导出）读的始终是单例，不需要知道这次取样跑在哪个通道上。
pub async fn sample_federated_sources(
    owner: &str,
    entries: Vec<FederatedSampleEntry>,
    config: &crate::model::ProbeConfig,
) -> Result<Vec<(String, String)>, CoreError> {
    if entries.is_empty() {
        return Err(CoreError::common(CommonError::General(
            "联邦取样没有来源：至少要有一条可挂的来源".to_string(),
        )));
    }
    // 同一个源挂两次 = 一次重复内省 + 一条多余只读副本；而两条源撞别名会让
    // 「SQL 里写哪个名字」变得不可判定——两类都在开会话之前拦掉。
    let mut seen_mounts: Vec<&engine::analysis::mount::MountSpec> = Vec::new();
    let mut seen_aliases: Vec<&str> = Vec::new();
    for entry in &entries {
        if seen_mounts.contains(&&entry.mount) {
            return Err(CoreError::common(CommonError::General(
                "联邦取样里有重复来源：同一个库/连接只能挂一次".to_string(),
            )));
        }
        seen_mounts.push(&entry.mount);
        let alias = entry.source.alias.trim();
        if alias.is_empty() {
            return Err(CoreError::common(CommonError::General(
                "联邦取样里的源缺别名：SQL 无法限定到哪个 catalog".to_string(),
            )));
        }
        if seen_aliases.contains(&alias) {
            return Err(CoreError::common(CommonError::General(format!(
                "联邦取样里别名 {alias} 撞名：两条源挂上去会是同一个 catalog"
            ))));
        }
        seen_aliases.push(alias);
    }

    let channel = engine::analysis::channel::AnalysisChannel::Federated {
        owner: owner.to_string(),
    };
    let mounts: Vec<engine::analysis::mount::MountSpec> =
        entries.iter().map(|e| e.mount.clone()).collect();
    let plan = engine::analysis::mount::plan_for(&channel, &mounts, None, owner)
        .map_err(|e| e)?;
    if plan.mounts.len() != entries.len() {
        return Err(CoreError::common(CommonError::General(format!(
            "计划里的源被去重成 {} 个，宿主给了 {} 个",
            plan.mounts.len(),
            entries.len()
        ))));
    }
    let prepared = engine::analysis::kernel::PreparedMounts::federated(
        entries
            .iter()
            .map(|e| (e.mount.clone(), e.source.clone()))
            .collect(),
    );
    let session = engine::analysis::kernel::open(&plan, &prepared)?;

    let mut landed = Vec::with_capacity(entries.len());
    for entry in &entries {
        // 复用单源那套取数口径（限 / 过滤 / 列投影 / 采样都在这里，不另写一份）
        let shape = SampleSource::on_duckdb(entry.sql.clone(), entry.label.clone());
        let sql = build_duckdb_sample_sql(&shape, config);
        let in_session = engine::analysis::materialize::materialize_sample(
            &session,
            &entry.label,
            &sql,
        )
        .map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "联邦会话里取样「{}」失败：{e}",
                entry.label
            )))
        })?;
        let table = engine::analysis::materialize::move_sample_to_process_db(
            &session,
            &in_session,
            &entry.label,
        )
        .map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "把「{}」搬进分析库失败：{e}",
                entry.label
            )))
        })?;
        landed.push((entry.label.clone(), table));
    }
    Ok(landed)
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
        crate::model::ProbeAmount::Full => {
            format!("SELECT {projection} FROM ({inner}) AS rds_sample")
        }
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
                lineage: None,
                dml_confirmed: false,
            };
            let sql = format!("SELECT * FROM ({inner}) AS rds_cols LIMIT 0");
            let result = service
                .execute(Some(conn_id.to_string()), &sql, opts)
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
        lineage: None,
        dml_confirmed: false,
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
    first.map(|n| n.max(0) as u64).ok_or_else(|| {
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
        lineage: None,
        dml_confirmed: false,
    };
    let result = service
        .execute(Some(conn_id.to_string()), sample_sql, opts)
        .await?;
    // **Arrow 直灌优先**（P1-8b 第一步，2026-11-06）：类型从 Arrow schema 直接映射
    // （时间列保真，不再 JSON 打型成 VARCHAR）；批次缺失 / 空行回退 JSON 打型路径
    // （保列名，K20 已保证空结果也有列）。
    if let Some(batch) = result.result.batches.first() {
        if !batch.schema().fields().is_empty() {
            let duckdb = insight_engine::get_or_create_duckdb()?;
            let conn = duckdb.lock().map_err(|e| {
                CoreError::common(CommonError::General(format!("DuckDB lock error: {e}")))
            })?;
            return engine::duckdb::analysis::create_analysis_temp_table_arrow(
                &conn,
                batch,
                "source_sample",
            );
        }
    }

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
    // 一次令牌 + 一次锁内取齐四段（与 profile_table_view 同一口径）
    let bundle = insight_engine::get_table_eda_bundle(&temp_table)?;
    let mut view = TableProfileView::from_profile(&bundle.profile, table_name);
    if let Some(stats) = bundle.column_summary {
        view.fill_column_stats(&stats);
    }
    view.dup_rows = bundle.dup_rows;
    view.sample = bundle.sample;
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

    /// P1-8a 全量档护栏：低于/等于上限放行，超限拒拉且文案给出**替代路径**
    /// （取样档 / 先过滤）——不做静默截断，截断的「全量」是假全量。
    #[test]
    fn full_guard_rejects_oversized_tables_with_guidance() {
        assert!(check_full_guard(0).is_ok(), "空表放行");
        assert!(
            check_full_guard(SOURCE_FULL_GUARD_ROWS).is_ok(),
            "恰好等于上限放行（边界含）"
        );
        let err = check_full_guard(SOURCE_FULL_GUARD_ROWS + 1).expect_err("超限必须拒拉");
        let msg = err.to_string();
        assert!(msg.contains("百分比"), "要给出取样档引导：{msg}");
        assert!(msg.contains("过滤"), "要给出先过滤引导：{msg}");
        // 错误码前缀由面板层统一剥离（describe_error → strip_error_code），此处不重复断言
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
            sql.starts_with(
                "SELECT \"id\", \"note\" FROM (SELECT * FROM orders) AS rds_sample LIMIT 10"
            ),
            "子集列应投影进 SELECT：{sql}"
        );

        // 全列（默认口径）= SELECT *
        let sql = build_source_sample_sql(&source, ProbeAmount::RowCount(10), None, None);
        assert!(
            sql.starts_with("SELECT * FROM"),
            "无子集应保持 SELECT *：{sql}"
        );
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

    /// 多源取样（口径 = 每源各取一份）：两个来源 → 两张**不同**的登记版 `tmp_i_` 表，
    /// 顺序与标签严格对应输入。
    #[tokio::test]
    async fn a_sample_set_lands_one_registered_table_per_source() {
        let set = crate::model::SampleSet::new(
            "C_insight_multi",
            vec![
                SampleSource::on_duckdb("SELECT 1 AS a", "源一"),
                SampleSource::on_duckdb("SELECT 2 AS b", "源二"),
            ],
        );
        let landed = sample_set_to_analysis_tables(
            &set,
            &config(
                ProbeAmount::RowCount(10),
                ProbeColumns::All,
                None,
                None,
            ),
        )
        .await
        .expect("多源取样");

        assert_eq!(landed.len(), 2, "每源一份，不做合并：{landed:?}");
        assert_eq!(landed[0].0, "源一");
        assert_eq!(landed[1].0, "源二");
        assert!(landed[0].1.starts_with("tmp_i_"), "实际 {}", landed[0].1);
        assert!(landed[1].1.starts_with("tmp_i_"), "实际 {}", landed[1].1);
        assert_ne!(
            landed[0].1, landed[1].1,
            "两次取样必须各自建表（同描述也不撞名）"
        );
    }

    /// 空来源清单是可读错误，不是「静默成功返回空」。
    #[tokio::test]
    async fn an_empty_sample_set_is_rejected() {
        let set = crate::model::SampleSet::new("C_insight_multi", Vec::new());
        let err = sample_set_to_analysis_tables(
            &set,
            &config(ProbeAmount::RowCount(1), ProbeColumns::All, None, None),
        )
        .await
        .expect_err("空来源必须拒");
        assert!(err.to_string().contains("至少要有一个"), "{err}");
    }

    /// 打开多源开关（≥1 源 + 有 owner）→ 通道会升级成 `Federated`；这条**逐源入口**
    /// 必须明确指向 `sample_federated_sources`，不能让用户以为取样降级了。
    #[tokio::test]
    async fn federated_toggle_points_at_the_host_driven_path() {
        let set = crate::model::SampleSet::new(
            "C_insight_multi",
            vec![
                SampleSource::on_duckdb("SELECT 1 AS a", "源一"),
                SampleSource::on_duckdb("SELECT 2 AS b", "源二"),
            ],
        )
        .with_federated(true);
        // 通道判定先钉住：这组输入确实会升级
        assert!(matches!(
            set.channel(),
            engine::analysis::channel::AnalysisChannel::Federated { .. }
        ));
        let err = sample_set_to_analysis_tables(
            &set,
            &config(ProbeAmount::RowCount(1), ProbeColumns::All, None, None),
        )
        .await
        .expect_err("逐源入口不处理联邦档");
        assert!(err.to_string().contains("sample_federated_sources"), "{err}");

        // 单源 + 开关：刀11a（2026-11-27）起**也升级**——`from_toggle` 门槛由 ≥2 放到 ≥1，
        // 「点联邦就挂底座」不再看源数量；与 engine 侧
        // `toggle_on_with_one_source_upgrades_to_single_source_federation` 同口径。
        let single = crate::model::SampleSet::new(
            "C_insight_multi",
            vec![SampleSource::on_duckdb("SELECT 1 AS a", "源一")],
        )
        .with_federated(true);
        assert!(matches!(
            single.channel(),
            engine::analysis::channel::AnalysisChannel::Federated { .. }
        ));
    }
}
