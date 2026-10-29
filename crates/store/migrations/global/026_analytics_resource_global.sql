-- 026: M6 资产库「全局实例」资源登记表（双实例 §13）
--
-- 背景：M6 资源登记原本只在项目库（project_meta 迁移 020/023/024 的
-- `analytics_resources`）。2026-10-22 双实例需求（原型 §13）要求：
--   1. 展示两个 DuckDB 实例的资源表（全局分析库 `{system}/analytics.duckdb`
--      ↔ 项目分析库 `{项目}/.RSmeta/analytics.duckdb`）；
--   2. 项目 → 全局升级（分析表 / 数据文件映射出表 / 脚本内容表），
--      升级 = **表 + 版本信息**进全局 DuckDB；
--   3. 全局 → 项目分发（跨项目唯一法定路径：项目 A → 全局 → 项目 B，
--      项目 ↔ 项目禁止直传）。
--
-- 结构镜像：与项目库 `analytics_resources` 同一列集（020 存档语义 + 023
-- payload_mode/source_path + 既有 scope 列），保证映射代码（map_resource_row）
-- 与列序常量（RESOURCE_COLUMNS）可跨库复用。区别只在：
--   * 本表**只存 scope = 'global' 的行**（全局资源全局可见）；
--   * scope 默认 'global'（项目行仍在项目库写 'project'）。
--
-- 幂等：CREATE TABLE IF NOT EXISTS，迁移按版本号只执行一次。
-- 全局库独立于任一项目，跨项目共享的唯一中枢（架构原则，2026-10-22）。

CREATE TABLE IF NOT EXISTS analytics_resources (
    id                   TEXT PRIMARY KEY,
    resource_type        TEXT NOT NULL,
    name                 TEXT NOT NULL,
    alias                TEXT,
    config               TEXT NOT NULL DEFAULT 'null',
    scope                TEXT NOT NULL DEFAULT 'global',
    row_count            INTEGER,
    column_count         INTEGER,
    file_size            INTEGER,
    version              INTEGER NOT NULL DEFAULT 1,
    parent_version_id    TEXT,
    parent_resource_id   TEXT,
    source_query         TEXT,
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL,
    created_by           TEXT,
    deleted_at           TEXT,
    kind                 TEXT NOT NULL DEFAULT 'file'
                         CHECK (kind IN ('file', 'analysis', 'table_ref')),
    content_hash         TEXT,
    -- 全局行的「本体相对路径」语义 = 分发到项目时的重建目标
    -- （`resources/<path>`，脚本内容表分发时重建为文件；分析表/数据文件无此概念）。
    file_rel_path        TEXT,
    readonly             INTEGER NOT NULL DEFAULT 1
                         CHECK (readonly IN (0, 1)),
    promoted_from        TEXT,
    source_connection_id TEXT,
    source_table         TEXT,
    definition_sql       TEXT,
    archived_at          TEXT,
    payload_mode         TEXT NOT NULL DEFAULT 'copy'
                         CHECK (payload_mode IN ('copy', 'reference')),
    source_path          TEXT
);

-- 按种类筛选（面板 facet）：与项目库同。
CREATE INDEX IF NOT EXISTS idx_ar_global_kind
    ON analytics_resources (kind);

-- 内容指纹查重（同一内容是否已升级过全局）：与项目库同。
CREATE INDEX IF NOT EXISTS idx_ar_global_content_hash
    ON analytics_resources (content_hash)
    WHERE content_hash IS NOT NULL;

-- 按显示名查重（升级冲突检测：全局 DuckDB 表名 = 显示名，不能撞名）。
CREATE INDEX IF NOT EXISTS idx_ar_global_name
    ON analytics_resources (name)
    WHERE deleted_at IS NULL;

-- 版本链（镜像项目库 018/020 的 analytics_resource_versions）：全局资源
-- 的版本快照独立于项目侧（§13.3「版本链与项目侧独立」），分发到项目时
-- 版本号接续全局链（§13.6）。
CREATE TABLE IF NOT EXISTS analytics_resource_versions (
    id          TEXT PRIMARY KEY,
    resource_id TEXT NOT NULL,
    version     INTEGER NOT NULL,
    snapshot    TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    UNIQUE (resource_id, version)
);

CREATE INDEX IF NOT EXISTS idx_arv_global_resource
    ON analytics_resource_versions (resource_id, version DESC);
