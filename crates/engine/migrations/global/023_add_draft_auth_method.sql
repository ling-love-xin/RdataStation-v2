-- 023: 暂存草稿的认证方法（auth_method）
--
-- 背景（接口一致性修复）：连接记录有 `auth_method` 字段，连接链路在“引用认证配置”时
-- 用它决定如何注入凭据（见 connection_service::connect_with_type）。此前对话框既没有该
-- 字段的 UI，草稿快照也不存 → 引用认证配置的连接在连接时被静默跳过注入，切换条目 /
-- 重开后用户的选择也会丢失。
--
-- 现对话框常规 Tab「数据库认证」卡片增加「认证方法」下拉（选项来自
-- drivers.supported_auth_types，数据库驱动声明），草稿快照同步持久化
-- （空串 = 未选，落库时该字段为 None）。
--
-- 幂等：只加列、带默认值；迁移按版本号只执行一次。

ALTER TABLE connection_drafts ADD COLUMN auth_method TEXT NOT NULL DEFAULT '';
