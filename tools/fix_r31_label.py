# -*- coding: utf-8 -*-
"""测：label 去掉全角括号。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

old = '''            Button::new("run-mock")
                .secondary()
                .label("生成 Mock（首表）"),'''
new = '''            Button::new("run-mock")
                .secondary()
                .label("生成 Mock"),'''
assert t.count(old) == 1, f'count={t.count(old)}'
p.write_text(t.replace(old, new), encoding='utf-8')
print('label no parens')
