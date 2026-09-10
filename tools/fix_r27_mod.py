# -*- coding: utf-8 -*-
"""注册 query_export 模块。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\services\mod.rs')
t = p.read_text(encoding='utf-8')
old = 'pub mod query_runner;\n'
new = 'pub mod query_export;\npub mod query_runner;\n'
assert t.count(old) == 1
t = t.replace(old, new)
p.write_text(t, encoding='utf-8')
print('query_export registered')
