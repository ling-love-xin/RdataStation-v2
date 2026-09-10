# -*- coding: utf-8 -*-
"""R31：注册 mock_generator 模块 + workbench 加 mock 依赖。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\services\mod.rs')
t = p.read_text(encoding='utf-8')
old = 'pub mod query_runner;\n'
new = 'pub mod mock_generator;\npub mod query_runner;\n'
assert t.count(old) == 1
p.write_text(t.replace(old, new), encoding='utf-8')

c = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\Cargo.toml')
t2 = c.read_text(encoding='utf-8')
old2 = 'shared = { path = "../shared", package = "rds-shared" }\n'
new2 = 'shared = { path = "../shared", package = "rds-shared" }\nmock = { path = "../mock", package = "rds-mock" }\n'
assert t2.count(old2) == 1
c.write_text(t2.replace(old2, new2), encoding='utf-8')
print('registered + dep added')
