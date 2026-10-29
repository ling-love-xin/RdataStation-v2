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

ALTER TABLE columns -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN is_updatable INTEGER NOT NULL DEFAULT 1;
ALTER TABLE columns -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN is_readonly INTEGER NOT NULL DEFAULT 0;
ALTER TABLE columns -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN auto_increment_next_value TEXT;
ALTER TABLE columns -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN is_searchable INTEGER NOT NULL DEFAULT 1;
ALTER TABLE columns -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN is_signed INTEGER NOT NULL DEFAULT 1;
ALTER TABLE columns -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN is_currency INTEGER NOT NULL DEFAULT 0;
ALTER TABLE columns -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN scope_catalog TEXT;
ALTER TABLE columns -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN scope_schema TEXT;
ALTER TABLE columns -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN scope_table TEXT;
ALTER TABLE columns -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN source_data_type SMALLINT;

-- ===========================================================================
-- ======================== B. tables 表补全 =================================
-- ===========================================================================
-- 对应 JDBC: DatabaseMetaData.getTables (TABLESPACE / PAGES 等扩展信息)
--            MySQL: SHOW TABLE STATUS (Data_free / Max_data_length 等)

ALTER TABLE tables -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN tablespace_name TEXT;
ALTER TABLE tables -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN pages_count INTEGER;
ALTER TABLE tables -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN avg_row_length REAL;
ALTER TABLE tables -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN max_data_length BIGINT;
ALTER TABLE tables -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN index_free_space INTEGER;
ALTER TABLE tables -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN data_free_space INTEGER;

-- 分区信息（JDBC 无直接对应，MySQL SHOW TABLE STATUS / PG partitioned table）
ALTER TABLE tables -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN is_partitioned INTEGER NOT NULL DEFAULT 0;
ALTER TABLE tables -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN partition_expression TEXT;
ALTER TABLE tables -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN partition_description TEXT;

-- 统计信息更新时间（JDBC 无直接对应，用于增量同步判断）
ALTER TABLE tables -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN stats_last_updated INTEGER;

-- ===========================================================================
-- ======================== C. indexes 表补全 ================================
-- ===========================================================================
-- 对应 JDBC: DatabaseMetaData.getIndexInfo (FILTER_CONDITION / CARDINALITY /
--            PAGES / PostgreSQL ambuildempty FILLFACTOR)

ALTER TABLE indexes -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN filter_condition TEXT;
ALTER TABLE indexes -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN include_column_names TEXT;
ALTER TABLE indexes -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN cardinality BIGINT;
ALTER TABLE indexes -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN pages INTEGER;
ALTER TABLE indexes -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN fill_factor INTEGER;

-- ===========================================================================
-- ======================== D. foreign_keys 表补全 ============================
-- ===========================================================================
-- 对应 JDBC: DatabaseMetaData.getImportedKeys / getExportedKeys / getCrossReference
--            (MATCH_OPTION / DEFERRABILITY 已部分存在，补充缺失字段)

ALTER TABLE foreign_keys -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN match_option TEXT DEFAULT 'SIMPLE';
ALTER TABLE foreign_keys -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN is_valid INTEGER NOT NULL DEFAULT 1;

-- ===========================================================================
-- ======================== E. routines 表补全 ===============================
-- ===========================================================================
-- 对应 JDBC: DatabaseMetaData.getProcedures / getProcedureColumns
--            (SPECIFIC_NAME / SQL_DATA_ACCESS / IS_NULL_CALL / DETERMINISTIC)

ALTER TABLE routines -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN specific_name TEXT;
ALTER TABLE routines -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN deterministic INTEGER NOT NULL DEFAULT 0;
ALTER TABLE routines -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN sql_data_access TEXT;
ALTER TABLE routines -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN is_null_call INTEGER NOT NULL DEFAULT 0;

-- ===========================================================================
-- ======================== F. view_definitions 表补全 =======================
-- ===========================================================================
-- 对应 JDBC: DatabaseMetaData.getTables (CHECK_OPTION / IS_UPDATABLE)
--            JDBC 扩展: INSERTABLE_INTO / TRIGGER_UPDATABLE 等

ALTER TABLE view_definitions -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN is_insertable_into INTEGER NOT NULL DEFAULT 0;
ALTER TABLE view_definitions -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN is_trigger_updatable INTEGER NOT NULL DEFAULT 0;
ALTER TABLE view_definitions -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN is_trigger_deletable INTEGER NOT NULL DEFAULT 0;
ALTER TABLE view_definitions -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN is_trigger_insertable_into INTEGER NOT NULL DEFAULT 0;

-- ===========================================================================
-- ======================== G. triggers 表补全 ===============================
-- ===========================================================================
-- 对应 JDBC: DatabaseMetaData.getSuperTables (间接) / SQL:2003 INFORMATION_SCHEMA.TRIGGERS
--            字段重命名对齐 JDBC INFORMATION_SCHEMA 命名风格

ALTER TABLE triggers -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN action_timing TEXT;
ALTER TABLE triggers -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN event_manipulation TEXT;
ALTER TABLE triggers -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN action_statement TEXT;
ALTER TABLE triggers -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN action_orientation TEXT;
ALTER TABLE triggers -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN condition_timing TEXT;
ALTER TABLE triggers -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN created_at INTEGER;

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

ALTER TABLE sequences -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN current_value BIGINT;
ALTER TABLE sequences -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN is_ordered INTEGER NOT NULL DEFAULT 0;

-- ===========================================================================
-- ======================== I. check_constraints 表补全 ======================
-- ===========================================================================
-- 对应 JDBC: DatabaseMetaData.getTableConstraints (CONSTRAINT_TYPE = 'CHECK')
--            (IS_DEFERRABLE / INITIALLY_DEFERRED / VALIDATED)

ALTER TABLE check_constraints -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN is_deferrable INTEGER NOT NULL DEFAULT 0;
ALTER TABLE check_constraints -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN initially_deferred INTEGER NOT NULL DEFAULT 0;
ALTER TABLE check_constraints -- v1 缺陷修复：SQLite 不支持 ADD COLUMN IF NOT EXISTS
    ADD COLUMN validated INTEGER NOT NULL DEFAULT 1;

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

-- v1 缺陷修复：cache_version 列名为 version/upgraded_at/upgrade_reason/created_at/updated_at，
-- 无 description/applied_at；改为 UPDATE 版本号
UPDATE cache_version
SET version = 9,
    upgraded_at = strftime('%s', 'now'),
    updated_at = strftime('%s', 'now'),
    upgrade_reason = 'JDBC DatabaseMetaData alignment: extended columns/tables/indexes/fk/routines/views/triggers/sequences/checks/privileges'
WHERE id = 1;

-- 记录迁移历史
-- v1 缺陷修复：cache_migration_history 列名为 from_version/to_version/reason
INSERT INTO cache_migration_history (from_version, to_version, migrated_at, reason, duration_ms, success)
SELECT 
    8,
    9,
    strftime('%s', 'now'),
    '%s',
    0,
    1
WHERE NOT EXISTS (
    SELECT 1 FROM cache_migration_history WHERE to_version = 9
);
