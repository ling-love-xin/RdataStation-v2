-- 035: 全局侧把历史 `analysis` 串归一为 `file`（镜像 032）
--
-- 全局行的 `kind = 'analysis'` 有两种来历：
--   * 「数据文件映射成表」的升级（`PromoteKind::DataFile`）→ 已由 034 回填
--     `analytics_table = name`，是表型，**不动**；
--   * 「脚本内容表」的升级（`PromoteKind::Script`）→ 本体是内容表、分发时重建为
--     文件（`file_rel_path` 非空），本质是文件档，归一为 `file`。
--
-- 判据与项目库一致：`analytics_table` 为空即文件档。

UPDATE analytics_resources
SET kind = 'file'
WHERE kind = 'analysis'
  AND (analytics_table IS NULL OR analytics_table = '');
