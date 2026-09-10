# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src\duckdb\executor.rs")
t = p.read_text(encoding="utf-8")

# 两处（execute_query / execute_query_with_params）：把列名提取移到语句执行（query）之后
old_a = """            let columns: Vec<String> = (0..stmt.column_count())
                .map(|i| stmt.column_name(i).map_or("unknown", |v| v).to_string())
                .collect();

            let row_data: Vec<Vec<duckdb::types::Value>>;"""
new_a = """            let row_data: Vec<Vec<duckdb::types::Value>>;"""
n_a = t.count(old_a)
assert n_a == 2, f"old_a count={n_a}"
t = t.replace(old_a, new_a)

old_b = """                row_data = data;
            }

            if row_data.is_empty() {"""
new_b = """                row_data = data;
            }

            // duckdb-rs 1.10505：列元数据需在语句执行（query）后才能读取
            let columns: Vec<String> = (0..stmt.column_count())
                .map(|i| stmt.column_name(i).map_or("unknown", |v| v).to_string())
                .collect();

            if row_data.is_empty() {"""
n_b = t.count(old_b)
assert n_b == 2, f"old_b count={n_b}"
t = t.replace(old_b, new_b)

p.write_text(t, encoding="utf-8")
print("executor.rs: column metadata after execution")
