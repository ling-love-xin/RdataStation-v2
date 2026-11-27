-- 038: 表资产快照模型（项目库）
--
-- 口径（2026-11-27 定稿，table-asset-snapshot-design.md）：
--   * 表资产指向不可变快照，不再指向用户活表；
--   * manifest 是逻辑内容，snapshot 是物理实例，version 是资产链；
--   * checkout 出工作表并写租约，归档消费租约。
--
-- 本文方案不做旧表资产迁移、不做双读双写；新表资产从 v1 开始。

CREATE TABLE IF NOT EXISTS table_manifests (
    manifest_id       TEXT PRIMARY KEY,
    schema_hash       TEXT NOT NULL,
    data_fingerprint  TEXT NOT NULL,
    row_count         INTEGER NOT NULL,
    column_count      INTEGER NOT NULL,
    schema_json       TEXT NOT NULL,
    created_at        TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS table_snapshots (
    snapshot_id       TEXT PRIMARY KEY,
    manifest_id       TEXT NOT NULL,
    instance          TEXT NOT NULL CHECK (instance IN ('project', 'global')),
    physical_schema   TEXT NOT NULL,
    physical_table    TEXT NOT NULL,
    state             TEXT NOT NULL CHECK (state IN ('live', 'missing')),
    created_at        TEXT NOT NULL,
    verified_at       TEXT,
    FOREIGN KEY (manifest_id) REFERENCES table_manifests(manifest_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS ux_table_snapshot_physical
    ON table_snapshots (physical_schema, physical_table);

CREATE TABLE IF NOT EXISTS table_asset_versions (
    asset_id        TEXT NOT NULL,
    version         INTEGER NOT NULL,
    snapshot_id     TEXT NOT NULL,
    manifest_id     TEXT NOT NULL,
    parent_version  INTEGER,
    created_at      TEXT NOT NULL,
    PRIMARY KEY (asset_id, version),
    FOREIGN KEY (asset_id) REFERENCES analytics_resources(id) ON DELETE CASCADE,
    FOREIGN KEY (snapshot_id) REFERENCES table_snapshots(snapshot_id),
    FOREIGN KEY (manifest_id) REFERENCES table_manifests(manifest_id)
);

CREATE INDEX IF NOT EXISTS idx_tav_snapshot
    ON table_asset_versions (snapshot_id);

CREATE TABLE IF NOT EXISTS table_checkout_leases (
    lease_id         TEXT PRIMARY KEY,
    asset_id         TEXT NOT NULL,
    base_version     INTEGER NOT NULL,
    work_schema      TEXT NOT NULL,
    work_table       TEXT NOT NULL,
    state            TEXT NOT NULL CHECK (state IN ('active', 'consumed', 'abandoned')),
    created_at       TEXT NOT NULL,
    closed_at        TEXT,
    consumed_version INTEGER,
    FOREIGN KEY (asset_id) REFERENCES analytics_resources(id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS ux_table_active_lease
    ON table_checkout_leases (asset_id)
    WHERE state = 'active';

CREATE UNIQUE INDEX IF NOT EXISTS ux_table_active_work
    ON table_checkout_leases (work_schema, work_table)
    WHERE state = 'active';
