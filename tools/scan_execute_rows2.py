# -*- coding: utf-8 -*-
import pathlib, re
root = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates")
pat = re.compile(r'\.execute\(\s*"(PRAGMA|SELECT)[^"]*"')
for f in root.rglob("*.rs"):
    t = f.read_text(encoding="utf-8", errors="ignore")
    for mm in pat.finditer(t):
        line_no = t[:mm.start()].count("\n") + 1
        # 打印该行及上下文
        lines = t.splitlines()
        ctx = " | ".join(x.strip() for x in lines[max(0, line_no - 2):line_no + 1])
        print(f"{f.relative_to(root)}:{line_no} :: {ctx[:200]}")
