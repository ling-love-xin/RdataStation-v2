-- 迁移版本：001
-- 数据库类型：SQLite
-- 作用：项目级核心数据存储（连接配置、SQL历史、设置、工作台状态）
-- 更新时间：2026-04-27

-- ===========================================================================
-- ======================== 项目信息表 =======================================
-- ===========================================================================

CREATE TABLE IF NOT EXISTS project (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    description     TEXT,
    created_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- ===========================================================================
-- ======================== 数据库连接表 =====================================
-- ===========================================================================

-- 连接信息表（兼容旧版本：使用 db_type 字段）
-- 注意：如果表已存在（旧项目），CREATE TABLE IF NOT EXISTS 会跳过
-- 索引由 002_refactor_connections.sql 在重建表时创建
CREATE TABLE IF NOT EXISTS connections (
    id                 TEXT PRIMARY KEY,
    name               TEXT NOT NULL,
    db_type            TEXT NOT NULL,           -- 数据库驱动类型 (mysql, postgres, sqlite, duckdb)
    host               TEXT,                    -- 主机地址
    port               INTEGER,                 -- 端口号
    database           TEXT,                    -- 数据库名
    username           TEXT,                    -- 用户名
    password_encrypted TEXT,                    -- 加密后的密码
    options            TEXT,                    -- JSON 格式的额外配置
    tags               TEXT,                    -- JSON 格式的标签数组
    is_active          BOOLEAN DEFAULT 1,       -- 是否激活
    created_at         TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at         TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- ===========================================================================
-- ======================== SQL 查询历史表 ===================================
-- ===========================================================================

-- 查询历史表（企业级，支持去重、分析、慢查询）
CREATE TABLE IF NOT EXISTS query_history (
    id              TEXT PRIMARY KEY,                     -- 历史ID
    connection_id   TEXT,                                 -- 关联连接ID
    database_name   TEXT,                                 -- 执行时数据库
    schema_name     TEXT,                                 -- 执行时Schema
    sql             TEXT NOT NULL,                        -- 执行SQL
    sql_hash        TEXT NOT NULL,                        -- SQL哈希值（去重）
    exec_mode       TEXT NOT NULL DEFAULT 'native',       -- 执行模式：native/duckdb_fed
    category        TEXT NOT NULL DEFAULT 'query',        -- 类型：query/ddl/dml
    success         BOOLEAN DEFAULT 1,                    -- 是否成功
    error_message   TEXT,                                 -- 错误信息
    duration_ms     INTEGER,                              -- 耗时（毫秒）
    rows_returned   INTEGER,                              -- 返回行数
    rows_affected   INTEGER,                              -- 影响行数
    is_pinned       BOOLEAN DEFAULT 0,                    -- 是否固定置顶
    created_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP,  -- 执行时间
    FOREIGN KEY (connection_id) REFERENCES connections(id)
);

-- 索引
CREATE INDEX IF NOT EXISTS idx_qh_conn ON query_history(connection_id);
CREATE INDEX IF NOT EXISTS idx_qh_time ON query_history(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_qh_hash ON query_history(sql_hash);
CREATE INDEX IF NOT EXISTS idx_qh_category ON query_history(category);
CREATE INDEX IF NOT EXISTS idx_qh_pinned ON query_history(is_pinned);

-- 兼容视图（保持旧代码兼容）
CREATE VIEW IF NOT EXISTS sql_history AS
SELECT 
    id,
    connection_id,
    sql AS sql_text,
    duration_ms AS execution_time_ms,
    rows_affected,
    error_message,
    is_pinned AS is_favorite,
    created_at
FROM query_history;

-- ===========================================================================
-- ======================== 项目设置表 =======================================
-- ===========================================================================

CREATE TABLE IF NOT EXISTS project_settings (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- ===========================================================================
-- ======================== 工作台状态表 =====================================
-- ===========================================================================

CREATE TABLE IF NOT EXISTS workbench_state (
    id              TEXT PRIMARY KEY DEFAULT 'default',
    layout          TEXT,
    open_panels     TEXT,
    active_panel_id TEXT,
    updated_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- ===========================================================================
-- ======================== 插件引用表（预留） ===============================
-- ===========================================================================

-- 项目使用插件表
CREATE TABLE IF NOT EXISTS project_used_plugins (
    plugin_code         TEXT NOT NULL,
    plugin_version      TEXT NOT NULL,
    enabled             INTEGER DEFAULT 1,
    required            INTEGER DEFAULT 1,
    PRIMARY KEY (plugin_code, plugin_version)
);

-- 项目插件配置表
CREATE TABLE IF NOT EXISTS project_plugin_config (
    plugin_code         TEXT NOT NULL,
    plugin_version      TEXT NOT NULL,
    "key"               TEXT NOT NULL,
    "value"             TEXT,
    updated_at          TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (plugin_code, plugin_version, "key")
);
