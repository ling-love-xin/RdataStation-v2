-- 迁移版本：029
-- 数据库类型：SQLite（全局库 global.db）
-- 作用：退役内核 sqlx 驱动的两条 `drivers` 行（`mysql` / `postgres`）
-- 更新时间：2026-11-12
--
-- 背景（D3/D4，见 docs/architecture/plugin/sqlx-driver-extraction-plan.md）：
--   sqlx 实现已整体剥离为 sidecar 插件（插件 id `rds.driver.sqlx`），内核每族只留一条
--   官方实现。代码里的 `mysql_driver()` / `postgres_driver()` 已删除，但 008 / 013 迁移
--   播下的两行种子还在（`driver_kind = 'native'`、`enabled = 1`）—— 连接对话框会摆出两个
--   **点不动**的驱动（注册表里没有它们），老连接也只会报「驱动未注册」而不给去向。
--
-- 处置：**删除**这两行，而不是置 `enabled = 0`。
--   * 插件激活时会按同 id 重新 upsert 一行（`driver_kind = 'sidecar'`，`enabled` 跟随插件
--     启停），所以删掉不会丢信息：装插件后行自己回来；
--   * 留一行 `enabled = 0` 会在地下拉里留一个「已停用」的幽灵实现，且它的 `enabled`
--     语义归用户、插件的启停语义归插件，两套语义会打架（`plugin_driver_store` 模块头
--     写过这条边界）。
--
-- `data_source_types` 的 `mysql` / `postgresql` **族行必须保留**：它们是驱动行的外键，
-- 也是连接对话框里「数据库类型」那一列的唯一来源。本迁移不碰它。
--
-- 为什么带 `driver_kind = 'native'` 条件：插件已装过的机器上，这两行可能已经被 upsert 成
-- `sidecar`。那时的行是**活的插件行**，删除它等于把用户的驱动配置抹掉 —— 只在它还是内核
-- 种子行（`native`）时才删。

DELETE FROM drivers
 WHERE id IN ('mysql', 'postgres')
   AND driver_kind = 'native';
