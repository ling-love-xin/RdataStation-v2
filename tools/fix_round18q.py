# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src\driver\registry\mod.rs")
lines = p.read_text(encoding="utf-8").splitlines(keepends=True)
# 行号（1-based）-> 期望内容
fixes = {51: "```\n", 89: "```\n"}
for ln, content in fixes.items():
    idx = ln - 1
    if idx < len(lines):
        lines[idx] = content
p.write_text("".join(lines), encoding="utf-8")
print("closure fenced fixed")
