//! Round 26：SQL 查询执行器——对 DuckDB 分析库执行只读查询，返回列 + 行。
//!
//! 直接使用 duckdb-rs（workbench 已依赖）+ engine 的
//! `duckdb_value_to_json` 做值→字符串转换（duckdb::types::Value 无 Display）。
//! 供工作台「SQL 查询」区执行分析 SQL 并渲染结果集。
//! 当前定位只读（SELECT 类）；写操作（INSERT/UPDATE/DDL）走原生连接的
//! 独立执行路径（本轮不开放）。

use std::path::Path;

/// 查询输出（列名 + 已字符串化的行）
#[derive(Debug, Clone)]
pub struct QueryOutput {
    pub columns: Vec<String>,
    /// 行数据（每行已按列序转字符串；NULL → "NULL"）
    pub rows: Vec<Vec<String>>,
    pub row_count: usize,
}

fn value_to_string(v: &duckdb::types::Value) -> String {
    let j = engine::services::duckdb_service::duckdb_value_to_json(v);
    match j {
        serde_json::Value::Null => "NULL".to_string(),
        serde_json::Value::String(s) => s,
        other => other.to_string(),
    }
}

/// 对指定 DuckDB 分析库执行 SQL。
///
/// 打开失败 / 执行失败 / 读取失败均返回中文错误提示。
pub fn execute_sql(duckdb_path: &Path, sql: &str) -> Result<QueryOutput, String> {
    if sql.trim().is_empty() {
        return Err("SQL 不能为空".to_string());
    }

    let conn = duckdb::Connection::open(duckdb_path).map_err(|e| format!("打开分析库失败: {e}"))?;
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| format!("执行失败: {e}"))?;

    // duckdb-rs：必须先在 stmt 上执行 query 才能访问列元数据；
    // 且 Rows 持有 stmt 的可变借用，需先收集完数据再读列名。
    let mut data: Vec<Vec<String>> = Vec::new();
    {
        let mut rows = stmt.query([]).map_err(|e| format!("执行失败: {e}"))?;
        while let Some(row) = rows.next().map_err(|e| format!("读取行失败: {e}"))? {
            let mut vals = Vec::new();
            for i in 0.. {
                match row.get::<usize, duckdb::types::Value>(i) {
                    Ok(v) => vals.push(value_to_string(&v)),
                    Err(_) => break,
                }
            }
            data.push(vals);
        }
    }

    let columns: Vec<String> = (0..stmt.column_count())
        .map(|i| stmt.column_name(i).map(|v| v.to_string()).unwrap_or_else(|_| "unknown".to_string()))
        .collect();

    let row_count = data.len();
    Ok(QueryOutput {
        columns,
        rows: data,
        row_count,
    })
}
