-- 迁移版本：008
-- 数据库类型：SQLite
-- 作用：为 columns 表添加 is_primary 字段，支持主键标记独立于 is_identity（自增标识）
-- 背景：
--   1. 之前 is_primary 被映射到 is_identity 列，但主键 ≠ 自增列
--   2. MetadataBrowser::get_table_detail() 返回 ColumnDetail 包含 is_primary_key 字段
--   3. save_node_detail() / load_node_detail() 需要独立的 is_primary 列
--   4. list_columns_normalized() 使用 COALESCE(c.is_primary, 0) AS is_primary_key 查询
-- 更新时间：2026-05-09

-- 为 columns 表添加 is_primary 字段
ALTER TABLE columns ADD COLUMN is_primary INTEGER DEFAULT 0;

-- 从对应的 indexes 表同步已有主键信息（如果存在）
-- 主键在 indexes 表中以 is_primary=1 标记，通过 index_columns 关联到 columns
UPDATE columns
SET is_primary = 1
WHERE id IN (
    SELECT DISTINCT c.id
    FROM columns c
    JOIN index_columns ic ON c.column_name = ic.column_name AND c.table_id = (
        SELECT i.table_id FROM indexes i WHERE i.id = ic.index_id
    )
    JOIN indexes idx ON ic.index_id = idx.id
    WHERE idx.is_primary = 1
);

-- 更新缓存版本
UPDATE cache_version 
SET version = 8, 
    upgraded_at = strftime('%s', 'now'),
    updated_at = strftime('%s', 'now')
WHERE id = 1;

-- 记录迁移历史
INSERT INTO cache_migration_history (version, migrated_at, duration_ms, success, error_message)
SELECT 
    8,
    strftime('%s', 'now'),
    0,
    1,
    'V8: 添加 columns.is_primary 字段，支持主键标记独立于 is_identity'
WHERE NOT EXISTS (
    SELECT 1 FROM cache_migration_history WHERE version = 8
);