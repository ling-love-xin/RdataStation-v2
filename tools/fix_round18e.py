# -*- coding: utf-8 -*-
import pathlib, re

base = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src")

# ========== 1) native/duckdb.rs: is_read_only 填值 + ping SELECT ==========
f = base / r"driver\native\duckdb.rs"
t = f.read_text(encoding="utf-8")

# 1a) 变量改名（3 处 is_read_only_sql + 1 处 starts_with 链）
t = t.replace(
    "let _is_read_only = is_read_only_sql(&sql_owned);",
    "let is_read_only = is_read_only_sql(&sql_owned);",
)
t = t.replace(
    "let _is_read_only = sql_upper.starts_with(\"SELECT\")\n            || sql_upper.starts_with(\"SHOW\")\n            || sql_upper.st",
    "let is_read_only = sql_upper.starts_with(\"SELECT\")\n            || sql_upper.starts_with(\"SHOW\")\n            || sql_upper.st",
)

# 1b) 构造处填 is_read_only（空结果与 batch 结果，两种缩进变体）
t = t.replace(
    "Ok(QueryResult {\n                    columns,\n                    batches: vec![],\n                    ..Default::default()\n                })",
    "Ok(QueryResult {\n                    columns,\n                    batches: vec![],\n                    is_read_only: Some(is_read_only),\n                    ..Default::default()\n                })",
)
t = t.replace(
    "Ok(QueryResult {\n                columns,\n                batches: vec![],\n                ..Default::default()\n            })",
    "Ok(QueryResult {\n                columns,\n                batches: vec![],\n                is_read_only: Some(is_read_only),\n                ..Default::default()\n            })",
)
t = t.replace(
    "Ok(QueryResult {\n                    columns,\n                    batches: vec![batch],\n                    ..Default::default()\n                })",
    "Ok(QueryResult {\n                    columns,\n                    batches: vec![batch],\n                    is_read_only: Some(is_read_only),\n                    ..Default::default()\n                })",
)
t = t.replace(
    "Ok(QueryResult {\n                columns,\n                batches: vec![batch],\n                ..Default::default()\n            })",
    "Ok(QueryResult {\n                columns,\n                batches: vec![batch],\n                is_read_only: Some(is_read_only),\n                ..Default::default()\n            })",
)

# 1c) ping() 的 execute("SELECT 1") -> query_row
t = t.replace(
    'conn.execute("SELECT 1", [])\n            .map_err(|e| CoreError::database(DatabaseError::query("SELECT 1", e.to_string())))?;',
    'conn.query_row("SELECT 1", [], |r| r.get::<_, i32>(0))\n            .map_err(|e| CoreError::database(DatabaseError::query("SELECT 1", e.to_string())))?;',
)
f.write_text(t, encoding="utf-8")
print("native/duckdb.rs: is_read_only + ping fixed")

# ========== 2) metadata_cache.rs 测试 PRAGMA journal_mode=WAL -> execute_batch ==========
f = base / r"persistence\metadata_cache.rs"
t = f.read_text(encoding="utf-8")
t = t.replace(
    'conn.execute("PRAGMA journal_mode=WAL", [])?;',
    'conn.execute_batch("PRAGMA journal_mode=WAL")?;',
)
f.write_text(t, encoding="utf-8")
print("metadata_cache.rs: journal_mode execute_batch")

# ========== 3) project_db.rs wal_checkpoint -> execute_batch ==========
f = base / r"persistence\project_db.rs"
t = f.read_text(encoding="utf-8")
t = t.replace(
    'if let Err(e) = conn.execute("PRAGMA wal_checkpoint(TRUNCATE)", []) {',
    'if let Err(e) = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)") {',
)
f.write_text(t, encoding="utf-8")
print("project_db.rs: wal_checkpoint execute_batch")

# ========== 4) cache_version_migration.rs 测试幂等（删除残留文件） ==========
f = base / r"persistence\cache_version_migration.rs"
t = f.read_text(encoding="utf-8")
old = """        let db_path = test_temp_dir("version").join("test_cache_version.sqlite");

        let conn = Connection::open(&db_path)?;"""
new = """        let db_path = test_temp_dir("version").join("test_cache_version.sqlite");

        // 幂等：清理上次运行残留，避免 CREATE TABLE 重复报错
        let _ = std::fs::remove_file(&db_path);

        let conn = Connection::open(&db_path)?;"""
assert old in t, "cache_version test block not found"
t = t.replace(old, new)
f.write_text(t, encoding="utf-8")
print("cache_version_migration.rs: idempotent test")
