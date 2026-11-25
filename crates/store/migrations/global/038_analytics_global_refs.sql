-- 038: 全局引用登记（项目引用版本 → 全局版本）
--
-- 编号说明：原为 037，与 `037_network_two_layer.sql` 撞号——迁移管理器按数字
-- 记 `schema_version`，两个 037 会让第二个永远记不上（UNIQUE 冲突），故后到的
-- 这一份改 038（先到的网络两层保持 037，不重写别人的版本号）。
--
-- 口径（2026-11-25 用户锁定）：保留和固化都按版本。项目 B 的每个引用版本
-- 单独 pin 一个全局版本；只有 released_at IS NULL 的登记才阻止裁剪/删除。
-- 固化选中版本、删除引用版本、删除引用资产时，只释放对应登记。
--
-- 幂等：CREATE TABLE / INDEX IF NOT EXISTS，迁移按版本号只执行一次。

CREATE TABLE IF NOT EXISTS analytics_global_refs (
    id                  TEXT PRIMARY KEY,
    project_root        TEXT NOT NULL,
    project_resource_id TEXT NOT NULL,
    project_version     INTEGER NOT NULL,
    global_name         TEXT NOT NULL,
    global_version      INTEGER NOT NULL,
    created_at          TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    released_at         TEXT,
    UNIQUE (project_root, project_resource_id, project_version)
);

CREATE INDEX IF NOT EXISTS idx_agr_active_version
    ON analytics_global_refs (global_name, global_version)
    WHERE released_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_agr_project_resource
    ON analytics_global_refs (project_root, project_resource_id);
