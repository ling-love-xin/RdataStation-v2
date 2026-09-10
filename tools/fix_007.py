# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\migrations\connection_metadata\007_incremental_sync.sql")
t = p.read_text(encoding="utf-8")
old = "ALTER TABLE views  ADD COLUMN object_hash TEXT;"
new = "-- v1 缺陷修复：无 views 表（视图在 tables，table_type='VIEW'），移除该 ALTER"
assert old in t, "views alter not found"
t = t.replace(old, new)
p.write_text(t, encoding="utf-8")
print("007 views alter removed")
