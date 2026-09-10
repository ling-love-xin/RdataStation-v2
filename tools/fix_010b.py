# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\migrations\connection_metadata\010_enterprise_statistics_and_display.sql")
t = p.read_text(encoding="utf-8")
old = """INSERT INTO cache_migration_history (version, migrated_at, duration_ms, success, error_message)
SELECT
    10,
    strftime('%s', 'now'),
    0,
    1,
    'V10: 企业级统计 + 显示控制: schemata(+6聚合列), tables(+5显示控制列), schema_stats视图, connection_stats视图'
WHERE NOT EXISTS (
    SELECT 1 FROM cache_migration_history WHERE version = 10
);"""
new = """-- v1 缺陷修复：cache_migration_history 列名为 from_version/to_version/reason
INSERT INTO cache_migration_history (from_version, to_version, migrated_at, reason, duration_ms, success)
SELECT
    9,
    10,
    strftime('%s', 'now'),
    'V10: 企业级统计 + 显示控制: schemata(+6聚合列), tables(+5显示控制列), schema_stats视图, connection_stats视图',
    0,
    1
WHERE NOT EXISTS (
    SELECT 1 FROM cache_migration_history WHERE to_version = 10
);"""
assert old in t, "010 history insert not found"
t = t.replace(old, new)
p.write_text(t, encoding="utf-8")
print("010 history insert fixed")
