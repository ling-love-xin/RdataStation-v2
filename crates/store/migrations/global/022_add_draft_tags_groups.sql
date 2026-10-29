-- 022: 暂存草稿的标签与分组
--
-- 背景（M3 C11）：连接对话框常规 Tab 新增「组织（标签 / 分组）」卡片：
--   - tags：标签文本（逗号分隔，保存时解析为 JSON 数组写入连接 `tags` 并同步
--     `connection_tags` 权威检索表）；
--   - groups_json：已勾选的项目分组 id（JSON 数组；保存时以替换语义同步
--     `connection_group_members`，分组为项目级能力）。
-- 草稿快照需与表单一致（切换条目 / 关闭重开不丢），故随表持久化。
--
-- 幂等：只加列、带默认值；迁移按版本号只执行一次。

ALTER TABLE connection_drafts ADD COLUMN tags TEXT NOT NULL DEFAULT '';
ALTER TABLE connection_drafts ADD COLUMN groups_json TEXT NOT NULL DEFAULT '[]';
