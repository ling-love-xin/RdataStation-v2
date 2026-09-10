# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\migrations\connection_metadata\004_refactor_to_normalized.sql")
t = p.read_text(encoding="utf-8")
old = """-- 重建 FTS 索引以包含新数据
INSERT INTO tables_fts(tables_fts, schema_name, table_name, table_comment)
SELECT schema_name, schema_name, table_name, table_comment FROM tables;"""
new = """-- 重建 FTS 索引以包含新数据
-- v1 缺陷修复：tables 表无 schema_name 列（schema_name 在 schemata），需 JOIN
INSERT INTO tables_fts(tables_fts, schema_name, table_name, table_comment)
SELECT 'rebuild', s.schema_name, t.table_name, t.table_comment
FROM tables t INNER JOIN schemata s ON t.schema_id = s.id;"""
assert old in t, "fts insert not found"
t = t.replace(old, new)
p.write_text(t, encoding="utf-8")
print("004 fts fixed")
