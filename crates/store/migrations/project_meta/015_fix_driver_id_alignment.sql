-- 迁移版本：015
-- 数据库类型：SQLite（项目库 project.db）
-- 作用：修复项目级 driver_id 与 Registry key 对齐
-- 更新时间：2026-05-27
--
-- 背景：
--   迁移 010 最初使用 -native 后缀映射 driver_id
--   Runtime DriverRegistry 使用 bare ID（mysql, postgres, sqlite, duckdb）
--   本迁移将 project_drivers 和 connections 中的旧 ID 统一修正
--
-- 注意：项目库的 project_drivers 表不依赖 drivers 表的外键约束，
--   仅通过 driver_id 字段引用全局 drivers.id，修正后引用链恢复正确

-- =============================================================================
-- Step 1: 更新 project_drivers 表的 driver_id
-- =============================================================================

-- 先检查旧 ID 是否存在，避免重复键冲突
UPDATE project_drivers SET driver_id = 'mysql' WHERE driver_id = 'mysql-native';
UPDATE project_drivers SET driver_id = 'postgres' WHERE driver_id = 'postgres-native';
UPDATE project_drivers SET driver_id = 'sqlite' WHERE driver_id = 'sqlite-native';
UPDATE project_drivers SET driver_id = 'duckdb' WHERE driver_id = 'duckdb-native';

-- =============================================================================
-- Step 2: 更新 connections 表的 driver_id 引用
-- =============================================================================

UPDATE connections SET driver_id = 'mysql' WHERE driver_id = 'mysql-native';
UPDATE connections SET driver_id = 'postgres' WHERE driver_id = 'postgres-native';
UPDATE connections SET driver_id = 'sqlite' WHERE driver_id = 'sqlite-native';
UPDATE connections SET driver_id = 'duckdb' WHERE driver_id = 'duckdb-native';

-- =============================================================================
-- Step 3: 验证索引完整性
-- =============================================================================

-- idx_project_drivers_driver_id 是 UNIQUE INDEX，修正后自动生效
-- idx_conn_driver_id 自动更新