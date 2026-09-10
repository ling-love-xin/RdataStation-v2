# -*- coding: utf-8 -*-
import pathlib
f = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src\persistence\cache_version_migration.rs")
t = f.read_text(encoding="utf-8")
old = """        // 记录迁移历史
        conn.execute(
            "INSERT INTO cache_migration_history (from_version, to_version, migrated_at, reason, success)
             VALUES (?1, ?2, ?3, ?4, 1)",
            rusqlite::params![1, CURRENT_CACHE_VERSION, now, "升级到版本 2：添加缓存版本控制和压缩支持"],"""
new = """        // 确保迁移历史表存在（正常流程由 connection_metadata 迁移创建；
        // 独立使用时（如测试）需幂等建表，否则记录历史报 no such table）
        conn.execute(
            "CREATE TABLE IF NOT EXISTS cache_migration_history (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                from_version    INTEGER NOT NULL,
                to_version      INTEGER NOT NULL,
                migrated_at     INTEGER NOT NULL,
                reason          TEXT,
                duration_ms     INTEGER,
                success         INTEGER NOT NULL DEFAULT 1
            )",
            [],
        )
        .map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "ensure_migration_history_table".to_string(),
                reason: e.to_string(),
            })
        })?;

        // 记录迁移历史
        conn.execute(
            "INSERT INTO cache_migration_history (from_version, to_version, migrated_at, reason, success)
             VALUES (?1, ?2, ?3, ?4, 1)",
            rusqlite::params![1, CURRENT_CACHE_VERSION, now, "升级到版本 2：添加缓存版本控制和压缩支持"],"""
assert old in t, "insert block not found"
t = t.replace(old, new)
f.write_text(t, encoding="utf-8")
print("cache_version_migration.rs: ensure history table")
