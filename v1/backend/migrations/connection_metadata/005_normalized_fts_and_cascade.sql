-- 迁移版本：005
-- 数据库类型：SQLite
-- 作用：规范化表 FTS 同步、级联删除支持、索引优化
-- 更新时间：2026-05-04

-- ===========================================================================
-- ======================== FTS5 搜索表重构 ==================================
-- ===========================================================================

-- 1. 删除旧的 FTS5 表（基于旧的 metadata 表）
DROP TABLE IF EXISTS metadata_fts;

-- 2. 创建新的规范化 FTS5 搜索表
-- 搜索范围：schemata, tables, columns, views, routines
CREATE VIRTUAL TABLE IF NOT EXISTS metadata_fts USING fts5(
    search_type,
    schema_name,
    object_name,
    parent_name,
    search_content,
    content='',
    contentless_delete='true'
);

-- 3. 重新构建 FTS 索引（从规范化表）
-- 插入 schema
INSERT INTO metadata_fts (search_type, schema_name, object_name, parent_name, search_content)
SELECT 'schema', schema_name, schema_name, '', schema_name
FROM schemata WHERE is_loaded = 1;

-- 插入 tables
INSERT INTO metadata_fts (search_type, schema_name, object_name, parent_name, search_content)
SELECT 'table', s.schema_name, t.table_name, s.schema_name,
       s.schema_name || ' ' || t.table_name || ' ' || COALESCE(t.table_comment, '')
FROM tables t
INNER JOIN schemata s ON t.schema_id = s.id
WHERE t.is_loaded = 1;

-- 插入 columns
INSERT INTO metadata_fts (search_type, schema_name, object_name, parent_name, search_content)
SELECT 'column', s.schema_name, c.column_name, t.table_name,
       s.schema_name || ' ' || t.table_name || ' ' || c.column_name || ' ' || c.data_type || ' ' || COALESCE(c.column_comment, '')
FROM columns c
INNER JOIN tables t ON c.table_id = t.id
INNER JOIN schemata s ON t.schema_id = s.id
WHERE c.is_loaded = 1;

-- 插入 views
INSERT INTO metadata_fts (search_type, schema_name, object_name, parent_name, search_content)
SELECT 'view', s.schema_name, v.view_name, s.schema_name,
       s.schema_name || ' ' || v.view_name || ' ' || COALESCE(v.view_comment, '')
FROM views v
INNER JOIN schemata s ON v.schema_id = s.id
WHERE v.is_loaded = 1;

-- 插入 routines
INSERT INTO metadata_fts (search_type, schema_name, object_name, parent_name, search_content)
SELECT 'routine', s.schema_name, r.routine_name, s.schema_name,
       s.schema_name || ' ' || r.routine_name || ' ' || r.routine_type || ' ' || COALESCE(r.routine_comment, '')
FROM routines r
INNER JOIN schemata s ON r.schema_id = s.id
WHERE r.is_loaded = 1;

-- ===========================================================================
-- ======================== 外键与级联删除 ====================================
-- ===========================================================================

-- 启用外键约束
PRAGMA foreign_keys = ON;

-- tables 表添加外键约束（如果不存在）
-- 注意：需要先删除并重建才能添加外键

-- 获取现有外键状态
PRAGMA foreign_key_list(tables);

-- columns 表：添加级联删除（如果外键不存在）
-- 由于 SQLite 外键需要表重建，我们通过触发器实现级联删除

-- 创建删除表的触发器（级联删除 columns）
DROP TRIGGER IF EXISTS delete_table_columns;
CREATE TRIGGER delete_table_columns AFTER DELETE ON tables
BEGIN
    DELETE FROM columns WHERE table_id = OLD.id;
    DELETE FROM indexes WHERE table_id = OLD.id;
END;

-- 创建删除 schema 的触发器（级联删除所有关联数据）
DROP TRIGGER IF EXISTS delete_schema_all;
CREATE TRIGGER delete_schema_all AFTER DELETE ON schemata
BEGIN
    DELETE FROM tables WHERE schema_id = OLD.id;
    -- columns 和 indexes 会通过上面的触发器级联删除
END;

-- ===========================================================================
-- ======================== 索引优化 =========================================
-- ===========================================================================

-- 为常用查询添加复合索引
CREATE INDEX IF NOT EXISTS idx_tables_schema_type ON tables(schema_id, table_type);
CREATE INDEX IF NOT EXISTS idx_columns_table_ordinal ON columns(table_id, ordinal_position);
CREATE INDEX IF NOT EXISTS idx_indexes_table ON indexes(table_id);
CREATE INDEX IF NOT EXISTS idx_routines_schema_type ON routines(schema_id, routine_type);
CREATE INDEX IF NOT EXISTS idx_routine_params_routine ON routine_parameters(routine_id);

-- FTS 搜索优化索引
CREATE INDEX IF NOT EXISTS idx_fts_schema_object ON metadata_fts(schema_name, object_name);

-- ===========================================================================
-- ======================== 迁移记录 =========================================
-- ===========================================================================

UPDATE cache_version SET version = 5, updated_at = strftime('%s', 'now') WHERE id = 1;

INSERT INTO cache_migration_history (from_version, to_version, migrated_at, reason, duration_ms, success)
VALUES (4, 5, strftime('%s', 'now'), '规范化表 FTS 同步、级联删除支持、索引优化', 0, 1);
