# -*- coding: utf-8 -*-
import pathlib, re

d = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\migrations\connection_metadata")
for f in ["008_add_is_primary_column.sql", "009_jdbc_metadata_alignment.sql"]:
    p = d / f
    if not p.exists():
        print(f, "MISSING, skip")
        continue
    t = p.read_text(encoding="utf-8")
    m = re.search(
        r"INSERT INTO cache_migration_history \(version, migrated_at, duration_ms, success, error_message\)\s*SELECT\s*\n\s*(\d+),",
        t,
    )
    if not m:
        print(f, "no version-style insert, check manually")
        continue
    ver = int(m.group(1))
    # 捕获整条语句
    start = m.start()
    end = t.find(";", m.end())
    seg = t[start:end + 1]
    reason_m = re.search(r"'([^']*)'", seg)
    reason = reason_m.group(1) if reason_m else f"V{ver}"
    new = (
        f"-- v1 缺陷修复：cache_migration_history 列名为 from_version/to_version/reason\n"
        f"INSERT INTO cache_migration_history (from_version, to_version, migrated_at, reason, duration_ms, success)\n"
        f"SELECT \n"
        f"    {ver - 1},\n"
        f"    {ver},\n"
        f"    strftime('%s', 'now'),\n"
        f"    '{reason}',\n"
        f"    0,\n"
        f"    1\n"
        f"WHERE NOT EXISTS (\n"
        f"    SELECT 1 FROM cache_migration_history WHERE to_version = {ver}\n"
        f");"
    )
    t = t[:start] + new + t[end + 1:]
    p.write_text(t, encoding="utf-8")
    print(f, f"history insert fixed (v{ver})")
