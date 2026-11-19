-- 迁移版本：037
-- 数据库类型：SQLite（全局库）
-- 作用：网络两层模型——`net_resources`（最小单位：ssh / http / socks / ssl）+ `chains`（资源引用序列）
-- 更新时间：2026-11-19
--
-- 设计依据：`docs/architecture/connection/prototypes/manager-network.html`（Tab5 表模型）
-- 与架构决策 #122。口径要点：
--   1. 凭据只在 `auth_configs` 里（本表只留 `auth_config_id`），不再把密码散进每个资源的 props；
--   2. `props` 存「单跳原生配置 JSON」（与旧 `network_configs.config` 同形），因此链展开后
--      运行时拿到的 hop JSON 与旧实现逐字等价——两层模型是**归一化视图**，不是新执行语义；
--   3. `chains.hops` 是**有序资源 id 数组**（JSON `["NR_…","NR_…"]`），可重复引用、不内嵌内容。
--
-- ⚠️ 旧 `network_configs` 表保留：存量行的迁移是**幂等幂**补齐（见
--    `store::persistence::network_two_layer::migrate_legacy_network_configs`），
--    资源 / 链记录沿用旧档案 id，引用关系（连接上的 `network_config_id`）无需改写。

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
