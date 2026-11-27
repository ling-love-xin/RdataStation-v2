-- 迁移版本：036
-- 数据库类型：SQLite
-- 作用：文件资产 CAS 版本索引与取回租约（设计见 docs/architecture/analytics_resource/file-asset-cas-design.md）
--
-- 只增表，不改 analytics_resources 的列：
--   * 文件内容/元数据的权威在 .RSmeta/cas/；
--   * 版本行仍由 analytics_resource_versions 保存快照；
--   * 本表把 manifest_id 与 lease 语义单独承载，旧资产没有行时走旧副本回退。

CREATE TABLE IF NOT EXISTS file_asset_versions (
    resource_id        TEXT NOT NULL,
    version            INTEGER NOT NULL,
    manifest_id        TEXT NOT NULL,
    content_hash       TEXT NOT NULL,
    meta_hash          TEXT,
    parent_version     INTEGER,
    content_pruned     INTEGER NOT NULL DEFAULT 0,
    created_at         TEXT NOT NULL,
    PRIMARY KEY (resource_id, version),
    FOREIGN KEY (resource_id) REFERENCES analytics_resources(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_fav_manifest
    ON file_asset_versions (manifest_id);

CREATE TABLE IF NOT EXISTS checkout_leases (
    lease_id           TEXT PRIMARY KEY,
    resource_id        TEXT NOT NULL,
    base_scope         TEXT NOT NULL,          -- project | global
    base_version       INTEGER NOT NULL,
    dest_rel           TEXT NOT NULL,
    state              TEXT NOT NULL,          -- active | consumed | abandoned
    created_at         TEXT NOT NULL,
    closed_at          TEXT,
    consumed_version   INTEGER,
    FOREIGN KEY (resource_id) REFERENCES analytics_resources(id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS ux_active_lease
    ON checkout_leases (resource_id, base_scope)
    WHERE state = 'active';

CREATE INDEX IF NOT EXISTS idx_lease_dest
    ON checkout_leases (dest_rel);
