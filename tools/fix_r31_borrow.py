# -*- coding: utf-8 -*-
"""修 mock crate engine.rs SqlInsert：先收集数据 → drop rows → 再取列名（E0502）。"""
import pathlib

f = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\mock\src\engine.rs')
t = f.read_text(encoding='utf-8')

old = '''                let select_sql = SqlEngine::build_select_all(temp_table_name, None);
                let mut stmt = conn.prepare(&select_sql)?;
                // 列元数据必须在 query 之后读取（duckdb-rs 时序要求）。
                let mut rows = stmt.query([])?;
                let columns: Vec<String> = stmt
                    .column_names()
                    .iter()
                    .map(|c| format!("\\"{}\\"", c))
                    .collect();
                let col_list = columns.join(", ");

                let mut insert_statements = Vec::new();

                while let Some(row) = rows.next()? {
                    let values: Vec<String> = (0..columns.len())
                        .map(|i| {
                            let val: duckdb::types::Value =
                                row.get(i).unwrap_or(duckdb::types::Value::Null);
                            value_to_sql_literal(&val)
                        })
                        .collect();
                    let target_name = table_name
                        .unwrap_or(temp_table_name)
                        .trim_start_matches(TEMP_MOCK_PREFIX);
                    insert_statements.push(format!(
                        "INSERT INTO \\"{}\\" ({}) VALUES ({});",
                        target_name,
                        col_list,
                        values.join(", ")
                    ));
                }
                std::fs::write(path, insert_statements.join("\\n"))?;'''
new = '''                let select_sql = SqlEngine::build_select_all(temp_table_name, None);
                let mut stmt = conn.prepare(&select_sql)?;
                // Rows 持 stmt 可变借用：先收集数据，drop 后再读列名（duckdb-rs 时序要求）。
                let mut data: Vec<Vec<duckdb::types::Value>> = Vec::new();
                {
                    let mut rows = stmt.query([])?;
                    while let Some(row) = rows.next()? {
                        let mut vals = Vec::new();
                        for i in 0.. {
                            match row.get::<usize, duckdb::types::Value>(i) {
                                Ok(v) => vals.push(v),
                                Err(_) => break,
                            }
                        }
                        data.push(vals);
                    }
                }
                let columns: Vec<String> = stmt
                    .column_names()
                    .iter()
                    .map(|c| format!("\\"{}\\"", c))
                    .collect();
                let col_list = columns.join(", ");

                let mut insert_statements = Vec::new();
                for vals in &data {
                    let values: Vec<String> =
                        vals.iter().map(value_to_sql_literal).collect();
                    let target_name = table_name
                        .unwrap_or(temp_table_name)
                        .trim_start_matches(TEMP_MOCK_PREFIX);
                    insert_statements.push(format!(
                        "INSERT INTO \\"{}\\" ({}) VALUES ({});",
                        target_name,
                        col_list,
                        values.join(", ")
                    ));
                }
                std::fs::write(path, insert_statements.join("\\n"))?;'''
assert t.count(old) == 1
t = t.replace(old, new)
f.write_text(t, encoding='utf-8')
print('mock sqlinsert borrow fixed')
