-- 迁移版本：010
-- 数据库类型：SQLite（全局库 global.db）
-- 作用：为 global_connections 表添加 auth_method 字段，记录数据库认证方式
-- 更新时间：2026-05-22
-- 注：项目级 migrations/project_meta/012_add_auth_method.sql 为对应功能（connections 表），版本号独立

ALTER TABLE global_connections ADD COLUMN auth_method TEXT;