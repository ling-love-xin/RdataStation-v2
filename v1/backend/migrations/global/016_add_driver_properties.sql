-- 迁移版本：016
-- 数据库类型：SQLite（全局库 global.db）
-- 作用：为 drivers 表添加 driver_properties 列，补全 capabilities 和默认驱动属性
-- 更新时间：2026-05-27
--
-- 背景：
--   1. drivers 表缺少 driver_properties 列，前端 DriverPropsTab 无法获取默认属性
--   2. SQLite 驱动缺少 index_analysis 能力（rusqlite 支持 EXPLAIN QUERY PLAN）
--   3. DuckDB 驱动缺少 transactions 和 schema_browser 能力（DuckDB 均支持）
--
-- driver_properties 存储 JSON 键值对，格式：{"key":"value", ...}
-- 前端在新建连接时从该字段获取默认属性填充 DriverPropsTab

ALTER TABLE drivers ADD COLUMN driver_properties TEXT;

-- ===== 更新现有驱动的 capabilities（修正遗漏的能力） =====

-- SQLite: 补充 index_analysis
UPDATE drivers SET capabilities = '["tree","health_check","transactions","index_analysis","sql_autocomplete","table_editor"]'
WHERE id = 'sqlite' AND capabilities IS NOT NULL;

-- DuckDB: 补充 transactions + schema_browser
UPDATE drivers SET capabilities = '["tree","health_check","transactions","sql_autocomplete","schema_browser","analytics","federation","table_editor"]'
WHERE id = 'duckdb' AND capabilities IS NOT NULL;

-- ===== 填充 driver_properties 默认值 =====

-- MySQL (sqlx) 驱动属性
UPDATE drivers SET driver_properties = '{"connectTimeout":"10000","socketTimeout":"30000","maxAllowedPacket":"67108864","useCompression":"true","characterEncoding":"utf8mb4","allowMultiQueries":"true"}'
WHERE id = 'mysql' AND driver_properties IS NULL;

-- MySQL (Official / mysql_async) 驱动属性
UPDATE drivers SET driver_properties = '{"connectTimeout":"10000","socketTimeout":"30000","maxAllowedPacket":"67108864","useCompression":"true","characterEncoding":"utf8mb4","allowMultiQueries":"true"}'
WHERE id = 'mysql_native' AND driver_properties IS NULL;

-- PostgreSQL (sqlx) 驱动属性
UPDATE drivers SET driver_properties = '{"connectTimeout":"10000","socketTimeout":"30000","applicationName":"RdataStation","sslmode":"prefer","keepalivesIdle":"60","statementTimeout":"0"}'
WHERE id = 'postgres' AND driver_properties IS NULL;

-- PostgreSQL (Official / tokio-postgres) 驱动属性
UPDATE drivers SET driver_properties = '{"connectTimeout":"10000","socketTimeout":"30000","applicationName":"RdataStation","sslmode":"prefer","keepalivesIdle":"60","statementTimeout":"0","tcpUserTimeout":"0"}'
WHERE id = 'postgres_native' AND driver_properties IS NULL;

-- SQLite (rusqlite) 驱动属性
UPDATE drivers SET driver_properties = '{"journalMode":"WAL","synchronous":"NORMAL","busyTimeout":"5000","cacheSize":"-2000","foreignKeys":"true","tempStore":"MEMORY"}'
WHERE id = 'sqlite' AND driver_properties IS NULL;

-- DuckDB (duckdb-rs) 驱动属性
UPDATE drivers SET driver_properties = '{"memoryLimit":"1GB","threads":"4","enableObjectCache":"true","tempDirectory":"","accessMode":"automatic","preserveInsertionOrder":"true"}'
WHERE id = 'duckdb' AND driver_properties IS NULL;