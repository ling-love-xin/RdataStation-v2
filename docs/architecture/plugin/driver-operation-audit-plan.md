# 驱动 / 插件操作留痕与可查看（方案 · 待评审）

> **一句话**：凡是会改变"系统状态"的操作（装/卸/启/停/重载/声明同步/连接因缺驱动被拒）都要留下**一行可查的历史**；凡是执行过的 SQL 都要能追到**哪个驱动的哪个连接**。
>
> **参考的既有设计**（用户点名）：`drivers` / `driver_files` / `plugins` 三张表的登记形状、`query_history` 的载荷、`connection_audit`（A1）的规划。
>
> **状态**：方案稿，待评审；**不改代码**。

---

## 1. 诉求

| # | 诉求 | 落到本方案 |
| --- | --- | --- |
| 1 | 操作都要留痕 | 新增 `driver_operations`（全局库）；生命周期事件从"内存广播"升级为"落库 + 可查" |
| 2 | 可以查看 | 插件管理页详情栏「操作记录」；历史面板增「驱动」列；能力 Tab 显示声明来源与最近变更 |
| 3 | 参考驱动系统既有设计 | 沿用 `drivers` / `driver_files` / `plugins` 的字段习惯（version / checksum / installed_at），不新造语义 |

---

## 2. 现有底座（代码实证）

| 载体 | 位置 | 已有什么 | 缺什么 |
| --- | --- | --- | --- |
| `drivers` | `store/migrations/global/008_add_data_source_module.sql:23-40` | `driver_kind` / `download_checksum` / `version` / `enabled` / `created_at` | **只有当前状态，没有历史**（改了哪次、谁改的查不到） |
| `driver_files` | 同上 `:46-56` | `file_path` / `file_size` / `checksum` / `version` / `installed_at` / `updated_at` | 同上；且**只服务文件型驱动登记**，与驱动插件无关 |
| `plugins` | `store/migrations/global/001_init.sql:128-144` | `code` / `version` / `install_path` / `is_enabled` / `installed_at` / `updated_at` | 同样只有当前状态 |
| `query_history` | `store/migrations/project_meta/001_init.sql:46`；载荷 `store/src/persistence/history_store.rs:35-63` | `sql` / `db_type` / `connection_id` / `timestamp` / `duration_ms` / `success` / `error` / `rows_aff/ret` / `channel` / `sources` / `lineage` | **没有 `driver_id`**——同族两个实现分不清 |
| 插件生命周期事件 | `plugin/src/events.rs`（`PluginEvent` + `emit_plugin_*`） | 内存 `broadcast`，供面板/管理页刷新 | **不落库**，关掉窗口即忘 |
| `connection_audit` | `docs/project-progress.md:40`（A1，未勾选） | 计划：create/edit/sync/test 审计 | 未实现 |
| 查询留痕（环境策略 ⑥） | `docs/project-progress.md:46`（未勾选） | 计划：强制记录 / 仅本地 | 未实现 |

**三处真实缺口**

1. 查询历史有 `db_type` 无 `driver_id` —— `mysql` 与 `mysql_native` 在历史里不可区分；抽出 sqlx 后这个歧义会变成常态。
2. 驱动/插件操作**没有历史**，只有终态快照。
3. 生命周期事件只进内存广播 + tracing 文本日志，用户"查看"不到。

---

## 3. 设计

### 3.1 记录模型：`driver_operations`（全局库，新增迁移）

对齐既有字段习惯（`plugins.installed_at` 那一套 `TIMESTAMP DEFAULT CURRENT_TIMESTAMP`）：

```sql
CREATE TABLE IF NOT EXISTS driver_operations (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    occurred_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    scope        TEXT NOT NULL,           -- global | project
    actor        TEXT NOT NULL,           -- ui | runtime | migration | hotplug | cli
    action       TEXT NOT NULL,           -- install/upgrade/uninstall/enable/disable/
                                          -- reload/declare_sync/missing_driver/capability_change
    plugin_id    TEXT,                    -- 插件贡献点为插件 id（内置驱动为空）
    driver_id    TEXT,                    -- 驱动 id（如 mysql / mysql_native）
    result       TEXT NOT NULL,           -- ok | fail
    reason       TEXT,                    -- 失败原因（可读文案）
    before_json  TEXT,                    -- 动作前状态（version/checksum/provider_kind/capabilities）
    after_json   TEXT,                    -- 动作后状态（同上）
    created_at   TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_driver_ops_time  ON driver_operations(occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_driver_ops_driver ON driver_operations(driver_id);
CREATE INDEX IF NOT EXISTS idx_driver_ops_action ON driver_operations(action);
```

**口径**：

- `before_json` / `after_json` 只存**状态差异相关字段**（version / checksum / provider_kind / capabilities / enabled），不存凭据、不存 URL 里的密码；
- `provider_kind` 新增字段（`builtin` / `plugin`）——抽出 sqlx 后，"这个驱动是内置还是插件提供的"必须可追溯（见 `sqlx-driver-extraction-plan.md` §6.2）；
- 项目级动作（项目引用某驱动）记 `scope = project`，并带 `project_id` 进 `before_json`（**待定**：是否单开一列）。

### 3.2 写入点清单

| 动作 | 现有代码位置 | 记录内容 |
| --- | --- | --- |
| 插件安装 / 覆盖安装 | `plugin/src/installer.rs::install_from_path`（prepare/validate/stage/rename） | `action=install`，`before/after` 版本与 checksum，失败带 `reason` |
| 启用 / 停用 | `manager.rs::enable_plugin` / `disable_plugin` | `action=enable/disable`，`after.enabled` |
| 卸载 | `manager.rs::uninstall_plugin` | `action=uninstall`，记录被保留的 `plugin-data` 与依赖拒绝原因 |
| 全量重载 / 热插拔变更 | `manager.rs::reload_plugins` / `scan_and_diff_plugins` | `action=reload`，`reason` 写"清单变化 / 目录消失" |
| 驱动声明同步 | `driver/src/declaration.rs::sync_driver_declarations` | `action=declare_sync`，逐驱动记差异（新增/更新/能力键变化） |
| 能力位变化 | 同上（`capabilities` 列变更时） | `action=capability_change`，`before/after.capabilities` |
| 缺驱动被拒 | `driver/src/missing_driver.rs` 的调用方 | `action=missing_driver`，`driver_id` + 连接 id，`reason` 指向"由插件 X 提供" |
| 连接变更（A1） | 未实现 | **不在本方案范围**，沿用 `connection_audit` 规划；本方案只保证它将来可以复用同一套查看入口 |

**写入方式**：`store` 侧提供一个薄的 `record_operation(tx, row)`，由上述入口在**同一事务**内调用（安装/卸载已有事务；声明同步是 upsert，追加一条即可）。不引入新的服务层抽象——避免"两份契约"。

> 失败也要留痕：现在多处失败只回 UI 状态行（`nav_view/actions.rs:1147` 已有注释"失败必须留痕"），操作历史是它们的落点。

### 3.3 查询留痕补 `driver_id`

`HistoryRecord`（`store/src/persistence/history_store.rs:35`）增：

```rust
/// 驱动 id（`drivers.id`，如 mysql_native）。老记录没有 → `None`。
pub driver_id: Option<String>,
```

- 写入点：`engine/src/services/sql_service.rs` 建记录时填当前连接的 `config.driver`；
- 读取兼容：JSON 手写解析按 `Option` 给 `None`（与 `channel` / `sources` / `lineage` 同一演进方式，`history_store.rs:46-62` 已写明）；
- 历史面板：加一列「驱动」，可按驱动过滤（复用 `HistoryFilter` 的 `with_db_type` 形状，新增 `with_driver_id`）。

### 3.4 查看入口（"可以查看"必须是一个真实入口，不是日志文件）

| 入口 | 内容 |
| --- | --- |
| 插件管理页 · 详情栏「操作记录」 | 最近 N 条该插件的 install/enable/disable/reload/uninstall，含时间、结果、原因；复用现有详情栏（`plugin-manager-ui.md` 的 340px 栏） |
| 驱动能力 Tab | 每个驱动行显示「来源：内置 / 插件 `<id>`」+ 最近一次 `declare_sync` 时间 |
| 右 Dock 历史面板 | 增「驱动」列 + 过滤；点一行可看 `channel/sources/lineage`（已有） |
| 属性分区 hover（置灰时） | 能力位 + 来源 + 最后同步时间（见 `driver-capability-gating-plan.md` §5） |

### 3.5 容量与保留

| 项 | 取值（初稿） | 理由 |
| --- | --- | --- |
| 保留条数 | 1000 条 / 表 | 操作类事件低频（每天几条），1000 条够回溯半年 |
| 保留期 | 90 天 | 与条数取先到者；启动时按时间清理一次 |
| 导出 | 支持导出 JSON | 用户要"留痕"通常是排障，导出比翻页更实用 |
| 脱敏 | 不记录 URL 密码段、不记录凭据 | 与 `plugin/README.md` §5 #1「不外泄凭据」一致 |

---

## 4. 与另两份方案的关系

| 方案 | 依赖本方案的哪部分 |
| --- | --- |
| `driver-capability-gating-plan.md` | 能力位来源与变更时间（§3.2 `capability_change` + `declare_sync`） |
| `sqlx-driver-extraction-plan.md` | 安装/启停/卸载/缺驱动的历史 + `provider_kind` 变更（内置 → 插件） |

---

## 5. 验收清单

| # | 断言 | 落点 |
| --- | --- | --- |
| A-1 | 安装一个插件后，`driver_operations` 有 1 行 `install`，含版本与 checksum | `crates/plugin/tests/lifecycle.rs` 扩 |
| A-2 | 安装失败（坏清单 / 依赖缺失）也留一行 `install + fail + reason` | 同上 |
| A-3 | 启停/卸载/重载各留一行，`scope` 与 `actor` 正确 | 同上 |
| A-4 | 声明同步产生 `declare_sync`，能力键变化额外产 `capability_change` | `crates/driver` 单测 |
| A-5 | 老连接因缺驱动被打开时留 `missing_driver` 且文案指向插件 | `missing_driver` 用例 |
| A-6 | `HistoryRecord` 带 `driver_id`；老 JSON 读回为 `None` 不报错 | `history_store` 单测 |
| A-7 | 历史面板按驱动过滤生效 | `crates/workbench` UI 契约测试 |
| A-8 | 管理页「操作记录」可翻页、可按结果过滤 | `plugin_manager` 页面测试 |

---

## 6. 待定

- Q1 `project_id` 是否单列（现在塞 `before_json`）。
- Q2 `connection_audit`（A1）与本表是否合并成一张 `operations`（类型字段区分）——合并的好处是"一个入口看全部操作"，坏处是迁移历史计划。
- Q3 查询留痕的"强制记录 / 仅本地"（环境策略 ⑥）落到 `driver_operations` 还是继续走 `query_history`；建议**继续走 `query_history`**，本表只放状态变更类操作，避免一张表两种量级。
