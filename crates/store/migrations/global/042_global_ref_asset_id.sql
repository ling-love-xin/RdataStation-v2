-- 042: 全局引用改按稳定 asset_id 登记
--
-- 旧 `global_name + global_version` 会因显示名改动而失去指向；表资产的身份是
-- `analytics_resources.id`。按 T9 口径直接重建，不迁移旧 pin 数据。

DROP TABLE IF EXISTS analytics_global_refs;

CREATE TABLE analytics_global_refs (
    id                  TEXT PRIMARY KEY,
    project_root        TEXT NOT NULL,
    project_resource_id TEXT NOT NULL,
    project_version     INTEGER NOT NULL,
    global_asset_id     TEXT NOT NULL,
    global_version      INTEGER NOT NULL,
    created_at          TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    released_at         TEXT,
    UNIQUE (project_root, project_resource_id, project_version)
);

CREATE INDEX idx_agr_active_version
    ON analytics_global_refs (global_asset_id, global_version)
    WHERE released_at IS NULL;

CREATE INDEX idx_agr_project_resource
    ON analytics_global_refs (project_root, project_resource_id);
