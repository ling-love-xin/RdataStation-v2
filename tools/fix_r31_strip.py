# -*- coding: utf-8 -*-
"""二分：Mock 小节去掉 on_click（只渲染标题+按钮），验证是否渲染结构问题。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

a = t.find('                        .child(\n                            Button::new("run-mock")')
assert a > 0, "anchor not found"
# 找到 on_click 块结束：从 a 到 '                                ),\n                        ),' 的收尾
# on_click({ ... }) 结束标记：'                                }\n                                }),\n                        ),'
end = t.find('                                        entity_mock.update(app, |_, cx| cx.notify());\n                                    }\n                                }),', a)
assert end > a
# 替换：把 on_click({...}) 整块换成无闭包
seg_old = t[a:end + len('                                        entity_mock.update(app, |_, cx| cx.notify());\n                                    }\n                                }),')]
seg_new = '''                        .child(
                            Button::new("run-mock")
                                .secondary()
                                .label("生成 Mock（首表）"),'''
t = t[:a] + seg_new + t[end + len('                                        entity_mock.update(app, |_, cx| cx.notify());\n                                    }\n                                }),'):]
p.write_text(t, encoding='utf-8')
print('on_click stripped')
