# -*- coding: utf-8 -*-
import pathlib, re
t = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\migrations\connection_metadata\004_refactor_to_normalized.sql").read_text(encoding="utf-8")
i = t.find("view_definitions")
j = t.find("CREATE TABLE", i)
print(t[j:j + 700])
print("=== 005 views 段")
t5 = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\migrations\connection_metadata\005_normalized_fts_and_cascade.sql").read_text(encoding="utf-8")
k = t5.find("SELECT 'view'")
print(t5[k - 200:k + 400])
