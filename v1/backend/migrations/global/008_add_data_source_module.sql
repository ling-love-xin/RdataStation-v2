-- 迁移版本：008
-- 数据库类型：SQLite
-- 作用：数据源模块 - 数据源类型、驱动注册、驱动文件、环境管理、认证配置、网络配置
-- 更新时间：2026-05-18

CREATE TABLE IF NOT EXISTS data_source_types (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    category    TEXT NOT NULL,
    icon        TEXT,
    enabled     BOOLEAN DEFAULT 1,
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

DROP TABLE IF EXISTS global_drivers;

CREATE TABLE IF NOT EXISTS drivers (
    id                    TEXT PRIMARY KEY,
    type_id               TEXT NOT NULL REFERENCES data_source_types(id),
    name                  TEXT NOT NULL,
    driver_kind           TEXT DEFAULT 'native',
    is_file               BOOLEAN DEFAULT 0,
    default_port          INTEGER,
    url_template          TEXT,
    download_url          TEXT,
    download_checksum     TEXT,
    version               TEXT,
    config_schema         TEXT NOT NULL,
    supported_auth_types  TEXT,
    capabilities          TEXT,
    driver_properties     TEXT,
    enabled               BOOLEAN DEFAULT 1,
    created_at            TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_drivers_type_id ON drivers(type_id);
CREATE INDEX IF NOT EXISTS idx_drivers_driver_kind ON drivers(driver_kind);
CREATE INDEX IF NOT EXISTS idx_drivers_enabled ON drivers(enabled);

CREATE TABLE IF NOT EXISTS driver_files (
    id            TEXT PRIMARY KEY,
    driver_id     TEXT NOT NULL REFERENCES drivers(id),
    file_path     TEXT NOT NULL,
    file_name     TEXT NOT NULL,
    file_size     INTEGER,
    checksum      TEXT,
    version       TEXT NOT NULL,
    installed_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_driver_files_driver_id ON driver_files(driver_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_driver_files_driver_version ON driver_files(driver_id, version);

CREATE TABLE IF NOT EXISTS environments (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    description TEXT,
    color       TEXT,
    sort_order  INTEGER DEFAULT 0,
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS environment_policies (
    id              TEXT PRIMARY KEY,
    environment_id  TEXT NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    policy_type     TEXT NOT NULL,
    policy_config   TEXT,
    enabled         BOOLEAN DEFAULT 1,
    created_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_env_policies_env_id ON environment_policies(environment_id);

CREATE TABLE IF NOT EXISTS auth_configs (
    id          TEXT PRIMARY KEY,
    name        TEXT,
    auth_type   TEXT NOT NULL,
    auth_data   TEXT NOT NULL,
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS network_configs (
    id            TEXT PRIMARY KEY,
    name          TEXT,
    network_type  TEXT NOT NULL,
    config        TEXT NOT NULL,
    created_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

ALTER TABLE global_connections ADD COLUMN description TEXT;
ALTER TABLE global_connections ADD COLUMN driver_id TEXT;
ALTER TABLE global_connections ADD COLUMN environment_id TEXT;
ALTER TABLE global_connections ADD COLUMN auth_config_id TEXT;
ALTER TABLE global_connections ADD COLUMN network_config_id TEXT;
ALTER TABLE global_connections ADD COLUMN driver_properties TEXT;
ALTER TABLE global_connections ADD COLUMN advanced_options TEXT;

-- driver_id 直接复制 driver 字段（与 Registry key 对齐）
-- Registry keys: mysql, postgres, sqlite, duckdb
UPDATE global_connections SET driver_id = driver WHERE driver_id IS NULL;

CREATE INDEX IF NOT EXISTS idx_gc_driver_id ON global_connections(driver_id);
CREATE INDEX IF NOT EXISTS idx_gc_env ON global_connections(environment_id);

INSERT OR IGNORE INTO data_source_types (id, name, category, icon, enabled) VALUES
    ('mysql',      'MySQL',      'relational',  '🐬', 1),
    ('postgresql', 'PostgreSQL', 'relational',  '🐘', 1),
    ('sqlite',     'SQLite',     'file-based',  '🪶', 1),
    ('duckdb',     'DuckDB',     'analytics',   '🦆', 1),
    ('mariadb',    'MariaDB',    'relational',  '🦭', 1),
    ('oracle',     'Oracle',     'relational',  '🔴', 1),
    ('mssql',      'SQL Server', 'relational',  '🟢', 1),
    ('clickhouse', 'ClickHouse', 'analytics',   '⚡', 1),
    ('mongodb',    'MongoDB',    'nosql',       '🍃', 0),
    ('redis',      'Redis',      'nosql',       '🔶', 0);

-- 驱动 ID 与 Registry key 严格对齐：
--   mysql   → MySqlDriverFactory (sqlx)
--   postgres → PostgresDriverFactory (sqlx)
--   sqlite  → SqliteDriverFactory (rusqlite)
--   duckdb  → DuckDbDriverFactory (duckdb-rs)
INSERT OR IGNORE INTO drivers (id, type_id, name, driver_kind, is_file, default_port, url_template, version, config_schema, supported_auth_types, capabilities, driver_properties, enabled) VALUES
    ('mysql', 'mysql', 'MySQL (sqlx)',
     'native', 0, 3306,
     'mysql://{username}:{password}@{host}:{port}/{database}',
     '1.0.0',
     '{"fields":[{"key":"host","label":"主机","type":"text","required":true,"default":"localhost","placeholder":"localhost 或 IP 地址"},{"key":"port","label":"端口","type":"number","required":true,"default":"3306"},{"key":"database","label":"数据库","type":"text","required":false,"placeholder":"可选，留空显示所有数据库"},{"key":"username","label":"用户名","type":"text","required":true,"default":"root"},{"key":"password","label":"密码","type":"password","required":false}],"options":[{"key":"ssl_mode","label":"SSL 模式","type":"select","default":"PREFERRED","values":["DISABLED","PREFERRED","REQUIRED","VERIFY_CA","VERIFY_IDENTITY"]}]}',
     '["password","ssl"]',
     '["tree","health_check","transactions","index_analysis","sql_autocomplete","table_editor"]',
     '{"connectTimeout":"10000","socketTimeout":"30000","maxAllowedPacket":"67108864","useCompression":"true","characterEncoding":"utf8mb4","allowMultiQueries":"true"}',
     1);

INSERT OR IGNORE INTO drivers (id, type_id, name, driver_kind, is_file, default_port, url_template, version, config_schema, supported_auth_types, capabilities, driver_properties, enabled) VALUES
    ('postgres', 'postgresql', 'PostgreSQL (sqlx)',
     'native', 0, 5432,
     'postgres://{username}:{password}@{host}:{port}/{database}',
     '1.0.0',
     '{"fields":[{"key":"host","label":"主机","type":"text","required":true,"default":"localhost","placeholder":"localhost 或 IP 地址"},{"key":"port","label":"端口","type":"number","required":true,"default":"5432"},{"key":"database","label":"数据库","type":"text","required":true,"default":"postgres"},{"key":"username","label":"用户名","type":"text","required":true,"default":"postgres"},{"key":"password","label":"密码","type":"password","required":false}],"options":[{"key":"ssl_mode","label":"SSL 模式","type":"select","default":"prefer","values":["disable","allow","prefer","require","verify-ca","verify-full"]}]}',
     '["password","ssl","kerberos"]',
     '["tree","health_check","transactions","index_analysis","sql_autocomplete","schema_browser","table_editor"]',
     '{"connectTimeout":"10000","socketTimeout":"30000","applicationName":"RdataStation","sslmode":"prefer","keepalivesIdle":"60","statementTimeout":"0"}',
     1);

INSERT OR IGNORE INTO drivers (id, type_id, name, driver_kind, is_file, default_port, url_template, version, config_schema, supported_auth_types, capabilities, driver_properties, enabled) VALUES
    ('sqlite', 'sqlite', 'SQLite (rusqlite)',
     'native', 1, NULL,
     'sqlite://{file_path}',
     '1.0.0',
     '{"fields":[{"key":"file_path","label":"数据库文件","type":"file","required":true,"placeholder":"选择 .db 或 .sqlite 文件"}],"options":[{"key":"mode","label":"打开模式","type":"select","default":"rwc","values":["ro","rw","rwc"]}]}',
     '["password"]',
     '["tree","health_check","transactions","index_analysis","sql_autocomplete","table_editor"]',
     '{"journalMode":"WAL","synchronous":"NORMAL","busyTimeout":"5000","cacheSize":"-2000","foreignKeys":"true","tempStore":"MEMORY"}',
     1);

INSERT OR IGNORE INTO drivers (id, type_id, name, driver_kind, is_file, default_port, url_template, version, config_schema, supported_auth_types, capabilities, driver_properties, enabled) VALUES
    ('duckdb', 'duckdb', 'DuckDB (duckdb-rs)',
     'native', 1, NULL,
     'duckdb://{file_path}',
     '1.0.0',
     '{"fields":[{"key":"file_path","label":"数据库文件","type":"file","required":true,"placeholder":"选择 .duckdb 文件或 :memory:"}],"options":[{"key":"memory_limit","label":"内存限制","type":"text","default":"","placeholder":"例如: 1GB, 512MB（留空表示无限制）"}]}',
     '["password"]',
     '["tree","health_check","transactions","sql_autocomplete","schema_browser","analytics","federation","table_editor"]',
     '{"memoryLimit":"1GB","threads":"4","enableObjectCache":"true","tempDirectory":"","accessMode":"automatic","preserveInsertionOrder":"true"}',
     1);