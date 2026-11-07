//! 列画像与统计计算

use std::collections::HashMap;

use crate::{RuleExecutor, RuleRegistry};
use engine::services::duckdb_service::duckdb_value_to_json;
use shared::error::{CommonError, CoreError};

use super::*;
use crate::model::types::{
    ColumnInsightFull, ColumnStats, ColumnStatsDetail, DateTimeStats,
    NumericStats, TextFrequency, TextStats,
};
use crate::model::{
    is_array_type, is_binary_type, is_datetime_type, is_numeric_type,
};

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
    let _permit = insight_semaphore().try_acquire().map_err(|_| {
        CoreError::common(CommonError::General(ERR_TOO_MANY_CONCURRENT.to_string()))
    })?;

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
            histogram::get_column_histogram_internal(registry, conn, temp_table, column_name).ok()
        }
        _ => None,
    };

    let extremes = match &stats.stats_detail {
        ColumnStatsDetail::Numeric(_) => {
            get_column_extremes_internal(registry, conn, temp_table, column_name).ok()
        }
        _ => None,
    };

    Ok(ColumnInsightFull {
        stats,
        sample,
        histogram,
        extremes,
    })
}

/// [`get_column_insight_full`] 的**预计算基础量**版：basics 来自表级摘要 + `DESCRIBE`
/// （评估全表逐列用），统计构建免 `COUNT / COUNT(DISTINCT) / typeof` 三查，
/// 数值列直方图的行数下限判定也复用 `basics.non_null`（再省一查）。
pub fn get_column_insight_full_with_basics(
    registry: &RuleRegistry,
    temp_table: &str,
    column_name: &str,
    basics: &ColumnBasics,
) -> Result<ColumnInsightFull, CoreError> {
    let _permit = insight_semaphore().try_acquire().map_err(|_| {
        CoreError::common(CommonError::General(ERR_TOO_MANY_CONCURRENT.to_string()))
    })?;

    let duckdb = get_or_create_duckdb()?;
    let conn = duckdb.lock().map_err(|e| {
        CoreError::common(CommonError::General(format!("DuckDB lock error: {}", e)))
    })?;

    get_column_insight_full_on_with_basics(registry, &conn, temp_table, column_name, basics)
}

/// [`get_column_insight_full_on`] 的预计算基础量版（已持连接场景，同 `*_on` 口径）。
pub fn get_column_insight_full_on_with_basics(
    registry: &RuleRegistry,
    conn: &duckdb::Connection,
    temp_table: &str,
    column_name: &str,
    basics: &ColumnBasics,
) -> Result<ColumnInsightFull, CoreError> {
    let stats = get_column_stats_with_basics(registry, conn, temp_table, column_name, basics)?;
    let sample = get_column_sample_internal(conn, temp_table, column_name)?;

    let histogram = match &stats.stats_detail {
        ColumnStatsDetail::Numeric(_) => histogram::get_column_histogram_with_non_null(
            registry,
            conn,
            temp_table,
            column_name,
            Some(basics.non_null),
        )
        .ok(),
        _ => None,
    };

    // 极值样本（数值列；失败为 None 不阻塞画像——与直方图同口径）
    let extremes = match &stats.stats_detail {
        ColumnStatsDetail::Numeric(_) => {
            get_column_extremes_internal(registry, conn, temp_table, column_name).ok()
        }
        _ => None,
    };

    Ok(ColumnInsightFull {
        stats,
        sample,
        histogram,
        extremes,
    })
}

/// 列基础统计（行数 / 空值 / 唯一值 + 类型专属详情）。
pub fn get_column_insights(
    registry: &RuleRegistry,
    temp_table: &str,
    column_name: &str,
) -> Result<ColumnStats, CoreError> {
    let _permit = insight_semaphore().try_acquire().map_err(|_| {
        CoreError::common(CommonError::General(ERR_TOO_MANY_CONCURRENT.to_string()))
    })?;

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
    // 基础量自算（调用方没有预计算可喂时的路径）：计数 + typeof 两条查询。
    let count_sql = format!(
        "SELECT COUNT(*), COUNT({col}), COUNT(DISTINCT {col}) FROM {table}",
        col = quote_ident(column_name),
        table = quote_ident(temp_table)
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

    let type_sql = format!(
        "SELECT typeof({col}) FROM {table} WHERE {col} IS NOT NULL LIMIT 1",
        col = quote_ident(column_name),
        table = quote_ident(temp_table)
    );
    let data_type: String = conn
        .query_row(&type_sql, [], |row| row.get(0))
        .unwrap_or_else(|_| "VARCHAR".to_string());

    get_column_stats_with_basics(
        registry,
        conn,
        temp_table,
        column_name,
        &ColumnBasics {
            total,
            non_null,
            unique: unique_cnt,
            data_type,
        },
    )
}

/// 列画像的**预计算基础量**（总行数 / 非空 / 唯一值 / 列类型）。
///
/// 表级摘要（[`super::profile::compute_table_column_summary_on`]）一次查询就为**全部列**算出
/// 前三样；`DESCRIBE` 给出列类型。逐列评估把它们喂进来（[`get_column_stats_with_basics`]），
/// 免去每列重跑 `COUNT / COUNT(DISTINCT)` 全表聚合与 `typeof` 探查
/// （200 列表 ≈ 省 400 次查询，其中 `COUNT(DISTINCT)` 是重聚合）。
///
/// 一致性口径：basics 与后续规则查询**不是同一快照**——分析临时表是取样产物、
/// 建表后不再写入，可视为不变；若表被回收，规则查询自己会失败（列标灰态可补算）。
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnBasics {
    /// 总行数
    pub total: i64,
    /// 非空行数
    pub non_null: i64,
    /// 唯一值数（`COUNT(DISTINCT)`，不含 NULL）
    pub unique: i64,
    /// 列类型名（`DESCRIBE` 的 `column_type`；仅 `non_null > 0` 时参与类型分派）
    pub data_type: String,
}

/// 在**预计算基础量**上构建列统计（免 `COUNT / COUNT(DISTINCT) / typeof` 三查）。
///
/// 与 [`get_column_stats_internal`] 的关系：后者 = 自算 basics + 本函数；
/// 数字口径完全一致（同一张表两条路径的 `ColumnStats` 应相等——有等价性测试钉住）。
pub fn get_column_stats_with_basics(
    registry: &RuleRegistry,
    conn: &duckdb::Connection,
    temp_table: &str,
    column_name: &str,
    basics: &ColumnBasics,
) -> Result<ColumnStats, CoreError> {
    let ColumnBasics {
        total,
        non_null,
        unique: unique_cnt,
        data_type,
    } = basics;

    let null_count = (*total - non_null).max(0);
    let null_rate = if *total > 0 {
        null_count as f64 / *total as f64
    } else {
        0.0
    };

    let dt_lower = data_type.to_lowercase();

    let stats_detail = if *non_null == 0 {
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

    // 饱和转换：领域类型是 u32，超界（>42.9 亿行）封顶而不是回绕成小数
    Ok(ColumnStats {
        column_name: column_name.to_string(),
        data_type: data_type.clone(),
        total_count: u32::try_from(*total).unwrap_or(u32::MAX),
        null_count: u32::try_from(null_count).unwrap_or(u32::MAX),
        null_rate,
        unique_count: Some(u32::try_from(*unique_cnt).unwrap_or(u32::MAX)),
        stats_detail,
    })
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
            // 必填字段缺失 = 规则被覆盖成了输出契约不兼容的版本（K10 的静默面：
            // 旧实现 `unwrap_or(0.0)` 会把缺字段显示成 0，用户看到的是假统计）。
            // 早失败优于编造值——错误文案点名规则与缺失字段。
            let require = |key: &str| -> Result<f64, CoreError> {
                map.get(key).and_then(|v| v.as_f64()).ok_or_else(|| {
                    CoreError::common(CommonError::General(format!(
                        "规则 'numeric-stats' 的输出缺少字段 '{}'（被项目/全局规则覆盖？输出契约见内置版本）",
                        key
                    )))
                })
            };
            let extract_opt = |key: &str| -> Option<f64> { map.get(key).and_then(|v| v.as_f64()) };
            let min_v = require("min")?;
            let max_v = require("max")?;
            let stddev_v = extract_opt("stddev");
            let is_extreme = super::extremes::detect_extremes(min_v, max_v, stddev_v.unwrap_or(0.0));

            Ok(ColumnStatsDetail::Numeric(NumericStats {
                min: min_v,
                max: max_v,
                avg: require("avg")?,
                median: require("median")?,
                p25: require("p25")?,
                p75: require("p75")?,
                sum: require("sum")?,
                stddev: stddev_v,
                skewness: extract_opt("skewness"),
                kurtosis: extract_opt("kurtosis"),
                zeros: map
                    .get("zeros")
                    .and_then(|v| v.as_i64())
                    .map(|v| v.max(0) as u32),
                negatives: map
                    .get("negatives")
                    .and_then(|v| v.as_i64())
                    .map(|v| v.max(0) as u32),
                p95: extract_opt("p95"),
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
                    // 兼容档（无百分位函数的旧 DuckDB）：min/max/avg/median/sum 仍是
                    // 必填；p25/p75 无来源、如实回 0（降级口径，视图按有无 stddev 展示）。
                    let require = |key: &str| -> Result<f64, CoreError> {
                        map.get(key).and_then(|v| v.as_f64()).ok_or_else(|| {
                            CoreError::common(CommonError::General(format!(
                                "规则 'numeric-basic' 的输出缺少字段 '{}'（被项目/全局规则覆盖？输出契约见内置版本）",
                                key
                            )))
                        })
                    };
                    Ok(ColumnStatsDetail::Numeric(NumericStats {
                        min: require("min")?,
                        max: require("max")?,
                        avg: require("avg")?,
                        median: require("median")?,
                        p25: 0.0,
                        p75: 0.0,
                        sum: require("sum")?,
                        stddev: map.get("stddev").and_then(|v| v.as_f64()),
                        skewness: None,
                        kurtosis: None,
                        zeros: None,
                        negatives: None,
                        p95: None,
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

/// 数值列极值样本（YData Extreme Values 对应物）：最小 5 / 最大 5 个不同值
/// 及出现次数（`numeric-extremes` 规则驱动，规则分层随项目传入）。
/// 值经 SQL `::VARCHAR` 文本化——DECIMAL/大整数不经 f64，显示保真。
///
/// **内部接缝**：只吃显式连接（同 `get_column_stats_internal`）。
pub fn get_column_extremes_internal(
    registry: &RuleRegistry,
    conn: &duckdb::Connection,
    temp_table: &str,
    column_name: &str,
) -> Result<Vec<crate::model::types::ExtremeSample>, CoreError> {
    let mut params = HashMap::new();
    params.insert("table".to_string(), temp_table.to_string());
    params.insert("col".to_string(), column_name.to_string());

    let rule = registry.get("numeric-extremes").ok_or_else(|| {
        CoreError::common(CommonError::General(
            "Rule 'numeric-extremes' not found".to_string(),
        ))
    })?;

    match RuleExecutor::execute(rule, conn, &params) {
        Ok(serde_json::Value::Array(arr)) => {
            let rows: Vec<crate::model::types::ExtremeSample> = arr
                .iter()
                .filter_map(|item| {
                    let obj = item.as_object()?;
                    Some(crate::model::types::ExtremeSample {
                        side: obj.get("side")?.as_str()?.to_string(),
                        value: obj.get("value")?.as_str()?.to_string(),
                        count: obj.get("count")?.as_i64()?,
                    })
                })
                .collect();
            Ok(rows)
        }
        _ => Ok(Vec::new()),
    }
}

/// 列样本值（条数取 [`super::DEFAULT_SAMPLE_SIZE`]）。
///
/// **内部接缝**：只吃显式连接（同 [`get_column_stats_internal`]）。
pub fn get_column_sample_internal(
    conn: &duckdb::Connection,
    temp_table: &str,
    column_name: &str,
) -> Result<Vec<serde_json::Value>, CoreError> {
    let sql = format!(
        "SELECT {col} FROM {table} LIMIT {n}",
        col = quote_ident(column_name),
        table = quote_ident(temp_table),
        n = super::DEFAULT_SAMPLE_SIZE
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
