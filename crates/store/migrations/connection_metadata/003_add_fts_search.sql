-- 迁移版本：003
-- 数据库类型：SQLite
-- 作用：添加 FTS5 全文搜索、自省级别支持和聚合视图
-- 更新时间：2026-05-04

-- ===========================================================================
-- ======================== FTS5 全文搜索 =====================================
-- ===========================================================================

-- 表名和列名全文搜索虚拟表
-- 用于快速搜索数据库对象，支持模糊匹配
CREATE VIRTUAL TABLE IF NOT EXISTS tables_fts USING fts5(
    schema_name,
    table_name,
    table_comment,
    content='metadata',
    content_rowid='rowid'
);

-- 为 FTS5 添加反向索引引用（如果 metadata 表已有数据）
-- 注意：首次创建时 metadata 表可能为空，后续同步会自动填充

-- ===========================================================================
-- ======================== 自省级别支持 =====================================
-- ===========================================================================

-- 为 metadata 表添加自省级别字段
-- 用于支持 DataGrip 风格的多级懒加载
-- Level 1: 仅名称
-- Level 2: 除源码外的所有内容
-- Level 3: 完整元数据
ALTER TABLE metadata ADD COLUMN introspect_level INTEGER DEFAULT 3;

-- 为 metadata 表添加加载状态追踪
-- 用于懒加载：标识哪些对象已加载完整信息
ALTER TABLE metadata ADD COLUMN is_loaded INTEGER DEFAULT 0;

-- 为 metadata 表添加最后访问时间（用于 LRU 淘汰）
ALTER TABLE metadata ADD COLUMN last_accessed INTEGER;

-- 更新现有记录
UPDATE metadata SET introspect_level = 3, is_loaded = 1 WHERE introspect_level IS NULL;

-- ===========================================================================
-- ======================== 聚合视图 =========================================
-- ===========================================================================

-- 表详情聚合视图（高频查询优化）
-- 将表、列、索引、外键数量聚合在一起，减少 JOIN
CREATE VIEW IF NOT EXISTS v_table_details AS
SELECT 
    m.schema_name,
    m.table_name,
    m.obj_type,
    m.comment,
    m.last_sync,
    -- 统计信息
    COUNT(DISTINCT CASE WHEN c.obj_type = 'column' AND c.name IS NOT NULL THEN c.rowid END) as column_count,
    COUNT(DISTINCT CASE WHEN c.obj_type = 'index' AND c.name IS NOT NULL THEN c.rowid END) as index_count,
    COUNT(DISTINCT CASE WHEN c.obj_type = 'foreign_key' AND c.name IS NOT NULL THEN c.rowid END) as fk_count,
    -- 加载状态
    MAX(COALESCE(c.is_loaded, 1)) as is_loaded,
    MAX(COALESCE(c.introspect_level, 3)) as introspect_level
FROM metadata m
LEFT JOIN metadata c ON c.database_name = m.database_name 
    AND c.schema_name = m.schema_name 
    AND c.table_name = m.table_name
WHERE m.obj_type = 'table'
GROUP BY m.schema_name, m.table_name, m.obj_type, m.comment, m.last_sync;

-- ===========================================================================
-- ======================== 同步标记表 =======================================
-- ===========================================================================

-- 用于追踪增量同步的状态
CREATE TABLE IF NOT EXISTS sync_marker (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    last_full_sync INTEGER,          -- 最后全量同步时间戳
    last_incremental_sync INTEGER,   -- 最后增量同步时间戳
    objects_total INTEGER DEFAULT 0, -- 总对象数
    objects_loaded INTEGER DEFAULT 0, -- 已加载对象数
    updated_at INTEGER NOT NULL       -- 更新时间戳
);

-- 初始化默认记录
INSERT OR IGNORE INTO sync_marker (id, updated_at)
VALUES (1, strftime('%s', 'now'));

-- ===========================================================================
-- ======================== 搜索历史表 =======================================
-- ===========================================================================

-- 记录用户的搜索历史，用于优化搜索建议
CREATE TABLE IF NOT EXISTS search_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    search_term TEXT NOT NULL,
    search_type TEXT DEFAULT 'table',  -- table/column/index/procedure
    schema_filter TEXT,
    result_count INTEGER DEFAULT 0,
    searched_at INTEGER NOT NULL
);

-- 搜索历史索引
CREATE INDEX IF NOT EXISTS idx_search_history_term ON search_history(search_term);
CREATE INDEX IF NOT EXISTS idx_search_history_time ON search_history(searched_at DESC);

-- ===========================================================================
-- ======================== 更新缓存版本 =====================================
-- ===========================================================================

-- 更新缓存版本号
UPDATE cache_version SET version = 3, upgraded_at = strftime('%s', 'now'), updated_at = strftime('%s', 'now') WHERE id = 1;

-- 记录迁移历史
INSERT INTO cache_migration_history (from_version, to_version, migrated_at, reason, duration_ms, success)
VALUES (2, 3, strftime('%s', 'now'), '添加 FTS5 全文搜索、自省级别支持和聚合视图', 0, 1);

-- ===========================================================================
-- ======================== 重建 FTS 索引 ===================================
-- ===========================================================================

-- 如果 metadata 表已有数据，触发 FTS 索引重建
-- 这是一个可选的后台操作，用于填充已有的元数据到 FTS 表
-- 注意：在实际使用中，同步时会自动维护 FTS 索引
