-- 草稿箱标签关联表（标签统一规格 docs/architecture/tag/tag-system-spec.md §7.3）。
-- 草稿条目是磁盘文件（无 SQLite 条目表），entry_id = 模块内相对路径（如 `drafts/notes.sql`）；
-- 标签定义在项目库 `tags` 表（scope=project，由 rds-tag 懒建 ensure_tables）。
-- 不建外键：tags 表建表时机由 TagTable 懒加载决定，迁移不与其耦合；
-- 引用完整性由应用层保证（写入顺序「定义先写、关联后写」，读取侧 JOIN 过滤 deleted_at，规格 §3.4 D1）。
CREATE TABLE IF NOT EXISTS scratchpad_tags (
    entry_id TEXT NOT NULL,
    tag_id   TEXT NOT NULL,
    PRIMARY KEY (entry_id, tag_id)
);

-- 按标签反查草稿（打标列表 / 归档继承扫描用）。
CREATE INDEX IF NOT EXISTS idx_scratchpad_tags_tag ON scratchpad_tags(tag_id);
