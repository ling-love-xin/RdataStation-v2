# -*- coding: utf-8 -*-
import pathlib, re
f = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src\driver\native\duckdb.rs")
t = f.read_text(encoding="utf-8")
# db() helper
i = t.find("fn db(")
if i < 0:
    i = t.find("fn db()")
print("===== db() helper")
print(t[i - 300:i + 900])
# is_read_only 赋值处
for mm in re.finditer(r"is_read_only\s*[:=]", t):
    j = mm.start()
    print("-----", j)
    print(t[j - 250:j + 120].replace("\n", " | "))
