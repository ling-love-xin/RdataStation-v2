-- 迁移版本：010
-- 数据库类型：SQLite
-- 作用：数据源模块 - 项目驱动映射、环境管理、认证配置、网络配置
-- 更新时间：2026-05-18

CREATE TABLE IF NOT EXISTS project_drivers (
    id            TEXT PRIMARY KEY,
    driver_id     TEXT NOT NULL,
    enabled       BOOLEAN DEFAULT 1,
    installed_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_project_drivers_driver_id ON project_drivers(driver_id);
CREATE INDEX IF NOT EXISTS idx_project_drivers_enabled ON project_drivers(enabled);

CREATE TABLE IF NOT EXISTS environments (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    description TEXT,
    color       TEXT,
    sort_order  INTEGER DEFAULT 0,
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS environment_policies (
    id              TEXT PRIMARY KEY,
    environment_id  TEXT NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    policy_type     TEXT NOT NULL,
    policy_config   TEXT,
    enabled         BOOLEAN DEFAULT 1,
    created_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_env_policies_env_id ON environment_policies(environment_id);

CREATE TABLE IF NOT EXISTS auth_configs (
    id          TEXT PRIMARY KEY,
    name        TEXT,
    auth_type   TEXT NOT NULL,
    auth_data   TEXT NOT NULL,
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS network_configs (
    id            TEXT PRIMARY KEY,
    name          TEXT,
    network_type  TEXT NOT NULL,
    config        TEXT NOT NULL,
    created_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

ALTER TABLE connections ADD COLUMN server_version TEXT;
ALTER TABLE connections ADD COLUMN description TEXT;
ALTER TABLE connections ADD COLUMN driver_id TEXT;
ALTER TABLE connections ADD COLUMN environment_id TEXT;
ALTER TABLE connections ADD COLUMN auth_config_id TEXT;
ALTER TABLE connections ADD COLUMN network_config_id TEXT;
ALTER TABLE connections ADD COLUMN driver_properties TEXT;
ALTER TABLE connections ADD COLUMN advanced_options TEXT;

-- driver_id 直接复制 driver 字段（与 Registry key 对齐）
-- Registry keys: mysql, postgres, sqlite, duckdb
UPDATE connections SET driver_id = driver WHERE driver_id IS NULL;

CREATE INDEX IF NOT EXISTS idx_conn_driver_id ON connections(driver_id);
CREATE INDEX IF NOT EXISTS idx_conn_env ON connections(environment_id);