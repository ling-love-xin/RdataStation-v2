-- 迁移版本：014
-- 数据库类型：SQLite（全局库 global.db）
-- 作用：修复 drivers 表 ID 与 Registry key 对齐
-- 更新时间：2026-05-27
--
-- 背景：
--   迁移 008 最初使用 -native 后缀（mysql-native, postgres-native, sqlite-native, duckdb-native）
--   但 Runtime DriverRegistry 的 key 是 bare ID（mysql, postgres, sqlite, duckdb）
--   013 迁移新增的 mysql_native/postgres_native 使用下划线，与 Registry 一致
--
--   本迁移将旧 ID 统一为 Registry key，确保路由正确：
--     mysql-native  → mysql    （sqlx）
--     postgres-native → postgres（sqlx）
--     sqlite-native → sqlite   （rusqlite）
--     duckdb-native → duckdb   （duckdb-rs）
--
--   同时更新引用链：global_connections.driver_id
--   注意：mysql_native 和 postgres_native 的 ID 已是下划线，Registry 中匹配，无需修改

-- =============================================================================
-- Step 1: 更新 drivers 表 ID
-- =============================================================================

-- 先检查旧 ID 是否存在，避免 UPDATE 0 行后 INSERT OR IGNORE 产生重复
UPDATE drivers SET id = 'mysql' WHERE id = 'mysql-native';
UPDATE drivers SET id = 'postgres' WHERE id = 'postgres-native';
UPDATE drivers SET id = 'sqlite' WHERE id = 'sqlite-native';
UPDATE drivers SET id = 'duckdb' WHERE id = 'duckdb-native';

-- =============================================================================
-- Step 2: 更新 global_connections 表的 driver_id 引用
-- =============================================================================

UPDATE global_connections SET driver_id = 'mysql' WHERE driver_id = 'mysql-native';
UPDATE global_connections SET driver_id = 'postgres' WHERE driver_id = 'postgres-native';
UPDATE global_connections SET driver_id = 'sqlite' WHERE driver_id = 'sqlite-native';
UPDATE global_connections SET driver_id = 'duckdb' WHERE driver_id = 'duckdb-native';

-- =============================================================================
-- Step 3: 更新 auth_configs 表中可能的 driver 引用（如有）
-- =============================================================================

-- auth_configs 不直接引用 driver，跳过

-- =============================================================================
-- Step 4: 更新 network_configs 表中可能的 driver 引用（如有）
-- =============================================================================

-- network_configs 不直接引用 driver，跳过