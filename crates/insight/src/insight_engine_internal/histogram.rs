//! 数值列直方图计算

use std::collections::HashMap;

use crate::RuleRegistry;
use shared::error::{CommonError, CoreError};

use super::*;
use crate::model::types::DistributionBin;

/// 数值列直方图分桶（行数低于 [`HISTOGRAM_MIN_ROWS`] 时返回空）。
///
/// **内部接缝**：只吃显式连接（同 `super::column::get_column_stats_internal`）。
pub fn get_column_histogram_internal(
    registry: &RuleRegistry,
    conn: &duckdb::Connection,
    temp_table: &str,
    column_name: &str,
) -> Result<Vec<DistributionBin>, CoreError> {
    get_column_histogram_with_non_null(registry, conn, temp_table, column_name, None)
}

/// [`get_column_histogram_internal`] 的非空行数预计算版：`Some(n)` 时免一次
/// `COUNT … WHERE IS NOT NULL` 查询（行数下限判定直接用预计算值）。
pub fn get_column_histogram_with_non_null(
    registry: &RuleRegistry,
    conn: &duckdb::Connection,
    temp_table: &str,
    column_name: &str,
    non_null_hint: Option<i64>,
) -> Result<Vec<DistributionBin>, CoreError> {
    let count: i64 = match non_null_hint {
        Some(n) => n,
        None => {
            let count_sql = format!(
                "SELECT COUNT(*) FROM {table} WHERE {col} IS NOT NULL",
                table = quote_ident(temp_table),
                col = quote_ident(column_name)
            );
            conn.query_row(&count_sql, [], |row| row.get(0))
                .unwrap_or(0)
        }
    };

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

    match crate::RuleExecutor::execute(rule, conn, &params) {
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
