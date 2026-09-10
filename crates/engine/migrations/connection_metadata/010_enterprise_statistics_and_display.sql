-- 迁移版本：010
-- 数据库类型：SQLite
-- 作用：企业级元数据统计 & 显示控制
-- 背景：
--   1. tables 表已有 row_count_estimate / data_length / index_length（来自 004）
--   2. schemata 表缺少聚合统计（总表数、总视图数、总数据量）
--   3. 缺少表/视图比例的显示控制和用户偏好存储
-- 新增内容：
--   - schemata 表：新增 total_tables / total_views / total_size_bytes / row_count_total
--   - tables 表：新增 display_order / hidden / favorite / color_label / user_comment
--   - 创建 schema_stats 视图：实时聚合每个 Schema 的统计信息
--   - 创建 connection_stats 视图：跨 Schema 的连接级统计汇总
-- 更新时间：2026-05-28

-- ===========================================================================
-- ======================== A. schemata 表补全 =================================
-- ===========================================================================
-- 聚合统计列，由缓存预热流程计算并写入

ALTER TABLE schemata -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN total_tables INTEGER DEFAULT 0;
ALTER TABLE schemata -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN total_views INTEGER DEFAULT 0;
ALTER TABLE schemata -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN total_procedures INTEGER DEFAULT 0;
ALTER TABLE schemata -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN total_functions INTEGER DEFAULT 0;
ALTER TABLE schemata -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN total_size_bytes INTEGER DEFAULT 0;
ALTER TABLE schemata -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN row_count_total INTEGER DEFAULT 0;

-- ===========================================================================
-- ======================== B. tables 表补全（显示控制）========================
-- ===========================================================================
-- 用户级显示偏好，支持自定义排序、隐藏、收藏、标签

ALTER TABLE tables -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN display_order INTEGER DEFAULT 0;
ALTER TABLE tables -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN hidden INTEGER NOT NULL DEFAULT 0;
ALTER TABLE tables -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN favorite INTEGER NOT NULL DEFAULT 0;
ALTER TABLE tables -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN color_label TEXT;
ALTER TABLE tables -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN user_comment TEXT;

-- ===========================================================================
-- ======================== C. schema_stats 视图 ==============================
-- ===========================================================================
-- 实时聚合每个 Schema 下各类型对象数量和大小

CREATE VIEW IF NOT EXISTS schema_stats AS
SELECT
    s.id as schema_id,
    s.catalog_name,
    s.schema_name,
    COUNT(CASE WHEN t.table_type IN ('TABLE', 'PARTITIONED TABLE', 'SYSTEM TABLE', 'GLOBAL TEMPORARY', 'LOCAL TEMPORARY') AND t.hidden = 0 THEN 1 END) as table_count,
    COUNT(CASE WHEN t.table_type IN ('VIEW', 'MATERIALIZED VIEW') AND t.hidden = 0 THEN 1 END) as view_count,
    COUNT(CASE WHEN t.table_type IN ('TABLE', 'MATERIALIZED VIEW', 'PARTITIONED TABLE') THEN 1 END) as table_total,
    COALESCE(SUM(t.row_count_estimate), 0) as row_count_total,
    COALESCE(SUM(t.data_length), 0) + COALESCE(SUM(t.index_length), 0) as total_size_bytes,
    COALESCE(SUM(t.data_length), 0) as data_size_bytes,
    COALESCE(SUM(t.index_length), 0) as index_size_bytes,
    COUNT(DISTINCT r.id) as routine_count,
    COUNT(DISTINCT seq.id) as sequence_count,
    MAX(t.last_sync) as last_sync,
    MAX(t.stats_last_updated) as stats_last_updated
FROM schemata s
LEFT JOIN tables t ON t.schema_id = s.id
LEFT JOIN routines r ON r.schema_id = s.id
LEFT JOIN sequences seq ON seq.schema_id = s.id
GROUP BY s.id, s.catalog_name, s.schema_name;

-- ===========================================================================
-- ======================== D. connection_stats 视图 ===========================
-- ===========================================================================
-- 跨 Schema 的连接级统计汇总

CREATE VIEW IF NOT EXISTS connection_stats AS
SELECT
    COALESCE(s.catalog_name, '') as connection_id,
    COUNT(DISTINCT s.id) as schema_count,
    COALESCE(SUM(ss.table_count), 0) as table_count,
    COALESCE(SUM(ss.view_count), 0) as view_count,
    COALESCE(SUM(ss.row_count_total), 0) as row_count_total,
    COALESCE(SUM(ss.total_size_bytes), 0) as total_size_bytes,
    MAX(ss.last_sync) as last_sync
FROM schemata s
LEFT JOIN schema_stats ss ON ss.schema_id = s.id
GROUP BY COALESCE(s.catalog_name, '');

-- ===========================================================================
-- ======================== E. 版本号更新 ====================================
-- ===========================================================================

PRAGMA user_version = 10;

-- v1 缺陷修复：cache_version 列名为 version/upgraded_at/upgrade_reason/created_at/updated_at
UPDATE cache_version
SET version = 10,
    upgraded_at = strftime('%s', 'now'),
    updated_at = strftime('%s', 'now'),
    upgrade_reason = 'Enterprise metadata statistics: schema-level aggregates (total_tables/views/size/rows), display control (order/hidden/favorite/color), schema_stats & connection_stats views'
WHERE id = 1;

-- 记录迁移历史
-- v1 缺陷修复：cache_migration_history 列名为 from_version/to_version/reason
INSERT INTO cache_migration_history (from_version, to_version, migrated_at, reason, duration_ms, success)
SELECT
    9,
    10,
    strftime('%s', 'now'),
    'V10: 企业级统计 + 显示控制: schemata(+6聚合列), tables(+5显示控制列), schema_stats视图, connection_stats视图',
    0,
    1
WHERE NOT EXISTS (
    SELECT 1 FROM cache_migration_history WHERE to_version = 10
);