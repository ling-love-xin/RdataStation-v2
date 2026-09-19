use std::sync::Arc;

use crate::DuckDBManager;
use shared::error::{CommonError, CoreError};

pub struct DuckDbService;

impl DuckDbService {
    pub fn get_or_create_duckdb() -> Result<Arc<std::sync::Mutex<duckdb::Connection>>, CoreError> {
        DuckDBManager::get_or_create_in_memory()
    }

    // 2026-09-19 删除 `accelerate_query`：它零调用，且依赖的 `dbi` 层整体已废弃
    // （2124 行里只有 `DuckDBEngine::file_reader_function` 是活的，现挂在
    // `crate::duckdb::file_reader`）。加速档的实际实现是 `crate::duckdb::accel`，
    // 它自己拼 `ATTACH … (READ_ONLY)`，不经过这里。

    pub fn create_duckdb_temp_table(
        columns: &[String],
        rows: &[Vec<serde_json::Value>],
    ) -> Result<String, CoreError> {
        let duckdb = Self::get_or_create_duckdb()?;
        let mut conn = duckdb.lock().map_err(|e| {
            CoreError::common(CommonError::General(format!("DuckDB lock error: {}", e)))
        })?;
        Self::create_temp_table_internal(&mut conn, columns, rows)
    }

    /// 建一张**查询结果**临时表（表名带 `tmp_q_` 前缀、建完即登记）并回填数据。
    ///
    /// 命名走 [`crate::duckdb::generate_unique_name`]：前缀不是装饰——
    /// TTL / 上限 / 按来源清理 / 关项目清场全靠它识别；历史上的 `rs_<uuid>` 不属于任何前缀，
    /// 于是那些机制对它全部失效（K16）。
    ///
    /// 回收责任在**建表方**：结果集被丢弃 / 替换 / 关文档时调
    /// [`crate::duckdb::drop_temp_table`]（定向），项目切换 / 关闭时调
    /// [`crate::duckdb::DuckDBManager::drop_in_memory_temp_tables`]（清场）。
    pub fn create_temp_table_internal(
        conn: &mut duckdb::Connection,
        columns: &[String],
        rows: &[Vec<serde_json::Value>],
    ) -> Result<String, CoreError> {
        let table_name = crate::duckdb::generate_unique_name(
            crate::duckdb::TempTableSource::Query,
            "result",
        );
        let col_defs: Vec<String> = columns
            .iter()
            .enumerate()
            .map(|(i, col)| {
                let dtype = if rows.is_empty() {
                    "VARCHAR"
                } else {
                    infer_type(rows, i)
                };
                format!("\"{}\" {}", col, dtype)
            })
            .collect();

        conn.execute_batch(&format!(
            "CREATE TABLE \"{}\" ({})",
            table_name,
            col_defs.join(", ")
        ))
        .map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Failed to create DuckDB table: {}",
                e
            )))
        })?;

        if !rows.is_empty() {
            let placeholders: Vec<String> = (0..columns.len()).map(|_| "?".to_string()).collect();
            let insert_sql = format!(
                "INSERT INTO \"{}\" VALUES ({})",
                table_name,
                placeholders.join(", ")
            );
            let mut stmt = conn.prepare(&insert_sql).map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "Prepare insert failed: {}",
                    e
                )))
            })?;

            for row in rows {
                let params: Vec<duckdb::types::Value> =
                    row.iter().map(json_to_duckdb_value).collect();
                let params_refs: Vec<&dyn duckdb::types::ToSql> = params
                    .iter()
                    .map(|p| p as &dyn duckdb::types::ToSql)
                    .collect();
                stmt.execute(&params_refs[..]).map_err(|e| {
                    CoreError::common(CommonError::General(format!("Insert row failed: {}", e)))
                })?;
            }
        }

        DuckDBManager::register_temp_table(&table_name);
        Ok(table_name)
    }

    pub fn query_duckdb(
        conn: &mut duckdb::Connection,
        sql: &str,
    ) -> Result<(Vec<String>, Vec<Vec<serde_json::Value>>), CoreError> {
        let mut stmt = conn.prepare(sql).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "DuckDB prepare failed: {}",
                e
            )))
        })?;

        // 顺序不能反：duckdb-rs 1.10505 的 `Statement::column_names()` / `schema()` 读的是
        // **执行结果**（`raw_statement.rs` 的 `executed()`），prepare 之后、执行之前调会直接
        // panic（`The statement was not executed yet`）。所以先 `query` 把语句跑起来，再从
        // `Rows::as_ref()` 取回那条已执行的语句读列名——这也是 duckdb-rs 文档推荐的写法。
        let mut query_rows = stmt.query([]).map_err(|e| {
            CoreError::common(CommonError::General(format!("DuckDB query failed: {}", e)))
        })?;
        let col_names = query_rows
            .as_ref()
            .map(|statement| statement.column_names())
            .unwrap_or_default();
        let col_count = col_names.len();

        let mut rows = Vec::new();
        while let Some(row) = query_rows.next().map_err(|e| {
            CoreError::common(CommonError::General(format!("DuckDB row error: {}", e)))
        })? {
            let mut values = Vec::with_capacity(col_count);
            for i in 0..col_count {
                let v: duckdb::types::Value = row.get(i).map_err(|e| {
                    CoreError::common(CommonError::General(format!("DuckDB cell error: {}", e)))
                })?;
                values.push(duckdb_value_to_json(&v));
            }
            rows.push(values);
        }

        Ok((col_names, rows))
    }
}

pub fn extract_rows_from_serialized(
    result_json: &serde_json::Value,
) -> Vec<Vec<serde_json::Value>> {
    let columns = match result_json["columns"].as_array() {
        Some(c) => c
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect::<Vec<_>>(),
        None => return vec![],
    };

    let batches = match result_json["batches"].as_array() {
        Some(b) => b,
        None => return vec![],
    };

    let mut rows = Vec::new();
    for batch in batches {
        if let Some(data) = batch["data"].as_object() {
            let num_rows = columns
                .first()
                .and_then(|c| data.get(c))
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);

            for ri in 0..num_rows {
                let mut row = Vec::with_capacity(columns.len());
                for col in &columns {
                    let val = data
                        .get(col)
                        .and_then(|v| v.as_array())
                        .and_then(|a| a.get(ri))
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    row.push(val);
                }
                rows.push(row);
            }
        }
    }

    rows
}

/// JSON 行 → DuckDB 列类型。
///
/// 同 crate 内公开：`duckdb::analysis`（分析临时表）灌样本时要与结果集那条路径
/// **用同一套打型规则**，否则同一个 `serde_json` 值在两处会变成不同类型。
pub(crate) fn infer_type(rows: &[Vec<serde_json::Value>], col_idx: usize) -> &str {
    for row in rows {
        if col_idx < row.len() {
            match &row[col_idx] {
                serde_json::Value::Null => continue,
                serde_json::Value::Number(n) => {
                    return if n.is_f64() { "DOUBLE" } else { "BIGINT" };
                }
                serde_json::Value::Bool(_) => return "BOOLEAN",
                _ => return "VARCHAR",
            }
        }
    }
    "VARCHAR"
}

/// JSON 值 → DuckDB 值（插入参数）。
///
/// 同 `infer_type`：与 `duckdb::analysis` 共用一份转换。
pub(crate) fn json_to_duckdb_value(v: &serde_json::Value) -> duckdb::types::Value {
    match v {
        serde_json::Value::Null => duckdb::types::Value::Null,
        serde_json::Value::Bool(b) => duckdb::types::Value::Boolean(*b),
        serde_json::Value::Number(n) => n
            .as_f64()
            .map(duckdb::types::Value::Double)
            .or_else(|| n.as_i64().map(duckdb::types::Value::BigInt))
            .unwrap_or(duckdb::types::Value::Text(n.to_string())),
        serde_json::Value::String(s) => duckdb::types::Value::Text(s.clone()),
        _ => duckdb::types::Value::Text(v.to_string()),
    }
}

pub fn duckdb_value_to_json(v: &duckdb::types::Value) -> serde_json::Value {
    match v {
        duckdb::types::Value::Null => serde_json::Value::Null,
        duckdb::types::Value::Boolean(b) => serde_json::json!(b),
        duckdb::types::Value::TinyInt(n) => serde_json::json!(n),
        duckdb::types::Value::SmallInt(n) => serde_json::json!(n),
        duckdb::types::Value::Int(n) => serde_json::json!(n),
        duckdb::types::Value::BigInt(n) => serde_json::json!(n),
        duckdb::types::Value::Float(f) => serde_json::json!(f),
        duckdb::types::Value::Double(f) => serde_json::json!(f),
        duckdb::types::Value::Text(s) => serde_json::Value::String(s.clone()),
        _ => serde_json::Value::Null,
    }
}

// 列类型族判定（`is_numeric_type` / `is_datetime_type` / …）已移入 `crates/insight`：
// 它们唯一的调用方是洞察的类型分派，且编码的是「哪种类型用哪些统计量」的业务语义。

#[allow(dead_code)]
pub fn is_json_type(dt_lower: &str) -> bool {
    matches!(dt_lower, "json" | "jsonb")
}
// ─── 数据导出 ────────────────────────────────────────────

#[derive(Debug, Clone, Copy, serde::Deserialize)]
pub enum ExportFormat {
    #[serde(rename = "csv")]
    Csv,
    #[serde(rename = "parquet")]
    Parquet,
    #[serde(rename = "xlsx")]
    Xlsx,
}

impl ExportFormat {
    fn sql_format(&self) -> &'static str {
        match self {
            ExportFormat::Csv => "FORMAT CSV, HEADER true",
            ExportFormat::Parquet => "FORMAT PARQUET",
            ExportFormat::Xlsx => "FORMAT XLSX, HEADER true",
        }
    }

    fn extension(&self) -> &'static str {
        match self {
            ExportFormat::Csv => "csv",
            ExportFormat::Parquet => "parquet",
            ExportFormat::Xlsx => "xlsx",
        }
    }
}

impl DuckDbService {
    pub fn export_temp_table(
        temp_table: &str,
        file_path: &str,
        format: ExportFormat,
    ) -> Result<String, shared::error::CoreError> {
        use crate::duckdb::DuckDBManager;
        use shared::error::CommonError;

        let arc = DuckDBManager::get_or_create_in_memory()?;
        let conn = arc.lock().map_err(|e| {
            shared::error::CoreError::common(CommonError::General(format!(
                "DuckDB lock error during export: {}",
                e
            )))
        })?;

        let escaped_path = file_path.replace('\'', "''");
        let sql = format!(
            "COPY \"{}\" TO '{}' ({})",
            temp_table,
            escaped_path,
            format.sql_format()
        );
        conn.execute_batch(&sql).map_err(|e| {
            shared::error::CoreError::common(CommonError::General(format!(
                "导出 {} 失败: {}",
                format.extension(),
                e
            )))
        })?;

        Ok(file_path.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::DuckDbService;
    use crate::duckdb::{drop_temp_table, TempTableSource};
    use crate::DuckDBManager;
    use serde_json::json;

    /// K16：结果集临时表必须带 `tmp_q_` 前缀并登记——前缀是 TTL / 上限 /
    /// 按来源清理 / 关项目清场唯一的识别依据（历史上的 `rs_<uuid>` 什么机制都识别不了）。
    #[test]
    fn test_create_temp_table_uses_query_prefix_and_registers() {
        let columns = vec!["id".to_string(), "name".to_string()];
        let rows = vec![vec![json!(1), json!("a")]];

        let table = DuckDbService::create_duckdb_temp_table(&columns, &rows).expect("建表");
        assert!(
            table.starts_with("tmp_q_"),
            "结果集临时表名要在 tmp_q_ 前缀下: {table}"
        );
        assert_eq!(
            DuckDBManager::temp_table_manager().count_by_prefix(&table),
            1,
            "建完应当被登记（按来源清理才看得见它）"
        );

        // 定向回收口能用（建表方在结果集丢弃 / 替换时调它）
        let conn = DuckDbService::get_or_create_duckdb().expect("内存连接");
        let guard = conn.lock().expect("锁");
        drop_temp_table(&guard, TempTableSource::Query, &table).expect("删表");
    }
}
