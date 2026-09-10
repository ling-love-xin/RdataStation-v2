# -*- coding: utf-8 -*-
import pathlib, re
f = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src\driver\native\duckdb.rs")
t = f.read_text(encoding="utf-8")
i = t.find("pub async fn query(")
print("===== query 定义")
print(t[i:i + 900])
print("===== is_read_only 字段声明")
for mm in re.finditer(r"is_read_only", t):
    j = mm.start()
    ln = t[:j].count("\n") + 1
    print(ln, "::", t[j - 90:j + 40].replace("\n", " | "))
