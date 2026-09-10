# -*- coding: utf-8 -*-
"""对照实验：临时摘除 mock 依赖链（Cargo.toml + lib.rs mod + panels mock_section）。"""
import pathlib

# 1. Cargo.toml
c = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\Cargo.toml')
t = c.read_text(encoding='utf-8')
old = 'mock = { path = "../mock", package = "rds-mock" }'
assert t.count(old) == 1
c.write_text(t.replace(old, '#mock = { path = "../mock", package = "rds-mock" }'), encoding='utf-8')
print('Cargo.toml mocked')

# 2. lib.rs mod mock_generator
l = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\lib.rs')
t = l.read_text(encoding='utf-8')
i = t.find('pub mod mock_generator')
assert i > 0, 'mod mock_generator not found'
line_start = t.rfind('\n', 0, i) + 1
line_end = t.find('\n', i)
l.write_text(t[:line_start] + '// ' + t[line_start:line_end] + t[line_end:], encoding='utf-8')
print('lib.rs mod commented')

# 3. panels.rs mock_section（title only 版本）
p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')
a = t.find('                // Round 31：M7 Mock——生成测试数据到分析引擎库（仅落 duckdb，不回传源库）。')
assert a > 0
b = t.find('                content = content.child(sql_ui);', a)
assert b > a
p.write_text(t[:a] + t[b:], encoding='utf-8')
print('panels mock_section removed')
