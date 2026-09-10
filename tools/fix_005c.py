# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\migrations\connection_metadata\005_normalized_fts_and_cascade.sql")
t = p.read_text(encoding="utf-8")
old = "-- CREATE INDEX IF NOT EXISTS idx_fts_schema_object ON metadata_fts(schema_name, object_name);"
# 实际文本可能是无注释前缀
for variant in [
    "CREATE INDEX IF NOT EXISTS idx_fts_schema_object ON metadata_fts(schema_name, object_name);",
    "-- CREATE INDEX IF NOT EXISTS idx_fts_schema_object ON metadata_fts(schema_name, object_name);",
]:
    if variant in t:
        t = t.replace(
            variant,
            "-- v1 缺陷修复：FTS5 虚拟表不可建索引（自带倒排索引），移除该语句",
        )
        p.write_text(t, encoding="utf-8")
        print("fts index removed:", variant[:50])
        break
else:
    print("variant not found")
