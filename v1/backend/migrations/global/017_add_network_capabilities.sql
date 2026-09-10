-- 迁移版本：017
-- 数据库类型：SQLite（全局库 global.db）
-- 作用：为网络数据库驱动（MySQL/PostgreSQL）补齐网络协议 capabilities
--       新增 ssh_tunnel、ssl_tls、proxy 三个网络能力关键词
-- 更新时间：2026-05-31
--
-- 背景：
--   NetworkTab 组件通过 drivers.capabilities 判断驱动支持哪些网络协议。
--   原有 capabilities 仅包含数据库功能级能力（tree/health_check/transactions
--   等），不含网络能力关键词，导致 NetworkTab 中 SSH/SSL/Proxy 三个默认条目
--   被过滤隐藏。
--
--   本次迁移为 MySQL 和 PostgreSQL 系列驱动显式声明网络能力，确保 NetworkTab
--   网络协议链正常展示。
--
--   注意：SQLite/DuckDB 为文件型数据库（is_file=1），NetworkTab 已通过
--   v-if="driver?.is_file" 前置判別，无需网络能力。

-- MySQL (sqlx)
UPDATE drivers SET capabilities = '["tree","health_check","transactions","index_analysis","sql_autocomplete","table_editor","ssh_tunnel","ssl_tls","proxy"]' WHERE id = 'mysql';

-- MySQL (Official native)
UPDATE drivers SET capabilities = '["tree","health_check","transactions","index_analysis","sql_autocomplete","table_editor","ssh_tunnel","ssl_tls","proxy"]' WHERE id = 'mysql_native';

-- PostgreSQL (sqlx)
UPDATE drivers SET capabilities = '["tree","health_check","transactions","index_analysis","sql_autocomplete","schema_browser","table_editor","ssh_tunnel","ssl_tls","proxy"]' WHERE id = 'postgres';

-- PostgreSQL (Official native)
UPDATE drivers SET capabilities = '["tree","health_check","transactions","index_analysis","sql_autocomplete","schema_browser","table_editor","ssh_tunnel","ssl_tls","proxy"]' WHERE id = 'postgres_native';