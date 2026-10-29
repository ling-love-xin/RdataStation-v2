-- 类别级扫描完成标记（2026-10-08）
--
-- 背景：routines / sequences / triggers 三类「空结果」在读取侧被当作未命中
-- （cache.rs 的 `routines(sid)` 对空行集返回 None），`put_*` 对空 vec 又不落任何
-- 记录——于是空类别（如 rds_nav_probe_a 的 routines/sequences/triggers 全空）每次
-- 展开都回源实时查询。views 有 `is_schema_complete` 守卫（表/视图那一趟扫描的完成
-- 标记），但该标记只证明表/视图扫描过，不能用于其余三类（会把「从未查过」误判为空）。
--
-- 本表记录「某 schema 的某类别已完成一次完整回源」：put_* 成功后置标记（**空也算**），
-- 读取侧先查标记——已扫描且表内无行 = 合法空，直接返回空不回源；未扫描才回源。
--
-- 失效语义：刷新（fresh）时 put_* 会重写并再次 mark；schema 重建后 schema_id 变化，
-- 旧标记随 CASCADE 清除。与 L2 既有 TTL/失效策略一致，不引入新机制。
CREATE TABLE IF NOT EXISTS category_done (
    schema_id INTEGER NOT NULL REFERENCES schemata(id) ON DELETE CASCADE,
    category  TEXT    NOT NULL CHECK (category IN ('routines', 'sequences', 'triggers')),
    done_at   INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (schema_id, category)
);
