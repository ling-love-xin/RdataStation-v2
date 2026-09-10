# -*- coding: utf-8 -*-
"""测：run-mock 移到 if 块外（new-connection 旁，content 顶层）。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

# 1. 移除块内的 div+on_click Mock 小节
a = t.find('''                content = content.child(
                    div()
                        .id("run-mock-div")''')
assert a > 0, f'a={a}'
# 找到收尾 '                );'
end = t.find('                );', a)
assert end > a
t = t[:a] + t[end + len('                );'):]

# 2. 在 new-connection 按钮前插入 run-mock 按钮（块外）
old = '''        content = content.child(
            Button::new("new-connection")'''
new = '''        content = content.child(
            Button::new("run-mock")
                .secondary()
                .label("生成 Mock（首表）"),
        );

        content = content.child(
            Button::new("new-connection")'''
assert t.count(old) == 1, f'count={t.count(old)}'
t = t.replace(old, new)
p.write_text(t, encoding='utf-8')
print('run-mock moved outside if block')
