-- 迁移版本：009
-- 数据库类型：SQLite
-- 作用：ID 前缀规范化 — 全局表主键统一使用 G_ 前缀，区分项目级 P_ 前缀
-- 更新时间：2026-05-19
--
-- 设计原则：
--   G_xxx  = 全局（Application）表主键，存储于 global.db/system/global.db
--   P_xxx  = 项目（Project）表主键，存储于 project_meta/project.db
--   GP_xxx = 从全局快照到项目的数据（P_前缀变体），存储于 project_meta/project.db
--
-- 本迁移不做已有数据的 ID 重命名（避免破坏外键引用），仅：
--   1. 记录设计规范
--   2. 为新版 Seed 数据添加 G_ 前缀（仅对尚不存在的行）
--   3. 确保 Rust 代码中新增记录的 ID 生成逻辑使用 G_ 前缀

-- ===========================================================================
-- ======================== 环境表 Seed 数据（带 G_ 前缀）====================
-- ===========================================================================

-- 仅当内置环境不存在时才插入（兼容已有数据）
INSERT OR IGNORE INTO environments (id, name, description, color, sort_order) VALUES
    ('G_env_dev',      '开发环境', '本地开发、调试数据库',      '#a6e3a1', 1),
    ('G_env_test',     '测试环境', '集成测试、QA 验证',          '#f9e2af', 2),
    ('G_env_staging',  '预发布',   '灰度验证、预发布环境',      '#89b4fa', 3),
    ('G_env_prod',     '生产环境', '线上生产数据库，谨慎操作',  '#f38ba8', 4),
    ('G_env_sandbox',  '沙箱环境', '安全隔离的沙箱数据库',      '#cba6f7', 5);

-- ===========================================================================
-- ======================== 环境策略 Seed 数据（带 G_ 前缀）==================
-- ===========================================================================

-- env-dev 策略
INSERT OR IGNORE INTO environment_policies (id, environment_id, policy_type, policy_config, enabled) VALUES
    ('G_ep_dev_sec',   'G_env_dev', 'security',    '{"readonly":false,"writeConfirm":false,"ddlConfirm":false,"dropConfirm":"false","autocommit":true,"rowLimit":0,"sizeLimit":0}', 1),
    ('G_ep_dev_sch',   'G_env_dev', 'schema',      '{"autoLoad":true,"loadDepth":2,"showSystem":true,"refreshInterval":0}', 1),
    ('G_ep_dev_perf',  'G_env_dev', 'performance', '{"poolSize":10,"queryTimeout":0,"connectTimeout":30,"heartbeat":60,"maxReconnect":3}', 1),
    ('G_ep_dev_audit', 'G_env_dev', 'audit',       '{"sqlLog":false,"operationRecord":false,"sensitiveTableAlert":false}', 1),
    ('G_ep_dev_ui',    'G_env_dev', 'ui',          '{"topBarColor":"#a6e3a1","tabIndicator":true,"sqlWarningBanner":false,"writeBtnStyle":"normal"}', 1);

-- env-test 策略
INSERT OR IGNORE INTO environment_policies (id, environment_id, policy_type, policy_config, enabled) VALUES
    ('G_ep_test_sec',   'G_env_test', 'security',    '{"readonly":false,"writeConfirm":false,"ddlConfirm":true,"dropConfirm":"true","autocommit":true,"rowLimit":10000,"sizeLimit":100}', 1),
    ('G_ep_test_sch',   'G_env_test', 'schema',      '{"autoLoad":true,"loadDepth":2,"showSystem":true,"refreshInterval":60}', 1),
    ('G_ep_test_perf',  'G_env_test', 'performance', '{"poolSize":10,"queryTimeout":120,"connectTimeout":30,"heartbeat":60,"maxReconnect":3}', 1),
    ('G_ep_test_audit', 'G_env_test', 'audit',       '{"sqlLog":false,"operationRecord":false,"sensitiveTableAlert":false}', 1),
    ('G_ep_test_ui',    'G_env_test', 'ui',          '{"topBarColor":"#f9e2af","tabIndicator":true,"sqlWarningBanner":false,"writeBtnStyle":"normal"}', 1);

-- env-staging 策略
INSERT OR IGNORE INTO environment_policies (id, environment_id, policy_type, policy_config, enabled) VALUES
    ('G_ep_stg_sec',   'G_env_staging', 'security',    '{"readonly":false,"writeConfirm":true,"ddlConfirm":true,"dropConfirm":"true","autocommit":false,"rowLimit":5000,"sizeLimit":50}', 1),
    ('G_ep_stg_sch',   'G_env_staging', 'schema',      '{"autoLoad":true,"loadDepth":2,"showSystem":false,"refreshInterval":120}', 1),
    ('G_ep_stg_perf',  'G_env_staging', 'performance', '{"poolSize":15,"queryTimeout":180,"connectTimeout":30,"heartbeat":60,"maxReconnect":5}', 1),
    ('G_ep_stg_audit', 'G_env_staging', 'audit',       '{"sqlLog":true,"operationRecord":true,"sensitiveTableAlert":true}', 1),
    ('G_ep_stg_ui',    'G_env_staging', 'ui',          '{"topBarColor":"#89b4fa","tabIndicator":true,"sqlWarningBanner":true,"writeBtnStyle":"confirm"}', 1);

-- env-prod 策略
INSERT OR IGNORE INTO environment_policies (id, environment_id, policy_type, policy_config, enabled) VALUES
    ('G_ep_prod_sec',   'G_env_prod', 'security',    '{"readonly":true,"writeConfirm":true,"ddlConfirm":true,"dropConfirm":"disable","autocommit":false,"rowLimit":1000,"sizeLimit":20}', 1),
    ('G_ep_prod_sch',   'G_env_prod', 'schema',      '{"autoLoad":false,"loadDepth":1,"showSystem":false,"refreshInterval":300}', 1),
    ('G_ep_prod_perf',  'G_env_prod', 'performance', '{"poolSize":20,"queryTimeout":60,"connectTimeout":15,"heartbeat":30,"maxReconnect":3}', 1),
    ('G_ep_prod_audit', 'G_env_prod', 'audit',       '{"sqlLog":true,"operationRecord":true,"sensitiveTableAlert":true}', 1),
    ('G_ep_prod_ui',    'G_env_prod', 'ui',          '{"topBarColor":"#f38ba8","tabIndicator":true,"sqlWarningBanner":true,"writeBtnStyle":"danger"}', 1);

-- env-sandbox 策略
INSERT OR IGNORE INTO environment_policies (id, environment_id, policy_type, policy_config, enabled) VALUES
    ('G_ep_sbx_sec',   'G_env_sandbox', 'security',    '{"readonly":false,"writeConfirm":false,"ddlConfirm":false,"dropConfirm":"false","autocommit":true,"rowLimit":1000,"sizeLimit":50}', 1),
    ('G_ep_sbx_sch',   'G_env_sandbox', 'schema',      '{"autoLoad":true,"loadDepth":1,"showSystem":false,"refreshInterval":0}', 1),
    ('G_ep_sbx_perf',  'G_env_sandbox', 'performance', '{"poolSize":5,"queryTimeout":60,"connectTimeout":30,"heartbeat":60,"maxReconnect":2}', 1),
    ('G_ep_sbx_audit', 'G_env_sandbox', 'audit',       '{"sqlLog":false,"operationRecord":false,"sensitiveTableAlert":false}', 1),
    ('G_ep_sbx_ui',    'G_env_sandbox', 'ui',          '{"topBarColor":"#cba6f7","tabIndicator":true,"sqlWarningBanner":false,"writeBtnStyle":"normal"}', 1);

-- ===========================================================================
-- 说明：data_source_types / drivers / driver_files 的 ID 已在 008 迁移中定义
-- 这些为系统级内置数据，ID 格式固定为类型名（mysql/postgresql/sqlite/duckdb），
-- 不在 G_/P_ 前缀管理范围。
-- ===========================================================================