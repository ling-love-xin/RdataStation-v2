-- 031: 给「本体 = 项目 analytics.duckdb 持久表」这类资产加登记位
--
-- 背景：020 立的三档（file / analysis / table_ref）都不覆盖「本体是项目分析库里的
-- 一张持久表」。但库里**已经有这种行**——全局分发到项目时，
-- `global/register.rs` 的 `register_project_table` 把全局表 COPY 成项目表后登记成
-- `kind = 'analysis'` + `file_rel_path = NULL`。于是同一个 `analysis` 串同时代表
-- 两种本体（文件快照 / 分析库表），`payload_path` 只能按文件路径找本体，
-- 这类行于是被误报成「没有登记本体路径（旧行）」，双击与查看统计都静默失败。
--
-- 本迁移只做两件事：加列、把历史行摆正。
--
-- 为什么**不改 `kind` 的 CHECK**（020 的 `CHECK (kind IN ('file','analysis','table_ref'))`）：
--   1. SQLite 改 CHECK 只能重建表，而本仓迁移执行器把每个迁移整个包在一个事务里
--      （`crates/store/src/migration/executor.rs` 的 `unchecked_transaction`），
--      `PRAGMA foreign_keys` 在事务内不生效；
--   2. `analytics_resources` 被 5 处外键引用（007 的 versions / folders / tags /
--      recycle_bin / assertions，024 重建过的 tags），项目库是 `foreign_keys=ON`。
--   所以**权威判据落在 `analytics_table` 列**：非空即表型。`kind` 串暂时仍写
--   `analysis`（代码侧 `ArchiveKind::resolve` 负责还原真实档位），
--   等将来有事务外的维护窗口再一次性收口 CHECK 与落库串。

-- 本体表名（表型资产的权威标识；文件型为 NULL）
ALTER TABLE analytics_resources ADD COLUMN analytics_table TEXT;

-- 出处（两列替代 `promoted_from` 的多义字符串；旧列保留做双写过渡）
ALTER TABLE analytics_resources ADD COLUMN origin_kind TEXT;
ALTER TABLE analytics_resources ADD COLUMN origin_ref TEXT;

-- 能否被 DuckDB 直接加载（1 = 结构化数据文件）。
-- 归档时探测一次即定，用于筛选「可参与分析的数据文件」，不再靠扩展名二次猜。
ALTER TABLE analytics_resources ADD COLUMN structured INTEGER;

-- ===== 历史行回填：把已经存在的「表型行」认出来 =====
-- 判据：analysis + 无文件路径 + copy 模式 = 全局分发落下来的项目表，
-- 它的本体就是项目分析库里那张**同名表**（分发时 `CREATE TABLE <target_name>`）。
UPDATE analytics_resources
SET analytics_table = name
WHERE kind = 'analysis'
  AND file_rel_path IS NULL
  AND analytics_table IS NULL;

-- 历史 `analysis` 行里凡有本体路径的，都是能读的数据文件（归档时按扩展名判进来的）。
UPDATE analytics_resources
SET structured = 1
WHERE kind = 'analysis'
  AND file_rel_path IS NOT NULL
  AND structured IS NULL;

-- ===== 出处回填：把 `promoted_from` 的两义（草稿路径 / global:<id>）拆开 =====
-- `'global:'` 是 7 个字符，实例 id 从第 8 位起。
UPDATE analytics_resources
SET origin_kind = 'global',
    origin_ref = json_object('instance', substr(promoted_from, 8))
WHERE promoted_from IS NOT NULL
  AND promoted_from LIKE 'global:%'
  AND origin_kind IS NULL;

UPDATE analytics_resources
SET origin_kind = 'scratchpad',
    origin_ref = json_object('rel', promoted_from)
WHERE promoted_from IS NOT NULL
  AND promoted_from LIKE 'scratchpad/%'
  AND origin_kind IS NULL;

-- 表名反查（G2 之后全局/项目都可能一版一张表，枚举面要从表名回到资产）
CREATE INDEX IF NOT EXISTS idx_ar_analytics_table
    ON analytics_resources (analytics_table)
    WHERE analytics_table IS NOT NULL;
