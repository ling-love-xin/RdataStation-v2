# -*- coding: utf-8 -*-
import pathlib, re
f = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src\driver\native\duckdb.rs")
t = f.read_text(encoding="utf-8")
for mm in re.finditer(r"_is_read_only", t):
    j = mm.start()
    ln = t[:j].count("\n") + 1
    print(ln, "::", t[j - 60:j + 80].replace("\n", " | "))
print("---- is_read_only 构造 ----")
for mm in re.finditer(r"is_read_only\s*:", t):
    j = mm.start()
    ln = t[:j].count("\n") + 1
    print(ln, "::", t[j - 150:j + 40].replace("\n", " | "))
print("---- DuckDBResult 定义 ----")
i = t.find("struct DuckDBResult")
if i < 0:
    i = t.find("pub struct DuckDBResult")
print(t[i:i + 400])
