# -*- coding: utf-8 -*-
import pathlib, re
root = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates")
for f in root.rglob("*.rs"):
    t = f.read_text(encoding="utf-8", errors="ignore")
    if re.search(r"(pub\s+)?struct QueryResult\b", t):
        for mm in re.finditer(r"(pub\s+)?struct QueryResult\b", t):
            j = mm.start()
            print(f"===== {f.relative_to(root)}")
            print(t[j - 150:j + 500])
        break
