# -*- coding: utf-8 -*-
"""修 mock crate engine.rs SqlInsert：锚点切片替换（先收数据 → drop → 取列名）。"""
import pathlib

f = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\mock\src\engine.rs')
t = f.read_text(encoding='utf-8')

a = t.find('                let mut rows = stmt.query([])?;')
b = t.find('                std::fs::write(path, insert_statements.join("\\n"))')
assert a > 0 and b > a, f"anchors: a={a} b={b}"

seg_new = '''                let mut data: Vec<Vec<duckdb::types::Value>> = Vec::new();
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
'''
t = t[:a] + seg_new + t[b:]
f.write_text(t, encoding='utf-8')
print('mock sqlinsert rewritten')
