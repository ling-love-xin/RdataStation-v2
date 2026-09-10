# -*- coding: utf-8 -*-
"""修 R29 E0382：sql_state 提前克隆 sql_state_hist 给历史按钮。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

old = 'let sql_state = self.sql_textarea.clone().expect("lazy init");'
new = 'let sql_state = self.sql_textarea.clone().expect("lazy init");\n                let sql_state_hist = sql_state.clone();'
assert t.count(old) == 1
t = t.replace(old, new)

old2 = 'let st = sql_state.clone();'
new2 = 'let st = sql_state_hist.clone();'
assert t.count(old2) == 1
t = t.replace(old2, new2)

p.write_text(t, encoding='utf-8')
print('sql_state_hist fixed')
