-- 迁移版本：001
-- 数据库类型：SQLite
-- 作用：每个连接独立的元数据存储（表、视图、字段、函数、索引、触发器）
-- 归属：项目级，每个数据库连接一个独立的元数据文件
-- 更新时间：2026-04-27

-- ===========================================================================
-- ======================== 元数据总表 =======================================
-- ===========================================================================

CREATE TABLE IF NOT EXISTS metadata (
    id              TEXT PRIMARY KEY,
    obj_type        TEXT NOT NULL,              -- 对象类型: table/view/column/index/function/trigger/procedure
    database_name   TEXT NOT NULL,              -- 数据库名
    schema_name     TEXT NOT NULL,              -- 模式名
    table_name      TEXT NOT NULL,              -- 表名
    name            TEXT,                       -- 对象名称
    data_type       TEXT,                       -- 数据类型（仅列）
    is_nullable     INTEGER,                    -- 是否可空
    is_primary      INTEGER DEFAULT 0,          -- 是否主键
    is_unique       INTEGER DEFAULT 0,          -- 是否唯一
    comment         TEXT,                       -- 注释
    definition      TEXT,                       -- 定义（视图/函数/过程）
    extra           TEXT,                       -- JSON 格式的额外信息
    last_sync       INTEGER NOT NULL            -- 最后同步时间戳
);

-- 索引
CREATE INDEX IF NOT EXISTS idx_meta_obj ON metadata(obj_type);
CREATE INDEX IF NOT EXISTS idx_meta_search ON metadata(database_name, schema_name, table_name, name);
CREATE INDEX IF NOT EXISTS idx_meta_schema ON metadata(database_name, schema_name);
CREATE INDEX IF NOT EXISTS idx_meta_table ON metadata(database_name, schema_name, table_name);

-- ===========================================================================
-- ======================== 元数据同步日志表 =================================
-- ===========================================================================

CREATE TABLE IF NOT EXISTS sync_log (
    id              TEXT PRIMARY KEY,
    start_at        INTEGER NOT NULL,           -- 开始时间戳
    end_at          INTEGER NOT NULL,           -- 结束时间戳
    success         INTEGER DEFAULT 1,          -- 是否成功
    message         TEXT,                       -- 日志消息
    objects_fetched INTEGER DEFAULT 0           -- 获取的对象数量
);

-- 索引
CREATE INDEX IF NOT EXISTS idx_sync_log_time ON sync_log(start_at DESC);
CREATE INDEX IF NOT EXISTS idx_sync_log_success ON sync_log(success);
