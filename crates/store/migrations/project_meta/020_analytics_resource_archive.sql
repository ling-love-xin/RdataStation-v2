-- 020_analytics_resource_archive.sql
--
-- M6 资产库 / 分析存档：把 v1 的"元数据指针"模型升级为"分析存档"模型。
-- 设计依据：docs/architecture/analytics_resource/analytics-resource-architecture.md §4.2。
--
-- 原则：
--   1. **只增列，不改 007**（老库直接升级，表结构自 v1 起未动）；
--   2. 旧行（v1 时代的"数据源连接引用 / DuckDB 表"）一律默认 kind = 'file'，
--      content_hash 为空（首次由界面标为"待指纹"，不是错误）；
--   3. 核心语义**必须进独立列**（不塞进 config JSON）——否则无法建索引、无法做表单、
--      搜索只能 LIKE name OR alias（v1 的代价）。

-- 存档种类：决定本体在哪（file → resources/；analysis → analytics.duckdb；table_ref → 远端）。
-- 说明：SQLite 的 ALTER TABLE ADD COLUMN 支持 CHECK，故约束落在库层（不是只靠代码枚举）。
ALTER TABLE analytics_resources
    ADD COLUMN kind TEXT NOT NULL DEFAULT 'file'
    CHECK (kind IN ('file', 'analysis', 'table_ref'));

-- 内容指纹：版本是否递增、是否"内容已变"的唯一依据（架构 §5.1）。
ALTER TABLE analytics_resources ADD COLUMN content_hash TEXT;

-- 本体相对路径（kind = 'file'）：`{项目}/resources/` 下，归档时定，**不随重命名变化**。
ALTER TABLE analytics_resources ADD COLUMN file_rel_path TEXT;

-- 归档后只读标记（应用层守卫为主，文件系统只读属性为辅）。
ALTER TABLE analytics_resources
    ADD COLUMN readonly INTEGER NOT NULL DEFAULT 1
    CHECK (readonly IN (0, 1));

-- 归档凭证的"出处"（架构 §2.3）：来源草稿 / 来源连接 / 来源表。
ALTER TABLE analytics_resources ADD COLUMN promoted_from TEXT;
ALTER TABLE analytics_resources ADD COLUMN source_connection_id TEXT;
ALTER TABLE analytics_resources ADD COLUMN source_table TEXT;

-- kind = 'analysis' 的重建定义（回答"怎么算的"）。
ALTER TABLE analytics_resources ADD COLUMN definition_sql TEXT;

-- 归档时刻（与 updated_at 区分：改显示名/标签只动 updated_at）。
ALTER TABLE analytics_resources ADD COLUMN archived_at TEXT;

-- 同一本体只能被一个存档占用（软删除的行不参与约束，否则删除后无法再归档同名文件）。
CREATE UNIQUE INDEX IF NOT EXISTS idx_ar_file_rel_path
    ON analytics_resources (file_rel_path)
    WHERE file_rel_path IS NOT NULL AND deleted_at IS NULL;

-- 按种类筛选（面板 facet）与按指纹查重（"同一内容是否已归档过"）。
CREATE INDEX IF NOT EXISTS idx_ar_kind ON analytics_resources (kind);
CREATE INDEX IF NOT EXISTS idx_ar_content_hash
    ON analytics_resources (content_hash)
    WHERE content_hash IS NOT NULL;
