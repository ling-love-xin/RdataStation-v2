-- 迁移版本：017
-- 数据库类型：SQLite
-- 作用：数据源导航模块（项目连接）——导航状态 + 连接标签 + 自定义分组（多对多）
-- 更新时间：2026-09-11

-- 导航状态：每个连接一份（展开态 / 选中 / 过滤词）。永不自动删除。
CREATE TABLE IF NOT EXISTS navigator_state (
    conn_id       TEXT PRIMARY KEY,
    scope         TEXT NOT NULL DEFAULT 'project',
    expanded_keys TEXT NOT NULL DEFAULT '[]',
    selected_key  TEXT,
    filter_text   TEXT NOT NULL DEFAULT '',
    version       INTEGER NOT NULL DEFAULT 1,
    updated_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 连接标签（多值）
CREATE TABLE IF NOT EXISTS connection_tags (
    connection_id TEXT NOT NULL,
    tag           TEXT NOT NULL,
    created_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (connection_id, tag)
);

CREATE INDEX IF NOT EXISTS idx_connection_tags_tag ON connection_tags(tag);

-- 自定义分组（项目级）
CREATE TABLE IF NOT EXISTS connection_groups (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    description TEXT,
    sort_order  INTEGER NOT NULL DEFAULT 0,
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 分组 ↔ 连接（多对多）
CREATE TABLE IF NOT EXISTS connection_group_members (
    group_id      TEXT NOT NULL,
    connection_id TEXT NOT NULL,
    sort_order    INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (group_id, connection_id)
);

CREATE INDEX IF NOT EXISTS idx_cgm_connection ON connection_group_members(connection_id);
