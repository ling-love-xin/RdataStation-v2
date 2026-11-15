-- ===========================================================================
-- 分发回执：一个全局存档被分发到哪些项目、用过哪一版（全局侧视角）
--
-- 项目侧登记只记 `promoted_from`（「我来自哪」），全局侧原本完全不知道
-- 「我被谁用过」——于是「删掉这个全局资源之前，先看有没有项目依赖它」
-- 没有任何依据。
--
-- 口径：按 (global_id, project_root, target_name) 收口成一行，重复覆盖分发
-- 只累加 `dispatch_count` 并刷新版本与时间。不做逐次流水：同一项目反复覆盖
-- 分发会把真正的「使用面」淹掉，而业务问题问的是「哪些项目在用、用到第几版、
-- 第一次和最近一次是什么时候」。
-- ===========================================================================

CREATE TABLE IF NOT EXISTS analytics_resource_dispatch (
    id                    TEXT PRIMARY KEY,
    global_id             TEXT NOT NULL,      -- 全局 analytics_resources.id
    global_name           TEXT NOT NULL,      -- 全局名（= 全局 DuckDB 表名）
    project_root          TEXT NOT NULL,      -- 目标项目根（规范化：'\' → '/'）
    target_name           TEXT NOT NULL,      -- 目标名（改名分发时与 global_name 不同）
    version               INTEGER NOT NULL,   -- 最近一次分发时的全局版本
    shape                 TEXT NOT NULL,      -- 'table'（分析表）/ 'file'（脚本重建文件）
    dispatch_count        INTEGER NOT NULL DEFAULT 1,
    first_distributed_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    last_distributed_at   TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_ard_key
    ON analytics_resource_dispatch(global_id, project_root, target_name);
CREATE INDEX IF NOT EXISTS idx_ard_global
    ON analytics_resource_dispatch(global_id, last_distributed_at DESC);
