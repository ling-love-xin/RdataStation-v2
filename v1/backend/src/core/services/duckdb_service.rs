use std::sync::Arc;

use crate::core::dbi::context::QueryContext;
use crate::core::dbi::engine::duckdb_engine::DuckDBEngine;
use crate::core::dbi::engine::{ExecutionEngine, ExecutionMode};
use crate::core::error::{CommonError, CoreError};
use crate::core::models::QueryResult;
use crate::core::DuckDBManager;

pub(crate) struct DuckDbService;

impl DuckDbService {
    pub fn get_or_create_duckdb() -> Result<Arc<std::sync::Mutex<duckdb::Connection>>, CoreError> {
        DuckDBManager::get_or_create_in_memory()
    }

    /// 使用 DuckDB 加速引擎执行外部数据库查询
    ///
    /// 流程：ATTACH 外部数据库 → 执行 SQL → DETACH → 返回 QueryResult
    /// 内部调用 dbi::engine::duckdb_engine::DuckDBEngine（DBI 层）
    pub async fn accelerate_query(
        db_type: &str,
        url: &str,
        conn_name: &str,
        sql: &str,
        engine: &DuckDBEngine,
        data_dir: Option<&str>,
    ) -> Result<QueryResult, CoreError> {
        let attach_type = match db_type.to_lowercase().as_str() {
            "mysql" => "mysql",
            "postgresql" | "postgres" => "postgres",
            "sqlite" => "sqlite",
            other => {
                return Err(CoreError::common(CommonError::General(format!(
                    "Unsupported database type for DuckDB acceleration: {}",
                    other
                ))))
            }
        };

        let sanitized = conn_name.replace(|c: char| !c.is_alphanumeric() && c != '_', "_");
        let attach_name = format!("ext_{}", sanitized);
        let attach_sql = format!("ATTACH '{}' AS {} (TYPE {})", url, attach_name, attach_type);

        {
            let conn = engine
                .conn()
                .map_err(|e| CoreError::common(CommonError::General(e.to_string())))?;
            if let Some(dir) = data_dir {
                DuckDBEngine::init_extensions(&conn, dir)
                    .map_err(|e| CoreError::common(CommonError::General(e.to_string())))?;
            }
            conn.execute_batch(&attach_sql).map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "Failed to ATTACH source database: {}",
                    e
                )))
            })?;
        }

        let ctx = QueryContext::new(None, ExecutionMode::DuckDB);
        let result = engine
            .execute(sql, &ctx)
            .await
            .map_err(|e| CoreError::common(CommonError::General(e.to_string())))?;

        {
            let conn = engine
                .conn()
                .map_err(|e| CoreError::common(CommonError::General(e.to_string())))?;
            let _ = conn.execute_batch(&format!("DETACH IF EXISTS {}", attach_name));
        }

        Ok(result)
    }

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

    pub(crate) fn create_temp_table_internal(
        conn: &mut duckdb::Connection,
        columns: &[String],
        rows: &[Vec<serde_json::Value>],
    ) -> Result<String, CoreError> {
        let table_name = format!("rs_{}", uuid::Uuid::new_v4().to_string().replace('-', "_"));
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

    pub(crate) fn query_duckdb(
        conn: &mut duckdb::Connection,
        sql: &str,
    ) -> Result<(Vec<String>, Vec<Vec<serde_json::Value>>), CoreError> {
        let mut stmt = conn.prepare(sql).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "DuckDB prepare failed: {}",
                e
            )))
        })?;
        let col_count = stmt.column_count();
        let col_names: Vec<String> = (0..col_count)
            .map(|i| {
                stmt.column_name(i)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|_| format!("c{}", i))
            })
            .collect();

        let rows_result = stmt
            .query_map([], |row| {
                (0..col_count)
                    .map(|i| {
                        let v: duckdb::types::Value = row.get(i)?;
                        Ok(duckdb_value_to_json(&v))
                    })
                    .collect::<Result<Vec<serde_json::Value>, duckdb::Error>>()
            })
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("DuckDB query failed: {}", e)))
            })?;

        let mut rows = Vec::new();
        for r in rows_result {
            rows.push(r.map_err(|e| {
                CoreError::common(CommonError::General(format!("DuckDB row error: {}", e)))
            })?);
        }

        Ok((col_names, rows))
    }
}

pub(crate) fn extract_rows_from_serialized(
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

fn infer_type(rows: &[Vec<serde_json::Value>], col_idx: usize) -> &str {
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

fn json_to_duckdb_value(v: &serde_json::Value) -> duckdb::types::Value {
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

pub(crate) fn duckdb_value_to_json(v: &duckdb::types::Value) -> serde_json::Value {
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

pub(crate) fn is_numeric_type(dt_lower: &str) -> bool {
    matches!(
        dt_lower,
        "bigint"
            | "integer"
            | "int"
            | "smallint"
            | "tinyint"
            | "double"
            | "float"
            | "hugeint"
            | "decimal"
            | "numeric"
            | "real"
    )
}

// ─── 数据导出 ────────────────────────────────────────────

#[derive(Debug, Clone, Copy, serde::Deserialize)]
pub(crate) enum ExportFormat {
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
    ) -> Result<String, crate::core::error::CoreError> {
        use crate::core::duckdb::DuckDBManager;
        use crate::core::error::CommonError;

        let arc = DuckDBManager::get_or_create_in_memory()?;
        let conn = arc.lock().map_err(|e| {
            crate::core::error::CoreError::common(CommonError::General(format!(
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
            crate::core::error::CoreError::common(CommonError::General(format!(
                "导出 {} 失败: {}",
                format.extension(),
                e
            )))
        })?;

        Ok(file_path.to_string())
    }
}

pub(crate) fn is_datetime_type(dt_lower: &str) -> bool {
    matches!(
        dt_lower,
        "date" | "timestamp" | "datetime" | "time" | "timestamp with time zone" | "timestamptz"
    )
}

pub(crate) fn is_binary_type(dt_lower: &str) -> bool {
    matches!(dt_lower, "blob" | "bytea" | "binary" | "varbinary")
}

#[allow(dead_code)]
pub(crate) fn is_json_type(dt_lower: &str) -> bool {
    matches!(dt_lower, "json" | "jsonb")
}

pub(crate) fn is_array_type(dt_lower: &str) -> bool {
    dt_lower.starts_with('[')
        || dt_lower.ends_with(']')
        || dt_lower.contains("list")
        || dt_lower.contains("array")
}

pub(crate) fn detect_extremes(
    _min: f64,
    _max: f64,
    _stddev: f64,
) -> Vec<crate::core::services::result_service::ExtremeValue> {
    let mut results = Vec::new();
    if _stddev > 0.0 && _max > 0.0 {
        let range = _max - _min;
        if range > 10.0 * _stddev && range > 1000.0 {
            results.push(crate::core::services::result_service::ExtremeValue {
                value: _max,
                kind: "outlier_high".to_string(),
            });
        }
    }
    results
}
