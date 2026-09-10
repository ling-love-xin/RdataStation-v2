# -*- coding: utf-8 -*-
"""测：Button 直接 content child（无 div 包裹）。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

old = '''                content = content.child(
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
new = '''                content = content.child(
                    Button::new("run-mock")
                        .secondary()
                        .label("生成 Mock（首表）"),
                );
'''
assert t.count(old) == 1, f'count={t.count(old)}'
p.write_text(t.replace(old, new), encoding='utf-8')
print('Button direct content child')
