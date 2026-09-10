# -*- coding: utf-8 -*-
"""二分：v_flex+gap_1 包单 child 标题（无说明行）。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

# 当前是 fix_r31_td 版（标题+说明），替换为单 child
old = '''                sql_ui = sql_ui.child(
                    div()
                        .v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child("Mock 数据（生成测试数据）"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child("基于导航树表结构自动生成 50 行，仅落分析引擎库，不回传源库"),
                        ),
                );
'''
new = '''                sql_ui = sql_ui.child(
                    div()
                        .v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child("Mock 数据（生成测试数据）"),
                        ),
                );
'''
assert t.count(old) == 1, f'count={t.count(old)}'
p.write_text(t.replace(old, new), encoding='utf-8')
print('vflex single child (title only)')
