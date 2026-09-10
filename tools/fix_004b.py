# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\migrations\connection_metadata\004_refactor_to_normalized.sql")
t = p.read_text(encoding="utf-8")
old = "COALESCE(name, chr(39)||chr(39)) as column_name,  -- v1 缺陷修复：metadata 表无 column_name 列，原名 name"
new = "COALESCE(name, '') as column_name,  -- v1 缺陷修复：metadata 表无 column_name 列，原名 name"
assert old in t
t = t.replace(old, new)
p.write_text(t, encoding="utf-8")
print("004 fixed")
