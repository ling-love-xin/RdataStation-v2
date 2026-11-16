-- 034: 全局登记表的表名与出处两列（镜像 031）
--
-- 背景：全局分析库（`<RDS_HOME>/data/system/analytics.duckdb`）里的表一直是
-- 「显示名 = 表名」，所以登记行从不记表名。G2 口径确定后（全局遵守自己的版本，
-- 一版一张 `res_<描述>_<时间戳>` 表），表名不再等于资产名，必须单独登记。
--
-- 与项目库同一取舍：**不改 `kind` 的 CHECK**（原因见 031 的注释：迁移在事务内执行，
-- `PRAGMA foreign_keys` 无效，且本表被 `analytics_resource_versions` 外键引用）。
-- 权威判据是 `analytics_table`；`kind` 串仍是 `analysis`。

ALTER TABLE analytics_resources ADD COLUMN analytics_table TEXT;
ALTER TABLE analytics_resources ADD COLUMN origin_kind TEXT;
ALTER TABLE analytics_resources ADD COLUMN origin_ref TEXT;
ALTER TABLE analytics_resources ADD COLUMN structured INTEGER;

-- 历史全局行回填：`PromoteKind::DataFile` 升级时建的是**同名全局表**，登记行
-- 没有本体路径（`file_rel_path` 只给脚本内容表用）——这批行本来就是表型资产，
-- 只是过去没有列能表达。脚本内容表（`file_rel_path` 非空）不算，分发时重建为文件。
UPDATE analytics_resources
SET analytics_table = name
WHERE scope = 'global'
  AND kind = 'analysis'
  AND file_rel_path IS NULL
  AND analytics_table IS NULL;

UPDATE analytics_resources
SET structured = 1
WHERE scope = 'global'
  AND kind = 'analysis'
  AND file_rel_path IS NOT NULL
  AND structured IS NULL;

CREATE INDEX IF NOT EXISTS idx_ar_global_analytics_table
    ON analytics_resources (analytics_table)
    WHERE analytics_table IS NOT NULL;
