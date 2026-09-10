# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\migrations\connection_metadata\006_add_metadata_index.sql")
t = p.read_text(encoding="utf-8")
old = """-- 迁移 views 到索引表
INSERT OR IGNORE INTO metadata_index (
    connection_id, schema_id, object_type, object_name, parent_name, path,
    introspect_level, is_loaded, last_sync, sort_weight
)
SELECT 
    'legacy' as connection_id,
    v.schema_id,
    'view' as object_type,
    v.view_name as object_name,
    s.schema_name as parent_name,
    s.schema_name || '/' || v.view_name as path,
    3 as introspect_level,
    1 as is_loaded,
    v.last_sync,
    80 as sort_weight
FROM views v
INNER JOIN schemata s ON v.schema_id = s.id;"""
new = """-- 迁移 views 到索引表
-- v1 缺陷修复：规范化模型无 views 表，视图是 tables 的一种（table_type='VIEW'）
INSERT OR IGNORE INTO metadata_index (
    connection_id, schema_id, object_type, object_name, parent_name, path,
    introspect_level, is_loaded, last_sync, row_count_estimate, sort_weight
)
SELECT 
    'legacy' as connection_id,
    t.schema_id,
    'view' as object_type,
    t.table_name as object_name,
    s.schema_name as parent_name,
    s.schema_name || '/' || t.table_name as path,
    COALESCE(t.introspect_level, 3) as introspect_level,
    COALESCE(t.is_loaded, 1) as is_loaded,
    t.last_sync,
    t.row_count_estimate,
    80 as sort_weight
FROM tables t
INNER JOIN schemata s ON t.schema_id = s.id
WHERE t.table_type = 'VIEW';"""
assert old in t, "006 views insert not found"
t = t.replace(old, new)
p.write_text(t, encoding="utf-8")
print("006 views fixed")
