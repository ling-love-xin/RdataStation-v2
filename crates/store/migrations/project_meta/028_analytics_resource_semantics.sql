-- 028_analytics_resource_semantics.sql
-- 业务断言与效力：把「这个数是什么意思、现在还成不成立」变成可留痕的一等公民。
--
-- 为什么不往 analytics_resources 加列：那张表的列顺序被 RESOURCE_COLUMNS / map_resource_row
-- 的硬编码下标锁住，加列要同时改行模型与所有 struct 字面量；断言是「选填的、一条资源最多
-- 一份」的扩展语义，独立表更贴切，也让老行天然等于「没有断言」。
--
-- 与 027 同一纪律：不加外键。撤销归档会硬删资源行，履历与口径记录要留下。
CREATE TABLE IF NOT EXISTS analytics_resource_assertions (
    resource_id  TEXT PRIMARY KEY,
    question     TEXT,
    metric_key   TEXT,
    metric_name  TEXT,
    definition   TEXT,
    assumptions  TEXT,
    time_window  TEXT,
    conclusion   TEXT,
    owner        TEXT,
    confirmed_by TEXT,
    confirmed_at TEXT,
    semantic_hash TEXT NOT NULL,
    updated_at   TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_ara_metric
    ON analytics_resource_assertions (metric_key);

CREATE TABLE IF NOT EXISTS analytics_resource_lifecycle (
    resource_id   TEXT PRIMARY KEY,
    state         TEXT NOT NULL DEFAULT 'draft',
    valid_until   TEXT,
    superseded_by TEXT,
    updated_at    TEXT NOT NULL
);
