#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""构建 RDS 实例项目（示例项目）数据支持结构。

在 examples/instance-project/ 下生成：
  data/sample.sqlite              —— 业务示例数据（SQLite：customers + orders）
  data/sample.duckdb              —— 业务示例数据（DuckDB：customers + orders）
  .RSmeta/project.db              —— 项目元数据（SQLite：按 crates/engine/migrations/project_meta/ 原样迁移 + 示例连接）
  .RSmeta/analytics.duckdb        —— 项目分析库（DuckDB：按 migrations/project_analysis/ 迁移 + 示例分析记录）

幂等：迁移 SQL 均为 IF NOT EXISTS；示例数据用 INSERT OR REPLACE（连接按 id 覆盖）。
迁移账本：schema_version（version/name/applied_at），与 RDS MigrationManager 同构。
"""
import pathlib
import sqlite3
import time

REPO = pathlib.Path(__file__).resolve().parent.parent
EX = REPO / "examples" / "instance-project"
MIG_PROJECT = REPO / "crates" / "engine" / "migrations" / "project_meta"
MIG_ANALYSIS = REPO / "crates" / "engine" / "migrations" / "project_analysis"
DATA_DIR = EX / "data"
RSMETA = EX / ".RSmeta"

NOW_SQLITE = "2026-10-03 10:00:00"
NOW_ISO = "2026-10-03T10:00:00Z"


def run_migrations(conn, mig_dir, backend):
    """按版本顺序执行**未应用**的迁移 SQL + 记录账本（与 RDS MigrationManager 语义一致）。

    backend: 'sqlite' 用 executescript；'duckdb' 用 execute（duckdb python 无 executescript）。
    单文件失败仅告警（与 016 兜底设计一致）。
    """
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at INTEGER NOT NULL)"
    )
    max_applied = conn.execute("SELECT COALESCE(MAX(version), 0) FROM schema_version").fetchone()[0]
    files = sorted(mig_dir.glob("*.sql"), key=lambda p: int(p.name.split("_", 1)[0]))
    applied = 0
    skipped = 0
    for f in files:
        version = int(f.name.split("_", 1)[0])
        if version <= max_applied:
            skipped += 1
            continue
        name = f.name.split("_", 1)[1][:-4]
        try:
            sql = f.read_text(encoding="utf-8")
            if backend == "duckdb":
                conn.execute(sql)  # duckdb 支持多语句批处理
            else:
                conn.executescript(sql)
            conn.execute(
                "INSERT OR REPLACE INTO schema_version VALUES (?, ?, ?)",
                (version, name, int(time.time())),
            )
            applied += 1
        except Exception as e:  # noqa: BLE001 —— 与 RDS 016 兜底语义一致，失败继续
            print(f"  [warn] {backend} 迁移 {version}_{name} 失败（已跳过）: {e}")
    print(f"  {backend} 迁移：本次应用 {applied} / 已跳过 {skipped} / 共 {len(files)} 个")
    return applied


def build_business_sqlite(path: pathlib.Path):
    """业务示例库：customers + orders（销售场景）。"""
    conn = sqlite3.connect(path)
    conn.executescript(
        """
        CREATE TABLE IF NOT EXISTS customers (
            customer_id INTEGER PRIMARY KEY,
            name        TEXT NOT NULL,
            segment     TEXT,
            country     TEXT,
            city        TEXT
        );
        CREATE TABLE IF NOT EXISTS orders (
            order_id    INTEGER PRIMARY KEY,
            customer_id INTEGER NOT NULL,
            region      TEXT,
            amount      DECIMAL(12,2),
            status      TEXT,
            created_at  TEXT
        );
        """
    )
    conn.execute("DELETE FROM customers")
    conn.execute("DELETE FROM orders")
    conn.executemany(
        "INSERT INTO customers VALUES (?, ?, ?, ?, ?)",
        [
            (101, "张伟", "企业", "中国", "上海"),
            (102, "李娜", "个人", "中国", "北京"),
            (103, "Alice Chen", "企业", "美国", "New York"),
            (104, "王强", "个人", "中国", "深圳"),
            (105, "Bob Miller", "企业", "德国", "Berlin"),
            (106, "赵敏", "个人", "中国", "成都"),
        ],
    )
    conn.executemany(
        "INSERT INTO orders VALUES (?, ?, ?, ?, ?, ?)",
        [
            (1, 101, "华东", 1299.00, "paid", "2026-08-01 09:12:44"),
            (2, 102, "华北", 88.50, "paid", "2026-08-01 09:15:02"),
            (3, 103, "北美", 2450.00, "pending", "2026-08-02 11:03:19"),
            (4, 104, "华南", 560.00, "paid", "2026-08-03 14:22:51"),
            (5, 101, "华东", 320.00, "refunded", "2026-08-04 08:40:10"),
            (6, 105, "欧洲", 7800.00, "paid", "2026-08-05 16:05:33"),
            (7, 106, "西南", 150.00, "paid", "2026-08-06 10:30:00"),
            (8, 102, "华北", 990.00, "pending", "2026-08-07 13:47:28"),
            (9, 103, "北美", 3100.00, "paid", "2026-08-08 09:02:15"),
            (10, 104, "华南", 210.00, "paid", "2026-08-09 17:55:42"),
            (11, 105, "欧洲", 520.00, "refunded", "2026-08-10 12:18:03"),
            (12, 106, "西南", 880.00, "paid", "2026-08-11 15:33:27"),
        ],
    )
    conn.commit()
    conn.close()
    print(f"  业务 SQLite: {path}（customers 6 / orders 12）")


def build_business_duckdb(path: pathlib.Path):
    """业务示例库（DuckDB 1.5.5，与 RDS 内核同版本）。"""
    import duckdb

    conn = duckdb.connect(str(path))
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS customers (
            customer_id INTEGER PRIMARY KEY,
            name        VARCHAR NOT NULL,
            segment     VARCHAR,
            country     VARCHAR,
            city        VARCHAR
        );
        CREATE TABLE IF NOT EXISTS orders (
            order_id    INTEGER PRIMARY KEY,
            customer_id INTEGER NOT NULL,
            region      VARCHAR,
            amount      DECIMAL(12,2),
            status      VARCHAR,
            created_at  TIMESTAMP
        );
        """
    )
    conn.execute("DELETE FROM customers")
    conn.execute("DELETE FROM orders")
    conn.executemany(
        "INSERT INTO customers VALUES (?, ?, ?, ?, ?)",
        [
            (101, "张伟", "企业", "中国", "上海"),
            (102, "李娜", "个人", "中国", "北京"),
            (103, "Alice Chen", "企业", "美国", "New York"),
            (104, "王强", "个人", "中国", "深圳"),
            (105, "Bob Miller", "企业", "德国", "Berlin"),
            (106, "赵敏", "个人", "中国", "成都"),
        ],
    )
    conn.executemany(
        "INSERT INTO orders VALUES (?, ?, ?, ?, ?, ?)",
        [
            (1, 101, "华东", 1299.00, "paid", "2026-08-01 09:12:44"),
            (2, 102, "华北", 88.50, "paid", "2026-08-01 09:15:02"),
            (3, 103, "北美", 2450.00, "pending", "2026-08-02 11:03:19"),
            (4, 104, "华南", 560.00, "paid", "2026-08-03 14:22:51"),
            (5, 101, "华东", 320.00, "refunded", "2026-08-04 08:40:10"),
            (6, 105, "欧洲", 7800.00, "paid", "2026-08-05 16:05:33"),
            (7, 106, "西南", 150.00, "paid", "2026-08-06 10:30:00"),
            (8, 102, "华北", 990.00, "pending", "2026-08-07 13:47:28"),
            (9, 103, "北美", 3100.00, "paid", "2026-08-08 09:02:15"),
            (10, 104, "华南", 210.00, "paid", "2026-08-09 17:55:42"),
            (11, 105, "欧洲", 520.00, "refunded", "2026-08-10 12:18:03"),
            (12, 106, "西南", 880.00, "paid", "2026-08-11 15:33:27"),
        ],
    )
    conn.close()
    print(f"  业务 DuckDB: {path}（customers 6 / orders 12）")


def build_project_db():
    """项目元数据 SQLite：迁移 + 示例连接 / 历史 / 设置 / 分组标签。"""
    db_path = RSMETA / "project.db"
    conn = sqlite3.connect(db_path)
    run_migrations(conn, MIG_PROJECT, "sqlite")

    # 列自适应：按最终 schema 取 connections 的列名（不硬编码）。
    cols = [r[1] for r in conn.execute("PRAGMA table_info(connections)").fetchall()]
    print(f"  connections 最终列: {cols}")

    def ins(table, row: dict):
        keys = [k for k in row if k in cols] if table == "connections" else list(row)
        conn.execute(
            f"INSERT OR REPLACE INTO {table} ({','.join(keys)}) VALUES ({','.join('?' * len(keys))})",
            [row[k] for k in keys],
        )

    sqlite_abs = str((DATA_DIR / "sample.sqlite").resolve())
    duckdb_abs = str((DATA_DIR / "sample.duckdb").resolve())

    conn.execute(
        "INSERT OR REPLACE INTO project (id, name, description, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
        ("prj-instance-001", "RDS 实例项目", "演示项目数据支持结构：SQLite 元数据 + DuckDB 分析库 + 业务示例库", NOW_SQLITE, NOW_SQLITE),
    )

    common = dict(created_at=NOW_SQLITE, updated_at=NOW_SQLITE, is_active=1)
    ins("connections", {
        "id": "P_conn_sample_sqlite", "name": "实例 SQLite（销售库）", "driver": "sqlite",
        "host": None, "port": None, "database": sqlite_abs, "schema_name": None,
        "username": None, "password_encrypted": None, "options": None,
        "tags": '["demo","local","sqlite"]', "use_duckdb_fed": 0, "metadata_path": None,
        "description": "项目内业务示例库（SQLite）：customers/orders，可直接连接浏览", **common,
    })
    ins("connections", {
        "id": "P_conn_sample_duckdb", "name": "实例 DuckDB（销售库）", "driver": "duckdb",
        "host": None, "port": None, "database": duckdb_abs, "schema_name": None,
        "username": None, "password_encrypted": None, "options": None,
        "tags": '["demo","local","duckdb"]', "use_duckdb_fed": 1, "metadata_path": None,
        "description": "项目内业务示例库（DuckDB）：customers/orders，已开启联邦（可作为分析源）", **common,
    })
    ins("connections", {
        "id": "P_conn_sample_pg_refused", "name": "实例 PostgreSQL（参考·本机未监听）", "driver": "postgres",
        "host": "127.0.0.1", "port": 5432, "database": "postgres", "schema_name": None,
        "username": "postgres", "password_encrypted": None, "options": None,
        "tags": '["demo","test","error"]', "use_duckdb_fed": 0, "metadata_path": None,
        "description": "指向本机 5432；未启动 PG 时用于验证「连接被拒」提示", **common,
    })

    # query_history：两条示例（关联 P_conn_sample_sqlite）
    qh_cols = [r[1] for r in conn.execute("PRAGMA table_info(query_history)").fetchall()]
    for i, (sql, cat, ok, dur, rows) in enumerate([
        ("SELECT region, COUNT(*) AS cnt, SUM(amount) AS total FROM orders GROUP BY region", "query", 1, 12, 5),
        ("SELECT c.segment, SUM(o.amount) AS total FROM customers c JOIN orders o ON c.customer_id = o.customer_id GROUP BY c.segment", "query", 1, 18, 3),
    ], start=1):
        row = {
            "id": f"P_qh_sample_{i:03d}", "connection_id": "P_conn_sample_sqlite",
            "database_name": None, "schema_name": None, "sql": sql,
            "sql_hash": f"hash-demo-{i:03d}", "exec_mode": "native", "category": cat,
            "success": ok, "error_message": None, "duration_ms": dur,
            "rows_returned": rows, "rows_affected": 0, "is_pinned": 0, "created_at": NOW_SQLITE,
        }
        keys = [k for k in row if k in qh_cols]
        conn.execute(
            f"INSERT OR REPLACE INTO query_history ({','.join(keys)}) VALUES ({','.join('?' * len(keys))})",
            [row[k] for k in keys],
        )

    # project_settings
    conn.executemany(
        "INSERT OR REPLACE INTO project_settings (key, value, updated_at) VALUES (?, ?, ?)",
        [("theme_mode", "dark", NOW_SQLITE), ("default_editor", "monaco", NOW_SQLITE)],
    )

    # 分组 / 成员 / 标签（017 迁移建表；表可能不存在则跳过）
    groups_exist = conn.execute(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name='connection_groups'"
    ).fetchone()
    if groups_exist:
        conn.execute(
            "INSERT OR REPLACE INTO connection_groups (id, name, description, sort_order) VALUES (?, ?, ?, ?)",
            ("grp_instance_core", "核心库", "演示分组：本实例的本地数据源", 0),
        )
        try:
            conn.execute(
                "INSERT OR REPLACE INTO connection_group_members (group_id, connection_id, sort_order) VALUES (?, ?, ?)",
                ("grp_instance_core", "P_conn_sample_sqlite", 0),
            )
            conn.execute(
                "INSERT OR REPLACE INTO connection_group_members (group_id, connection_id, sort_order) VALUES (?, ?, ?)",
                ("grp_instance_core", "P_conn_sample_duckdb", 1),
            )
        except sqlite3.Error as e:
            print(f"  [warn] 分组成员写入失败: {e}")
        conn.execute(
            "INSERT OR REPLACE INTO connection_tags (connection_id, tag) VALUES (?, ?)",
            ("P_conn_sample_sqlite", "demo"),
        )
        conn.execute(
            "INSERT OR REPLACE INTO connection_tags (connection_id, tag) VALUES (?, ?)",
            ("P_conn_sample_duckdb", "demo"),
        )

    conn.commit()
    conn.close()
    print(f"  项目元数据库: {db_path}（迁移 22 + 连接 3 + 历史 2 + 分组/标签）")


def build_analytics_duckdb():
    """项目分析库 DuckDB：迁移 + 示例分析记录。"""
    import duckdb

    db_path = RSMETA / "analytics.duckdb"
    conn = duckdb.connect(str(db_path))
    run_migrations(conn, MIG_ANALYSIS, "duckdb")

    conn.execute("DELETE FROM query_results")
    conn.execute("DELETE FROM analytics")
    conn.executemany(
        "INSERT INTO query_results (id, query_id, sql_hash, connection_id, result_json, row_count, execution_time_ms, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        [
            ("P_qr_sample_001", "P_qh_sample_001", "hash-demo-001", "P_conn_sample_sqlite",
             '{"region":"华东","cnt":3,"total":1979.0}', 5, 12, NOW_ISO),
            ("P_qr_sample_002", "P_qh_sample_002", "hash-demo-002", "P_conn_sample_sqlite",
             '{"segment":"企业","total":15069.0}', 3, 18, NOW_ISO),
        ],
    )
    conn.executemany(
        "INSERT INTO analytics (id, analysis_type, source_connection, source_table, analysis_json, created_at) VALUES (?, ?, ?, ?, ?, ?)",
        [
            ("P_an_sample_001", "sales_summary", "P_conn_sample_sqlite", "orders",
             '{"metric":"region_sales","note":"按区域聚合销售额"}', NOW_ISO),
            ("P_an_sample_002", "segment_analysis", "P_conn_sample_duckdb", "customers",
             '{"metric":"segment_lifetime_value","note":"客户分层价值"}', NOW_ISO),
        ],
    )
    conn.close()
    print(f"  项目分析库: {db_path}（迁移 2 + query_results 2 + analytics 2）")


def main():
    print(f"构建实例项目 → {EX}")
    DATA_DIR.mkdir(parents=True, exist_ok=True)
    RSMETA.mkdir(parents=True, exist_ok=True)
    (RSMETA / "config").mkdir(parents=True, exist_ok=True)
    (RSMETA / "project_metadata").mkdir(parents=True, exist_ok=True)
    (RSMETA / "secrets").mkdir(parents=True, exist_ok=True)

    build_business_sqlite(DATA_DIR / "sample.sqlite")
    build_business_duckdb(DATA_DIR / "sample.duckdb")
    build_project_db()
    build_analytics_duckdb()
    print("完成。")


if __name__ == "__main__":
    main()
