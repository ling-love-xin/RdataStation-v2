-- 迁移版本：011
-- 数据库类型：SQLite（全局库 global.db）
-- 作用：为 auth_configs 和 network_configs 表添加类型过滤索引
-- 更新时间：2026-05-22
-- 注：项目级 migrations/project_meta/013_add_config_indexes.sql 为对应功能，版本号独立

-- auth_configs 按 auth_type 过滤索引（前端按认证类型筛选）
CREATE INDEX IF NOT EXISTS idx_auth_configs_type ON auth_configs(auth_type);

-- network_configs 按 network_type 过滤索引（前端按网络类型筛选 ssh/ssl/proxy）
CREATE INDEX IF NOT EXISTS idx_network_configs_type ON network_configs(network_type);