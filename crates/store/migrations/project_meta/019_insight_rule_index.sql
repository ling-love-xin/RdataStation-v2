-- 019: 洞察规则索引（项目层）
--
-- 与 global/024_insight_rule_index.sql **同构**；差异只在数据归属：
-- 本表随项目走（`{项目}/.RSmeta/project.db`），因此项目规则可随项目版本化分发。
--
-- 为什么项目层能承载「内置规则的抑制记录」：内置规则的正文本就不可写，
-- 但「禁用某条内置规则」是**项目级**决策（甲项目要关掉相关性分析，乙项目要用），
-- 所以抑制记录落在项目库里，scope 记为 'project'、rule_id 指向内置规则的 id。
-- 装配时先载入三层正文，再用本表的 enabled = 0 把对应规则摘掉。
--
-- 幂等：CREATE TABLE / INDEX IF NOT EXISTS，迁移按版本号只执行一次。

CREATE TABLE IF NOT EXISTS insight_rule_index (
    rule_id      TEXT NOT NULL,               -- 规则 meta.id（可指向内置规则的 id）
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
