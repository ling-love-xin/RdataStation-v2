# -*- coding: utf-8 -*-
"""修 value_to_string：JSON String 去引号。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\services\query_runner.rs')
t = p.read_text(encoding='utf-8')

old = '''fn value_to_string(v: &duckdb::types::Value) -> String {
    let j = engine::services::duckdb_service::duckdb_value_to_json(v);
    if j.is_null() {
        "NULL".to_string()
    } else {
        j.to_string()
    }
}'''
new = '''fn value_to_string(v: &duckdb::types::Value) -> String {
    let j = engine::services::duckdb_service::duckdb_value_to_json(v);
    match j {
        serde_json::Value::Null => "NULL".to_string(),
        serde_json::Value::String(s) => s,
        other => other.to_string(),
    }
}'''
assert t.count(old) == 1
t = t.replace(old, new)
p.write_text(t, encoding='utf-8')
print('value_to_string fixed')
