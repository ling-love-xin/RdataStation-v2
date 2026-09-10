# -*- coding: utf-8 -*-
"""二分：Mock 小节整体替换为单行文本。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

a = t.find('                // Round 31：M7 Mock——生成测试数据到分析引擎库（仅落 duckdb，不回传源库）。')
assert a > 0, 'anchor not found'
# 从注释到 content = content.child(sql_ui); 前
b = t.find('                content = content.child(sql_ui);', a)
assert b > a
seg = '''                sql_ui = sql_ui.child(
                    div().text_xs().child("Mock 数据（生成测试数据）"),
                );

'''
t = t[:a] + seg + t[b:]
p.write_text(t, encoding='utf-8')
print('mock section -> single text')
