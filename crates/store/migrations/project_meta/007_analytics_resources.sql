-- 迁移版本：007
-- 数据库类型：SQLite
-- 作用：分析资源管理器（连接、表、文件的统一管理）
-- 更新时间：2026-05-07

-- ===========================================================================
-- ======================== 分析资源表 ======================================
-- ===========================================================================

CREATE TABLE IF NOT EXISTS analytics_resources (
    id                  TEXT PRIMARY KEY,
    resource_type       TEXT NOT NULL,       -- connection / table / file
    name                TEXT NOT NULL,       -- 显示名称
    alias               TEXT,                 -- 用户自定义别名
    config              TEXT NOT NULL CHECK (json_valid(config)),
                                              -- JSON: {connection_id, table_name, file_path, ...}
    scope               TEXT NOT NULL CHECK (scope IN ('global', 'project', 'session')),
    row_count           INTEGER,
    column_count        INTEGER,
    file_size           INTEGER,
    version             INTEGER DEFAULT 1,
    parent_version_id   TEXT,
    parent_resource_id  TEXT,
    source_query        TEXT,
    created_at          TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at          TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    created_by          TEXT,
    deleted_at          TIMESTAMP,
    FOREIGN KEY (parent_resource_id) REFERENCES analytics_resources(id)
);

CREATE INDEX IF NOT EXISTS idx_ar_type ON analytics_resources(resource_type);
CREATE INDEX IF NOT EXISTS idx_ar_scope ON analytics_resources(scope);
CREATE INDEX IF NOT EXISTS idx_ar_deleted ON analytics_resources(deleted_at);
CREATE INDEX IF NOT EXISTS idx_ar_name ON analytics_resources(name);

-- updated_at 自动更新触发器（仅在业务列变更时触发，避免递归）
CREATE TRIGGER IF NOT EXISTS trg_ar_updated_at
    AFTER UPDATE OF name, alias, config, scope, row_count, column_count, file_size,
                   version, parent_version_id, parent_resource_id, source_query, deleted_at
    ON analytics_resources
BEGIN
    UPDATE analytics_resources
    SET updated_at = CURRENT_TIMESTAMP
    WHERE id = NEW.id;
END;

-- ===========================================================================
-- ======================== 资源版本历史表 ====================================
-- ===========================================================================

CREATE TABLE IF NOT EXISTS analytics_resource_versions (
    id                  TEXT PRIMARY KEY,
    resource_id         TEXT NOT NULL,
    version             INTEGER NOT NULL,
    snapshot            TEXT NOT NULL,       -- JSON: 该版本的完整资源数据快照
    created_at          TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (resource_id) REFERENCES analytics_resources(id),
    UNIQUE(resource_id, version)
);

CREATE INDEX IF NOT EXISTS idx_arv_resource ON analytics_resource_versions(resource_id);
CREATE INDEX IF NOT EXISTS idx_arv_version ON analytics_resource_versions(resource_id, version DESC);

-- ===========================================================================
-- ======================== 文件夹表 ======================================
-- ===========================================================================

CREATE TABLE IF NOT EXISTS analytics_folders (
    id                  TEXT PRIMARY KEY,
    name                TEXT NOT NULL,
    scope               TEXT NOT NULL CHECK (scope IN ('global', 'project', 'session')),
    parent_folder_id    TEXT,
    sort_order          INTEGER DEFAULT 0,
    color               TEXT,
    icon                TEXT,
    created_at          TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at          TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    deleted_at          TIMESTAMP,
    FOREIGN KEY (parent_folder_id) REFERENCES analytics_folders(id)
);

CREATE INDEX IF NOT EXISTS idx_af_scope ON analytics_folders(scope);
CREATE INDEX IF NOT EXISTS idx_af_deleted ON analytics_folders(deleted_at);
CREATE INDEX IF NOT EXISTS idx_af_parent ON analytics_folders(parent_folder_id);

CREATE TRIGGER IF NOT EXISTS trg_af_updated_at
    AFTER UPDATE OF name, scope, parent_folder_id, sort_order, color, icon, deleted_at
    ON analytics_folders
BEGIN
    UPDATE analytics_folders
    SET updated_at = CURRENT_TIMESTAMP
    WHERE id = NEW.id;
END;

-- ===========================================================================
-- ======================== 资源-文件夹关联表 =================================
-- ===========================================================================

CREATE TABLE IF NOT EXISTS analytics_resource_folder (
    resource_id         TEXT NOT NULL,
    folder_id           TEXT NOT NULL,
    sort_order          INTEGER DEFAULT 0,
    PRIMARY KEY (resource_id, folder_id),
    FOREIGN KEY (resource_id) REFERENCES analytics_resources(id),
    FOREIGN KEY (folder_id) REFERENCES analytics_folders(id)
);

CREATE INDEX IF NOT EXISTS idx_arf_resource ON analytics_resource_folder(resource_id);
CREATE INDEX IF NOT EXISTS idx_arf_folder ON analytics_resource_folder(folder_id);

-- ===========================================================================
-- ======================== 标签表 ===========================================
-- ===========================================================================

CREATE TABLE IF NOT EXISTS analytics_tags (
    id                  TEXT PRIMARY KEY,
    name                TEXT NOT NULL,
    color               TEXT,
    icon                TEXT,
    scope               TEXT NOT NULL CHECK (scope IN ('global', 'project')),
    created_at          TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    deleted_at          TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_at_scope ON analytics_tags(scope);
CREATE INDEX IF NOT EXISTS idx_at_deleted ON analytics_tags(deleted_at);
CREATE UNIQUE INDEX IF NOT EXISTS idx_at_name_scope ON analytics_tags(name, scope) WHERE deleted_at IS NULL;

-- ===========================================================================
-- ======================== 资源-标签关联表 ===================================
-- ===========================================================================

CREATE TABLE IF NOT EXISTS analytics_resource_tags (
    resource_id         TEXT NOT NULL,
    tag_id              TEXT NOT NULL,
    PRIMARY KEY (resource_id, tag_id),
    FOREIGN KEY (resource_id) REFERENCES analytics_resources(id),
    FOREIGN KEY (tag_id) REFERENCES analytics_tags(id)
);

CREATE INDEX IF NOT EXISTS idx_art_resource ON analytics_resource_tags(resource_id);
CREATE INDEX IF NOT EXISTS idx_art_tag ON analytics_resource_tags(tag_id);

-- ===========================================================================
-- ======================== 回收站表 =========================================
-- ===========================================================================

CREATE TABLE IF NOT EXISTS analytics_recycle_bin (
    id                  TEXT PRIMARY KEY,
    resource_id         TEXT NOT NULL,
    resource_type       TEXT NOT NULL,
    resource_name       TEXT NOT NULL,
    resource_data       TEXT NOT NULL,       -- JSON: 完整资源数据快照
    deleted_by          TEXT,
    deleted_at          TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_rb_deleted_at ON analytics_recycle_bin(deleted_at DESC);
CREATE INDEX IF NOT EXISTS idx_rb_type ON analytics_recycle_bin(resource_type);
