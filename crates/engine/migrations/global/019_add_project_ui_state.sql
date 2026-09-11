-- 019: 项目名册 UI 状态（固定 / 软删）
--
-- 项目管理模块（docs/architecture/project/project-dev-plan.md Phase B1）需要：
--   - is_pinned / pinned_at：项目固定，固定项在最近/全部列表置顶，跨会话保留；
--   - removed_at：软删（移出名册）标记，磁盘保留，可经「已移除」视图恢复。
--
-- 只加列、带默认值，旧行安全；迁移按版本号幂等，不会重复执行。

ALTER TABLE project_info ADD COLUMN is_pinned INTEGER NOT NULL DEFAULT 0;
ALTER TABLE project_info ADD COLUMN pinned_at TIMESTAMP;
ALTER TABLE project_info ADD COLUMN removed_at TIMESTAMP;

-- 名册排序（固定置顶 + 最近打开）与已移除过滤
CREATE INDEX IF NOT EXISTS idx_project_info_pinned ON project_info(is_pinned DESC, last_opened_at DESC);
CREATE INDEX IF NOT EXISTS idx_project_info_removed ON project_info(removed_at);
