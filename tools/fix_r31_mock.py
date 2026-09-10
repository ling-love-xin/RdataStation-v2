# -*- coding: utf-8 -*-
"""修 mock crate engine.rs：SqlInsert export 列名必须在 query 之后（R26 同类坑）。

同时修 parse_data_type 的 DECIMAL 匹配（starts_with）。
"""
import pathlib

# 1) mock crate：column_names 移到 query 之后
f = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\mock\src\engine.rs')
t = f.read_text(encoding='utf-8')

old = '''                let select_sql = SqlEngine::build_select_all(temp_table_name, None);
                let mut stmt = conn.prepare(&select_sql)?;
                let columns: Vec<String> = stmt
                    .column_names()
                    .iter()
                    .map(|c| format!("\\"{}\\"", c))
                    .collect();
                let col_list = columns.join(", ");

                let mut rows = stmt.query([])?;
                let mut insert_statements = Vec::new();

                while let Some(row) = rows.next()? {'''
new = '''                let select_sql = SqlEngine::build_select_all(temp_table_name, None);
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

                while let Some(row) = rows.next()? {'''
assert t.count(old) == 1
t = t.replace(old, new)
f.write_text(t, encoding='utf-8')
print('mock engine fixed')

# 2) parse_data_type：DECIMAL starts_with
p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\services\mock_generator.rs')
t2 = p.read_text(encoding='utf-8')
old2 = "} else if up == \"DECIMAL\" || up == \"NUMERIC\" {"
new2 = "} else if up.starts_with(\"DECIMAL\") || up == \"NUMERIC\" {"
assert t2.count(old2) == 1
t2 = t2.replace(old2, new2)
p.write_text(t2, encoding='utf-8')
print('parse_data_type fixed')
