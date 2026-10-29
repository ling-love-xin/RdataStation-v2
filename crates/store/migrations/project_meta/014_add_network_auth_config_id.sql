-- 迁移版本：014
-- 数据库类型：SQLite（项目库 project.db）
-- 作用：network_configs 追加 auth_config_id 列，引用关联的认证配置
-- 更新时间：2026-05-25
--
-- 注：全局级 migrations/global/012_add_network_auth_config_id.sql 为对应功能，版本号独立

ALTER TABLE network_configs ADD COLUMN auth_config_id TEXT;

CREATE INDEX IF NOT EXISTS idx_network_configs_auth_id ON network_configs(auth_config_id);