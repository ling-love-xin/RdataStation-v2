-- 038: 全局引用登记（项目引用版本 → 全局 asset_id + 版本）
--
-- 口径（2026-11-27 用户锁定）：引用按稳定 `global_asset_id` 登记，显示名改动不影响
-- 指向；只有 released_at IS NULL 的登记才阻止裁剪/删除。旧 name 列不保留。

CREATE TABLE IF NOT EXISTS analytics_global_refs (
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

CREATE INDEX IF NOT EXISTS idx_agr_active_version
    ON analytics_global_refs (global_asset_id, global_version)
    WHERE released_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_agr_project_resource
    ON analytics_global_refs (project_root, project_resource_id);
