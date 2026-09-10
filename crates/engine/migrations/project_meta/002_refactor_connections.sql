-- 迁移版本：002
-- 数据库类型：SQLite
-- 作用：重构 connections 表（db_type → driver，添加新字段）
-- 更新时间：2026-04-27

-- SQLite 不支持直接重命名列，需要重建表
-- 此迁移处理各种旧表结构，统一升级到最新结构

-- 1. 重命名旧表
ALTER TABLE connections RENAME TO connections_old;

-- 2. 创建新表结构（完整版）
CREATE TABLE connections (
    id                 TEXT PRIMARY KEY,
    name               TEXT NOT NULL,
    driver             TEXT NOT NULL,           -- 数据库驱动类型
    host               TEXT,                    -- 主机地址
    port               INTEGER,                 -- 端口号
    database           TEXT,                    -- 数据库名
    schema_name        TEXT,                    -- 默认 Schema 名
    username           TEXT,                    -- 用户名
    password_encrypted TEXT,                    -- 加密后的密码
    options            TEXT,                    -- JSON 格式的额外配置
    tags               TEXT,                    -- JSON 格式的标签数组
    use_duckdb_fed     BOOLEAN DEFAULT 0,       -- 是否启用 DuckDB 联邦分析
    metadata_path      TEXT,                    -- 元数据缓存文件路径
    is_active          BOOLEAN DEFAULT 1,       -- 是否激活
    created_at         TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at         TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 3. 迁移旧数据
-- 只使用最核心的列（所有旧版本都有的列）
-- 其他字段使用默认值
INSERT INTO connections (
    id, name, driver, host, port, database
)
SELECT 
    id, 
    name, 
    COALESCE(db_type, 'unknown') as driver,
    host, 
    port, 
    database
FROM connections_old;

-- 4. 删除旧表
DROP TABLE connections_old;

-- 5. 创建索引
CREATE INDEX idx_connections_driver ON connections(driver);
CREATE INDEX idx_connections_active ON connections(is_active);
CREATE INDEX idx_connections_updated ON connections(updated_at DESC);
CREATE INDEX idx_connections_duckdb_fed ON connections(use_duckdb_fed);
