-- 043: 表资产 fork 关系（全局库）
--
-- 全局发布 base 落后时只允许 fork：子资产以新 asset_id 从 v1 起链，
-- v1 直接引用父资产某一版的快照（同实例零复制）。与文件资产的 name-based
-- `asset_forks` 分开存，避免两种主键口径混在一张表里。

CREATE TABLE IF NOT EXISTS table_asset_forks (
    fork_id         TEXT PRIMARY KEY,
    parent_asset_id TEXT NOT NULL,
    parent_version  INTEGER NOT NULL,
    child_asset_id  TEXT NOT NULL,
    child_version   INTEGER NOT NULL,
    created_at      TEXT NOT NULL,
    FOREIGN KEY (parent_asset_id) REFERENCES analytics_resources(id),
    FOREIGN KEY (child_asset_id) REFERENCES analytics_resources(id)
);

CREATE INDEX IF NOT EXISTS idx_taf_parent
    ON table_asset_forks (parent_asset_id, parent_version);

CREATE INDEX IF NOT EXISTS idx_taf_child
    ON table_asset_forks (child_asset_id);
