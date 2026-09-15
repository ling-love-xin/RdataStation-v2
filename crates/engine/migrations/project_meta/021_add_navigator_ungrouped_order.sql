-- 迁移版本：021
-- 数据库类型：SQLite
-- 作用：数据源导航模块——「未分组」容器的连接手动排序
-- 更新时间：2026-09-16

-- 「未分组」不是真实分组（`connection_groups` 里没有它的行），它的成员是**推导**出来的
-- （不属于任何分组），所以顺序无处安放；单开一张表只存这个容器的顺序。
-- 无 `sort_order` 的行 = 未手动排序，回退到名称升序。永不自动删除。
CREATE TABLE IF NOT EXISTS navigator_ungrouped_order (
    connection_id TEXT PRIMARY KEY,
    sort_order    INTEGER NOT NULL DEFAULT 0,
    updated_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
