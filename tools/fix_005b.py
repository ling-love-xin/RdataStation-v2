# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\migrations\connection_metadata\005_normalized_fts_and_cascade.sql")
t = p.read_text(encoding="utf-8")
old = """-- 插入 views
INSERT INTO metadata_fts (search_type, schema_name, object_name, parent_name, search_content)
SELECT 'view', s.schema_name, v.view_name, s.schema_name,
       s.schema_name || ' ' || v.view_name || ' ' || COALESCE(v.view_comment, '')
FROM views v
INNER JOIN schemata s ON v.schema_id = s.id
WHERE v.is_loaded = 1;"""
new = """-- 插入 views
-- v1 缺陷修复：规范化模型无独立 views 表，视图是 tables 的一种（table_type='VIEW'）
INSERT INTO metadata_fts (search_type, schema_name, object_name, parent_name, search_content)
SELECT 'view', s.schema_name, t.table_name, s.schema_name,
       s.schema_name || ' ' || t.table_name || ' ' || COALESCE(t.table_comment, '')
FROM tables t
INNER JOIN schemata s ON t.schema_id = s.id
WHERE t.table_type = 'VIEW' AND t.is_loaded = 1;"""
assert old in t, "views insert not found"
t = t.replace(old, new)
p.write_text(t, encoding="utf-8")
print("005 views fixed")
