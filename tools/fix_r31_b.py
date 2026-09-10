# -*- coding: utf-8 -*-
"""二分 B：v_flex 包单 child。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

old = '''                sql_ui = sql_ui.child(
                    div().text_xs().child("Mock 数据（生成测试数据）"),
                );
'''
new = '''                sql_ui = sql_ui.child(
                    div()
                        .v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .child("Mock 数据（生成测试数据）"),
                        ),
                );
'''
assert t.count(old) == 1
t = t.replace(old, new)
p.write_text(t, encoding='utf-8')
print('vflex single child')
