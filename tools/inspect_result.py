# -*- coding: utf-8 -*-
import pathlib, re
root = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates")
for f in root.rglob("*.rs"):
    t = f.read_text(encoding="utf-8", errors="ignore")
    if "struct DuckDBResult" in t:
        i = t.find("struct DuckDBResult")
        print(f"===== {f.relative_to(root)}")
        print(t[i:i + 420])
        break
