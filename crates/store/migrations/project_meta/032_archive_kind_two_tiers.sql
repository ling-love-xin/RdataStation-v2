-- 032: 把历史 `analysis` 串归一为 `file`（kind 收口两档的库侧一步）
--
-- 本轮之前，归档数据文件时写的是 `kind = 'analysis'`：那档的本体其实一直是
-- `resources/` 下的受管文件，只是额外记了 `definition_sql` 与列结构指纹。
-- 2026-11-16 把种类收成两档（文件 / 分析库表）后，结构指纹改由「`definition_sql`
-- 有没有值」表达，不再占用一个种类值。
--
-- 为什么不删 `analysis` 这个值就够：020 的 `CHECK` 仍允许它，读面
-- （`ArchiveKind::from_db_str`）已经把它读成 `File`，所以本迁移是**幂等语义的
-- 归一**——不做也不会有功能差异，做了能让库里的枚举面与代码一致，
-- 并且让 `analytics_table IS NULL` 成为「文件档」的唯一判据。
--
-- 表型行（`analytics_table` 非空）刻意保持 `analysis`：库层 CHECK 还没扩到
-- `analytics_table`，那一刀要等事务外的重建窗口（见 031 的注释）。

UPDATE analytics_resources
SET kind = 'file'
WHERE kind = 'analysis'
  AND (analytics_table IS NULL OR analytics_table = '');
