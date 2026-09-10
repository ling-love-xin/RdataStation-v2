# -*- coding: utf-8 -*-
import pathlib
f = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\engine\src\persistence\cache_version_migration.rs")
t = f.read_text(encoding="utf-8")

old = """    #[test]
    fn test_cache_version_manager() -> Result<(), CoreError> {
        let db_path = test_temp_dir("version").join("test_cache_version.sqlite");

        // 幂等：清理上次运行残留，避免 CREATE TABLE 重复报错
        let _ = std::fs::remove_file(&db_path);

        let conn = Connection::open(&db_path)?;

        // 创建 cache_version 表
        conn.execute(
            "CREATE TABLE cache_version (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                version INTEGER NOT NULL DEFAULT 1,
                upgraded_at INTEGER,
                upgrade_reason TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "INSERT INTO cache_version (id, version, created_at, updated_at) VALUES (1, 1, strftime('%s', 'now'), strftime('%s', 'now'))",
            [],
        )?;

        let manager = CacheVersionManager::new();"""
new = """    #[test]
    fn test_cache_version_manager() -> Result<(), CoreError> {
        let db_path = test_temp_dir("version").join("test_cache_version.sqlite");

        // 幂等：清理上次运行残留
        let _ = std::fs::remove_file(&db_path);

        // 用迁移管理器初始化完整 connection_metadata schema
        // （V1ToV2 迁移策略依赖 columns 等完整表结构，测试需与真实库一致）
        crate::migration::MigrationManager::new().migrate(
            &db_path,
            crate::migration::MigrationType::ConnectionMetadata,
        )?;

        let conn = Connection::open(&db_path)?;

        let manager = CacheVersionManager::new();"""
assert old in t, "test block not found"
t = t.replace(old, new)

f.write_text(t, encoding="utf-8")
print("cache_version_migration.rs: test uses MigrationManager")
