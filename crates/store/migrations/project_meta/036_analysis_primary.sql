-- 迁移版本：036
-- 数据库类型：SQLite（项目库 .RSmeta/project.db）
-- 作用：主源（未限定名的解析者）进**意图清单**，重启后按它恢复（K3）
-- 更新时间：2026-11-27
--
-- 为什么单开一张单行表而不是给 analysis_sources 加列：
-- - 主源是**单值意图**（"这条项目里未限定名在哪个源里解析"），登记行是 N 条；
--   单行表用 `singleton = 1` + CHECK 让"至多一条"由库自己保证，不需要应用层维护互斥。
-- - `CREATE TABLE IF NOT EXISTS` 天然幂等：不存在 `ALTER ... ADD COLUMN` 与
--   `ensure_tables` 兜底的顺序冲突（那个冲突会让老项目库在迁移时直接打不开）。
-- - 主源与登记行的一致性由应用层维持：删登记行时顺带清掉指向它的主源（见 `delete`）。
--
-- 口径见 docs/architecture/federation/catalog-context-model.md §5（意图清单是唯一持久状态）。

CREATE TABLE IF NOT EXISTS analysis_primary (
    singleton  INTEGER PRIMARY KEY CHECK (singleton = 1),
    alias      TEXT NOT NULL,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
