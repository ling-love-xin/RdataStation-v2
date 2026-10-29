-- 迁移版本：002
-- 数据库类型：SQLite
-- 作用：添加缓存版本控制和压缩支持
-- 更新时间：2026-04-28

-- ===========================================================================
-- ======================== 缓存版本表 =======================================
-- ===========================================================================

-- 用于追踪缓存 schema 版本，支持自动迁移
CREATE TABLE IF NOT EXISTS cache_version (
    id              INTEGER PRIMARY KEY CHECK (id = 1),  -- 单行记录，id 固定为 1
    version         INTEGER NOT NULL DEFAULT 1,          -- 当前缓存版本号
    upgraded_at     INTEGER,                              -- 最后升级时间戳
    upgrade_reason  TEXT,                                 -- 升级原因
    created_at      INTEGER NOT NULL,                     -- 创建时间戳
    updated_at      INTEGER NOT NULL                      -- 更新时间戳
);

-- 初始化默认版本记录
INSERT OR IGNORE INTO cache_version (id, version, created_at, updated_at)
VALUES (1, 1, strftime('%s', 'now'), strftime('%s', 'now'));

-- ===========================================================================
-- ======================== 压缩数据表 =======================================
-- ===========================================================================

-- 用于存储压缩后的大型元数据对象
-- 当元数据超过阈值时，使用 gzip 压缩后存储在此表
CREATE TABLE IF NOT EXISTS compressed_metadata (
    id              TEXT PRIMARY KEY,                     -- 原始元数据 ID
    obj_type        TEXT NOT NULL,                        -- 对象类型
    database_name   TEXT NOT NULL,                        -- 数据库名
    schema_name     TEXT NOT NULL,                        -- 模式名
    table_name      TEXT NOT NULL,                        -- 表名
    compressed_data BLOB NOT NULL,                        -- gzip 压缩后的数据
    original_size   INTEGER NOT NULL,                     -- 原始数据大小
    compressed_size INTEGER NOT NULL,                     -- 压缩后大小
    compression_ratio REAL,                               -- 压缩率
    last_sync       INTEGER NOT NULL,                     -- 最后同步时间戳
    created_at      INTEGER NOT NULL                      -- 创建时间戳
);

-- 索引
CREATE INDEX IF NOT EXISTS idx_compressed_meta_obj ON compressed_metadata(obj_type);
CREATE INDEX IF NOT EXISTS idx_compressed_meta_table ON compressed_metadata(database_name, schema_name, table_name);
CREATE INDEX IF NOT EXISTS idx_compressed_meta_sync ON compressed_metadata(last_sync);

-- ===========================================================================
-- ======================== 迁移历史表 =======================================
-- ===========================================================================

-- 记录缓存版本迁移历史
CREATE TABLE IF NOT EXISTS cache_migration_history (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    from_version    INTEGER NOT NULL,                     -- 迁移前版本
    to_version      INTEGER NOT NULL,                     -- 迁移后版本
    migrated_at     INTEGER NOT NULL,                     -- 迁移时间戳
    reason          TEXT,                                 -- 迁移原因
    duration_ms     INTEGER,                              -- 迁移耗时（毫秒）
    success         INTEGER DEFAULT 1                     -- 是否成功
);

-- 索引
CREATE INDEX IF NOT EXISTS idx_migration_history_time ON cache_migration_history(migrated_at DESC);
CREATE INDEX IF NOT EXISTS idx_migration_history_versions ON cache_migration_history(from_version, to_version);

-- ===========================================================================
-- ======================== 升级现有表 =======================================
-- ===========================================================================

-- 为 metadata 表添加版本字段（如果不存在）
ALTER TABLE metadata ADD COLUMN cache_version INTEGER DEFAULT 1;

-- 为 metadata 表添加压缩标记
ALTER TABLE metadata ADD COLUMN is_compressed INTEGER DEFAULT 0;

-- 为 metadata 表添加原始数据大小
ALTER TABLE metadata ADD COLUMN original_size INTEGER;

-- 更新现有记录的版本号为 1
UPDATE metadata SET cache_version = 1 WHERE cache_version IS NULL;
