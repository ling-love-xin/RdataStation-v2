# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\migrations\connection_metadata\009_jdbc_metadata_alignment.sql")
t = p.read_text(encoding="utf-8")
old = """INSERT OR REPLACE INTO cache_version (version, description, applied_at)
VALUES (
    9,
    'JDBC DatabaseMetaData alignment: extended columns/tables/indexes/fk/routines/views/triggers/sequences/checks/privileges',
    strftime('%s', 'now')
);"""
new = """-- v1 缺陷修复：cache_version 列名为 version/upgraded_at/upgrade_reason/created_at/updated_at，
-- 无 description/applied_at；改为 UPDATE 版本号
UPDATE cache_version
SET version = 9,
    upgraded_at = strftime('%s', 'now'),
    updated_at = strftime('%s', 'now'),
    upgrade_reason = 'JDBC DatabaseMetaData alignment: extended columns/tables/indexes/fk/routines/views/triggers/sequences/checks/privileges'
WHERE id = 1;"""
assert old in t, "009 cache_version insert not found"
t = t.replace(old, new)
p.write_text(t, encoding="utf-8")
print("009 cache_version fixed")
