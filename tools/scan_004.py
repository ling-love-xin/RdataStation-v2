# -*- coding: utf-8 -*-
import pathlib, re
f = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\migrations\connection_metadata\004_refactor_to_normalized.sql")
t = f.read_text(encoding="utf-8")
allowed = {
    "id", "obj_type", "database_name", "schema_name", "table_name", "name",
    "data_type", "is_nullable", "is_primary", "is_unique", "comment", "definition",
    "extra", "last_sync", "cache_version", "is_compressed", "original_size",
    "introspect_level", "is_loaded", "last_accessed",
}
# 提取 metadata 上下文的列引用：metadata.col 与 FROM metadata 后 SELECT 里的裸列
cols = set()
for mm in re.finditer(r"metadata\.(\w+)", t):
    cols.add(mm.group(1))
# 裸列引用（排除常见 SQL 关键字/函数）
for mm in re.finditer(r"COALESCE\((\w+)", t):
    if mm.group(1) != "name":
        cols.add("COALESCE:" + mm.group(1))
bad = sorted(c for c in cols if c not in allowed and not c.startswith("COALESCE:"))
bad2 = sorted(c for c in cols if c.startswith("COALESCE:") and c.split(":")[1] not in allowed)
print("metadata.* 引用:", sorted(cols))
print("非法列:", bad, bad2)
