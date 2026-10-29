-- 迁移版本：011
-- 数据库类型：SQLite
-- 作用：metadata_fts 改为「存内容 + trigram 分词」
-- 更新时间：2026-09-19
--
-- 为什么必须改（2026-09-19 实测，探针见 metadata_cache.rs 的 FTS 测试）：
--   1) 旧表 `content=''` 是 **contentless**：MATCH 能命中，但 SELECT 出来的
--      search_type / schema_name / object_name / parent_name 全是 NULL
--      （读取时报 `Invalid column type Null at index: 0`）——拿不到「是哪个对象命中」，
--      snippet() 也无从生成；整条 FTS 读路径其实不可用。
--   2) 默认 unicode61 分词器把**连续中文当一个 token**（「含渠道与优惠信息」是一个词），
--      于是注释里搜「渠道」0 命中——中文注释等于搜不了。
--      实测 trigram：`含渠道`（3 字）命中该注释；2 字查询因不足一个 trigram 无命中。
--
-- 代价与口径：
--   * 存内容 + trigram 的索引体积约为文本的 3 倍量级（元数据文本本就短，可接受）；
--   * **查询至少 3 个字符**（不足一个 trigram 必然无命中）——UI 侧的门槛按 3 字；
--   * 缓存库是**派生数据**，drop 重建安全：重建后由
--     `MetadataCacheOps::rebuild_fts_schema`（内省一个 schema 时同批执行）回填。

DROP TABLE IF EXISTS metadata_fts;

CREATE VIRTUAL TABLE metadata_fts USING fts5(
    search_type,
    schema_name,
    object_name,
    parent_name,
    search_content,
    tokenize='trigram'
);

UPDATE cache_version SET version = 11, updated_at = strftime('%s', 'now') WHERE id = 1;

INSERT INTO cache_migration_history (from_version, to_version, migrated_at, reason, duration_ms, success)
SELECT
    10,
    11,
    strftime('%s', 'now'),
    'V11: metadata_fts 改为存内容 + trigram（snippet 可用；中文注释需 ≥3 字查询）',
    0,
    1
WHERE NOT EXISTS (
    SELECT 1 FROM cache_migration_history WHERE to_version = 11
);
