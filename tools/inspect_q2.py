# -*- coding: utf-8 -*-
import pathlib, re
f = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src\driver\native\duckdb.rs")
t = f.read_text(encoding="utf-8")
print("===== fn query")
for mm in re.finditer(r"fn query", t):
    j = mm.start()
    ln = t[:j].count("\n") + 1
    print(ln, "::", t[j - 80:j + 260].replace("\n", " | "))
print("===== is_read_only 结构字段（pub）")
for mm in re.finditer(r"pub\s+is_read_only", t):
    j = mm.start()
    ln = t[:j].count("\n") + 1
    print(ln, "::", t[j - 200:j + 60].replace("\n", " | "))
print("===== struct ...Result")
for mm in re.finditer(r"struct \w*Result\w* \{", t):
    j = mm.start()
    ln = t[:j].count("\n") + 1
    print(ln, "::", t[j - 80:j + 320].replace("\n", " | "))
