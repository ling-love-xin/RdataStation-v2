-- 018: 项目版本台账（project_versions）
--
-- 项目管理模块「版本」分节（docs/architecture/project/project-dev-plan.md Phase B8）
-- 需要一个只读可列的版本链存储：当前先支持手动「创建版本快照」记录，
-- 为后续 DuckLake 多人协同（parent_id 版本链 + checksum）预留结构。

CREATE TABLE IF NOT EXISTS project_versions (
    id          TEXT PRIMARY KEY,
    parent_id   TEXT,
    message     TEXT NOT NULL,
    created_by  TEXT,
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_project_versions_created
    ON project_versions(created_at DESC);
