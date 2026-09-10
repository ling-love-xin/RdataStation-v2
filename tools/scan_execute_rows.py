# -*- coding: utf-8 -*-
import pathlib, re
root = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates")
pat = re.compile(r'\.execute\(\s*"(PRAGMA|SELECT|WITH|EXPLAIN|SHOW|VALUES)')
hits = []
for f in root.rglob("*.rs"):
    t = f.read_text(encoding="utf-8", errors="ignore")
    for mm in pat.finditer(t):
        line_no = t[:mm.start()].count("\n") + 1
        snippet = mm.group(0)[:90].replace("\n", " ")
        hits.append((str(f.relative_to(root)), line_no, snippet))
for h in hits:
    print(h)
print("total", len(hits))
