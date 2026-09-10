# -*- coding: utf-8 -*-
"""修 query_runner：列名提取移到 stmt.query() 之后（duckdb-rs 要求先执行）。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\services\query_runner.rs')
t = p.read_text(encoding='utf-8')

old = '''    let conn = duckdb::Connection::open(duckdb_path).map_err(|e| format!("打开分析库失败: {e}"))?;
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| format!("执行失败: {e}"))?;

    let columns: Vec<String> = (0..stmt.column_count())
        .map(|i| stmt.column_name(i).map(|v| v.to_string()).unwrap_or_else(|_| "unknown".to_string()))
        .collect();

    let mut rows_out: Vec<Vec<String>> = Vec::new();
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
            rows_out.push(vals);
        }
    }

    Ok(QueryOutput {
        columns,
        rows: rows_out.clone(),
        row_count: rows_out.len(),
    })'''
new = '''    let conn = duckdb::Connection::open(duckdb_path).map_err(|e| format!("打开分析库失败: {e}"))?;
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| format!("执行失败: {e}"))?;

    let mut rows_out: Vec<Vec<String>> = Vec::new();
    let columns: Vec<String>;
    {
        // duckdb-rs：必须先在 stmt 上执行 query 才能访问列元数据。
        let mut rows = stmt.query([]).map_err(|e| format!("执行失败: {e}"))?;
        columns = (0..stmt.column_count())
            .map(|i| stmt.column_name(i).map(|v| v.to_string()).unwrap_or_else(|_| "unknown".to_string()))
            .collect();
        while let Some(row) = rows.next().map_err(|e| format!("读取行失败: {e}"))? {
            let mut vals = Vec::new();
            for i in 0.. {
                match row.get::<usize, duckdb::types::Value>(i) {
                    Ok(v) => vals.push(value_to_string(&v)),
                    Err(_) => break,
                }
            }
            rows_out.push(vals);
        }
    }

    Ok(QueryOutput {
        columns,
        rows: rows_out,
        row_count: rows_out.len(),
    })'''
assert t.count(old) == 1
t = t.replace(old, new)
p.write_text(t, encoding='utf-8')
print('query-before-columns fixed')
