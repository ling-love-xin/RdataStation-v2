# -*- coding: utf-8 -*-
"""分析 panels.rs 括号结构。"""
import pathlib
import re

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
lines = p.read_text(encoding='utf-8').splitlines()

# 找到详情卡片的 content = content.child( 起点（'选中连接查看真实元数据' 附近）
start = None
for k, l in enumerate(lines):
    if 'content = content.child(' in l and k > 330:
        # 该行后应跟 div() 详情卡片
        start = k
        break
print('start at line', start + 1)

depth = 0
for k in range(start, min(start + 160, len(lines))):
    raw = lines[k]
    s = re.sub(r'"[^"]*"', '""', raw)  # 去字符串
    s = re.sub(r"'[^']*'", "''", s)
    for ch in s:
        if ch == '(':
            depth += 1
        elif ch == ')':
            depth -= 1
        elif ch == '{':
            depth += 1
        elif ch == '}':
            depth -= 1
    if k >= start:
        flag = '  <-- 0' if depth == 0 and k > start else ''
        print('%4d %5d %s%s' % (k + 1, depth, raw[:88], flag))
        if depth == 0 and k > start:
            break
