-- 迁移版本：022
-- 数据库类型：SQLite
-- 作用：数据源导航模块——把「从未手动排序」的分组成员标成未排哨兵
-- 更新时间：2026-09-16

-- 背景：`connection_group_members.sort_order` 的列缺省是 0，而手动排序写的是 `0..n`，
-- 于是「没排过」（0）与「手动排在第 0 位」（也是 0）在数据上不可分，视图只能按连接 ID 兜底，
-- 拿不到「未排的按名称升序」这条规则。
--
-- 判定：一次手动排序会把该组写成 **互不相同** 的 `0..n`，所以「组内成员 sort_order 全同」
-- 等价于「从未手动排序过」（单成员组也算）。这不是猜测，是写序方式决定的。
-- 标记值 -1 = `MEMBER_ORDER_UNSET`（必须为负，见 connection_org_store.rs）。
UPDATE connection_group_members SET sort_order = -1
WHERE group_id IN (
    SELECT group_id FROM connection_group_members
    GROUP BY group_id HAVING COUNT(DISTINCT sort_order) <= 1
);
