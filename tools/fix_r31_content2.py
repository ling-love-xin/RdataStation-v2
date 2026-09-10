# -*- coding: utf-8 -*-
"""方案 B 修正：用 Mock 数据独特锚点移除 sql_ui 小节，追加到 content 层。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

# 1. 移除 sql_ui 末尾的 Mock 小节（以 Mock 数据 为锚）
a = t.find('''                sql_ui = sql_ui.child(
                    div()
                        .v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child("Mock 数据（生成测试数据）"),
                        ),
                );''')
assert a > 0, f'a={a}'
b = a + len('''                sql_ui = sql_ui.child(
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

''')
t = t[:a] + t[b:]

# 2. content = content.child(sql_ui); 后追加
old2 = '''                content = content.child(sql_ui);
            }
        }
'''
new2 = '''                content = content.child(sql_ui);
                content = content.child(
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
            }
        }
'''
assert t.count(old2) == 1, f'count2={t.count(old2)}'
t = t.replace(old2, new2)
p.write_text(t, encoding='utf-8')
print('mock moved to content level (fixed)')
