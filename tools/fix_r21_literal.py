# -*- coding: utf-8 -*-
"""修复 panels.rs 空态文案字符字面量。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')
bad = "unwrap_or_else(|| '暂无连接，请先在「数据源连接」中创建。'.to_string())"
good = 'unwrap_or_else(|| "暂无连接，请先在「数据源连接」中创建。".to_string())'
assert bad in t, 'bad literal not found'
t = t.replace(bad, good)
p.write_text(t, encoding='utf-8')
print('literal fixed')
