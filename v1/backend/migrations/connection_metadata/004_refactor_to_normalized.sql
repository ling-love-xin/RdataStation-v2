-- 迁移版本：004
-- 数据库类型：SQLite
-- 作用：从统一表重构为规范化分表设计
-- 重构内容：
--   - 统一表 metadata → 多个独立表 (schemata/tables/columns/indexes...)
--   - 添加外键约束确保引用完整性
--   - 创建向后兼容视图
-- 更新时间：2026-05-04

-- ===========================================================================
-- ======================== 第一阶段：创建新表 =================================
-- ===========================================================================

-- 1. 模式/数据库表
CREATE TABLE IF NOT EXISTS schemata (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    catalog_name TEXT,                       -- 目录名 (MySQL 通常为 NULL)
    schema_name TEXT NOT NULL,              -- 模式名
    owner TEXT,                             -- 所有者
    default_character_set_name TEXT,         -- 默认字符集
    default_collation_name TEXT,             -- 默认排序规则
    sql_path TEXT,                           -- 路径 (PostgreSQL)
    comment TEXT,
    -- 缓存控制字段
    introspect_level INTEGER DEFAULT 3,
    is_loaded INTEGER DEFAULT 0,
    last_sync INTEGER,
    last_accessed INTEGER,
    extra TEXT,                             -- JSON 扩展
    UNIQUE (catalog_name, schema_name)
);

-- 2. 表 & 视图
CREATE TABLE IF NOT EXISTS tables (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    schema_id INTEGER NOT NULL REFERENCES schemata(id) ON DELETE CASCADE,
    table_name TEXT NOT NULL,
    table_type TEXT NOT NULL CHECK (table_type IN (
        'TABLE', 'VIEW', 'MATERIALIZED VIEW', 'FOREIGN TABLE',
        'PARTITIONED TABLE', 'SYSTEM TABLE', 'GLOBAL TEMPORARY', 'LOCAL TEMPORARY'
    )),
    -- 通用属性
    row_count_estimate INTEGER,
    data_length INTEGER,
    index_length INTEGER,
    engine TEXT,
    row_format TEXT,
    auto_increment_val INTEGER,
    table_comment TEXT,
    -- 时间信息
    created_at INTEGER,
    last_altered_at INTEGER,
    last_accessed_at INTEGER,
    -- 缓存控制
    introspect_level INTEGER DEFAULT 3,
    is_loaded INTEGER DEFAULT 0,
    last_sync INTEGER,
    last_accessed INTEGER,
    -- 扩展
    extra_attributes TEXT,
    UNIQUE (schema_id, table_name)
);

-- 3. 列
CREATE TABLE IF NOT EXISTS columns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    table_id INTEGER NOT NULL REFERENCES tables(id) ON DELETE CASCADE,
    column_name TEXT NOT NULL,
    ordinal_position INTEGER,
    column_default TEXT,
    is_nullable INTEGER,
    data_type TEXT NOT NULL,
    full_data_type TEXT,
    type_category TEXT,
    character_maximum_length INTEGER,
    character_octet_length INTEGER,
    numeric_precision INTEGER,
    numeric_scale INTEGER,
    datetime_precision INTEGER,
    interval_type TEXT,
    interval_precision INTEGER,
    character_set_name TEXT,
    collation_name TEXT,
    domain_schema TEXT,
    domain_name TEXT,
    is_identity INTEGER,
    identity_generation TEXT,
    identity_start INTEGER,
    identity_increment INTEGER,
    is_generated INTEGER,
    generation_expression TEXT,
    column_comment TEXT,
    -- 缓存控制
    introspect_level INTEGER DEFAULT 3,
    is_loaded INTEGER DEFAULT 0,
    last_sync INTEGER,
    last_accessed INTEGER,
    -- 扩展
    extra TEXT,
    UNIQUE (table_id, column_name)
);

-- 4. 索引
CREATE TABLE IF NOT EXISTS indexes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    table_id INTEGER NOT NULL REFERENCES tables(id) ON DELETE CASCADE,
    index_name TEXT NOT NULL,
    index_type TEXT,
    is_unique INTEGER DEFAULT 0,
    is_primary INTEGER DEFAULT 0,
    is_clustered INTEGER DEFAULT 0,
    index_comment TEXT,
    -- 缓存控制
    introspect_level INTEGER DEFAULT 3,
    is_loaded INTEGER DEFAULT 0,
    last_sync INTEGER,
    last_accessed INTEGER,
    -- 扩展
    extra_attributes TEXT,
    UNIQUE (table_id, index_name)
);

-- 5. 索引列
CREATE TABLE IF NOT EXISTS index_columns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    index_id INTEGER NOT NULL REFERENCES indexes(id) ON DELETE CASCADE,
    column_name TEXT NOT NULL,
    ordinal_position INTEGER,
    sort_order TEXT,
    is_included_column INTEGER DEFAULT 0,
    expression TEXT,
    UNIQUE (index_id, column_name, ordinal_position)
);

-- 6. 外键约束
CREATE TABLE IF NOT EXISTS foreign_keys (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    table_id INTEGER NOT NULL REFERENCES tables(id) ON DELETE CASCADE,
    constraint_name TEXT NOT NULL,
    ref_schema_id INTEGER REFERENCES schemata(id),
    ref_table_id INTEGER REFERENCES tables(id),
    delete_rule TEXT,
    update_rule TEXT,
    deferrability TEXT,
    -- 缓存控制
    introspect_level INTEGER DEFAULT 3,
    is_loaded INTEGER DEFAULT 0,
    last_sync INTEGER,
    last_accessed INTEGER,
    UNIQUE (table_id, constraint_name)
);

-- 7. 外键列映射
CREATE TABLE IF NOT EXISTS foreign_key_columns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    foreign_key_id INTEGER NOT NULL REFERENCES foreign_keys(id) ON DELETE CASCADE,
    ordinal_position INTEGER,
    column_name TEXT NOT NULL,
    ref_column_name TEXT NOT NULL,
    UNIQUE (foreign_key_id, column_name)
);

-- 8. 检查约束
CREATE TABLE IF NOT EXISTS check_constraints (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    table_id INTEGER NOT NULL REFERENCES tables(id) ON DELETE CASCADE,
    constraint_name TEXT NOT NULL,
    check_clause TEXT NOT NULL,
    -- 缓存控制
    introspect_level INTEGER DEFAULT 3,
    is_loaded INTEGER DEFAULT 0,
    last_sync INTEGER,
    UNIQUE (table_id, constraint_name)
);

-- 9. 视图定义
CREATE TABLE IF NOT EXISTS view_definitions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    table_id INTEGER NOT NULL REFERENCES tables(id) ON DELETE CASCADE,
    view_definition TEXT NOT NULL,
    is_updatable INTEGER,
    check_option TEXT,
    -- 缓存控制
    introspect_level INTEGER DEFAULT 3,
    is_loaded INTEGER DEFAULT 0,
    last_sync INTEGER
);

-- 10. 存储过程 / 函数
CREATE TABLE IF NOT EXISTS routines (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    schema_id INTEGER NOT NULL REFERENCES schemata(id) ON DELETE CASCADE,
    routine_name TEXT NOT NULL,
    routine_type TEXT NOT NULL CHECK (routine_type IN (
        'PROCEDURE', 'FUNCTION', 'PACKAGE', 'PACKAGE BODY', 'AGGREGATE', 'WINDOW'
    )),
    data_type TEXT,
    type_udt_schema TEXT,
    type_udt_name TEXT,
    routine_body TEXT,
    routine_definition TEXT,
    external_language TEXT,
    is_deterministic INTEGER,
    security_type TEXT,
    created_at INTEGER,
    last_altered_at INTEGER,
    routine_comment TEXT,
    -- 缓存控制
    introspect_level INTEGER DEFAULT 3,
    is_loaded INTEGER DEFAULT 0,
    last_sync INTEGER,
    last_accessed INTEGER,
    UNIQUE (schema_id, routine_name, routine_type)
);

-- 11. 函数参数
CREATE TABLE IF NOT EXISTS routine_parameters (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    routine_id INTEGER NOT NULL REFERENCES routines(id) ON DELETE CASCADE,
    parameter_name TEXT NOT NULL,
    ordinal_position INTEGER,
    parameter_mode TEXT,
    data_type TEXT,
    character_maximum_length INTEGER,
    numeric_precision INTEGER,
    numeric_scale INTEGER,
    parameter_default TEXT,
    UNIQUE (routine_id, parameter_name, ordinal_position)
);

-- 12. 触发器
CREATE TABLE IF NOT EXISTS triggers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    table_id INTEGER NOT NULL REFERENCES tables(id) ON DELETE CASCADE,
    trigger_name TEXT NOT NULL,
    trigger_timing TEXT,
    trigger_event TEXT,
    trigger_orientation TEXT,
    trigger_body TEXT,
    trigger_comment TEXT,
    -- 缓存控制
    introspect_level INTEGER DEFAULT 3,
    is_loaded INTEGER DEFAULT 0,
    last_sync INTEGER,
    UNIQUE (table_id, trigger_name)
);

-- 13. 序列
CREATE TABLE IF NOT EXISTS sequences (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    schema_id INTEGER NOT NULL REFERENCES schemata(id) ON DELETE CASCADE,
    sequence_name TEXT NOT NULL,
    data_type TEXT,
    start_value INTEGER,
    minimum_value INTEGER,
    maximum_value INTEGER,
    increment_by INTEGER,
    cycle_option INTEGER DEFAULT 0,
    cache_size INTEGER,
    last_value INTEGER,
    -- 缓存控制
    introspect_level INTEGER DEFAULT 3,
    is_loaded INTEGER DEFAULT 0,
    last_sync INTEGER,
    UNIQUE (schema_id, sequence_name)
);

-- ===========================================================================
-- ======================== 第二阶段：创建索引 =================================
-- ===========================================================================

CREATE INDEX IF NOT EXISTS idx_schemata_name ON schemata(schema_name);
CREATE INDEX IF NOT EXISTS idx_schemata_catalog ON schemata(catalog_name);
CREATE INDEX IF NOT EXISTS idx_tables_schema ON tables(schema_id);
CREATE INDEX IF NOT EXISTS idx_tables_name ON tables(table_name);
CREATE INDEX IF NOT EXISTS idx_tables_type ON tables(table_type);
CREATE INDEX IF NOT EXISTS idx_columns_table ON columns(table_id);
CREATE INDEX IF NOT EXISTS idx_columns_name ON columns(column_name);
CREATE INDEX IF NOT EXISTS idx_indexes_table ON indexes(table_id);
CREATE INDEX IF NOT EXISTS idx_indexcols_index ON index_columns(index_id);
CREATE INDEX IF NOT EXISTS idx_fk_table ON foreign_keys(table_id);
CREATE INDEX IF NOT EXISTS idx_fkcols_fkid ON foreign_key_columns(foreign_key_id);
CREATE INDEX IF NOT EXISTS idx_check_table ON check_constraints(table_id);
CREATE INDEX IF NOT EXISTS idx_triggers_table ON triggers(table_id);
CREATE INDEX IF NOT EXISTS idx_routines_schema ON routines(schema_id);
CREATE INDEX IF NOT EXISTS idx_routines_name_type ON routines(routine_name, routine_type);
CREATE INDEX IF NOT EXISTS idx_seq_schema ON sequences(schema_id);
CREATE INDEX IF NOT EXISTS idx_routineparams_routine ON routine_parameters(routine_id);

-- ===========================================================================
-- ======================== 第三阶段：数据迁移 ================================
-- ===========================================================================

-- 迁移 schemata
INSERT OR IGNORE INTO schemata (schema_name, catalog_name, last_sync, introspect_level, is_loaded)
SELECT
    COALESCE(schema_name, 'default') as schema_name,
    COALESCE(database_name, '') as catalog_name,
    MAX(last_sync) as last_sync,
    MAX(COALESCE(introspect_level, 3)) as introspect_level,
    MAX(COALESCE(is_loaded, 1)) as is_loaded
FROM metadata
WHERE obj_type = 'schema'
GROUP BY schema_name, database_name;

-- 获取 schemata ID 映射（用于后续外键关联）
-- 临时表存储映射关系
CREATE TEMP TABLE IF NOT EXISTS _schema_id_map AS
SELECT
    COALESCE(schema_name, 'default') as schema_name,
    COALESCE(database_name, '') as catalog_name,
    id as old_id,
    (SELECT id FROM schemata WHERE schema_name = COALESCE(metadata.schema_name, 'default') AND catalog_name = COALESCE(metadata.database_name, '')) as new_id
FROM metadata
WHERE obj_type = 'schema';

-- 迁移 tables
INSERT OR IGNORE INTO tables (schema_id, table_name, table_type, table_comment, last_sync, introspect_level, is_loaded, extra_attributes)
SELECT
    (SELECT new_id FROM _schema_id_map WHERE schema_name = COALESCE(metadata.schema_name, 'default') AND catalog_name = COALESCE(metadata.database_name, '')) as schema_id,
    COALESCE(name, table_name) as table_name,
    COALESCE(obj_type, 'TABLE') as table_type,
    comment as table_comment,
    last_sync,
    COALESCE(introspect_level, 3) as introspect_level,
    COALESCE(is_loaded, 1) as is_loaded,
    extra as extra_attributes
FROM metadata
WHERE obj_type IN ('table', 'view', 'materialized view', 'foreign table', 'partitioned table', 'system table');

-- 创建 table ID 映射临时表
CREATE TEMP TABLE IF NOT EXISTS _table_id_map AS
SELECT
    COALESCE(name, table_name) as table_name,
    (SELECT s.id FROM schemata s
     INNER JOIN _schema_id_map m ON m.schema_name = s.schema_name AND m.catalog_name = s.catalog_name
     WHERE s.schema_name = COALESCE(metadata.schema_name, 'default') AND s.catalog_name = COALESCE(metadata.database_name, '')) as schema_id,
    id as old_id,
    (SELECT id FROM tables WHERE table_name = COALESCE(metadata.name, metadata.table_name) LIMIT 1) as new_id
FROM metadata
WHERE obj_type IN ('table', 'view', 'materialized view', 'foreign table', 'partitioned table', 'system table');

-- 迁移 columns
INSERT OR IGNORE INTO columns (table_id, column_name, ordinal_position, data_type, is_nullable, column_default, column_comment, last_sync, introspect_level, is_loaded, extra)
SELECT
    (SELECT new_id FROM _table_id_map WHERE old_id = metadata.id LIMIT 1) as table_id,
    COALESCE(name, column_name) as column_name,
    CAST(COALESCE(definition, '0') AS INTEGER) as ordinal_position,
    COALESCE(data_type, 'TEXT') as data_type,
    is_nullable as is_nullable,
    NULL as column_default,
    comment as column_comment,
    last_sync,
    COALESCE(introspect_level, 3) as introspect_level,
    COALESCE(is_loaded, 1) as is_loaded,
    extra
FROM metadata
WHERE obj_type = 'column' AND table_id IS NOT NULL;

-- 迁移 indexes
INSERT OR IGNORE INTO indexes (table_id, index_name, index_type, is_unique, is_primary, index_comment, last_sync, introspect_level, is_loaded, extra_attributes)
SELECT
    (SELECT new_id FROM _table_id_map WHERE old_id = metadata.id LIMIT 1) as table_id,
    COALESCE(name, table_name) as index_name,
    'BTREE' as index_type,
    is_unique as is_unique,
    is_primary as is_primary,
    comment as index_comment,
    last_sync,
    COALESCE(introspect_level, 3) as introspect_level,
    COALESCE(is_loaded, 1) as is_loaded,
    extra as extra_attributes
FROM metadata
WHERE obj_type = 'index' AND table_id IS NOT NULL;

-- 迁移 routines (函数/存储过程)
INSERT OR IGNORE INTO routines (schema_id, routine_name, routine_type, routine_definition, routine_comment, last_sync, introspect_level, is_loaded)
SELECT
    (SELECT new_id FROM _schema_id_map WHERE schema_name = COALESCE(metadata.schema_name, 'default') AND catalog_name = COALESCE(metadata.database_name, '')) as schema_id,
    COALESCE(name, table_name) as routine_name,
    CASE
        WHEN obj_type = 'function' THEN 'FUNCTION'
        WHEN obj_type = 'procedure' THEN 'PROCEDURE'
        ELSE COALESCE(obj_type, 'FUNCTION')
    END as routine_type,
    definition as routine_definition,
    comment as routine_comment,
    last_sync,
    COALESCE(introspect_level, 3) as introspect_level,
    COALESCE(is_loaded, 1) as is_loaded
FROM metadata
WHERE obj_type IN ('function', 'procedure', 'routine');

-- 清理临时表
DROP TABLE IF EXISTS _schema_id_map;
DROP TABLE IF EXISTS _table_id_map;

-- ===========================================================================
-- ======================== 第四阶段：向后兼容视图 =============================
-- ===========================================================================

-- metadata 统一视图（兼容旧代码）
CREATE VIEW IF NOT EXISTS metadata AS
SELECT
    'schema' as obj_type,
    schema_name as database_name,
    schema_name,
    '' as table_name,
    '' as name,
    NULL as data_type,
    NULL as is_nullable,
    0 as is_primary,
    0 as is_unique,
    comment,
    NULL as definition,
    extra,
    last_sync,
    introspect_level,
    is_loaded,
    last_accessed
FROM schemata

UNION ALL

SELECT
    LOWER(table_type) as obj_type,
    COALESCE(s.catalog_name, '') as database_name,
    s.schema_name,
    t.table_name,
    '' as name,
    NULL as data_type,
    NULL as is_nullable,
    0 as is_primary,
    0 as is_unique,
    t.table_comment,
    NULL as definition,
    t.extra_attributes as extra,
    t.last_sync,
    t.introspect_level,
    t.is_loaded,
    t.last_accessed
FROM tables t
INNER JOIN schemata s ON t.schema_id = s.id

UNION ALL

SELECT
    'column' as obj_type,
    COALESCE(s.catalog_name, '') as database_name,
    s.schema_name,
    t.table_name,
    c.column_name,
    c.data_type,
    c.is_nullable,
    0 as is_primary,
    0 as is_unique,
    c.column_comment,
    CAST(c.ordinal_position AS TEXT) as definition,
    c.extra,
    c.last_sync,
    c.introspect_level,
    c.is_loaded,
    c.last_accessed
FROM columns c
INNER JOIN tables t ON c.table_id = t.id
INNER JOIN schemata s ON t.schema_id = s.id

UNION ALL

SELECT
    'index' as obj_type,
    COALESCE(s.catalog_name, '') as database_name,
    s.schema_name,
    t.table_name,
    i.index_name,
    i.index_type as data_type,
    NULL as is_nullable,
    i.is_primary,
    i.is_unique,
    i.index_comment,
    NULL as definition,
    i.extra_attributes,
    i.last_sync,
    i.introspect_level,
    i.is_loaded,
    i.last_accessed
FROM indexes i
INNER JOIN tables t ON i.table_id = t.id
INNER JOIN schemata s ON t.schema_id = s.id;

-- v_table_details 视图
CREATE VIEW IF NOT EXISTS v_table_details AS
SELECT
    s.schema_name,
    t.table_name,
    t.table_type,
    t.table_comment,
    t.row_count_estimate,
    t.engine,
    t.last_sync,
    COUNT(DISTINCT c.id) as column_count,
    COUNT(DISTINCT idx.id) as index_count,
    COUNT(DISTINCT fk.id) as fk_count,
    MAX(COALESCE(c.is_loaded, 1)) as is_loaded,
    MAX(COALESCE(c.introspect_level, 3)) as introspect_level
FROM tables t
INNER JOIN schemata s ON t.schema_id = s.id
LEFT JOIN columns c ON c.table_id = t.id
LEFT JOIN indexes idx ON idx.table_id = t.id
LEFT JOIN foreign_keys fk ON fk.table_id = t.id
GROUP BY s.schema_name, t.table_name, t.table_type, t.table_comment, t.row_count_estimate, t.engine, t.last_sync;

-- ===========================================================================
-- ======================== 第五阶段：更新版本 ================================
-- ===========================================================================

-- 更新缓存版本号
UPDATE cache_version SET version = 4, upgraded_at = strftime('%s', 'now'), updated_at = strftime('%s', 'now') WHERE id = 1;

-- 记录迁移历史
INSERT INTO cache_migration_history (from_version, to_version, migrated_at, reason, duration_ms, success)
VALUES (3, 4, strftime('%s', 'now'), '规范化重构：从统一表拆分为 schemata/tables/columns/indexes 等独立表，添加外键约束', 0, 1);

-- ===========================================================================
-- ======================== 第六阶段：重建 FTS 索引 ============================
-- ===========================================================================

-- 重建 FTS 索引以包含新数据
INSERT INTO tables_fts(tables_fts, schema_name, table_name, table_comment)
SELECT schema_name, schema_name, table_name, table_comment FROM tables;
