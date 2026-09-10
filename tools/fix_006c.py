# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\migrations\connection_metadata\006_add_metadata_index.sql")
t = p.read_text(encoding="utf-8")
old = """INSERT INTO cache_migration_history (version, migrated_at, duration_ms, success, error_message)
SELECT 
    6,
    strftime('%s', 'now'),
    0,
    1,
    'V6: 索引表支持分页懒加载、预热状态跟踪'
WHERE NOT EXISTS (
    SELECT 1 FROM cache_migration_history WHERE version = 6
);"""
new = """-- v1 缺陷修复：cache_migration_history 列名为 from_version/to_version/reason，
-- 无 version/error_message 列
INSERT INTO cache_migration_history (from_version, to_version, migrated_at, reason, duration_ms, success)
SELECT 
    5,
    6,
    strftime('%s', 'now'),
    'V6: 索引表支持分页懒加载、预热状态跟踪',
    0,
    1
WHERE NOT EXISTS (
    SELECT 1 FROM cache_migration_history WHERE to_version = 6
);"""
assert old in t, "006 history insert not found"
t = t.replace(old, new)
p.write_text(t, encoding="utf-8")
print("006 history insert fixed")
