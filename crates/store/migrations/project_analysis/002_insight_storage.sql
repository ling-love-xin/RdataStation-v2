-- 迁移版本：002
-- 数据库类型：DuckDB
-- 作用：洞察分析数据存储（列洞察快照、表探查报告、Schema洞察报告）
-- 更新时间：2026-05-07

-- ===========================================================================
-- ======================== 列洞察快照表 ======================================
-- ===========================================================================

CREATE TABLE IF NOT EXISTS insight_column_snapshots (
    snapshot_id         TEXT PRIMARY KEY,               -- UUID
    column_name         TEXT NOT NULL,
    data_type           TEXT,
    stats_json          TEXT NOT NULL,                  -- ColumnInsightFull JSON
    version_id          TEXT NOT NULL,                  -- 版本UUID
    parent_version_id   TEXT,                           -- 父版本UUID
    checksum            TEXT NOT NULL,                  -- SHA256 校验和
    created_at          TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_ics_column ON insight_column_snapshots(column_name);
CREATE INDEX IF NOT EXISTS idx_ics_version ON insight_column_snapshots(version_id);
CREATE INDEX IF NOT EXISTS idx_ics_created ON insight_column_snapshots(created_at DESC);

-- ===========================================================================
-- ======================== 表探查报告表（Phase 2 预留）========================
-- ===========================================================================

CREATE TABLE IF NOT EXISTS insight_table_reports (
    report_id           TEXT PRIMARY KEY,               -- UUID
    table_name          TEXT NOT NULL,
    report_json         TEXT NOT NULL,                  -- TableProfile JSON
    version_id          TEXT NOT NULL,
    parent_version_id   TEXT,
    checksum            TEXT NOT NULL,
    created_at          TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_itr_table ON insight_table_reports(table_name);
CREATE INDEX IF NOT EXISTS idx_itr_created ON insight_table_reports(created_at DESC);

-- ===========================================================================
-- ======================== Schema洞察报告表（Phase 3 预留）====================
-- ===========================================================================

CREATE TABLE IF NOT EXISTS insight_schema_reports (
    report_id           TEXT PRIMARY KEY,               -- UUID
    schema_name         TEXT NOT NULL,
    report_json         TEXT NOT NULL,                  -- SchemaReport JSON
    version_id          TEXT NOT NULL,
    parent_version_id   TEXT,
    checksum            TEXT NOT NULL,
    created_at          TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_isr_schema ON insight_schema_reports(schema_name);
CREATE INDEX IF NOT EXISTS idx_isr_created ON insight_schema_reports(created_at DESC);
