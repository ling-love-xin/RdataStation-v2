-- ============================================================
-- 连接元数据数据库 (一个连接对应一个 SQLite 文件)
-- ============================================================

-- ============================================================
-- 1. 数据库全局信息
-- ============================================================
CREATE TABLE database_info (
    key TEXT PRIMARY KEY,
    value TEXT
);
-- 常用键值: product_name, product_version, default_charset, max_identifier_length, etc.
-- 示例: ('product_name', 'PostgreSQL'), ('product_version', '15.4')

-- ============================================================
-- 2. 模式 (Schema) 列表
-- MySQL: schema = database 名称
-- PostgreSQL/SQL Server/Oracle: schema 为模式
-- ============================================================
CREATE TABLE schemata (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    schema_name TEXT NOT NULL,               -- 模式或数据库名称
    catalog_name TEXT,                       -- 目录名 (对 MySQL 通常为 NULL)
    owner TEXT,                              -- 所有者
    default_character_set_name TEXT,         -- 默认字符集
    default_collation_name TEXT,             -- 默认排序规则
    sql_path TEXT,                           -- 路径 (PostgreSQL)
    comment TEXT,
    UNIQUE (catalog_name, schema_name)
);
CREATE INDEX idx_schemata_name ON schemata(schema_name);

-- ============================================================
-- 3. 表 & 视图
-- ============================================================
CREATE TABLE tables (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    schema_id INTEGER NOT NULL REFERENCES schemata(id) ON DELETE CASCADE,
    table_name TEXT NOT NULL,
    table_type TEXT NOT NULL CHECK (table_type IN ('TABLE', 'VIEW', 'MATERIALIZED VIEW', 'FOREIGN TABLE', 'PARTITIONED TABLE', 'SYSTEM TABLE', 'GLOBAL TEMPORARY', 'LOCAL TEMPORARY')),
    -- 通用属性
    row_count_estimate INTEGER,              -- 预估行数 (从系统表获取)
    data_length INTEGER,                     -- 数据大小 (字节)
    index_length INTEGER,                    -- 索引大小 (字节)
    engine TEXT,                             -- 存储引擎 (MySQL: InnoDB, MyISAM)
    row_format TEXT,                         -- 行格式 (MySQL: Dynamic, Compact)
    auto_increment_val INTEGER,              -- 自增值 (MySQL)
    table_comment TEXT,
    -- 时间信息
    created_at TIMESTAMP,
    last_altered_at TIMESTAMP,
    last_accessed_at TIMESTAMP,
    -- 额外属性 (JSON)
    extra_attributes TEXT,                   -- 如 Oracle 的 tablespace 等特殊信息
    UNIQUE (schema_id, table_name)
);
CREATE INDEX idx_tables_schema ON tables(schema_id);
CREATE INDEX idx_tables_name ON tables(table_name);
CREATE INDEX idx_tables_type ON tables(table_type);

-- ============================================================
-- 4. 列
-- ============================================================
CREATE TABLE columns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    table_id INTEGER NOT NULL REFERENCES tables(id) ON DELETE CASCADE,
    column_name TEXT NOT NULL,
    ordinal_position INTEGER,                -- 列序号 (从1开始)
    column_default TEXT,                     -- 默认值表达式
    is_nullable BOOLEAN,                     -- 是否可为空
    data_type TEXT NOT NULL,                 -- 原始数据类型名称 (如 VARCHAR(255), INT4)
    -- 类型细分
    full_data_type TEXT,                     -- 完整原始类型字符串 (如 'character varying(255)')
    type_category TEXT,                      -- 分类: string, numeric, datetime, binary, spatial, json, array, etc.
    character_maximum_length INTEGER,
    character_octet_length INTEGER,
    numeric_precision INTEGER,
    numeric_scale INTEGER,
    datetime_precision INTEGER,
    interval_type TEXT,
    interval_precision INTEGER,
    character_set_name TEXT,
    collation_name TEXT,
    -- 域/用户定义类型
    domain_schema TEXT,
    domain_name TEXT,
    -- 标识
    is_identity BOOLEAN,
    identity_generation TEXT,                -- ALWAYS / BY DEFAULT
    identity_start INTEGER,
    identity_increment INTEGER,
    is_generated BOOLEAN,                    -- 是否为生成列
    generation_expression TEXT,              -- 生成表达式
    -- 注释
    column_comment TEXT,
    -- 额外属性 (JSON)
    extra_attributes TEXT,                   -- 例如 MySQL 的 enum_values, Oracle 的 hidden_column, 数组维数等
    UNIQUE (table_id, column_name)
);
CREATE INDEX idx_columns_table ON columns(table_id);
CREATE INDEX idx_columns_name ON columns(column_name);

-- ============================================================
-- 5. 索引
-- ============================================================
CREATE TABLE indexes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    table_id INTEGER NOT NULL REFERENCES tables(id) ON DELETE CASCADE,
    index_name TEXT NOT NULL,
    index_type TEXT,                         -- BTREE, FULLTEXT, HASH, GIST, SPGIST, GIN, BRIN, etc.
    is_unique BOOLEAN DEFAULT 0,
    is_primary BOOLEAN DEFAULT 0,            -- 是否主键索引
    is_clustered BOOLEAN DEFAULT 0,
    index_comment TEXT,
    -- 部分索引/函数索引等特殊属性 (JSON)
    extra_attributes TEXT,
    UNIQUE (table_id, index_name)
);
CREATE INDEX idx_indexes_table ON indexes(table_id);

-- 索引列
CREATE TABLE index_columns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    index_id INTEGER NOT NULL REFERENCES indexes(id) ON DELETE CASCADE,
    column_name TEXT NOT NULL,
    ordinal_position INTEGER,                -- 在索引中的位置
    sort_order TEXT,                         -- ASC / DESC
    is_included_column BOOLEAN DEFAULT 0,   -- 是否为包含列 (覆盖索引)
    -- 对于函数索引，需要存储表达式
    expression TEXT,                         -- 如果是函数索引，这里存储表达式而不是列名
    UNIQUE (index_id, column_name, ordinal_position)
);
CREATE INDEX idx_idxcols_index ON index_columns(index_id);

-- ============================================================
-- 6. 外键约束
-- ============================================================
CREATE TABLE foreign_keys (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    table_id INTEGER NOT NULL REFERENCES tables(id) ON DELETE CASCADE,
    constraint_name TEXT NOT NULL,
    ref_schema_id INTEGER REFERENCES schemata(id),
    ref_table_id INTEGER REFERENCES tables(id),
    delete_rule TEXT,                        -- CASCADE, SET NULL, RESTRICT, NO ACTION, SET DEFAULT
    update_rule TEXT,
    deferrability TEXT,                      -- INITIALLY_DEFERRED, INITIALLY_IMMEDIATE, NOT_DEFERRABLE
    UNIQUE (table_id, constraint_name)
);
CREATE INDEX idx_fk_table ON foreign_keys(table_id);

-- 外键列映射
CREATE TABLE foreign_key_columns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    foreign_key_id INTEGER NOT NULL REFERENCES foreign_keys(id) ON DELETE CASCADE,
    ordinal_position INTEGER,
    column_name TEXT NOT NULL,
    ref_column_name TEXT NOT NULL,
    UNIQUE (foreign_key_id, column_name)
);

-- ============================================================
-- 7. 检查约束
-- ============================================================
CREATE TABLE check_constraints (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    table_id INTEGER NOT NULL REFERENCES tables(id) ON DELETE CASCADE,
    constraint_name TEXT NOT NULL,
    check_clause TEXT NOT NULL,              -- 检查表达式
    UNIQUE (table_id, constraint_name)
);

-- ============================================================
-- 8. 视图定义
-- ============================================================
CREATE TABLE view_definitions (
    table_id INTEGER NOT NULL PRIMARY KEY REFERENCES tables(id) ON DELETE CASCADE,
    view_definition TEXT NOT NULL,           -- 视图的查询 SQL
    is_updatable BOOLEAN,
    check_option TEXT                        -- CASCADED / LOCAL
);

-- ============================================================
-- 9. 存储过程 / 函数
-- ============================================================
CREATE TABLE routines (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    schema_id INTEGER NOT NULL REFERENCES schemata(id) ON DELETE CASCADE,
    routine_name TEXT NOT NULL,
    routine_type TEXT NOT NULL CHECK (routine_type IN ('PROCEDURE', 'FUNCTION', 'PACKAGE', 'PACKAGE BODY', 'AGGREGATE', 'WINDOW')),
    data_type TEXT,                          -- 返回类型 (函数)
    type_udt_schema TEXT,
    type_udt_name TEXT,
    routine_body TEXT,                       -- 定义语言 (SQL, PLpgSQL, etc.)
    routine_definition TEXT,                 -- 源码
    external_language TEXT,                  -- 如 Java, Python
    is_deterministic BOOLEAN,
    security_type TEXT,                      -- INVOKER / DEFINER
    created_at TIMESTAMP,
    last_altered_at TIMESTAMP,
    routine_comment TEXT,
    UNIQUE (schema_id, routine_name, routine_type)
);
CREATE INDEX idx_routines_schema ON routines(schema_id);

-- 存储过程 / 函数参数
CREATE TABLE routine_parameters (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    routine_id INTEGER NOT NULL REFERENCES routines(id) ON DELETE CASCADE,
    parameter_name TEXT NOT NULL,
    ordinal_position INTEGER,
    parameter_mode TEXT,                     -- IN, OUT, INOUT, VARIADIC
    data_type TEXT,
    character_maximum_length INTEGER,
    numeric_precision INTEGER,
    numeric_scale INTEGER,
    parameter_default TEXT,
    UNIQUE (routine_id, parameter_name, ordinal_position)
);

-- ============================================================
-- 10. 触发器
-- ============================================================
CREATE TABLE triggers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    table_id INTEGER NOT NULL REFERENCES tables(id) ON DELETE CASCADE,
    trigger_name TEXT NOT NULL,
    trigger_timing TEXT,                     -- BEFORE, AFTER, INSTEAD OF
    trigger_event TEXT,                      -- INSERT, UPDATE, DELETE, or combination
    trigger_orientation TEXT,                -- ROW, STATEMENT
    trigger_body TEXT,                       -- 触发器定义
    trigger_comment TEXT,
    UNIQUE (table_id, trigger_name)
);

-- ============================================================
-- 11. 序列 (自增对象)
-- ============================================================
CREATE TABLE sequences (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    schema_id INTEGER NOT NULL REFERENCES schemata(id) ON DELETE CASCADE,
    sequence_name TEXT NOT NULL,
    data_type TEXT,
    start_value INTEGER,
    minimum_value INTEGER,
    maximum_value INTEGER,
    increment_by INTEGER,
    cycle_option BOOLEAN DEFAULT 0,
    cache_size INTEGER,
    last_value INTEGER,
    UNIQUE (schema_id, sequence_name)
);

-- ============================================================
-- 12. 分区信息 (可选，支持 PostgreSQL/Oracle 等)
-- ============================================================
CREATE TABLE partitioned_tables (
    table_id INTEGER NOT NULL PRIMARY KEY REFERENCES tables(id) ON DELETE CASCADE,
    partition_type TEXT,                     -- RANGE, LIST, HASH
    partition_expression TEXT,               -- 分区键或表达式
    partition_count INTEGER
);

-- 各个分区的子表 (可在 tables 表中通过 table_type 标识，但可以单独存储分区映射)
CREATE TABLE table_partitions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    parent_table_id INTEGER NOT NULL REFERENCES tables(id) ON DELETE CASCADE,
    partition_name TEXT NOT NULL,
    partition_description TEXT,              -- 边界描述
    partition_ordinal INTEGER
);

-- ============================================================
-- 13. 用户定义类型
-- ============================================================
CREATE TABLE user_defined_types (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    schema_id INTEGER NOT NULL REFERENCES schemata(id) ON DELETE CASCADE,
    type_name TEXT NOT NULL,
    type_category TEXT,                      -- DISTINCT, STRUCTURED, REF, ARRAY, RANGE, ENUM, etc.
    base_type TEXT,                          -- 基础类型
    type_definition TEXT,                    -- 定义详情 (可能为 SQL 或 JSON)
    UNIQUE (schema_id, type_name)
);

-- ============================================================
-- 14. 同义词 (Oracle, SQL Server)
-- ============================================================
CREATE TABLE synonyms (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    schema_id INTEGER NOT NULL REFERENCES schemata(id) ON DELETE CASCADE,
    synonym_name TEXT NOT NULL,
    object_owner TEXT,
    object_name TEXT NOT NULL,
    object_type TEXT,                        -- TABLE, VIEW, SEQUENCE, etc.
    is_public BOOLEAN DEFAULT 0,
    UNIQUE (schema_id, synonym_name)
);

-- ============================================================
-- 15. 权限 (可选，缓存最常用)
-- ============================================================
CREATE TABLE permissions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    grantee TEXT NOT NULL,
    object_type TEXT,                        -- TABLE, SCHEMA, ROUTINE, etc.
    object_schema TEXT,
    object_name TEXT,
    privilege_type TEXT,                     -- SELECT, INSERT, UPDATE, DELETE, EXECUTE, etc.
    is_grantable BOOLEAN DEFAULT 0
);

-- ============================================================
-- 16. 对象依赖关系 (可选)
-- ============================================================
CREATE TABLE object_dependencies (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    object_schema TEXT,
    object_name TEXT,
    object_type TEXT,
    depends_on_schema TEXT,
    depends_on_name TEXT,
    depends_on_type TEXT,
    dependency_type TEXT                     -- 'hard', 'soft'
);

-- ============================================================
-- 17. 表的扩展统计信息 (可选，来自数据库统计视图)
-- ============================================================
CREATE TABLE table_statistics (
    table_id INTEGER NOT NULL PRIMARY KEY REFERENCES tables(id) ON DELETE CASCADE,
    seq_scan_count INTEGER,
    seq_tup_read INTEGER,
    idx_scan_count INTEGER,
    idx_tup_fetch INTEGER,
    n_tup_ins INTEGER,
    n_tup_upd INTEGER,
    n_tup_del INTEGER,
    n_live_tup INTEGER,
    n_dead_tup INTEGER,
    last_vacuum TIMESTAMP,
    last_analyze TIMESTAMP,
    stats_updated_at TIMESTAMP
);

-- 列统计信息
CREATE TABLE column_statistics (
    column_id INTEGER NOT NULL PRIMARY KEY REFERENCES columns(id) ON DELETE CASCADE,
    n_distinct INTEGER,
    null_fraction REAL,
    most_common_vals TEXT,                  -- JSON array
    most_common_freqs TEXT,                 -- JSON array
    histogram_bounds TEXT,                  -- JSON array
    correlation REAL,
    stats_updated_at TIMESTAMP
);

-- ============================================================
-- 18. 同步日志
-- ============================================================
CREATE TABLE sync_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    synced_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    duration_ms INTEGER,
    status TEXT CHECK (status IN ('SUCCESS', 'PARTIAL', 'FAILED')),
    message TEXT,
    -- 记录同步的范围
    schema_filter TEXT,
    tables_synced INTEGER,
    columns_synced INTEGER,
    indexes_synced INTEGER
);

-- ============================================================
-- 为加快查询添加额外索引
-- ============================================================
CREATE INDEX idx_tables_schema_id ON tables(schema_id);
CREATE INDEX idx_columns_table_id ON columns(table_id);
CREATE INDEX idx_indexes_table_id ON indexes(table_id);
CREATE INDEX idx_indexcols_index_id ON index_columns(index_id);
CREATE INDEX idx_fk_table_id ON foreign_keys(table_id);
CREATE INDEX idx_fkcols_fkid ON foreign_key_columns(foreign_key_id);
CREATE INDEX idx_check_table_id ON check_constraints(table_id);
CREATE INDEX idx_triggers_table_id ON triggers(table_id);
CREATE INDEX idx_routines_schema_id ON routines(schema_id);
CREATE INDEX idx_seq_schema_id ON sequences(schema_id);
CREATE INDEX idx_part_parent ON table_partitions(parent_table_id);
CREATE INDEX idx_synonyms_schema ON synonyms(schema_id);