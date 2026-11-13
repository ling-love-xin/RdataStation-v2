-- 029_analytics_resource_refs.sql
-- 资产谱系：谁从谁派生、谁取代了谁。
--
-- 与 027 / 028 同一纪律：不加外键（撤销归档会硬删资源行，关系记录要留下）。
-- rel_type 用字符串而不是枚举列：谱系种类还会长（升级 / 分发 / 引用），
-- 加一种不该改表结构。
CREATE TABLE IF NOT EXISTS analytics_resource_refs (
    id         TEXT PRIMARY KEY,
    from_id    TEXT NOT NULL,
    to_id      TEXT NOT NULL,
    rel_type   TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_arr_from ON analytics_resource_refs (from_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_arr_to   ON analytics_resource_refs (to_id, created_at DESC);
