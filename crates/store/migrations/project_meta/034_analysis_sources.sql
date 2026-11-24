-- 迁移版本：034
-- 数据库类型：SQLite（项目库 .RSmeta/project.db）
-- 作用：分析源登记（数据对象面板「源」组的**意图侧**；事实侧是运行态 `duckdb_databases()`）
-- 更新时间：2026-11-25
--
-- 口径见 docs/architecture/layout/analysis-sources-spec.md 与铁律第 4 条（2026-11-25 改法）：
-- - 只存**项目级**意图；会话级加源不入库（切项目即清，写进库只会留下假记录）。
-- - `alias` 就是 SQL 里的引用名，等于挂载后 `duckdb_databases().database_name`；唯一。
-- - 不存表清单/挂载状态：那是运行态事实，写进库必然与实例漂移。
-- - 不存 `scope`：登记本身项目级；被引用连接的归属域从 `connections` 读，不复制一份。

CREATE TABLE IF NOT EXISTS analysis_sources (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL,
    kind          TEXT NOT NULL,
    connection_id TEXT,
    path          TEXT,
    alias         TEXT NOT NULL,
    mount_kind    TEXT NOT NULL,
    created_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_analysis_sources_alias ON analysis_sources(alias);
CREATE INDEX IF NOT EXISTS idx_analysis_sources_kind ON analysis_sources(kind);
