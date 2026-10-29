-- 迁移版本：018
-- 数据库类型：SQLite
-- 作用：数据源导航模块（全局连接）——导航状态 + 连接标签
-- 更新时间：2026-09-11

-- 导航状态：每个连接一份（展开态 / 选中 / 过滤词）。永不自动删除。
CREATE TABLE IF NOT EXISTS navigator_state (
    conn_id       TEXT PRIMARY KEY,
    scope         TEXT NOT NULL DEFAULT 'global',
    expanded_keys TEXT NOT NULL DEFAULT '[]',
    selected_key  TEXT,
    filter_text   TEXT NOT NULL DEFAULT '',
    version       INTEGER NOT NULL DEFAULT 1,
    updated_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 连接标签（多值，独立表便于 tag 检索）
CREATE TABLE IF NOT EXISTS connection_tags (
    connection_id TEXT NOT NULL,
    tag           TEXT NOT NULL,
    created_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (connection_id, tag)
);

CREATE INDEX IF NOT EXISTS idx_connection_tags_tag ON connection_tags(tag);
