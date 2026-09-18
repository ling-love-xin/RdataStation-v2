-- 迁移版本：013
-- 数据库类型：SQLite
-- 作用：名称索引改为 (connection_id, object_type, LOWER(object_name))，撤掉 012 建的那条
-- 更新时间：2026-09-19
--
-- 为什么改（012 落地后的第二轮实测）：
--   012 建的是 (connection_id, LOWER(object_name))，前缀段只能「先按名序取窗口 2000 条、
--   再在窗口内排序」——窗口是拿尾部精度换响应时间（名序靠后但更短的名字可能被挡在外面）。
--   要**既精确又快**，得让排序键整体可索引：排序键里的 `LENGTH(object_name)` 已拿掉
--   （见 search_index 的注释），于是排序退化为「相等 → 类别 → 名序」；
--   再把类别做成索引的第二列，每个类别各扫一段即可直出：
--     connection_id = ? AND object_type = ? AND LOWER(object_name) ∈ [下界, 上界]
--     ORDER BY LOWER(object_name) LIMIT n
--   等值列在前、范围列紧随 → 索引序即输出序，**不排序、可提前停**，命中多少都不影响耗时；
--   类别之间的次序由多次扫描的合并顺序（Rust 侧稳定排序）保证。
--
-- 代价：每查询最多 4 次索引探测（各 ≤ limit 条），换掉「一次扫描 + 全量排序 + 窗口近似」。
-- 012 的 (connection_id, LOWER(object_name)) 对本查询不再有用（类别不在索引里、也替不了
-- 回落段的中缀全扫），留着只是多付缓存库的写入与体积，故一并 DROP。

CREATE INDEX IF NOT EXISTS idx_metadata_index_conn_type_object_lower
    ON metadata_index(connection_id, object_type, LOWER(object_name));

DROP INDEX IF EXISTS idx_metadata_index_conn_object_lower;

UPDATE cache_version SET version = 13, updated_at = strftime('%s', 'now') WHERE id = 1;

INSERT INTO cache_migration_history (from_version, to_version, migrated_at, reason, duration_ms, success)
SELECT
    12,
    13,
    strftime('%s', 'now'),
    'V13: 名称索引改为 (connection_id, object_type, LOWER(object_name))，前缀段按类别索引序直出（撤掉 V12 那条）',
    0,
    1
WHERE NOT EXISTS (
    SELECT 1 FROM cache_migration_history WHERE to_version = 13
);
