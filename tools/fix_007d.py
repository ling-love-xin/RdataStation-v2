# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\migrations\connection_metadata\007_incremental_sync.sql")
t = p.read_text(encoding="utf-8")
old = """INSERT INTO cache_migration_history (version, migrated_at, duration_ms, success, error_message)
SELECT 
    7,
    strftime('%s', 'now'),
    0,
    1,
    'V7: 增量同步支持，减少 90%+ 预热时间'
WHERE NOT EXISTS (
    SELECT 1 FROM cache_migration_history WHERE version = 7
);"""
new = """-- v1 缺陷修复：cache_migration_history 列名为 from_version/to_version/reason
INSERT INTO cache_migration_history (from_version, to_version, migrated_at, reason, duration_ms, success)
SELECT 
    6,
    7,
    strftime('%s', 'now'),
    'V7: 增量同步支持，减少 90%+ 预热时间',
    0,
    1
WHERE NOT EXISTS (
    SELECT 1 FROM cache_migration_history WHERE to_version = 7
);"""
assert old in t, "007 history insert not found"
t = t.replace(old, new)
p.write_text(t, encoding="utf-8")
print("007 history insert fixed")
