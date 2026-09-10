-- 迁移版本：001
-- 数据库类型：SQLite
-- 作用：全局系统数据库（项目索引、全局连接、插件中心、系统设置）
-- 更新时间：2026-04-27

-- ===========================================================================
-- ======================== 应用基础信息表 ===================================
-- ===========================================================================

CREATE TABLE IF NOT EXISTS app_info (
    version         TEXT PRIMARY KEY,
    installed_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    last_run_at     TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- ===========================================================================
-- ======================== 项目索引表 =======================================
-- ===========================================================================

CREATE TABLE IF NOT EXISTS project_info (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    description     TEXT,
    path            TEXT NOT NULL,
    status          TEXT DEFAULT 'active',
    created_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    last_opened_at  TIMESTAMP
);

-- 索引
CREATE INDEX IF NOT EXISTS idx_project_info_last_opened ON project_info(last_opened_at DESC);
CREATE INDEX IF NOT EXISTS idx_project_info_status ON project_info(status);

-- ===========================================================================
-- ======================== 全局连接表 =======================================
-- ===========================================================================

CREATE TABLE IF NOT EXISTS global_connections (
    id                 TEXT PRIMARY KEY,
    name               TEXT NOT NULL,
    driver             TEXT NOT NULL,           -- 数据库驱动类型 (mysql, postgres, sqlite, duckdb)
    host               TEXT,                    -- 主机地址
    port               INTEGER,                 -- 端口号
    database           TEXT,                    -- 数据库名
    schema_name        TEXT,                    -- 默认 Schema 名（PostgreSQL/Oracle 等）
    username           TEXT,                    -- 用户名
    password_encrypted TEXT,                    -- 加密后的密码
    options            TEXT,                    -- JSON 格式的额外配置
    tags               TEXT,                    -- JSON 格式的标签数组
    use_duckdb_fed     BOOLEAN DEFAULT 0,       -- 是否启用 DuckDB 联邦分析
    metadata_path      TEXT,                    -- 元数据缓存文件路径（相对于全局数据目录）
    is_active          BOOLEAN DEFAULT 1,       -- 是否激活
    created_at         TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at         TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 索引
CREATE INDEX IF NOT EXISTS idx_global_connections_driver ON global_connections(driver);
CREATE INDEX IF NOT EXISTS idx_global_connections_active ON global_connections(is_active);
CREATE INDEX IF NOT EXISTS idx_global_connections_updated ON global_connections(updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_global_connections_duckdb_fed ON global_connections(use_duckdb_fed);

-- ===========================================================================
-- ======================== 最近连接表 =======================================
-- ===========================================================================

CREATE TABLE IF NOT EXISTS recent_connections (
    id              TEXT PRIMARY KEY,
    connection_id   TEXT NOT NULL,
    last_used       TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    usage_count     INTEGER DEFAULT 1
);

-- 索引
CREATE INDEX IF NOT EXISTS idx_recent_connections_last_used ON recent_connections(last_used DESC);

-- ===========================================================================
-- ======================== 全局设置表 =======================================
-- ===========================================================================

CREATE TABLE IF NOT EXISTS global_settings (
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL,
    updated_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- ===========================================================================
-- ======================== 导航器状态表 =====================================
-- ===========================================================================

CREATE TABLE IF NOT EXISTS navigator_states (
    id              TEXT PRIMARY KEY,
    connection_id   TEXT NOT NULL,
    expanded_keys   TEXT,
    selected_keys   TEXT,
    filter_config   TEXT,
    updated_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 索引
CREATE INDEX IF NOT EXISTS idx_navigator_states_connection ON navigator_states(connection_id);

-- ===========================================================================
-- ======================== 收藏对象表 =======================================
-- ===========================================================================

CREATE TABLE IF NOT EXISTS favorite_objects (
    id              TEXT PRIMARY KEY,
    connection_id   TEXT NOT NULL,
    database_name   TEXT,
    schema_name     TEXT,
    object_type     TEXT,
    object_name     TEXT,
    note            TEXT,
    added_at        TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 索引
CREATE INDEX IF NOT EXISTS idx_favorite_objects_connection ON favorite_objects(connection_id);
CREATE INDEX IF NOT EXISTS idx_favorite_objects_type ON favorite_objects(object_type);

-- ===========================================================================
-- ======================== 全局插件中心 =====================================
-- ===========================================================================

-- 插件注册表
CREATE TABLE IF NOT EXISTS plugins (
    id              TEXT PRIMARY KEY,
    code            TEXT NOT NULL,
    name            TEXT NOT NULL,
    version         TEXT NOT NULL,
    author          TEXT,
    description     TEXT,
    repo_url        TEXT,
    plugin_type     TEXT NOT NULL,
    manifest_json   TEXT,
    install_path    TEXT NOT NULL,
    is_enabled      INTEGER DEFAULT 1,
    is_builtin      INTEGER DEFAULT 0,
    installed_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(code, version)
);

-- 插件依赖表
CREATE TABLE IF NOT EXISTS plugin_dependencies (
    plugin_id           TEXT NOT NULL,
    dep_code            TEXT NOT NULL,
    dep_version_range   TEXT NOT NULL,
    is_optional         INTEGER DEFAULT 0,
    PRIMARY KEY (plugin_id, dep_code)
);

-- 插件全局配置表
CREATE TABLE IF NOT EXISTS plugin_global_config (
    plugin_id       TEXT NOT NULL,
    "key"           TEXT NOT NULL,
    "value"         TEXT,
    updated_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (plugin_id, "key")
);

-- ===========================================================================
-- ======================== 全局驱动模板表 ===================================
-- ===========================================================================

CREATE TABLE IF NOT EXISTS global_drivers (
    driver_id       TEXT PRIMARY KEY,
    display_name    TEXT NOT NULL,
    driver_type     TEXT NOT NULL,
    default_config  TEXT,
    is_builtin      INTEGER DEFAULT 1,
    enabled         INTEGER DEFAULT 1
);

-- ===========================================================================
-- ======================== 凭据存储表 =======================================
-- ===========================================================================

CREATE TABLE IF NOT EXISTS credential_slots (
    id              TEXT PRIMARY KEY,
    label           TEXT,
    system_key_id   TEXT NOT NULL,
    created_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- ===========================================================================
-- ======================== 全局收藏SQL表 ====================================
-- ===========================================================================

CREATE TABLE IF NOT EXISTS global_saved_queries (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    sql             TEXT NOT NULL,
    group_name      TEXT,
    driver_id       TEXT,
    created_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 索引
CREATE INDEX IF NOT EXISTS idx_global_saved_queries_driver ON global_saved_queries(driver_id);
CREATE INDEX IF NOT EXISTS idx_global_saved_queries_group ON global_saved_queries(group_name);
