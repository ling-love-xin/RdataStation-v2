# -*- coding: utf-8 -*-
"""外科手术：临时跳过详情卡 if 块（验证崩溃归属）。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

a = t.find('        eprintln!("[dbg] EP-before-detail");')
assert a > 0
b = t.find('        eprintln!("[dbg] EP-detail-done");')
assert b > a
# 把 a..b 整段替换为空
t = t[:a] + '        // [SKIP] 详情卡临时跳过\n' + t[b:]
p.write_text(t, encoding='utf-8')
print('detail card skipped')
