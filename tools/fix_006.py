# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\migrations\connection_metadata\006_add_metadata_index.sql")
t = p.read_text(encoding="utf-8")
old = """CREATE TABLE IF NOT EXISTS sync_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    connection_id TEXT NOT NULL,"""
new = """-- v1 缺陷修复：001_init.sql 已建旧结构 sync_log（start_at/end_at/message，
-- 无 connection_id），IF NOT EXISTS 会跳过导致下方索引报 no such column。
-- sync_log 为日志表，先 DROP 再按新结构重建。
DROP TABLE IF EXISTS sync_log;
CREATE TABLE IF NOT EXISTS sync_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    connection_id TEXT NOT NULL,"""
assert old in t, "sync_log create not found"
t = t.replace(old, new)
p.write_text(t, encoding="utf-8")
print("006 sync_log fixed")
