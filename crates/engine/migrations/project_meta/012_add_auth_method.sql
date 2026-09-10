-- 迁移版本：012
-- 数据库类型：SQLite（项目库 project.db）
-- 作用：为 connections 表添加 auth_method 字段，记录数据库认证方式
-- 更新时间：2026-05-22
-- 注：全局级 migrations/global/010_add_auth_method.sql 为对应功能（global_connections 表），版本号独立

ALTER TABLE connections ADD COLUMN auth_method TEXT;