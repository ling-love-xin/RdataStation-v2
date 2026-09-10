# -*- coding: utf-8 -*-
"""临时移除 Mock 按钮（二分定位 stack overflow）。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

a = t.find('                                        // Round 31：M7 mock——一键生成 50 行测试数据到分析引擎库（不回传源库）。')
b = t.find('                                )', a)
assert a > 0 and b > a, f"anchors a={a} b={b}"
# 找到按钮块结束：按钮块是 .child(Button::new(...)) 结构，从 a 到其收尾 '                                        ),'
end = t.find('                                        ),', a)
assert end > a
t = t[:a] + t[end + len('                                        ),'):]
p.write_text(t, encoding='utf-8')
print('mock button removed')
