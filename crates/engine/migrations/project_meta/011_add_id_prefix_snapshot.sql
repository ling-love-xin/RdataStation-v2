-- 迁移版本：011
-- 数据库类型：SQLite
-- 作用：ID 前缀规范化 + 快照溯源字段 — 项目表增加 origin / source_id / snapshot_at
-- 更新时间：2026-05-19
--
-- 设计原则：
--   G_xxx  = 全局表主键，存储于 global.db
--   P_xxx  = 项目表主键（本地创建），存储于 project.db
--   GP_xxx = 从全局快照到项目的数据，存储于 project.db
--
-- 新增字段：
--   origin       = 'project'（本地创建）| 'global_snapshot'（从全局快照）
--   source_id    = 快照来源的全局 G_xxx ID
--   snapshot_at  = 快照创建时间戳
--
-- 相关表：
--   environments         — 环境定义
--   network_configs      — 网络配置（SSH/SSL/Proxy）
--   auth_configs         — 认证配置

-- ===========================================================================
-- ======================== 环境表：增加快照溯源字段 ===========================
-- ===========================================================================

ALTER TABLE environments ADD COLUMN origin TEXT DEFAULT 'project';
ALTER TABLE environments ADD COLUMN source_id TEXT;
ALTER TABLE environments ADD COLUMN snapshot_at TIMESTAMP;

-- ===========================================================================
-- ======================== 网络配置表：增加快照溯源字段 =======================
-- ===========================================================================

ALTER TABLE network_configs ADD COLUMN origin TEXT DEFAULT 'project';
ALTER TABLE network_configs ADD COLUMN source_id TEXT;
ALTER TABLE network_configs ADD COLUMN snapshot_at TIMESTAMP;

-- ===========================================================================
-- ======================== 认证配置表：增加快照溯源字段 =======================
-- ===========================================================================

ALTER TABLE auth_configs ADD COLUMN origin TEXT DEFAULT 'project';
ALTER TABLE auth_configs ADD COLUMN source_id TEXT;
ALTER TABLE auth_configs ADD COLUMN snapshot_at TIMESTAMP;

-- ===========================================================================
-- 说明：
-- 1. 已存在的行 origin 为 'project'（默认值），表示本地创建
-- 2. environment_policies 不单独增加快照字段，跟随 environments 的 origin
-- 3. project_drivers 为映射表，不需要 origin 字段
-- 4. connections 表中的 environment_id / network_config_id / auth_config_id
--    字段取值为 P_xxx / GP_xxx，应用程序根据前缀识别来源
-- ===========================================================================