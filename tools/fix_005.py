# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\migrations\connection_metadata\005_normalized_fts_and_cascade.sql")
t = p.read_text(encoding="utf-8")
old = "contentless_delete='true'"
new = "contentless_delete=1"
assert old in t
t = t.replace(old, new)
p.write_text(t, encoding="utf-8")
print("005 fixed")
