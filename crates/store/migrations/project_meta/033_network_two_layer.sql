-- 迁移版本：033
-- 数据库类型：SQLite（项目库 .RSmeta/project.db）
-- 作用：网络两层模型（与全局库 037 同构）——项目级快照也按「资源 + 链」两表存。
-- 更新时间：2026-11-19
--
-- 说明：项目库是全局库的**快照副本**，列结构逐列一致（含 origin / source_id / snapshot_at），
-- 否则快照回灌与差异比对会因列缺失静默跳过。设计依据见全局库 037 与决策 #122。

CREATE TABLE IF NOT EXISTS net_resources (
    id             TEXT PRIMARY KEY,
    name           TEXT NOT NULL,
    kind           TEXT NOT NULL,
    host           TEXT NOT NULL DEFAULT '',
    port           INTEGER NOT NULL DEFAULT 0,
    auth_config_id TEXT,
    props          TEXT NOT NULL DEFAULT '{}',
    origin         TEXT,
    source_id      TEXT,
    snapshot_at    TEXT,
    created_at     TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at     TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_net_resources_kind ON net_resources(kind);
CREATE INDEX IF NOT EXISTS idx_net_resources_auth ON net_resources(auth_config_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_net_resources_name ON net_resources(name);

CREATE TABLE IF NOT EXISTS chains (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    hops       TEXT NOT NULL DEFAULT '[]',
    timeout_s  INTEGER,
    origin     TEXT,
    source_id  TEXT,
    snapshot_at TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_chains_name ON chains(name);
