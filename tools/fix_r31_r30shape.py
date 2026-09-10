# -*- coding: utf-8 -*-
"""恢复 R30 形态（mock 依赖保留、panels 删 mock_section），干净对照。"""
import pathlib

# 1. Cargo.toml 恢复
c = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\Cargo.toml')
t = c.read_text(encoding='utf-8')
old = '#mock = { path = "../mock", package = "rds-mock" }'
assert t.count(old) == 1
c.write_text(t.replace(old, 'mock = { path = "../mock", package = "rds-mock" }'), encoding='utf-8')
print('Cargo.toml restored')

# 2. panels.rs 删 mock_section（title-only 版本）
p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')
a = t.find('                // Round 31：M7 Mock——生成测试数据到分析引擎库（仅落 duckdb，不回传源库）。')
assert a > 0, f'a={a}'
b = t.find('                content = content.child(sql_ui);', a)
assert b > a, f'b={b}'
p.write_text(t[:a] + t[b:], encoding='utf-8')
print('panels mock_section removed (R30 shape)')

# 3. 保留打点（后续验证 OK 后再清）
print('done')
