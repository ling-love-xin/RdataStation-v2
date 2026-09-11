-- 021: 暂存草稿的数据库类型与驱动标识
--
-- 背景（M3 C5+）：Header 驱动下拉改为「左侧选类型 + 右侧选驱动实现（短名）」后，
-- 暂存条目需显示缩小的数据库类型 UI（emoji），并在恢复时精确定位驱动：
--   - type_id：数据库类型 id（data_source_types.id，如 mysql），条目类型徽标与驱动过滤依据；
--   - driver_id：驱动 id（drivers.id，如 mysql_native），落库/回读最稳定的驱动标识；
--   - driver_name 保留为下拉显示短名（如 sqlx），兼容旧行的完整名（MySQL (sqlx)）。
--
-- 幂等：只加列、带默认值；迁移按版本号只执行一次。

ALTER TABLE connection_drafts ADD COLUMN type_id TEXT NOT NULL DEFAULT '';
ALTER TABLE connection_drafts ADD COLUMN driver_id TEXT NOT NULL DEFAULT '';
