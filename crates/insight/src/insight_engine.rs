use std::collections::HashMap;
use std::sync::OnceLock;

use tokio::sync::Semaphore;

use shared::error::{CommonError, CoreError};
use crate as insight;
use crate::{RuleExecutor, RuleRegistry};
use engine::services::duckdb_service::{duckdb_value_to_json, DuckDbService};

use crate::model::types::{
    ColumnInsightFull, ColumnStats, ColumnStatsDetail, DateTimeStats, DistributionBin, ExtremeValue,
    NumericStats, TableColumnMeta, TableProfile, TextFrequency, TextStats,
};

/// Default number of sample rows pulled from a column for display
pub const DEFAULT_SAMPLE_SIZE: usize = 5;

/// Minimum rows required to generate a histogram
pub const HISTOGRAM_MIN_ROWS: i64 = 10;

/// 洞察分析的并发上限，用于防止资源耗尽。
///
/// DuckDB 单例连接由 `std::sync::Mutex` 全局串行化，本信号量额外提供背压。
/// **刻意采用快速失败**（`try_acquire`）而非排队：同步函数内无法 await，
/// 阻塞等待会把调用线程（可能是 UI 主线程）卡住。超限时调用方收到错误、
/// 由 UI 呈现「正在分析中，请稍候」并允许重试。
/// 批量场景（如「评估全表」）应由调用方串行化，不依赖本上限兜底。
const INSIGHT_MAX_CONCURRENT: usize = 4;

/// 并发受限时的统一错误文案（面向用户，UI 直接展示）。
pub const ERR_TOO_MANY_CONCURRENT: &str = "洞察分析任务过多，请稍候重试";

static INSIGHT_SEM: OnceLock<Semaphore> = OnceLock::new();

fn insight_semaphore() -> &'static Semaphore {
    INSIGHT_SEM.get_or_init(|| Semaphore::new(INSIGHT_MAX_CONCURRENT))
}

/// 获取全局内存 DuckDB 连接。
///
/// 实现为 `DuckDBManager` 的**进程级单例**（`Arc<Mutex<Connection>>`），
/// 不是连接池：所有调用方共享同一条内存连接，访问由 `std::sync::Mutex` 串行化。
/// 因此临时表在同一进程内全局可见——在一条调用链上创建的表，后续调用能直接读到。
pub fn get_or_create_duckdb(
) -> Result<std::sync::Arc<std::sync::Mutex<duckdb::Connection>>, CoreError> {
    DuckDbService::get_or_create_duckdb()
}

/// 列画像全量结果：类型专属统计 + 样本值 + （数值列的）直方图。
///
/// 重量级入口：内部申请并发令牌、取全局 DuckDB 单例连接并持锁，
/// 依次执行多次查询。适合「从零开始分析一列」的调用方。
/// 若调用方**已持有连接**（例：与建临时表同一把锁内），改用
/// [`get_column_insight_full_on`]——两者结果一致，但后者不会重复加锁（`Mutex` 非重入）。
///
/// `registry` 由调用方给出（`crate::registry_for(project_root)` / `crate::with_rules`）：
/// 基础统计本身也是由 TOML 规则（`numeric-stats` / `histogram` 等）驱动的，
/// 而规则分层随项目变化，不能取进程级全局。
pub fn get_column_insight_full(
    registry: &RuleRegistry,
    temp_table: &str,
    column_name: &str,
) -> Result<ColumnInsightFull, CoreError> {
    let _permit = insight_semaphore()
        .try_acquire()
        .map_err(|_| CoreError::common(CommonError::General(ERR_TOO_MANY_CONCURRENT.to_string())))?;

    let duckdb = get_or_create_duckdb()?;
    let conn = duckdb.lock().map_err(|e| {
        CoreError::common(CommonError::General(format!("DuckDB lock error: {}", e)))
    })?;

    get_column_insight_full_on(registry, &conn, temp_table, column_name)
}

/// 在**调用方已持有的连接**上计算列画像全量结果。
///
/// 与 [`get_column_insight_full`] 的唯一区别：不走全局单例、不申请并发令牌、不持锁。
/// 适用于调用方已经持有连接（如与建临时表同一把锁内）的场景——
/// 既避免 `std::sync::Mutex` 重复加锁导致自锁，也避免为同一份工作两次争抢并发额度。
pub fn get_column_insight_full_on(
    registry: &RuleRegistry,
    conn: &duckdb::Connection,
    temp_table: &str,
    column_name: &str,
) -> Result<ColumnInsightFull, CoreError> {
    let stats = get_column_stats_internal(registry, conn, temp_table, column_name)?;
    let sample = get_column_sample_internal(conn, temp_table, column_name)?;

    let histogram = match &stats.stats_detail {
        ColumnStatsDetail::Numeric(_) => {
            get_column_histogram_internal(registry, conn, temp_table, column_name).ok()
        }
        _ => None,
    };

    Ok(ColumnInsightFull {
        stats,
        sample,
        histogram,
    })
}

/// 临时表画像（表探查（Tab「表」）的数据源）：DuckDB 内省列元数据 + 行数。
///
/// 面板的表目标指向的是 **DuckDB 临时表**（结果集表 / `tmp_i_` 样本表）——
/// 源库表先取样再探查（D58/D59），所以这里既没有也不需要源库连接信息。
///
/// 走 `DESCRIBE` 而不是 `information_schema`：前者对临时表 / 视图 / CTE 一视同仁，
/// 也不需要自己按 `table_catalog = 'memory'` 过滤（`ATTACH` 进来的文件库表不该混进来）。
pub fn get_temp_table_profile(temp_table: &str) -> Result<TableProfile, CoreError> {
    let _permit = insight_semaphore()
        .try_acquire()
        .map_err(|_| CoreError::common(CommonError::General(ERR_TOO_MANY_CONCURRENT.to_string())))?;

    let duckdb = get_or_create_duckdb()?;
    let conn = duckdb.lock().map_err(|e| {
        CoreError::common(CommonError::General(format!("DuckDB lock error: {}", e)))
    })?;
    get_temp_table_profile_on(&conn, temp_table)
}

/// 在**调用方已持有的连接**上内省临时表（同 `*_on` 系列的口径）。
///
/// `DESCRIBE` 的输出列有六个且顺序稳定：`column_name` / `column_type` / `null` / `key` /
/// `default` / `extra`。这里只取前四个，用**下标**而不是列名定位——`Statement::column_names()`
/// 在查询未执行时会 panic，而 `DESCRIBE` 的列定义是 DuckDB 的稳定契约。
pub fn get_temp_table_profile_on(
    conn: &duckdb::Connection,
    temp_table: &str,
) -> Result<TableProfile, CoreError> {
    let sql = format!("DESCRIBE \"{}\"", temp_table);
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "DuckDB describe prepare failed: {}",
            e
        )))
    })?;

    let columns: Vec<TableColumnMeta> = stmt
        .query_map([], |row| {
            let name: String = row.get(0)?;
            let data_type: String = row.get(1)?;
            // `null` 列给的是 "YES" / "NO"
            let nullable: String = row.get::<_, Option<String>>(2)?.unwrap_or_default();
            let key: String = row.get::<_, Option<String>>(3)?.unwrap_or_default();
            Ok((name, data_type, nullable, key))
        })
        .map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "DuckDB describe query failed: {}",
                e
            )))
        })?
        .filter_map(|row| row.ok())
        .enumerate()
        .map(|(ix, (name, data_type, nullable, key))| TableColumnMeta {
            column_name: name,
            data_type,
            is_nullable: nullable.eq_ignore_ascii_case("YES"),
            // 临时表多半由查询结果建出，没有主键约束；有则如实标出
            is_primary_key: key.eq_ignore_ascii_case("PRI"),
            ordinal_position: ix as i32 + 1,
        })
        .collect();

    // 行数失败不算致命：列清单仍可看，只是没行数（与源库内省同一取舍）
    let row_count: i64 = conn
        .query_row(&format!("SELECT COUNT(*) FROM \"{}\"", temp_table), [], |row| {
            row.get(0)
        })
        .unwrap_or(0);

    Ok(TableProfile {
        table_name: temp_table.to_string(),
        db_type: "DuckDB".to_string(),
        columns,
        row_count: Some(row_count.clamp(0, i32::MAX as i64) as i32),
        schema_name: None,
    })
}

/// EDA 列统计摘要：对临时表**所有列一次查询**拿空值率与唯一值数。
///
/// 拼一条 `SELECT`（每列两个表达式：`AVG(col IS NULL)` 与 `COUNT(DISTINCT col)`），
/// 只跑一次 DuckDB 查询——业务表 200+ 列也不该逐列往返。
/// 唯一率 = 唯一值数 / 非空行数（`COUNT(DISTINCT)` 天然不算 NULL）；
/// 行数为 0 或统计本身不可用（如空表）返回 `None`，调用方不编假值。
pub fn compute_table_column_summary_on(
    conn: &duckdb::Connection,
    temp_table: &str,
    columns: &[String],
) -> Result<Vec<ColumnStatBrief>, CoreError> {
    let mut select_parts = Vec::with_capacity(columns.len() * 2 + 1);
    select_parts.push("COUNT(*) AS \"__rows\"".to_string());
    for (ix, column) in columns.iter().enumerate() {
        // 双引号内的引号翻倍转义，防列名注入 / 含引号列名
        let quoted = column.replace('"', "\"\"");
        select_parts.push(format!(
            "AVG(CAST(\"{quoted}\" IS NULL AS INTEGER)) AS \"__n{ix}\""
        ));
        select_parts.push(format!(
            "COUNT(DISTINCT \"{quoted}\") AS \"__u{ix}\""
        ));
    }
    let sql = format!(
        "SELECT {} FROM \"{}\"",
        select_parts.join(", "),
        temp_table.replace('"', "\"\"")
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "DuckDB column summary prepare failed: {}",
            e
        )))
    })?;

    let mut rows = stmt.query([]).map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "DuckDB column summary query failed: {}",
            e
        )))
    })?;
    let mut result = Vec::with_capacity(columns.len());
    if let Some(row) = rows.next().map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "DuckDB column summary row failed: {}",
            e
        )))
    })? {
        let total: i64 = row.get::<_, Option<i64>>(0)?.unwrap_or(0);
        for (ix, column) in columns.iter().enumerate() {
            let null_rate = row.get::<_, Option<f64>>(ix * 2 + 1)?;
            let unique: i64 = row.get::<_, Option<i64>>(ix * 2 + 2)?.unwrap_or(0);
            let unique_rate = if total > 0 {
                // 唯一率 = 唯一值数 / 非空行数（行数为 0 时无意义 → None）
                Some((unique as f64) / (total as f64).max(1.0))
            } else {
                None
            };
            result.push(ColumnStatBrief {
                name: column.clone(),
                null_rate,
                unique_rate,
            });
        }
    }
    Ok(result)
}

/// 带并发控制与全局连接的 EDA 列统计摘要入口（调用方拿不到连接时用）。
pub fn compute_table_column_summary(
    temp_table: &str,
    columns: &[String],
) -> Result<Vec<ColumnStatBrief>, CoreError> {
    let _permit = insight_semaphore()
        .try_acquire()
        .map_err(|_| CoreError::common(CommonError::General(ERR_TOO_MANY_CONCURRENT.to_string())))?;

    let duckdb = get_or_create_duckdb()?;
    let conn = duckdb.lock().map_err(|e| {
        CoreError::common(CommonError::General(format!("DuckDB lock error: {}", e)))
    })?;
    compute_table_column_summary_on(&conn, temp_table, columns)
}

/// 临时表样本（EDA 样本区）：`SELECT * LIMIT 5` 的轻查询，文本化前 5 行。
///
/// 与画像分开（画像不依赖样本，样本失败不阻塞画像）：只给探索用的一瞥。
/// 值经 `ValueRef` 手动文本化（`duckdb::types::Value` 没有 `Display`）；
/// 空值显示为 `NULL` 字样，未覆盖类型兜底用 Debug 表示（不编假值）。
pub fn get_temp_table_sample(temp_table: &str) -> Result<crate::model::TableSample, CoreError> {
    let _permit = insight_semaphore()
        .try_acquire()
        .map_err(|_| CoreError::common(CommonError::General(ERR_TOO_MANY_CONCURRENT.to_string())))?;

    let duckdb = get_or_create_duckdb()?;
    let conn = duckdb.lock().map_err(|e| {
        CoreError::common(CommonError::General(format!("DuckDB lock error: {}", e)))
    })?;
    get_temp_table_sample_on(&conn, temp_table)
}

/// 在**调用方已持有的连接**上取样本（同 `*_on` 系列的口径）。
pub fn get_temp_table_sample_on(
    conn: &duckdb::Connection,
    temp_table: &str,
) -> Result<crate::model::TableSample, CoreError> {
    use crate::model::TableSample;
    let sql = format!("SELECT * FROM \"{}\" LIMIT 5", temp_table.replace('"', "\"\""));
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "DuckDB sample prepare failed: {}",
            e
        )))
    })?;
    let mut rows = Vec::new();
    let mut q = stmt.query([]).map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "DuckDB sample query failed: {}",
            e
        )))
    })?;
    // 列名在查询执行后才可读（duckdb crate 的 `column_names()` 要求已 execute，
    // 否则断言 "The statement was not executed yet"）；`q` 内部持有执行后的
    // `&Statement`，走 `as_ref()` 读 schema，避免与 `stmt` 的可变借用冲突
    let columns: Vec<String> = q
        .as_ref()
        .map(|s| s.column_names().iter().map(|s| s.to_string()).collect())
        .unwrap_or_default();
    while let Some(row) = q.next().map_err(|e| {
        CoreError::common(CommonError::General(format!("DuckDB sample row failed: {}", e)))
    })? {
        let mut cells = Vec::with_capacity(columns.len());
        for ix in 0..columns.len() {
            let value = row.get_ref(ix).map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "DuckDB sample cell failed: {}",
                    e
                )))
            })?;
            cells.push(sample_cell_text(&value));
        }
        rows.push(cells);
    }
    Ok(TableSample { columns, rows })
}

/// 样本单元格文本化（`NULL` 显示为字样，未覆盖类型兜底 Debug——不编假值）。
///
/// DuckDB 的 `ValueRef` 按逻辑类型细分整型变体（Tiny/Small/Int/Big/Huge 与各无符号），
/// 归并成字符串即可；时间 / Interval / Decimal 等走 Debug 兜底（仍有可读输出）。
fn sample_cell_text(value: &duckdb::types::ValueRef) -> String {
    use duckdb::types::ValueRef;
    match value {
        ValueRef::Null => "NULL".into(),
        ValueRef::Boolean(b) => b.to_string(),
        ValueRef::TinyInt(i) => i.to_string(),
        ValueRef::SmallInt(i) => i.to_string(),
        ValueRef::Int(i) => i.to_string(),
        ValueRef::BigInt(i) => i.to_string(),
        ValueRef::HugeInt(i) => i.to_string(),
        ValueRef::UTinyInt(i) => i.to_string(),
        ValueRef::USmallInt(i) => i.to_string(),
        ValueRef::UInt(i) => i.to_string(),
        ValueRef::UBigInt(i) => i.to_string(),
        ValueRef::UHugeInt(i) => i.to_string(),
        ValueRef::Float(f) => f.to_string(),
        ValueRef::Double(f) => f.to_string(),
        ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
        ValueRef::Blob(b) => format!("<blob {} 字节>", b.len()),
        other => format!("{other:?}"),
    }
}

/// 列基础统计（行数 / 空值 / 唯一值 + 类型专属详情），不含样本与直方图。
///
/// 比 [`get_column_insight_full`] 轻：适合只需概览的调用方（如导航树「查看统计」）。
pub fn get_column_insights(
    registry: &RuleRegistry,
    temp_table: &str,
    column_name: &str,
) -> Result<ColumnStats, CoreError> {
    let _permit = insight_semaphore()
        .try_acquire()
        .map_err(|_| CoreError::common(CommonError::General(ERR_TOO_MANY_CONCURRENT.to_string())))?;

    let duckdb = get_or_create_duckdb()?;
    let conn = duckdb.lock().map_err(|e| {
        CoreError::common(CommonError::General(format!("DuckDB lock error: {}", e)))
    })?;
    get_column_stats_internal(registry, &conn, temp_table, column_name)
}

/// 列基础统计（行数 / 空值 / 唯一值 + 类型专属详情）。
///
/// **内部接缝**：只吃显式连接与注册表，不做并发控制与全局取连接。
/// 公开以便调用方在已持锁的场景（及单测）直接使用，不绕全局单例。
pub fn get_column_stats_internal(
    registry: &RuleRegistry,
    conn: &duckdb::Connection,
    temp_table: &str,
    column_name: &str,
) -> Result<ColumnStats, CoreError> {
    let count_sql = format!(
        "SELECT COUNT(*), COUNT(\"{}\"), COUNT(DISTINCT \"{}\") FROM \"{}\"",
        column_name, column_name, temp_table
    );
    let (total, non_null, unique_cnt): (i64, i64, i64) = conn
        .query_row(&count_sql, [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "DuckDB count failed for '{}': {}",
                column_name, e
            )))
        })?;

    let null_count = (total - non_null) as usize;
    let null_rate = if total > 0 {
        null_count as f64 / total as f64
    } else {
        0.0
    };

    let type_sql = format!(
        "SELECT typeof(\"{}\") FROM \"{}\" WHERE \"{}\" IS NOT NULL LIMIT 1",
        column_name, temp_table, column_name
    );
    let data_type: String = conn
        .query_row(&type_sql, [], |row| row.get(0))
        .unwrap_or_else(|_| "VARCHAR".to_string());

    let dt_lower = data_type.to_lowercase();

    let stats_detail = if non_null == 0 {
        ColumnStatsDetail::Unknown
    } else if is_numeric_type(&dt_lower) {
        compute_numeric_stats(registry, conn, temp_table, column_name)?
    } else if is_datetime_type(&dt_lower) {
        compute_datetime_stats(registry, conn, temp_table, column_name)?
    } else if dt_lower == "boolean" || dt_lower == "bool" {
        compute_boolean_stats(registry, conn, temp_table, column_name)?
    } else if is_binary_type(&dt_lower) || is_array_type(&dt_lower) {
        ColumnStatsDetail::Unknown
    } else {
        compute_text_stats(registry, conn, temp_table, column_name)?
    };

    Ok(ColumnStats {
        column_name: column_name.to_string(),
        data_type,
        total_count: total as u32,
        null_count: null_count as u32,
        null_rate,
        unique_count: Some(unique_cnt as u32),
        stats_detail,
    })
}

/// 数值列极端值检测（右上界启发式）。
///
/// 判定条件：标准差为正，且极差既大于 10 倍标准差、又大于 1000——
/// 两者同时成立才算，避免小量级数据的轻微离群被报成异常。
///
/// 归属变更（M8 Phase 0 / 0.2）：本函数自 `engine/services/duckdb_service.rs` 迁入。
/// 它实现的是**洞察的业务启发式**（阈值属领域知识），且返回洞察领域类型，
/// 住在 engine 会造成方向倒置（低层持有上层的业务规则与词汇）。
pub fn detect_extremes(min: f64, max: f64, stddev: f64) -> Vec<ExtremeValue> {
    let mut results = Vec::new();
    if stddev > 0.0 && max > 0.0 {
        let range = max - min;
        if range > 10.0 * stddev && range > 1000.0 {
            results.push(ExtremeValue {
                value: max,
                kind: "outlier_high".to_string(),
            });
        }
    }
    results
}

/// 数值列统计的细节（平均值 / 中位数 / 百分位 / 标准差 / 偏度 / 极端值）。
fn compute_numeric_stats(
    registry: &RuleRegistry,
    conn: &duckdb::Connection,
    temp_table: &str,
    column_name: &str,
) -> Result<ColumnStatsDetail, CoreError> {
    let mut params = HashMap::new();
    params.insert("table".to_string(), temp_table.to_string());
    params.insert("col".to_string(), column_name.to_string());

    let rule = registry.get("numeric-stats").ok_or_else(|| {
        CoreError::common(CommonError::General(
            "Rule 'numeric-stats' not found".to_string(),
        ))
    })?;

    match RuleExecutor::execute(rule, conn, &params) {
        Ok(serde_json::Value::Object(map)) => {
            let extract =
                |key: &str| -> f64 { map.get(key).and_then(|v| v.as_f64()).unwrap_or(0.0) };
            let extract_opt = |key: &str| -> Option<f64> { map.get(key).and_then(|v| v.as_f64()) };
            let min_v = extract("min");
            let max_v = extract("max");
            let stddev_v = extract_opt("stddev");
            let is_extreme = detect_extremes(min_v, max_v, stddev_v.unwrap_or(0.0));

            Ok(ColumnStatsDetail::Numeric(NumericStats {
                min: min_v,
                max: max_v,
                avg: extract("avg"),
                median: extract("median"),
                p25: extract("p25"),
                p75: extract("p75"),
                sum: extract("sum"),
                stddev: stddev_v,
                skewness: extract_opt("skewness"),
                kurtosis: extract_opt("kurtosis"),
                is_extreme,
            }))
        }
        Ok(_) => Err(CoreError::common(CommonError::General(
            "numeric-stats rule returned unexpected result type".to_string(),
        ))),
        Err(e) => {
            let basic_rule = registry.get("numeric-basic").ok_or_else(|| {
                CoreError::common(CommonError::General(
                    "Rule 'numeric-basic' not found".to_string(),
                ))
            })?;
            match RuleExecutor::execute(basic_rule, conn, &params) {
                Ok(serde_json::Value::Object(map)) => {
                    let extract =
                        |key: &str| -> f64 { map.get(key).and_then(|v| v.as_f64()).unwrap_or(0.0) };
                    Ok(ColumnStatsDetail::Numeric(NumericStats {
                        min: extract("min"),
                        max: extract("max"),
                        avg: extract("avg"),
                        median: extract("median"),
                        p25: 0.0,
                        p75: 0.0,
                        sum: extract("sum"),
                        stddev: map.get("stddev").and_then(|v| v.as_f64()),
                        skewness: None,
                        kurtosis: None,
                        is_extreme: vec![],
                    }))
                }
                Ok(_) => Err(CoreError::common(CommonError::General(
                    "numeric-basic rule returned unexpected result type".to_string(),
                ))),
                Err(e2) => Err(CoreError::common(CommonError::General(format!(
                    "DuckDB numeric stats failed: {} / fallback: {}",
                    e, e2
                )))),
            }
        }
    }
}

fn compute_text_stats(
    registry: &RuleRegistry,
    conn: &duckdb::Connection,
    temp_table: &str,
    column_name: &str,
) -> Result<ColumnStatsDetail, CoreError> {
    let mut params = HashMap::new();
    params.insert("table".to_string(), temp_table.to_string());
    params.insert("col".to_string(), column_name.to_string());

    let freq_rule = registry.get("text-frequency").ok_or_else(|| {
        CoreError::common(CommonError::General(
            "Rule 'text-frequency' not found".to_string(),
        ))
    })?;

    let top_values: Vec<TextFrequency> = match RuleExecutor::execute(freq_rule, conn, &params) {
        Ok(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|item| {
                let obj = item.as_object()?;
                Some(TextFrequency {
                    value: obj.get("value")?.as_str()?.to_string(),
                    count: obj.get("count")?.as_u64()? as u32,
                    ratio: obj.get("ratio")?.as_f64()?,
                })
            })
            .collect(),
        _ => vec![],
    };

    let len_rule = registry.get("text-length").ok_or_else(|| {
        CoreError::common(CommonError::General(
            "Rule 'text-length' not found".to_string(),
        ))
    })?;
    let (min_len, max_len): (u32, u32) = match RuleExecutor::execute(len_rule, conn, &params) {
        Ok(serde_json::Value::Object(map)) => (
            map.get("min_length").and_then(|v| v.as_i64()).unwrap_or(0) as u32,
            map.get("max_length").and_then(|v| v.as_i64()).unwrap_or(0) as u32,
        ),
        _ => (0, 0),
    };

    Ok(ColumnStatsDetail::Text(TextStats {
        min_length: min_len,
        max_length: max_len,
        top_values,
    }))
}

fn compute_datetime_stats(
    registry: &RuleRegistry,
    conn: &duckdb::Connection,
    temp_table: &str,
    column_name: &str,
) -> Result<ColumnStatsDetail, CoreError> {
    let mut params = HashMap::new();
    params.insert("table".to_string(), temp_table.to_string());
    params.insert("col".to_string(), column_name.to_string());

    let range_rule = registry.get("datetime-range").ok_or_else(|| {
        CoreError::common(CommonError::General(
            "Rule 'datetime-range' not found".to_string(),
        ))
    })?;

    let (earliest, latest, span) = match RuleExecutor::execute(range_rule, conn, &params) {
        Ok(serde_json::Value::Object(map)) => (
            map.get("earliest")
                .and_then(|v| v.as_str())
                .unwrap_or("N/A")
                .to_string(),
            map.get("latest")
                .and_then(|v| v.as_str())
                .unwrap_or("N/A")
                .to_string(),
            map.get("span_days").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
        ),
        _ => ("N/A".to_string(), "N/A".to_string(), 0),
    };

    let monthly_rule = registry.get("datetime-monthly").ok_or_else(|| {
        CoreError::common(CommonError::General(
            "Rule 'datetime-monthly' not found".to_string(),
        ))
    })?;
    let monthly_distribution: Vec<TextFrequency> =
        match RuleExecutor::execute(monthly_rule, conn, &params) {
            Ok(serde_json::Value::Array(arr)) => arr
                .iter()
                .filter_map(|item| {
                    let obj = item.as_object()?;
                    Some(TextFrequency {
                        value: obj.get("value")?.as_str()?.to_string(),
                        count: obj.get("count")?.as_u64()? as u32,
                        ratio: obj.get("ratio")?.as_f64()?,
                    })
                })
                .collect(),
            _ => vec![],
        };

    Ok(ColumnStatsDetail::DateTime(DateTimeStats {
        earliest,
        latest,
        span_days: span,
        monthly_distribution,
    }))
}

fn compute_boolean_stats(
    registry: &RuleRegistry,
    conn: &duckdb::Connection,
    temp_table: &str,
    column_name: &str,
) -> Result<ColumnStatsDetail, CoreError> {
    let mut params = HashMap::new();
    params.insert("table".to_string(), temp_table.to_string());
    params.insert("col".to_string(), column_name.to_string());

    let rule = registry.get("boolean-ratio").ok_or_else(|| {
        CoreError::common(CommonError::General(
            "Rule 'boolean-ratio' not found".to_string(),
        ))
    })?;

    match RuleExecutor::execute(rule, conn, &params) {
        Ok(serde_json::Value::Object(map)) => {
            let true_count = map.get("true_count").and_then(|v| v.as_i64()).unwrap_or(0) as u32;
            let false_count = map.get("false_count").and_then(|v| v.as_i64()).unwrap_or(0) as u32;
            let total = true_count + false_count;
            let true_ratio = if total > 0 {
                true_count as f64 / total as f64
            } else {
                0.0
            };
            Ok(ColumnStatsDetail::Boolean(
                crate::model::types::BooleanStats {
                    true_count,
                    false_count,
                    true_ratio,
                },
            ))
        }
        Ok(_) => Err(CoreError::common(CommonError::General(
            "boolean-ratio rule returned unexpected result type".to_string(),
        ))),
        Err(e) => Err(CoreError::common(CommonError::General(format!(
            "DuckDB boolean stats failed: {}",
            e
        )))),
    }
}

// ==================== 类型族判定 ====================
//
// 已迁至 `crate::model`（唯一来源）：列画像与表探查都要用它，
// 放在下层（领域词汇）后依赖方向才单一。此处仅导入使用。
use crate::model::{is_array_type, is_binary_type, is_datetime_type, is_numeric_type, ColumnStatBrief};

/// 列样本值（条数取 [`DEFAULT_SAMPLE_SIZE`]）。
///
/// **内部接缝**：只吃显式连接（同 [`get_column_stats_internal`]）。
pub fn get_column_sample_internal(
    conn: &duckdb::Connection,
    temp_table: &str,
    column_name: &str,
) -> Result<Vec<serde_json::Value>, CoreError> {
    let sql = format!(
        "SELECT \"{}\" FROM \"{}\" LIMIT {}",
        column_name, temp_table, DEFAULT_SAMPLE_SIZE
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "DuckDB sample prepare failed: {}",
            e
        )))
    })?;
    let samples: Vec<serde_json::Value> = stmt
        .query_map([], |row| {
            let v: duckdb::types::Value = row.get(0)?;
            Ok(duckdb_value_to_json(&v))
        })
        .map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "DuckDB sample query failed: {}",
                e
            )))
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(samples)
}

/// 数值列直方图分桶（行数低于 [`HISTOGRAM_MIN_ROWS`] 时返回空）。
///
/// **内部接缝**：只吃显式连接（同 [`get_column_stats_internal`]）。
pub fn get_column_histogram_internal(
    registry: &RuleRegistry,
    conn: &duckdb::Connection,
    temp_table: &str,
    column_name: &str,
) -> Result<Vec<DistributionBin>, CoreError> {
    let count_sql = format!(
        "SELECT COUNT(*) FROM \"{}\" WHERE \"{}\" IS NOT NULL",
        temp_table, column_name
    );
    let count: i64 = conn
        .query_row(&count_sql, [], |row| row.get(0))
        .unwrap_or(0);

    if count < HISTOGRAM_MIN_ROWS {
        return Ok(vec![]);
    }

    let mut params = HashMap::new();
    params.insert("table".to_string(), temp_table.to_string());
    params.insert("col".to_string(), column_name.to_string());

    let rule = registry.get("histogram").ok_or_else(|| {
        CoreError::common(CommonError::General(
            "Rule 'histogram' not found".to_string(),
        ))
    })?;

    match RuleExecutor::execute(rule, conn, &params) {
        Ok(serde_json::Value::Array(arr)) => {
            let bins: Vec<DistributionBin> = arr
                .iter()
                .filter_map(|item| {
                    let obj = item.as_object()?;
                    Some(DistributionBin {
                        label: obj.get("label")?.as_str()?.to_string(),
                        count: obj.get("count")?.as_u64()? as u32,
                        ratio: obj.get("ratio")?.as_f64()?,
                    })
                })
                .collect();
            Ok(bins)
        }
        _ => Ok(vec![]),
    }
}

/// 执行指定 id 的规则（如 `numeric-stats` / `histogram`），返回带列元数据的
/// [`insight::ExecutionResult`]（含 QualityRule 质量门控结果）。
///
/// 调用方需已持有 DuckDB 锁；规则由调用方传入（规则分层随项目变化，见
/// [`crate::registry_for`]）。
pub fn execute_insight_rule(
    registry: &RuleRegistry,
    rule_id: &str,
    conn: &duckdb::Connection,
    params: &HashMap<String, String>,
) -> Result<insight::ExecutionResult, CoreError> {
    let _permit = insight_semaphore()
        .try_acquire()
        .map_err(|_| CoreError::common(CommonError::General(ERR_TOO_MANY_CONCURRENT.to_string())))?;

    let rule = registry.get(rule_id).ok_or_else(|| {
        CoreError::common(CommonError::General(format!(
            "Rule '{}' not found",
            rule_id
        )))
    })?;
    RuleExecutor::execute_qualified(rule, conn, params)
}

/// 列出注册的规则（可按分类过滤，如 `column` / `multi` / `table` / `quality`）。
///
/// 返回面向前端的 JSON（含 `scope` 与 `source_path`，便于界面按作用域分组展示）。
pub fn list_insight_rules(
    registry: &RuleRegistry,
    category: Option<&str>,
) -> Result<Vec<serde_json::Value>, CoreError> {
    let rules: Vec<&crate::RuleFile> = match category {
        Some(cat) => registry.list_by_category(cat),
        None => registry.all_rules(),
    };
    let result: Vec<serde_json::Value> = rules
        .iter()
        .map(|r| {
            let source = registry.source_of(&r.meta.id);
            serde_json::json!({
                "id": r.meta.id,
                "name": r.meta.name,
                "description": r.meta.description,
                "version": r.meta.version,
                "category": r.meta.category,
                "applies_to": r.meta.applies_to,
                "builtin": r.meta.builtin,
                "parameters": r.query.parameters,
                "result_type": r.query.result_type,
                // 实际生效的来源与文件路径（被覆盖后指向胜出的那一层）
                "scope": source.map(|s| s.scope.label()).unwrap_or("未知"),
                "scope_path": source.map(|s| s.path.as_str()).unwrap_or(""),
            })
        })
        .collect();
    Ok(result)
}

/// 列出适用于指定列类型的规则（如 `Numeric` / `Text` / `Any`）。
pub fn list_rules_for_column(
    registry: &RuleRegistry,
    column_type: &str,
) -> Result<Vec<serde_json::Value>, CoreError> {
    let result: Vec<serde_json::Value> = registry
        .rules_for_column_type(column_type)
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.meta.id,
                "name": r.meta.name,
                "category": r.meta.category,
                "applies_to": r.meta.applies_to,
                "parameters": r.query.parameters,
            })
        })
        .collect();
    Ok(result)
}

// ==================== 跨表关系排查（T19） ====================

/// 读一张临时表的列名 → 类型映射（`information_schema`，仅内存库 main schema）。
///
/// 为什么限定 `table_catalog = 'memory'`：`ATTACH` 进来的文件库表不是洞察对象，
/// 列清单混进来只会让同名列比对失真（临时表 vs 文件库同名同型表，值域不相关）。
fn temp_table_column_types(
    conn: &duckdb::Connection,
    table: &str,
) -> Result<HashMap<String, String>, CoreError> {
    let sql = format!(
        "SELECT column_name, data_type FROM information_schema.columns \
         WHERE table_catalog IN ('memory', 'temp') AND table_schema = 'main' AND table_name = ? \
         ORDER BY ordinal_position"
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        CoreError::common(CommonError::General(format!("DuckDB error: {e}")))
    })?;
    let mut rows = stmt.query(duckdb::params![table]).map_err(|e| {
        CoreError::common(CommonError::General(format!("DuckDB error: {e}")))
    })?;
    let mut map = HashMap::new();
    while let Some(row) = rows.next().map_err(|e| {
        CoreError::common(CommonError::General(format!("DuckDB error: {e}")))
    })? {
        let name: String = row.get(0).unwrap_or_default();
        let ty: String = row.get(1).unwrap_or_default();
        map.insert(name, ty);
    }
    Ok(map)
}

/// 类型是否属于同一**比较族**（数值 / 文本 / 日期 / 布尔）。
///
/// 值重叠只对同族列有意义：`id INTEGER` 与 `id VARCHAR` 值域类型不同，
/// `INTERSECT` 虽能跑但比对的是不同语义，报出来只是噪音。
fn same_type_family(a: &str, b: &str) -> bool {
    let family = |t: &str| -> u8 {
        let t = t.to_ascii_lowercase();
        if is_numeric_type(&t) {
            1
        } else if is_datetime_type(&t) {
            2
        } else if t.starts_with("bool") {
            3
        } else if t.starts_with("varchar") || t.starts_with("text") || t == "string" || t.contains("char") {
            4
        } else {
            0
        }
    };
    family(a) != 0 && family(a) == family(b)
}

/// 两张洞察临时表的**同名列值重叠**报告（T19 关系排查引擎）。
///
/// 口径：对每个类型兼容的同名列，取两表各自去重值域的交集，
/// 报交集数 + 占双方值域的比例。`INTERSECT` 在 DuckDB 对 NULL 的处理是
/// 「NULL 与 NULL 相同」——但两表同列都是同一来源的样本时，NULL 重叠
/// 不携带「关系」信息，这里不把 NULL 算进值域（`DISTINCT` 已含 NULL，
/// 需要时再按列排除）。
///
/// 阻塞（内部加全局 DuckDB 锁），调用方放后台线程。
pub fn compute_column_overlap(
    conn: &duckdb::Connection,
    table_a: &str,
    table_b: &str,
) -> Result<Vec<crate::model::OverlapRow>, CoreError> {
    let cols_a = temp_table_column_types(conn, table_a)?;
    let cols_b = temp_table_column_types(conn, table_b)?;

    let mut rows = Vec::new();
    for (name, ty_a) in &cols_a {
        let Some(ty_b) = cols_b.get(name) else {
            continue;
        };
        if !same_type_family(ty_a, ty_b) {
            continue;
        }
        // 三个标量子查询一次查：避免三次往返（DuckDB 单连接串行，往返是主要开销）。
        let sql = format!(
            "SELECT \
               (SELECT COUNT(DISTINCT \"{c}\") FROM \"{a}\"), \
               (SELECT COUNT(DISTINCT \"{c}\") FROM \"{b}\"), \
               (SELECT COUNT(*) FROM (SELECT DISTINCT \"{c}\" FROM \"{a}\" INTERSECT SELECT DISTINCT \"{c}\" FROM \"{b}\"))",
            c = name,
            a = table_a,
            b = table_b
        );
        let (rate_a, rate_b, inter): (i64, i64, i64) = conn
            .query_row(&sql, [], |row| {
                Ok((
                    row.get::<_, i64>(0).unwrap_or(0),
                    row.get::<_, i64>(1).unwrap_or(0),
                    row.get::<_, i64>(2).unwrap_or(0),
                ))
            })
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("DuckDB error: {e}")))
            })?;
        if inter == 0 {
            continue; // 无重叠的列不进结果（值域不同是常态，不刷屏）
        }
        rows.push(crate::model::OverlapRow {
            column: name.clone(),
            overlap: inter,
            rate_a: inter as f64 / rate_a.max(1) as f64,
            rate_b: inter as f64 / rate_b.max(1) as f64,
        });
    }

    rows.sort_by(|x, y| y.overlap.cmp(&x.overlap));
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quality_scorer;

    fn duckdb_err(e: duckdb::Error) -> CoreError {
        CoreError::common(CommonError::General(e.to_string()))
    }

    /// 测试用规则集：只加载内置层，结果与机器上的用户目录无关，因此可断言。
    ///
    /// 基础统计本身由 TOML 规则驱动（如 `numeric-stats` / `histogram`），
    /// 所以这些单测必须显式给注册表——不能再依赖进程级全局单例。
    fn test_registry() -> RuleRegistry {
        crate::builtin_registry()
    }

    fn setup_test_table(
        conn: &duckdb::Connection,
    ) -> Result<(&'static str, &'static str), CoreError> {
        let table = "rs_test_insight";
        let col = "amount";
        conn.execute_batch(&format!(
            "CREATE TABLE \"{}\" (\"{}\" DOUBLE, name VARCHAR, created_date DATE, is_active BOOLEAN)",
            table, col
        ))
        .map_err(duckdb_err)?;
        conn.execute_batch(&format!(
            "INSERT INTO \"{}\" VALUES \
             (100.0, 'Alice', '2025-01-15', true), \
             (200.0, 'Bob', '2025-02-20', true), \
             (300.0, 'Charlie', '2025-03-25', false), \
             (400.0, 'Diana', '2025-04-10', true), \
             (500.0, 'Eve', '2025-05-05', true), \
             (null, 'Frank', null, null), \
             (150.0, 'Grace', '2025-01-30', false), \
             (250.0, 'Heidi', '2025-06-15', true), \
             (350.0, 'Ivan', '2025-07-20', false), \
             (450.0, 'Judy', '2025-08-25', true)",
            table
        ))
        .map_err(duckdb_err)?;

        Ok((table, col))
    }

    #[test]
    fn test_get_column_stats_numeric() -> Result<(), CoreError> {
        let registry = test_registry();
        let conn = duckdb::Connection::open_in_memory().map_err(duckdb_err)?;
        let (table, col) = setup_test_table(&conn)?;

        let stats = get_column_stats_internal(&registry, &conn, table, col)?;

        assert_eq!(stats.column_name, col);
        assert_eq!(stats.total_count, 10);
        assert_eq!(stats.null_count, 1);
        assert!((stats.null_rate - 0.1).abs() < 0.001);
        assert_eq!(stats.unique_count, Some(9));

        match &stats.stats_detail {
            ColumnStatsDetail::Numeric(n) => {
                assert!(n.min > 0.0);
                assert!(n.max > n.min);
                assert!(n.avg > n.min && n.avg < n.max);
                Ok(())
            }
            other => panic!(
                "Expected Numeric stats, got: {:?}",
                std::mem::discriminant(other)
            ),
        }
    }

    #[test]
    fn test_get_column_stats_all_null() -> Result<(), CoreError> {
        let conn = duckdb::Connection::open_in_memory().map_err(duckdb_err)?;
        conn.execute_batch("CREATE TABLE \"rs_null\" (\"col\" DOUBLE)")
            .map_err(duckdb_err)?;
        conn.execute_batch("INSERT INTO \"rs_null\" VALUES (null), (null)")
            .map_err(duckdb_err)?;

        let stats = get_column_stats_internal(&test_registry(), &conn, "rs_null", "col")?;
        assert_eq!(stats.total_count, 2);
        assert_eq!(stats.null_count, 2);
        assert_eq!(stats.null_rate, 1.0);
        Ok(())
    }

    #[test]
    fn test_get_column_stats_text() -> Result<(), CoreError> {
        let conn = duckdb::Connection::open_in_memory().map_err(duckdb_err)?;
        conn.execute_batch("CREATE TABLE \"rs_text\" (\"name\" VARCHAR)")
            .map_err(duckdb_err)?;
        conn.execute_batch(
            "INSERT INTO \"rs_text\" VALUES ('Alice'), ('Bob'), ('Alice'), ('Charlie'), ('Bob')",
        )
        .map_err(duckdb_err)?;

        let stats = get_column_stats_internal(&test_registry(), &conn, "rs_text", "name")?;

        assert_eq!(stats.total_count, 5);
        assert_eq!(stats.null_count, 0);
        assert_eq!(stats.unique_count, Some(3));

        match &stats.stats_detail {
            ColumnStatsDetail::Text(t) => {
                assert!(!t.top_values.is_empty());
                assert!(t.min_length > 0);
                assert!(t.max_length >= t.min_length);
                Ok(())
            }
            _other => panic!("Expected Text stats, got variant"),
        }
    }

    #[test]
    fn test_get_column_stats_boolean() -> Result<(), CoreError> {
        let conn = duckdb::Connection::open_in_memory().map_err(duckdb_err)?;
        conn.execute_batch("CREATE TABLE \"rs_bool\" (\"active\" BOOLEAN)")
            .map_err(duckdb_err)?;
        conn.execute_batch(
            "INSERT INTO \"rs_bool\" VALUES (true), (true), (false), (true), (null)",
        )
        .map_err(duckdb_err)?;

        let stats = get_column_stats_internal(&test_registry(), &conn, "rs_bool", "active")?;

        assert_eq!(stats.total_count, 5);
        assert_eq!(stats.null_count, 1);

        match &stats.stats_detail {
            ColumnStatsDetail::Boolean(b) => {
                assert_eq!(b.true_count, 3);
                assert_eq!(b.false_count, 1);
                assert!((b.true_ratio - 0.75).abs() < 0.01);
                Ok(())
            }
            _other => panic!("Expected Boolean stats, got variant"),
        }
    }

    #[test]
    fn test_get_column_sample_returns_limit_5() -> Result<(), CoreError> {
        let conn = duckdb::Connection::open_in_memory().map_err(duckdb_err)?;
        let (table, col) = setup_test_table(&conn)?;

        let sample = get_column_sample_internal(&conn, table, col)?;
        assert!(sample.len() <= 5, "sample should be at most 5 rows");
        Ok(())
    }

    #[test]
    fn test_quality_scorer_high_score() {
        let stats = ColumnInsightFull {
            stats: crate::model::types::ColumnStats {
                column_name: "score".into(),
                data_type: "DOUBLE".into(),
                total_count: 100,
                null_count: 2,
                null_rate: 0.02,
                unique_count: Some(95),
                stats_detail: ColumnStatsDetail::Numeric(
                    crate::model::types::NumericStats {
                        min: 1.0,
                        max: 100.0,
                        avg: 50.0,
                        median: 50.5,
                        p25: 25.0,
                        p75: 75.0,
                        sum: 4900.0,
                        stddev: Some(28.0),
                        skewness: None,
                        kurtosis: None,
                        is_extreme: vec![],
                    },
                ),
            },
            sample: vec![serde_json::json!(50.0)],
            histogram: Some(vec![
                DistributionBin {
                    label: "0-25".into(),
                    count: 25,
                    ratio: 0.25,
                },
                DistributionBin {
                    label: "25-50".into(),
                    count: 24,
                    ratio: 0.24,
                },
                DistributionBin {
                    label: "50-75".into(),
                    count: 26,
                    ratio: 0.26,
                },
                DistributionBin {
                    label: "75-100".into(),
                    count: 25,
                    ratio: 0.25,
                },
            ]),
        };

        let qs = quality_scorer::compute_column_quality(&stats);
        assert!(
            qs.overall_score > 70.0,
            "high quality data should score > 70"
        );

        assert_eq!(qs.dimensions.len(), 4);
        let dim_names: Vec<&str> = qs.dimensions.iter().map(|d| d.name.as_str()).collect();
        assert!(dim_names.contains(&"完整性"));
        assert!(dim_names.contains(&"唯一性"));
        assert!(dim_names.contains(&"类型一致"));
        assert!(dim_names.contains(&"分布均匀"));
    }

    #[test]
    fn test_quality_scorer_low_score() {
        let stats = ColumnInsightFull {
            stats: crate::model::types::ColumnStats {
                column_name: "bad".into(),
                data_type: "VARCHAR".into(),
                total_count: 100,
                null_count: 60,
                null_rate: 0.6,
                unique_count: Some(1),
                stats_detail: ColumnStatsDetail::Text(
                    crate::model::types::TextStats {
                        min_length: 3,
                        max_length: 3,
                        top_values: vec![],
                    },
                ),
            },
            sample: vec![],
            histogram: None,
        };

        let qs = quality_scorer::compute_column_quality(&stats);
        assert!(
            qs.overall_score < 50.0,
            "low quality data should score < 50"
        );
    }

    #[test]
    fn test_compute_table_quality() {
        let make_stats = |name: &str, score: f64| ColumnInsightFull {
            stats: crate::model::types::ColumnStats {
                column_name: name.into(),
                data_type: "INTEGER".into(),
                total_count: 100,
                null_count: ((1.0 - score / 100.0) * 100.0) as u32,
                null_rate: 1.0 - score / 100.0,
                unique_count: Some((score * 0.9) as u32),
                stats_detail: ColumnStatsDetail::Numeric(
                    crate::model::types::NumericStats {
                        min: 0.0,
                        max: 100.0,
                        avg: 50.0,
                        median: 50.0,
                        p25: 25.0,
                        p75: 75.0,
                        sum: 5000.0,
                        stddev: Some(10.0),
                        skewness: None,
                        kurtosis: None,
                        is_extreme: vec![],
                    },
                ),
            },
            sample: vec![],
            histogram: Some(vec![
                DistributionBin {
                    label: "a".into(),
                    count: 50,
                    ratio: 0.5,
                },
                DistributionBin {
                    label: "b".into(),
                    count: 50,
                    ratio: 0.5,
                },
            ]),
        };

        let stats_list = vec![
            make_stats("col_a", 90.0),
            make_stats("col_b", 30.0),
            make_stats("col_c", 80.0),
        ];

        let tq = quality_scorer::compute_table_quality("test_table", &stats_list);

        assert_eq!(tq.table_name, "test_table");
        assert_eq!(tq.scored_count, 3);
        assert_eq!(tq.total_columns, 3);
        assert_eq!(tq.column_scores.len(), 3);
        assert!(
            tq.column_scores[0].quality_score <= tq.column_scores[1].quality_score,
            "column scores should be sorted ascending"
        );

        // overall_score is weighted average of quality dimensions, not raw input
        // scores. Assert non-zero reasonable range and sorted column order.
        assert!(tq.overall_score > 0.0);
        assert!(tq.overall_score <= 100.0);
    }

    /// 临时表内省：列清单（名 / 类型 / 可空 / 序号）与行数都要真的从 DuckDB 读出来。
    ///
    /// `DESCRIBE` 的列序是接口契约（column_name / column_type / null / key），
    /// 这里靠断言把它钉住：一旦 DuckDB 改列序，本测试先红。
    #[test]
    fn test_temp_table_profile_reads_columns_and_row_count() -> Result<(), CoreError> {
        let duckdb = get_or_create_duckdb()?;
        let conn = duckdb.lock().expect("DuckDB 锁不应中毒");
        conn.execute_batch(
            "CREATE OR REPLACE TEMP TABLE t_insight_profile_probe (
                 id INTEGER NOT NULL, amount DECIMAL(12,2), note VARCHAR, flag BOOLEAN
             );
             INSERT INTO t_insight_profile_probe VALUES (1, 1.5, 'a', true), (2, NULL, NULL, false);",
        )
        .expect("建探针临时表");

        let profile = get_temp_table_profile_on(&conn, "t_insight_profile_probe")?;
        assert_eq!(profile.columns.len(), 4);
        let names: Vec<&str> = profile
            .columns
            .iter()
            .map(|c| c.column_name.as_str())
            .collect();
        assert_eq!(names, vec!["id", "amount", "note", "flag"]);
        assert_eq!(profile.columns[1].data_type, "DECIMAL(12,2)");
        assert_eq!(profile.columns[0].ordinal_position, 1);
        assert_eq!(profile.columns[3].ordinal_position, 4);
        assert_eq!(profile.row_count, Some(2));
        assert_eq!(profile.db_type, "DuckDB");

        // 临时表由查询结果建出时没有主键约束：不该凭空报一个 PK
        assert!(
            profile.columns.iter().all(|c| !c.is_primary_key),
            "临时表没有主键约束，不得凭空标 PK"
        );
        Ok(())
    }

    /// 内省不存在的表：报错而不是空清单（空清单会让界面显示「没有列」，误导）
    #[test]
    fn test_temp_table_profile_rejects_missing_table() {
        let duckdb = get_or_create_duckdb().expect("连接");
        let conn = duckdb.lock().expect("DuckDB 锁不应中毒");
        let err = get_temp_table_profile_on(&conn, "t_insight_profile_absent_table");
        assert!(err.is_err(), "不存在的表应报错：{err:?}");
    }

    #[test]
    fn test_list_insight_rules_all() -> Result<(), CoreError> {
        let registry = test_registry();
        let rules = list_insight_rules(&registry, None)?;
        assert!(!rules.is_empty(), "built-in rules should exist");
        for rule in &rules {
            assert!(rule["id"].is_string(), "every rule should have an id");
            assert!(rule["name"].is_string(), "every rule should have a name");
            // 来源信息必须随规则一并返回，供界面按作用域分组展示。
            assert_eq!(
                rule["scope"].as_str(),
                Some("内置"),
                "仅加载内置层时，来源应为内置"
            );
            assert!(
                rule["scope_path"]
                    .as_str()
                    .is_some_and(|p| p.starts_with("<builtin>/")),
                "内置规则应带 <builtin>/ 虚拟路径"
            );
        }
        Ok(())
    }

    #[test]
    fn test_list_insight_rules_filtered_by_category() -> Result<(), CoreError> {
        let registry = test_registry();
        let column_rules = list_insight_rules(&registry, Some("column"))?;
        assert!(!column_rules.is_empty(), "column 分类应有规则");
        for rule in &column_rules {
            assert_eq!(rule["category"].as_str(), Some("column"));
        }
        assert!(
            list_insight_rules(&registry, Some("nonexistent-category"))?
                .is_empty()
        );
        Ok(())
    }

    #[test]
    fn test_list_rules_for_numeric_column() -> Result<(), CoreError> {
        let registry = test_registry();
        let rules = list_rules_for_column(&registry, "numeric")?;
        assert!(!rules.is_empty(), "should have numeric rules");
        let ids: Vec<&str> = rules.iter().filter_map(|r| r["id"].as_str()).collect();
        assert!(
            ids.contains(&"numeric-stats"),
            "should contain numeric-stats"
        );
        Ok(())
    }

    /// 文本族不得漏进其它族（否则会去做不可能的统计）
    #[test]
    fn text_like_types_stay_out_of_other_families() {
        assert!(!is_numeric_type("varchar"));
        assert!(!is_numeric_type("varchar(255)"));
        assert!(!is_datetime_type("varchar"));
        assert!(!is_binary_type("varchar"));
        assert!(!is_array_type("varchar"));
        // `timestamp` 开头但不是时间类型的反例不存在；保留一条边界：空串
        assert!(!is_numeric_type(""));
        assert!(!is_datetime_type(""));
    }

    /// T19 跨洞察表列值重叠：A 表 id ⟷ B 表 id 部分重叠（1..=5），
    /// 重叠行与两个方向的占比都要对；完全不重叠的列不进结果。
    #[test]
    fn test_compute_column_overlap_counts_shared_values() -> Result<(), CoreError> {
        let duckdb = get_or_create_duckdb()?;
        let conn = duckdb.lock().expect("DuckDB 锁不应中毒");
        conn.execute_batch(
            "CREATE OR REPLACE TEMP TABLE t_overlap_a (
                 id INTEGER, name VARCHAR
             );
             INSERT INTO t_overlap_a VALUES
                 (1,'a'), (2,'b'), (3,'c'), (4,'d'), (5,'e'), (6,'f'), (7,'g');
             CREATE OR REPLACE TEMP TABLE t_overlap_b (
                 id INTEGER, tag VARCHAR
             );
             INSERT INTO t_overlap_b VALUES
                 (1,'x'), (2,'y'), (3,'z'), (8,'w'), (9,'v');",
        )
        .expect("建重叠探针表");

        let rows = compute_column_overlap(&conn, "t_overlap_a", "t_overlap_b")?;
        // id：A=7 行、B=5 行，交集 {1,2,3} = 3
        let id = rows.iter().find(|r| r.column == "id").expect("id 列应进结果");
        assert_eq!(id.overlap, 3);
        assert!((id.rate_a - 3.0 / 7.0).abs() < 1e-9);
        assert!((id.rate_b - 3.0 / 5.0).abs() < 1e-9);
        // name / tag：值域完全不同 → 不进结果（不刷屏）
        assert!(
            rows.iter().all(|r| r.column == "id"),
            "无重叠的列不应出现：{rows:?}"
        );
        Ok(())
    }
}
