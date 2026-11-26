-- 040: 全局文件资产 CAS 版本、取回租约、fork 与发布链。
--
-- 口径（2026-11-27 定稿，file-asset-cas-design.md §3.2 / §4.3 / §6.6）：
--   * 全局文件版本 = manifest + CAS 对象；`file_asset_versions` 只索引版本，不存字节；
--   * 全局取回落到某个项目草稿箱，租约仍以项目为作用域，但全局侧保留一条记录，
--     供 `purge_global` 在删除前确认没有活跃取回；
--   * fork 不复制字节，只在新资产名下复用同一 manifest，并记录来源；
--   * 发布链记录全局版本来自哪条项目资产/哪一版，项目资产删除后仍可追溯。
--
-- 幂等：CREATE TABLE / INDEX IF NOT EXISTS；迁移按版本号只执行一次。

CREATE TABLE IF NOT EXISTS file_asset_versions (
    resource_id     TEXT NOT NULL,
    version         INTEGER NOT NULL,
    manifest_id     TEXT NOT NULL,
    content_hash    TEXT NOT NULL,
    meta_hash       TEXT,
    parent_version  INTEGER,
    content_pruned  INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL,
    PRIMARY KEY (resource_id, version)
);

CREATE INDEX IF NOT EXISTS idx_gfav_content_hash
    ON file_asset_versions (content_hash);

CREATE TABLE IF NOT EXISTS global_checkout_leases (
    lease_id          TEXT PRIMARY KEY,
    resource_name     TEXT NOT NULL,
    project_root      TEXT NOT NULL,
    base_version      INTEGER NOT NULL,
    dest_rel          TEXT NOT NULL,
    state             TEXT NOT NULL,
    created_at        TEXT NOT NULL,
    closed_at         TEXT,
    consumed_version  INTEGER
);

CREATE UNIQUE INDEX IF NOT EXISTS ux_global_active_lease
    ON global_checkout_leases (resource_name, project_root)
    WHERE state = 'active';

CREATE TABLE IF NOT EXISTS asset_forks (
    fork_id          TEXT PRIMARY KEY,
    parent_name      TEXT NOT NULL,
    parent_version   INTEGER NOT NULL,
    child_name       TEXT NOT NULL,
    child_version    INTEGER NOT NULL,
    created_at       TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS global_publish_links (
    link_id          TEXT PRIMARY KEY,
    global_name      TEXT NOT NULL,
    global_version   INTEGER NOT NULL,
    source_resource_id TEXT,
    source_scope     TEXT NOT NULL,
    source_version   INTEGER,
    published_at     TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_gpl_source
    ON global_publish_links (source_resource_id, source_version);
