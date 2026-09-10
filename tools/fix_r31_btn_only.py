# -*- coding: utf-8 -*-
"""测：v_flex + Button（无嵌套 div child）。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

old = '''                content = content.child(
                    div()
                        .v_flex()
                        .gap_1()
                        .child("Mock 数据（生成测试数据）"),
                );
'''
new = '''                content = content.child(
                    div()
                        .v_flex()
                        .gap_1()
                        .child(
                            Button::new("run-mock")
                                .secondary()
                                .label("生成 Mock（首表）"),
                        ),
                );
'''
assert t.count(old) == 1, f'count={t.count(old)}'
p.write_text(t.replace(old, new), encoding='utf-8')
print('vflex + Button only')
