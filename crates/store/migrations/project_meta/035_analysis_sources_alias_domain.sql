-- 迁移版本：035
-- 数据库类型：SQLite（项目库 .RSmeta/project.db）
-- 作用：把 `analysis_sources.alias` 的唯一性**按档位分域**（2026-11-25 第 6 条）
-- 更新时间：2026-11-25
--
-- 为什么改：034 给 `alias` 建了全表唯一索引，但三个档位的别名语义并不一样——
-- - `federation`：别名就是 DuckDB 里的 schema 名（`duckdb_databases().database_name`），
--   同一个联邦实例里必须唯一，冲突真的会挂错源；
-- - `attach_file`：同上，别名就是挂载名；
-- - `accel`：ATTACH 别名固定是 `rds_src`（`accel::SOURCE_ALIAS`），一次只挂一个库，
--   登记里的 `alias` **只是展示名/期望名**。全表唯一会让"第二条加速源"永远登不进来
--   （两条都叫 `rds_src` 就撞索引），或者逼用户给展示名编一个假别名。
--
-- 改法：唯一约束只落在"别名即挂载名"的两档；加速档保留普通索引（展示名重复也允许，
-- 加速档的身份是 connection_id / path，不是 alias）。

DROP INDEX IF EXISTS idx_analysis_sources_alias;

CREATE UNIQUE INDEX IF NOT EXISTS idx_analysis_sources_alias
    ON analysis_sources(alias)
    WHERE mount_kind <> 'accel';

CREATE INDEX IF NOT EXISTS idx_analysis_sources_alias_accel
    ON analysis_sources(alias)
    WHERE mount_kind = 'accel';
