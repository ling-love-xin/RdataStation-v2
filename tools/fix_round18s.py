# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src\driver\registry\mod.rs")
lines = p.read_text(encoding="utf-8").splitlines(keepends=True)
for ln in (51, 89):
    idx = ln - 1
    if idx < len(lines) and lines[idx].strip() == "```":
        lines[idx] = "/// ```\n"
p.write_text("".join(lines), encoding="utf-8")
print("doc fences fixed")
