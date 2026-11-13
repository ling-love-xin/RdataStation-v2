-- 030_analytics_resource_checkouts.sql
-- 取回凭据：把「工作副本路径 → 来源存档 / 版本」落成可追溯的回连。
--
-- 为什么需要它：取回对话框与状态栏都告诉用户「改完再归档将生成 vN+1」，
-- 但归档入口此前无从知道这份草稿是从哪条存档取回的，只能新建一条独立存档。
-- 有了这张表，归档作业在工作线程里按工作副本路径反查，就能让新内容回到原存档升版，
-- 并在资产履历里留下「从 vN 取回 → 再归档生成 vN+1」的可回溯链条。
--
-- 与 027 / 028 / 029 同一纪律：不加外键（撤销归档后回连记录仍可留作审计）。
-- dest_path 用规范化后的绝对路径做主键：同一路径重复取回覆盖为最新一次，
-- 同一份工作副本可以连续多次归档，每次由服务层按内容指纹决定升版或幂等。
CREATE TABLE IF NOT EXISTS analytics_resource_checkouts (
    dest_path        TEXT PRIMARY KEY,
    resource_id      TEXT NOT NULL,
    from_version     INTEGER NOT NULL,   -- 取回时的存档版本（回溯起点）
    created_at       TEXT NOT NULL,
    consumed_at      TEXT,               -- 最近一次「凭此凭据再归档」的时间
    consumed_version INTEGER             -- 该次生成的新版本号
);

CREATE INDEX IF NOT EXISTS idx_arc_resource
    ON analytics_resource_checkouts (resource_id, created_at DESC);
