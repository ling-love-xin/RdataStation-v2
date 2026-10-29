-- 迁移版本：012
-- 数据库类型：SQLite
-- 作用：metadata_index 增加 (connection_id, LOWER(object_name)) 复合索引（前缀搜索的走索引前提）
-- 更新时间：2026-09-19
--
-- 背景（10 万对象实测，见 metadata_cache.rs 的 `scale_baseline_100k_objects`）：
--   `LOWER(object_name) LIKE '%x%'` 是无索引全表扫，搜索是按键驱动的交互操作，必须优化。
--
-- 处置：`search_index` 改为**两段式**——
--   1) 前缀段：`>= needle AND <= needle||U+10FFFF` 的范围扫描（走本索引），
--      内层按 `LOWER(object_name)` 名序只取 2000 条窗口，外层再对窗口做完整排序；
--   2) 回落段：前缀没装满 `limit` 时，回落到原来的中缀 LIKE 全扫补齐名额。
--
-- 为什么是复合索引、且 connection_id 放前面：
--   1) 曾先试过单列 `(LOWER(object_name))`：`EXPLAIN QUERY PLAN` 显示 SQLite 仍选
--      `idx_metadata_index_level(connection_id, introspect_level)` 只吃 connection_id 等值，
--      范围条件退化成逐行过滤——等于没优化。把等值列放前导、范围列紧随，规划器才会用上。
--   2) 搜索总是限定单个连接（`WHERE connection_id = ?`），连接做前导列同时缩小了索引扫描区间。
--   3) 本索引同时供「名序窗口」使用（`ORDER BY LOWER(object_name) LIMIT n` 可沿索引推进、提前停止），
--      这是前缀段能降到毫秒级的关键：只加过滤索引、仍全量排序时，8 万命中要 290+ ms（debug）。
--
-- 注意：LIKE 用不上这个索引（SQLite 的 LIKE 优化要求字面量模式或 NOCASE 排序规则），
-- 所以前缀段刻意写成 `>= / <=` 的范围比较。

CREATE INDEX IF NOT EXISTS idx_metadata_index_conn_object_lower
    ON metadata_index(connection_id, LOWER(object_name));

UPDATE cache_version SET version = 12, updated_at = strftime('%s', 'now') WHERE id = 1;

INSERT INTO cache_migration_history (from_version, to_version, migrated_at, reason, duration_ms, success)
SELECT
    11,
    12,
    strftime('%s', 'now'),
    'V12: metadata_index 增加 (connection_id, LOWER(object_name)) 索引（前缀按名序窗口取，中缀回落全扫）',
    0,
    1
WHERE NOT EXISTS (
    SELECT 1 FROM cache_migration_history WHERE to_version = 12
);
