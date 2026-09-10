# -*- coding: utf-8 -*-
"""测：Button 加 on_click（空闭包）。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')
old = '''            Button::new("new-connection2")
                .primary()
                .label("生成 Mock"),'''
new = '''            Button::new("new-connection2")
                .primary()
                .label("生成 Mock")
                .on_click(|_, _, _| {}),'''
assert t.count(old) == 1, f'count={t.count(old)}'
p.write_text(t.replace(old, new), encoding='utf-8')
print('on_click added (empty)')
