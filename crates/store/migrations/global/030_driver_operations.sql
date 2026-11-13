-- 030 驱动 / 插件操作留痕表（D8）
--
-- 一张「系统状态变更」的历史表：驱动的装/卸/启/停/重载、驱动声明同步、能力位变化、
-- 因缺驱动被拒的连接。与 `drivers` / `plugins` / `driver_files` 的关系是
-- 「历史 ↔ 终态」：那三张表记当前是什么，这张记**什么时候、被谁、改成了什么**。
--
-- 列口径（对齐 `plugins.installed_at` 那套写法）：
--   scope       global | project
--   actor       ui | runtime | migration | hotplug | cli
--   action      install / upgrade / uninstall / enable / disable / reload /
--               declare_sync / capability_change / missing_driver
--   plugin_id   插件贡献点（内置驱动为空）
--   driver_id   驱动 id（如 mysql / mysql_native）
--   result      ok | fail
--   reason      失败原因（可读文案；成功后也可能写「为什么变」）
--   before_json / after_json  只存状态差异相关字段（version / checksum /
--               driver_kind / capabilities / enabled），**不存凭据、不存 URL 里的密码**
--   project_id  项目级动作才有（Q1 定案：单列，不塞 JSON）
CREATE TABLE IF NOT EXISTS driver_operations (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    occurred_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    scope       TEXT NOT NULL,
    actor       TEXT NOT NULL,
    action      TEXT NOT NULL,
    plugin_id   TEXT,
    driver_id   TEXT,
    project_id  TEXT,
    result      TEXT NOT NULL,
    reason      TEXT,
    before_json TEXT,
    after_json  TEXT,
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_driver_ops_time ON driver_operations(occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_driver_ops_driver ON driver_operations(driver_id);
CREATE INDEX IF NOT EXISTS idx_driver_ops_action ON driver_operations(action);
