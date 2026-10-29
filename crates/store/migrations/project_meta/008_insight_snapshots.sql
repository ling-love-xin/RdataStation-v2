-- 迁移版本：008
-- 数据库类型：SQLite
-- 作用：洞察快照元数据表（版本化存储）
-- 更新时间：2026-05-07

-- ===========================================================================
-- ======================== 洞察快照元数据表 ===================================
-- ===========================================================================

CREATE TABLE IF NOT EXISTS insight_snapshots (
    id                  TEXT PRIMARY KEY,               -- UUID
    entity_type         TEXT NOT NULL,                  -- column/table/schema
    entity_name         TEXT NOT NULL,                  -- 列名/表名/Schema名
    entity_source       TEXT,                           -- JSON: {conn_id, db, schema, table}
    snapshot_id         TEXT NOT NULL,                  -- FK → DuckDB snapshot_id
    row_count           INTEGER,                        -- 数据行数
    elapsed_ms          INTEGER,                        -- 计算耗时(ms)
    version_id          TEXT NOT NULL,                  -- 版本UUID
    parent_version_id   TEXT,                           -- 父版本UUID（版本链）
    checksum            TEXT NOT NULL,                  -- SHA256 校验和
    created_at          TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 索引
CREATE INDEX IF NOT EXISTS idx_insight_entity ON insight_snapshots(entity_type, entity_name);
CREATE INDEX IF NOT EXISTS idx_insight_snapshot ON insight_snapshots(snapshot_id);
CREATE INDEX IF NOT EXISTS idx_insight_version ON insight_snapshots(version_id);
CREATE INDEX IF NOT EXISTS idx_insight_created ON insight_snapshots(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_insight_entity_source ON insight_snapshots(entity_source);
