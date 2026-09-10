-- 迁移版本：009
-- 数据库类型：SQLite
-- 作用：追平 JDBC DatabaseMetaData / ResultSetMetaData 规范，补全元数据表字段缺口
-- 背景：
--   1. 现有表结构（004 规范化）覆盖了基础元数据，但与 JDBC 规范存在缺口
--   2. JDBC DatabaseMetaData 提供了完整的数据库元数据描述能力
--   3. 本迁移补全 columns/tables/indexes/foreign_keys/routines/views/triggers/sequences/check_constraints 表
--   4. 新建 privileges 表对应 JDBC getColumnPrivileges/getTablePrivileges
-- 对应 JDBC API：
--   - ResultSetMetaData → columns 补全
--   - DatabaseMetaData.getTables → tables 补全
--   - DatabaseMetaData.getIndexInfo → indexes 补全
--   - DatabaseMetaData.getImportedKeys/CrossReference → foreign_keys 补全
--   - DatabaseMetaData.getProcedures/getProcedureColumns → routines 补全
--   - DatabaseMetaData.getTables(TABLE_TYPE='VIEW') → view_definitions 补全
--   - DatabaseMetaData.getTriggers → triggers 补全
--   - DatabaseMetaData.getSequences → sequences 补全
--   - DatabaseMetaData.getTableConstraints(CHECK) → check_constraints 补全
--   - DatabaseMetaData.getColumnPrivileges/getTablePrivileges → privileges 新建
-- 更新时间：2026-05-25

-- ===========================================================================
-- ======================== A. columns 表补全 =================================
-- ===========================================================================
-- 对应 JDBC: ResultSetMetaData.isAutoIncrement / isWritable / isReadOnly /
--            isSearchable / isSigned / isCurrency / getSchemaName /
--            getTableName / getPrecision (source_data_type)

ALTER TABLE columns ADD COLUMN IF NOT EXISTS is_updatable INTEGER NOT NULL DEFAULT 1;
ALTER TABLE columns ADD COLUMN IF NOT EXISTS is_readonly INTEGER NOT NULL DEFAULT 0;
ALTER TABLE columns ADD COLUMN IF NOT EXISTS auto_increment_next_value TEXT;
ALTER TABLE columns ADD COLUMN IF NOT EXISTS is_searchable INTEGER NOT NULL DEFAULT 1;
ALTER TABLE columns ADD COLUMN IF NOT EXISTS is_signed INTEGER NOT NULL DEFAULT 1;
ALTER TABLE columns ADD COLUMN IF NOT EXISTS is_currency INTEGER NOT NULL DEFAULT 0;
ALTER TABLE columns ADD COLUMN IF NOT EXISTS scope_catalog TEXT;
ALTER TABLE columns ADD COLUMN IF NOT EXISTS scope_schema TEXT;
ALTER TABLE columns ADD COLUMN IF NOT EXISTS scope_table TEXT;
ALTER TABLE columns ADD COLUMN IF NOT EXISTS source_data_type SMALLINT;

-- ===========================================================================
-- ======================== B. tables 表补全 =================================
-- ===========================================================================
-- 对应 JDBC: DatabaseMetaData.getTables (TABLESPACE / PAGES 等扩展信息)
--            MySQL: SHOW TABLE STATUS (Data_free / Max_data_length 等)

ALTER TABLE tables ADD COLUMN IF NOT EXISTS tablespace_name TEXT;
ALTER TABLE tables ADD COLUMN IF NOT EXISTS pages_count INTEGER;
ALTER TABLE tables ADD COLUMN IF NOT EXISTS avg_row_length REAL;
ALTER TABLE tables ADD COLUMN IF NOT EXISTS max_data_length BIGINT;
ALTER TABLE tables ADD COLUMN IF NOT EXISTS index_free_space INTEGER;
ALTER TABLE tables ADD COLUMN IF NOT EXISTS data_free_space INTEGER;

-- 分区信息（JDBC 无直接对应，MySQL SHOW TABLE STATUS / PG partitioned table）
ALTER TABLE tables ADD COLUMN IF NOT EXISTS is_partitioned INTEGER NOT NULL DEFAULT 0;
ALTER TABLE tables ADD COLUMN IF NOT EXISTS partition_expression TEXT;
ALTER TABLE tables ADD COLUMN IF NOT EXISTS partition_description TEXT;

-- 统计信息更新时间（JDBC 无直接对应，用于增量同步判断）
ALTER TABLE tables ADD COLUMN IF NOT EXISTS stats_last_updated INTEGER;

-- ===========================================================================
-- ======================== C. indexes 表补全 ================================
-- ===========================================================================
-- 对应 JDBC: DatabaseMetaData.getIndexInfo (FILTER_CONDITION / CARDINALITY /
--            PAGES / PostgreSQL ambuildempty FILLFACTOR)

ALTER TABLE indexes ADD COLUMN IF NOT EXISTS filter_condition TEXT;
ALTER TABLE indexes ADD COLUMN IF NOT EXISTS include_column_names TEXT;
ALTER TABLE indexes ADD COLUMN IF NOT EXISTS cardinality BIGINT;
ALTER TABLE indexes ADD COLUMN IF NOT EXISTS pages INTEGER;
ALTER TABLE indexes ADD COLUMN IF NOT EXISTS fill_factor INTEGER;

-- ===========================================================================
-- ======================== D. foreign_keys 表补全 ============================
-- ===========================================================================
-- 对应 JDBC: DatabaseMetaData.getImportedKeys / getExportedKeys / getCrossReference
--            (MATCH_OPTION / DEFERRABILITY 已部分存在，补充缺失字段)

ALTER TABLE foreign_keys ADD COLUMN IF NOT EXISTS match_option TEXT DEFAULT 'SIMPLE';
ALTER TABLE foreign_keys ADD COLUMN IF NOT EXISTS is_valid INTEGER NOT NULL DEFAULT 1;

-- ===========================================================================
-- ======================== E. routines 表补全 ===============================
-- ===========================================================================
-- 对应 JDBC: DatabaseMetaData.getProcedures / getProcedureColumns
--            (SPECIFIC_NAME / SQL_DATA_ACCESS / IS_NULL_CALL / DETERMINISTIC)

ALTER TABLE routines ADD COLUMN IF NOT EXISTS specific_name TEXT;
ALTER TABLE routines ADD COLUMN IF NOT EXISTS deterministic INTEGER NOT NULL DEFAULT 0;
ALTER TABLE routines ADD COLUMN IF NOT EXISTS sql_data_access TEXT;
ALTER TABLE routines ADD COLUMN IF NOT EXISTS is_null_call INTEGER NOT NULL DEFAULT 0;

-- ===========================================================================
-- ======================== F. view_definitions 表补全 =======================
-- ===========================================================================
-- 对应 JDBC: DatabaseMetaData.getTables (CHECK_OPTION / IS_UPDATABLE)
--            JDBC 扩展: INSERTABLE_INTO / TRIGGER_UPDATABLE 等

ALTER TABLE view_definitions ADD COLUMN IF NOT EXISTS is_insertable_into INTEGER NOT NULL DEFAULT 0;
ALTER TABLE view_definitions ADD COLUMN IF NOT EXISTS is_trigger_updatable INTEGER NOT NULL DEFAULT 0;
ALTER TABLE view_definitions ADD COLUMN IF NOT EXISTS is_trigger_deletable INTEGER NOT NULL DEFAULT 0;
ALTER TABLE view_definitions ADD COLUMN IF NOT EXISTS is_trigger_insertable_into INTEGER NOT NULL DEFAULT 0;

-- ===========================================================================
-- ======================== G. triggers 表补全 ===============================
-- ===========================================================================
-- 对应 JDBC: DatabaseMetaData.getSuperTables (间接) / SQL:2003 INFORMATION_SCHEMA.TRIGGERS
--            字段重命名对齐 JDBC INFORMATION_SCHEMA 命名风格

ALTER TABLE triggers ADD COLUMN IF NOT EXISTS action_timing TEXT;
ALTER TABLE triggers ADD COLUMN IF NOT EXISTS event_manipulation TEXT;
ALTER TABLE triggers ADD COLUMN IF NOT EXISTS action_statement TEXT;
ALTER TABLE triggers ADD COLUMN IF NOT EXISTS action_orientation TEXT;
ALTER TABLE triggers ADD COLUMN IF NOT EXISTS condition_timing TEXT;
ALTER TABLE triggers ADD COLUMN IF NOT EXISTS created_at INTEGER;

-- 从已有字段同步到新命名字段（向后兼容）
UPDATE triggers SET action_timing = trigger_timing WHERE action_timing IS NULL AND trigger_timing IS NOT NULL;
UPDATE triggers SET event_manipulation = trigger_event WHERE event_manipulation IS NULL AND trigger_event IS NOT NULL;
UPDATE triggers SET action_statement = trigger_body WHERE action_statement IS NULL AND trigger_body IS NOT NULL;
UPDATE triggers SET action_orientation = trigger_orientation WHERE action_orientation IS NULL AND trigger_orientation IS NOT NULL;

-- ===========================================================================
-- ======================== H. sequences 表补全 ==============================
-- ===========================================================================
-- 对应 JDBC: DatabaseMetaData.getSequences (Java 8+ / JDBC 4.3)
--            (MINIMUM_VALUE / MAXIMUM_VALUE / INCREMENT / CYCLE / CURRENT_VALUE)

ALTER TABLE sequences ADD COLUMN IF NOT EXISTS current_value BIGINT;
ALTER TABLE sequences ADD COLUMN IF NOT EXISTS is_ordered INTEGER NOT NULL DEFAULT 0;

-- ===========================================================================
-- ======================== I. check_constraints 表补全 ======================
-- ===========================================================================
-- 对应 JDBC: DatabaseMetaData.getTableConstraints (CONSTRAINT_TYPE = 'CHECK')
--            (IS_DEFERRABLE / INITIALLY_DEFERRED / VALIDATED)

ALTER TABLE check_constraints ADD COLUMN IF NOT EXISTS is_deferrable INTEGER NOT NULL DEFAULT 0;
ALTER TABLE check_constraints ADD COLUMN IF NOT EXISTS initially_deferred INTEGER NOT NULL DEFAULT 0;
ALTER TABLE check_constraints ADD COLUMN IF NOT EXISTS validated INTEGER NOT NULL DEFAULT 1;

-- ===========================================================================
-- ======================== J. privileges 表（新建）==========================
-- ===========================================================================
-- 对应 JDBC: DatabaseMetaData.getColumnPrivileges / getTablePrivileges
--            (GRANTEE / GRANTOR / PRIVILEGE_TYPE / IS_GRANTABLE)

CREATE TABLE IF NOT EXISTS privileges (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    grantee TEXT NOT NULL,
    grantor TEXT,
    privilege_type TEXT NOT NULL,
    is_grantable INTEGER NOT NULL DEFAULT 0,
    object_type TEXT NOT NULL,
    catalog_name TEXT,
    schema_name TEXT,
    object_name TEXT NOT NULL,
    column_name TEXT,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

CREATE INDEX IF NOT EXISTS idx_privileges_object ON privileges(object_type, schema_name, object_name);
CREATE INDEX IF NOT EXISTS idx_privileges_grantee ON privileges(grantee);

-- ===========================================================================
-- ======================== K. 版本号更新 ====================================
-- ===========================================================================

PRAGMA user_version = 9;

INSERT OR REPLACE INTO cache_version (version, description, applied_at)
VALUES (
    9,
    'JDBC DatabaseMetaData alignment: extended columns/tables/indexes/fk/routines/views/triggers/sequences/checks/privileges',
    strftime('%s', 'now')
);

-- 记录迁移历史
INSERT INTO cache_migration_history (version, migrated_at, duration_ms, success, error_message)
SELECT
    9,
    strftime('%s', 'now'),
    0,
    1,
    'V9: JDBC DatabaseMetaData 对齐: columns(+10), tables(+10), indexes(+5), foreign_keys(+2), routines(+4), views(+4), triggers(+6,含兼容同步), sequences(+2), checks(+4), privileges(新建)'
WHERE NOT EXISTS (
    SELECT 1 FROM cache_migration_history WHERE version = 9
);
