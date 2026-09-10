# -*- coding: utf-8 -*-
"""修 query_runner column_name Result 处理。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\services\query_runner.rs')
t = p.read_text(encoding='utf-8')
old = '.map(|i| stmt.column_name(i).map_or_else(|| "unknown".to_string(), |v| v.to_string()))'
new = '.map(|i| stmt.column_name(i).unwrap_or_else(|_| "unknown".to_string()))'
assert t.count(old) == 1
t = t.replace(old, new)
p.write_text(t, encoding='utf-8')
print('fixed')
