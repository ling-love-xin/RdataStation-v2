-- 全局一版一张表（G2，2026-11-16）：历史全局行的 `analytics_table` 回填成资产名。
--
-- 为什么是资产名：迁移之前建表时表名就是 `name`（例如 `orders`），所以回填成 name
-- 恰好是真实表名，读取面（分发 / 打开 / 列表档位判定）不需要额外分支。
-- 此后新升级的行由代码写 `res_<描述>_<时间戳>`，这张表不能覆盖它们（带 WHERE 守护）。
--
-- 幂等：重复执行只是把仍是空值的行再填一次，不会覆盖已写入的新表名。

UPDATE analytics_resources
SET analytics_table = name
WHERE scope = 'global'
  AND (analytics_table IS NULL OR analytics_table = '');
