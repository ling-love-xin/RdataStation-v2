#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""验证实例项目四库内容与 DuckDB 原生格式（非 SQLite 后端）。"""
import pathlib
import sqlite3
import duckdb

EX = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\examples\instance-project")

print("=== project.db (SQLite 元数据) ===")
c = sqlite3.connect(str(EX / ".RSmeta" / "project.db"))
print("integrity:", c.execute("PRAGMA integrity_check").fetchone()[0])
tabs = [r[0] for r in c.execute("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")]
print(f"表 {len(tabs)} 个:", ", ".join(tabs[:14]), "…" if len(tabs) > 14 else "")
print("connections:")
for r in c.execute("SELECT id, name, driver, database FROM connections ORDER BY id"):
    print("  ", r[0], "|", r[1], "|", r[2], "|", r[3])
print("query_history:", c.execute("SELECT COUNT(*) FROM query_history").fetchone()[0], "条")
print(
    "groups:",
    c.execute("SELECT COUNT(*) FROM connection_groups").fetchone()[0],
    "个 / members:",
    c.execute("SELECT COUNT(*) FROM connection_group_members").fetchone()[0],
    "条",
)
print("tags:", c.execute("SELECT COUNT(*) FROM connection_tags").fetchone()[0], "条")
print("迁移账本版本:", c.execute("SELECT MAX(version) FROM schema_version").fetchone()[0])
c.close()

print("=== .RSmeta/analytics.duckdb (分析库) ===")
d = duckdb.connect(str(EX / ".RSmeta" / "analytics.duckdb"))
print("打开库:", d.execute("SELECT database_name, path FROM duckdb_databases()").fetchall())
print(
    "表:",
    [r[0] for r in d.execute("SELECT table_name FROM information_schema.tables ORDER BY table_name").fetchall()],
)
print(
    "query_results:",
    d.execute("SELECT COUNT(*) FROM query_results").fetchone()[0],
    "/ analytics:",
    d.execute("SELECT COUNT(*) FROM analytics").fetchone()[0],
)
d.close()

print("=== data/sample.duckdb (业务库, 原生格式校验) ===")
d2 = duckdb.connect(str(EX / "data" / "sample.duckdb"))
print("打开库:", d2.execute("SELECT database_name, path FROM duckdb_databases()").fetchall())
print(
    "orders:",
    d2.execute("SELECT COUNT(*) FROM orders").fetchone()[0],
    "/ customers:",
    d2.execute("SELECT COUNT(*) FROM customers").fetchone()[0],
)
print(
    "样例 JOIN:",
    d2.execute(
        "SELECT c.segment, COUNT(*) cnt, ROUND(SUM(o.amount),2) total FROM customers c JOIN orders o USING(customer_id) GROUP BY c.segment"
    ).fetchall(),
)
d2.close()
# 原生格式反证：sqlite3 打不开 duckdb 文件 = 非 sqlite 存储后端
try:
    sc = sqlite3.connect(str(EX / "data" / "sample.duckdb"))
    sc.execute("SELECT COUNT(*) FROM sqlite_master").fetchone()
    print("sqlite3 反证: 能打开（说明是 SQLite 格式）⚠")
    sc.close()
except sqlite3.DatabaseError as e:
    print(f"sqlite3 反证: 打不开（{e}）→ 原生 DuckDB 格式 ✓")

print("=== data/sample.sqlite (业务库) ===")
s = sqlite3.connect(str(EX / "data" / "sample.sqlite"))
print(
    "orders:",
    s.execute("SELECT COUNT(*) FROM orders").fetchone()[0],
    "/ customers:",
    s.execute("SELECT COUNT(*) FROM customers").fetchone()[0],
)
s.close()
