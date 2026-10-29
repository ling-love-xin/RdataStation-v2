-- 迁移版本：005
-- 数据库类型：SQLite
-- 作用：global_connections 表增加 server_version 字段，缓存首次连接时的数据库版本
-- 更新时间：2026-05-09

ALTER TABLE global_connections ADD COLUMN server_version TEXT;