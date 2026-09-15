-- 024: 洞察规则索引（全局层）
--
-- 背景：洞察规则分三层（内置 / 全局 / 项目），三层正文都是**文件系统**上的 TOML，
-- 装配时按同名整体覆盖（内置 → 全局 → 项目）。以文件为正文的好处是可 diff、
-- 可进 git、可手工编辑；代价是**界面无从枚举、无法启停、校验错误无处展示**——
-- 解析失败此前只写 tracing::warn!，用户看到的现象是「规则莫名其妙不见了」
-- （内置规则里就有一条因此长期缺席，见 2026-09-15 的 F1 修复）。
--
-- 本表是那个代价的补位：**只存索引与状态，不存正文**。正文永远以文件为唯一真相源。
--   * enabled       —— 让内置规则也能被禁用：写一条同名抑制记录即可，
--                      不必伪造一个同名覆盖文件（后者要求逐字段照抄，极易写错）
--   * checksum      —— 正文内容哈希，增量热加载的判据（内容没变就不重解析）
--   * load_status / load_error —— 把严格 schema 的解析错误变成**可见**信息，
--                      供界面逐条展示错误原文
--
-- 分库口径：本表只承载**全局层**规则；内置层由 include_dir 编译期已知、不入库；
-- 项目层见 project_meta/019_insight_rule_index.sql（同构）。
--
-- 幂等：CREATE TABLE / INDEX IF NOT EXISTS，迁移按版本号只执行一次。

CREATE TABLE IF NOT EXISTS insight_rule_index (
    rule_id      TEXT NOT NULL,               -- 规则 meta.id
    scope        TEXT NOT NULL CHECK (scope IN ('global', 'project')),
    category     TEXT NOT NULL,               -- column / multi / table / quality
    name         TEXT NOT NULL,               -- 展示名
    version      TEXT NOT NULL,               -- 规则版本（meta.version）
    source_path  TEXT NOT NULL,               -- 规则文件路径
    checksum     TEXT NOT NULL,               -- 正文 SHA256
    enabled      INTEGER NOT NULL DEFAULT 1,  -- 0 = 禁用（内置层靠此抑制）
    load_status  TEXT NOT NULL CHECK (load_status IN ('ok', 'invalid', 'missing')),
    load_error   TEXT,                        -- 解析错误原文（load_status = 'invalid' 时）
    loaded_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (scope, rule_id)
);

CREATE INDEX IF NOT EXISTS idx_iri_category ON insight_rule_index(category);
CREATE INDEX IF NOT EXISTS idx_iri_status ON insight_rule_index(load_status);
CREATE INDEX IF NOT EXISTS idx_iri_enabled ON insight_rule_index(enabled);
