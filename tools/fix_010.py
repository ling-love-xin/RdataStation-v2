# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\migrations\connection_metadata\010_enterprise_statistics_and_display.sql")
t = p.read_text(encoding="utf-8")
i = t.find("INSERT OR REPLACE INTO cache_version (version, description, applied_at)")
assert i >= 0
j = t.find(";", i)
seg = t[i:j + 1]
new = """-- v1 缺陷修复：cache_version 列名为 version/upgraded_at/upgrade_reason/created_at/updated_at
UPDATE cache_version
SET version = 10,
    upgraded_at = strftime('%s', 'now'),
    updated_at = strftime('%s', 'now'),
    upgrade_reason = 'Enterprise metadata statistics: schema-level aggregates (total_tables/views/size/rows), display control (order/hidden/favorite/color), schema_stats & connection_stats views'
WHERE id = 1;"""
t = t[:i] + new + t[j + 1:]
p.write_text(t, encoding="utf-8")
print("010 cache_version fixed")
