-- 迁移版本：013
-- 数据库类型：SQLite（全局库 global.db）
-- 作用：为 drivers 表添加 MySQL 官方原生驱动 (mysql_async) 和 PostgreSQL 官方原生驱动 (tokio-postgres)
-- 更新时间：2026-05-27
--
-- 背景：
--   原有 drivers 表只有 4 条记录，均基于 sqlx 驱动实现（迁移 008 已修正 ID 为 Registry key）：
--     mysql    → 基于 sqlx::MySql
--     postgres → 基于 sqlx::Postgres
--     sqlite   → 基于 rusqlite
--     duckdb   → 基于 duckdb-rs
--
--   v0.5.3 新增多驱动架构，每个数据库类型可绑定多个驱动实现：
--     mysql_native    → 基于 mysql_async (MySQL 官方 Rust 驱动)
--     postgres_native → 基于 tokio-postgres (PostgreSQL 官方 Rust 驱动)
--
--  两者与原有 sqlx 驱动并存，用户可在连接时选择不同驱动进行对比测试。
--
-- 认证方式说明：
--   mysql_async:      支持 password (native), ssl (TLS)
--   tokio-postgres:   支持 password (SCRAM/MD5), ssl (TLS), kerberos (GSSAPI)
--   auth_type 值与 common-rules.md §10.1 中定义一致：
--     password, ssl, kerberos, ldap, oauth2, os_auth, trust

INSERT OR IGNORE INTO drivers (id, type_id, name, driver_kind, is_file, default_port, url_template, version, config_schema, supported_auth_types, capabilities, driver_properties, enabled) VALUES
    ('mysql_native', 'mysql', 'MySQL (Official)',
     'native', 0, 3306,
     'mysql://{username}:{password}@{host}:{port}/{database}',
     '1.0.0',
     '{"fields":[{"key":"host","label":"主机","type":"text","required":true,"default":"localhost","placeholder":"localhost 或 IP 地址"},{"key":"port","label":"端口","type":"number","required":true,"default":"3306"},{"key":"database","label":"数据库","type":"text","required":false,"placeholder":"可选，留空显示所有数据库"},{"key":"username","label":"用户名","type":"text","required":true,"default":"root"},{"key":"password","label":"密码","type":"password","required":false}],"options":[{"key":"ssl_mode","label":"SSL 模式","type":"select","default":"PREFERRED","values":["DISABLED","PREFERRED","REQUIRED","VERIFY_CA","VERIFY_IDENTITY"]}]}',
     '["password","ssl"]',
     '["tree","health_check","transactions","index_analysis","sql_autocomplete","table_editor"]',
     '{"connectTimeout":"10000","socketTimeout":"30000","maxAllowedPacket":"67108864","useCompression":"true","characterEncoding":"utf8mb4","allowMultiQueries":"true"}',
     1);

INSERT OR IGNORE INTO drivers (id, type_id, name, driver_kind, is_file, default_port, url_template, version, config_schema, supported_auth_types, capabilities, driver_properties, enabled) VALUES
    ('postgres_native', 'postgresql', 'PostgreSQL (Official)',
     'native', 0, 5432,
     'postgres://{username}:{password}@{host}:{port}/{database}',
     '1.0.0',
     '{"fields":[{"key":"host","label":"主机","type":"text","required":true,"default":"localhost","placeholder":"localhost 或 IP 地址"},{"key":"port","label":"端口","type":"number","required":true,"default":"5432"},{"key":"database","label":"数据库","type":"text","required":true,"default":"postgres"},{"key":"username","label":"用户名","type":"text","required":true,"default":"postgres"},{"key":"password","label":"密码","type":"password","required":false}],"options":[{"key":"ssl_mode","label":"SSL 模式","type":"select","default":"prefer","values":["disable","allow","prefer","require","verify-ca","verify-full"]}]}',
     '["password","ssl","kerberos"]',
     '["tree","health_check","transactions","index_analysis","sql_autocomplete","schema_browser","table_editor"]',
     '{"connectTimeout":"10000","socketTimeout":"30000","applicationName":"RdataStation","sslmode":"prefer","keepalivesIdle":"60","statementTimeout":"0","tcpUserTimeout":"0"}',
     1);