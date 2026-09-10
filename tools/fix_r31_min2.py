# -*- coding: utf-8 -*-
"""降级：完整 Mock 小节 → 单行文本（验证内存恢复后单行是否 OK）。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

a = t.find('                // Round 31：M7 Mock——基于导航树首表生成 50 行测试数据')
assert a > 0, f'a={a}'
b = t.find('                content = content.child(sql_ui);', a)
assert b > a, f'b={b}'
seg = '''                sql_ui = sql_ui.child(
                    div().text_xs().child("Mock 数据（生成测试数据）"),
                );

'''
t = t[:a] + seg + t[b:]
p.write_text(t, encoding='utf-8')
print('mock -> single text')
