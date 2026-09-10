# -*- coding: utf-8 -*-
"""测：run-mock 按钮 id 改为 new-connection2。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')
t = t.replace('Button::new("run-mock")', 'Button::new("new-connection2")')
p.write_text(t, encoding='utf-8')
print('id -> new-connection2')
