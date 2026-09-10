# -*- coding: utf-8 -*-
"""测：id 改为完全独立前缀 mock-generate-btn。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')
t = t.replace('Button::new("new-connection2")', 'Button::new("mock-generate-btn")')
p.write_text(t, encoding='utf-8')
print('id -> mock-generate-btn')
