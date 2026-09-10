-- 迁移版本：012
-- 数据库类型：SQLite（全局库 global.db）
-- 作用：network_configs 追加 auth_config_id 列，引用关联的认证配置
-- 更新时间：2026-05-25
--
-- 设计原则：
--   network_configs.config 保留完整配置信息（含 host/port/forwarding + auth 冗余）
--   network_configs.auth_config_id 引用 auth_configs.id，指向独立存储的认证凭据
--   该列可选（NULL = 无关联认证配置，如 SSL 纯证书配置）

ALTER TABLE network_configs ADD COLUMN auth_config_id TEXT;

CREATE INDEX IF NOT EXISTS idx_network_configs_auth_id ON network_configs(auth_config_id);