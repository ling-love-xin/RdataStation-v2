use std::collections::HashMap;
use std::sync::OnceLock;

use tokio::sync::Semaphore;

use crate::core::error::{CommonError, CoreError};
use crate::core::insight;
use crate::core::insight::RuleExecutor;
use crate::core::services::duckdb_service::{
    duckdb_value_to_json, is_array_type, is_binary_type, is_datetime_type, is_numeric_type,
    DuckDbService,
};
use crate::core::services::result_service::{
    ColumnInsightFull, ColumnStats, ColumnStatsDetail, DateTimeStats, DistributionBin,
    NumericStats, TextFrequency, TextStats,
};

/// Default number of sample rows pulled from a column for display
pub(crate) const DEFAULT_SAMPLE_SIZE: usize = 5;

/// Minimum rows required to generate a histogram
pub(crate) const HISTOGRAM_MIN_ROWS: i64 = 10;

/// Maximum concurrent insight operations to prevent resource exhaustion.
/// DuckDB Mutex serialises all queries; this semaphore provides back-pressure
/// so that callers receive an immediate error instead of queuing indefinitely.
const INSIGHT_MAX_CONCURRENT: usize = 4;

static INSIGHT_SEM: OnceLock<Semaphore> = OnceLock::new();

fn insight_semaphore() -> &'static Semaphore {
    INSIGHT_SEM.get_or_init(|| Semaphore::new(INSIGHT_MAX_CONCURRENT))
}

/// Acquires a DuckDB connection from the connection pool (round-robin).
///
/// Note: DuckDB in-memory connections are isolated — temp tables created on
/// one connection are invisible to others. The pool distributes connections
/// via round-robin, so concurrent operations may access different connections.
/// Access to each connection is serialised via `std::sync::Mutex` (not async).
/// For multi-table analytics, consider using a persistent database.
pub(crate) fn get_or_create_duckdb(
) -> Result<std::sync::Arc<std::sync::Mutex<duckdb::Connection>>, CoreError> {
    DuckDbService::get_or_create_duckdb()
}

/// Computes comprehensive column insights including statistics (type-specific
/// detail), a sample of raw values, and a histogram for numeric columns.
///
/// This is the heavyweight entry point for column analysis, performing multiple
/// DuckDB queries under a concurrency semaphore. Returns [ColumnInsightFull]
/// which is suitable for quality scoring and detailed UI rendering.
pub(crate) fn get_column_insight_full(
    temp_table: &str,
    column_name: &str,
) -> Result<ColumnInsightFull, CoreError> {
    let _permit = insight_semaphore().try_acquire().map_err(|_| {
        CoreError::common(CommonError::General(
            "Too many concurrent insight operations, please retry".to_string(),
        ))
    })?;

    let duckdb = get_or_create_duckdb()?;
    let conn = duckdb.lock().map_err(|e| {
        CoreError::common(CommonError::General(format!("DuckDB lock error: {}", e)))
    })?;

    let stats = get_column_stats_internal(&conn, temp_table, column_name)?;
    let sample = get_column_sample_internal(&conn, temp_table, column_name)?;

    let histogram = match &stats.stats_detail {
        ColumnStatsDetail::Numeric(_) => {
            get_column_histogram_internal(&conn, temp_table, column_name).ok()
        }
        _ => None,
    };

    Ok(ColumnInsightFull {
        stats,
        sample,
        histogram,
    })
}

/// Computes basic statistical profile for a column: count, null-rate,
/// uniqueness, and type-specific detail (numeric/text/datetime/boolean).
///
/// Lighter than [get_column_insight_full] — no sample or histogram.
pub(crate) fn get_column_insights(
    temp_table: &str,
    column_name: &str,
) -> Result<ColumnStats, CoreError> {
    let _permit = insight_semaphore().try_acquire().map_err(|_| {
        CoreError::common(CommonError::General(
            "Too many concurrent insight operations, please retry".to_string(),
        ))
    })?;

    let duckdb = get_or_create_duckdb()?;
    let conn = duckdb.lock().map_err(|e| {
        CoreError::common(CommonError::General(format!("DuckDB lock error: {}", e)))
    })?;
    get_column_stats_internal(&conn, temp_table, column_name)
}

fn get_column_stats_internal(
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
        compute_numeric_stats(conn, temp_table, column_name)?
    } else if is_datetime_type(&dt_lower) {
        compute_datetime_stats(conn, temp_table, column_name)?
    } else if dt_lower == "boolean" || dt_lower == "bool" {
        compute_boolean_stats(conn, temp_table, column_name)?
    } else if is_binary_type(&dt_lower) || is_array_type(&dt_lower) {
        ColumnStatsDetail::Unknown
    } else {
        compute_text_stats(conn, temp_table, column_name)?
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

fn compute_numeric_stats(
    conn: &duckdb::Connection,
    temp_table: &str,
    column_name: &str,
) -> Result<ColumnStatsDetail, CoreError> {
    let registry = insight::global_registry().read().map_err(|e| {
        CoreError::common(CommonError::General(format!("Registry lock error: {}", e)))
    })?;
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
            let is_extreme = crate::core::services::duckdb_service::detect_extremes(
                min_v,
                max_v,
                stddev_v.unwrap_or(0.0),
            );

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
    conn: &duckdb::Connection,
    temp_table: &str,
    column_name: &str,
) -> Result<ColumnStatsDetail, CoreError> {
    let registry = insight::global_registry().read().map_err(|e| {
        CoreError::common(CommonError::General(format!("Registry lock error: {}", e)))
    })?;
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
    conn: &duckdb::Connection,
    temp_table: &str,
    column_name: &str,
) -> Result<ColumnStatsDetail, CoreError> {
    let registry = insight::global_registry().read().map_err(|e| {
        CoreError::common(CommonError::General(format!("Registry lock error: {}", e)))
    })?;
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
    conn: &duckdb::Connection,
    temp_table: &str,
    column_name: &str,
) -> Result<ColumnStatsDetail, CoreError> {
    let registry = insight::global_registry().read().map_err(|e| {
        CoreError::common(CommonError::General(format!("Registry lock error: {}", e)))
    })?;
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
                crate::core::services::result_service::BooleanStats {
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

fn get_column_sample_internal(
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

fn get_column_histogram_internal(
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

    let registry = insight::global_registry().read().map_err(|e| {
        CoreError::common(CommonError::General(format!("Registry lock error: {}", e)))
    })?;
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

/// Executes a named insight rule (e.g. "numeric-stats", "histogram") against
/// an existing DuckDB connection using the provided parameter map.
///
/// The caller must already hold the DuckDB lock. The rule SQL template is
/// substituted with parameters and executed, returning a qualified
/// [insight::ExecutionResult] with column metadata.
pub(crate) fn execute_insight_rule(
    rule_id: &str,
    conn: &duckdb::Connection,
    params: &HashMap<String, String>,
) -> Result<insight::ExecutionResult, CoreError> {
    let _permit = insight_semaphore().try_acquire().map_err(|_| {
        CoreError::common(CommonError::General(
            "Too many concurrent insight operations, please retry".to_string(),
        ))
    })?;

    let registry = insight::global_registry().read().map_err(|e| {
        CoreError::common(CommonError::General(format!("Registry lock error: {}", e)))
    })?;
    let rule = registry.get(rule_id).ok_or_else(|| {
        CoreError::common(CommonError::General(format!(
            "Rule '{}' not found",
            rule_id
        )))
    })?;
    RuleExecutor::execute_qualified(rule, conn, params)
}

/// Lists all registered insight rules, optionally filtered by category
/// (e.g. "statistics", "quality", "distribution").
///
/// Returns JSON objects containing rule metadata (id, name, description,
/// category, parameters, etc.) suitable for UI rendering.
pub(crate) fn list_insight_rules(
    category: Option<&str>,
) -> Result<Vec<serde_json::Value>, CoreError> {
    let registry = insight::global_registry().read().map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "Failed to lock insight registry: {}",
            e
        )))
    })?;
    let rules: Vec<&crate::core::insight::RuleFile> = match category {
        Some(cat) => registry.list_by_category(cat),
        None => registry.all_rules(),
    };
    let result: Vec<serde_json::Value> = rules
        .iter()
        .map(|r| {
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
            })
        })
        .collect();
    Ok(result)
}

/// Lists rules applicable to a given column type (e.g. "numeric", "text",
/// "Any"). Uses the registry's column-type index for fast filtering.
pub(crate) fn list_rules_for_column(
    column_type: &str,
) -> Result<Vec<serde_json::Value>, CoreError> {
    let registry = insight::global_registry().read().map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "Failed to lock insight registry: {}",
            e
        )))
    })?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::services::quality_scorer;

    fn duckdb_err(e: duckdb::Error) -> CoreError {
        CoreError::common(CommonError::General(e.to_string()))
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

        let _ = crate::core::insight::global_registry();
        Ok((table, col))
    }

    #[test]
    fn test_get_column_stats_numeric() -> Result<(), CoreError> {
        let conn = duckdb::Connection::open_in_memory().map_err(duckdb_err)?;
        let (table, col) = setup_test_table(&conn)?;

        let stats = get_column_stats_internal(&conn, table, col)?;

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

        let stats = get_column_stats_internal(&conn, "rs_null", "col")?;
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

        let stats = get_column_stats_internal(&conn, "rs_text", "name")?;

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

        let stats = get_column_stats_internal(&conn, "rs_bool", "active")?;

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
            stats: crate::core::services::result_service::ColumnStats {
                column_name: "score".into(),
                data_type: "DOUBLE".into(),
                total_count: 100,
                null_count: 2,
                null_rate: 0.02,
                unique_count: Some(95),
                stats_detail: ColumnStatsDetail::Numeric(
                    crate::core::services::result_service::NumericStats {
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
            stats: crate::core::services::result_service::ColumnStats {
                column_name: "bad".into(),
                data_type: "VARCHAR".into(),
                total_count: 100,
                null_count: 60,
                null_rate: 0.6,
                unique_count: Some(1),
                stats_detail: ColumnStatsDetail::Text(
                    crate::core::services::result_service::TextStats {
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
            stats: crate::core::services::result_service::ColumnStats {
                column_name: name.into(),
                data_type: "INTEGER".into(),
                total_count: 100,
                null_count: ((1.0 - score / 100.0) * 100.0) as u32,
                null_rate: 1.0 - score / 100.0,
                unique_count: Some((score * 0.9) as u32),
                stats_detail: ColumnStatsDetail::Numeric(
                    crate::core::services::result_service::NumericStats {
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

    #[test]
    fn test_list_insight_rules_all() -> Result<(), CoreError> {
        let rules = list_insight_rules(None)?;
        assert!(!rules.is_empty(), "built-in rules should exist");
        for rule in &rules {
            assert!(rule["id"].is_string(), "every rule should have an id");
            assert!(rule["name"].is_string(), "every rule should have a name");
        }
        Ok(())
    }

    #[test]
    fn test_list_rules_for_numeric_column() -> Result<(), CoreError> {
        let rules = list_rules_for_column("numeric")?;
        assert!(!rules.is_empty(), "should have numeric rules");
        let ids: Vec<&str> = rules.iter().filter_map(|r| r["id"].as_str()).collect();
        assert!(
            ids.contains(&"numeric-stats"),
            "should contain numeric-stats"
        );
        Ok(())
    }
}
