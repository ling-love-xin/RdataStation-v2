# -*- coding: utf-8 -*-
"""修 query_runner：收集数据后 drop rows 再取列名；row_count 先算。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\services\query_runner.rs')
t = p.read_text(encoding='utf-8')

old = '''    let mut rows_out: Vec<Vec<String>> = Vec::new();
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
new = '''    // duckdb-rs：必须先在 stmt 上执行 query 才能访问列元数据；
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
    })'''
assert t.count(old) == 1
t = t.replace(old, new)
p.write_text(t, encoding='utf-8')
print('stmt borrow fixed')
