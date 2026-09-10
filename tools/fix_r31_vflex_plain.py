# -*- coding: utf-8 -*-
"""测：v_flex+gap_1 包纯文本（无嵌套 div child）。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

# 当前：content 层 v_flex 嵌套双 child——替换为 v_flex+gap_1 包纯文本
old = '''                content = content.child(
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
new = '''                content = content.child(
                    div()
                        .v_flex()
                        .gap_1()
                        .child("Mock 数据（生成测试数据）"),
                );
'''
assert t.count(old) == 1, f'count={t.count(old)}'
p.write_text(t.replace(old, new), encoding='utf-8')
print('vflex + plain text child')
