-- 迁移版本：016
-- 数据库类型：SQLite
-- 作用：确保数据源模块核心表存在（回退修复，兼容 010 迁移可能因 ALTER TABLE 冲突而失败的情况）
-- 更新时间：2026-06-21

-- 环境管理表
CREATE TABLE IF NOT EXISTS environments (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    description TEXT,
    color       TEXT,
    sort_order  INTEGER DEFAULT 0,
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 环境策略关联表
CREATE TABLE IF NOT EXISTS environment_policies (
    id              TEXT PRIMARY KEY,
    environment_id  TEXT NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    policy_type     TEXT NOT NULL,
    policy_config   TEXT,
    enabled         BOOLEAN DEFAULT 1,
    created_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_env_policies_env_id ON environment_policies(environment_id);

-- 认证配置表
CREATE TABLE IF NOT EXISTS auth_configs (
    id          TEXT PRIMARY KEY,
    name        TEXT,
    auth_type   TEXT NOT NULL,
    auth_data   TEXT NOT NULL,
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 网络配置表
CREATE TABLE IF NOT EXISTS network_configs (
    id            TEXT PRIMARY KEY,
    name          TEXT,
    network_type  TEXT NOT NULL,
    config        TEXT NOT NULL,
    created_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);