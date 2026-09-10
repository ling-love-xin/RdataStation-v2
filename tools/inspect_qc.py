# -*- coding: utf-8 -*-
import pathlib, re
f = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src\driver\native\duckdb.rs")
t = f.read_text(encoding="utf-8")
for mm in re.finditer(r"QueryResult \{", t):
    j = mm.start()
    ln = t[:j].count("\n") + 1
    print("----- line", ln)
    print(t[j - 200:j + 700].replace("\n", " | "))
