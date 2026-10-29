-- 迁移版本：001
-- 数据库类型：DuckDB
-- 作用：项目级分析数据存储（查询缓存、分析结果）
-- 更新时间：2026-04-27

-- ===========================================================================
-- ======================== 查询结果缓存表 ===================================
-- ===========================================================================

CREATE TABLE IF NOT EXISTS query_results (
    id                TEXT PRIMARY KEY,
    query_id          TEXT,
    sql_hash          TEXT,
    connection_id     TEXT,
    result_json       TEXT,
    row_count         INTEGER,
    execution_time_ms INTEGER,
    created_at        TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 索引
CREATE INDEX IF NOT EXISTS idx_query_results_connection ON query_results(connection_id);
CREATE INDEX IF NOT EXISTS idx_query_results_created ON query_results(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_query_results_hash ON query_results(sql_hash);

-- ===========================================================================
-- ======================== 数据分析表 =======================================
-- ===========================================================================

CREATE TABLE IF NOT EXISTS analytics (
    id                TEXT PRIMARY KEY,
    analysis_type     TEXT,
    source_connection TEXT,
    source_table      TEXT,
    analysis_json     TEXT,
    created_at        TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 索引
CREATE INDEX IF NOT EXISTS idx_analytics_type ON analytics(analysis_type);
CREATE INDEX IF NOT EXISTS idx_analytics_source ON analytics(source_connection);
