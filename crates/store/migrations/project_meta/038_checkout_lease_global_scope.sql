-- 迁移版本：037
-- 数据库类型：SQLite
-- 作用：取回租约允许 `base_scope = 'global'`（全局文件取回到项目草稿箱）。
--
-- 036 的 `checkout_leases.resource_id` 外键指向项目 `analytics_resources`；
-- 全局取回没有项目资产行，租约键是 `global:<name>`，会被外键挡住。
-- 这里重建表去掉外键；项目内取回仍由调用方保证 resource_id 是本地资产。
-- 旧租约数据原样保留。

DROP INDEX IF EXISTS ux_active_lease;
DROP INDEX IF EXISTS idx_lease_dest;

ALTER TABLE checkout_leases RENAME TO checkout_leases_036;

CREATE TABLE checkout_leases (
    lease_id           TEXT PRIMARY KEY,
    resource_id        TEXT NOT NULL,
    base_scope         TEXT NOT NULL,          -- project | global
    base_version       INTEGER NOT NULL,
    dest_rel           TEXT NOT NULL,
    state              TEXT NOT NULL,          -- active | consumed | abandoned
    created_at         TEXT NOT NULL,
    closed_at          TEXT,
    consumed_version   INTEGER
);

INSERT INTO checkout_leases (
    lease_id, resource_id, base_scope, base_version, dest_rel,
    state, created_at, closed_at, consumed_version
)
SELECT
    lease_id, resource_id, base_scope, base_version, dest_rel,
    state, created_at, closed_at, consumed_version
FROM checkout_leases_036;

DROP TABLE checkout_leases_036;

CREATE UNIQUE INDEX IF NOT EXISTS ux_active_lease
    ON checkout_leases (resource_id, base_scope)
    WHERE state = 'active';

CREATE INDEX IF NOT EXISTS idx_lease_dest
    ON checkout_leases (dest_rel);
