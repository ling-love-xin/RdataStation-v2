-- 041: 表资产快照模型（全局库）
--
-- 与项目库 038 同构：manifest 逻辑内容、snapshot 物理实例、version 资产链、
-- checkout 租约。全局表资产一条活跃登记行一个 asset_id，不再一版一行。

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
