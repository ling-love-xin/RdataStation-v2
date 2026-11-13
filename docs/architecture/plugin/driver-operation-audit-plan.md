# 驱动 / 插件操作留痕与可查看（方案 · 待评审）

> **一句话**：凡是会改变"系统状态"的操作（装/卸/启/停/重载/声明同步/连接因缺驱动被拒）都要留下**一行可查的历史**；凡是执行过的 SQL 都要能追到**哪个驱动的哪个连接**。
>
> **参考的既有设计**（用户点名）：`drivers` / `driver_files` / `plugins` 三张表的登记形状、`query_history` 的载荷、`connection_audit`（A1）的规划。
>
> **状态**：D8a～D8d 已落地（地基、生命周期写入点、缺驱动留痕、四处查看入口，见 §7）；剩 D8e（导出 JSON）待做。

> **D8a 落地了什么（2026-11-13）**
>
> | 项 | 落点 | 验证 |
> | --- | --- | --- |
> | `driver_operations` 表 | `crates/store/migrations/global/030_driver_operations.sql`（含三支索引；`project_id` 按 Q1 定案**单列**） | 迁移由 `include_dir!` 自动装载 |
> | 读写 API | `crates/store/src/persistence/driver_operation_store.rs`：`record_operation` / `list_operations` / `latest_operation` / `prune_operations` / `table_exists` | 该模块 7 条单测 |
> | 声明同步留痕 | `driver/src/declaration.rs::upsert_all`：只在**真变了**或首次出现时写 `declare_sync`；能力键变化额外写 `capability_change`（带 before/after 的能力位） | `declare_sync_is_recorded_only_when_something_changed`、`capability_change_is_recorded_separately` |
> | 保留策略 | `sync_driver_declarations` 启动时 `prune_operations(1000, 90)` | `prune_keeps_the_newest_rows` |
> | 查询留痕补 `driver_id` | `HistoryRecord` / `SqlHistoryRecord` / `SqlHistoryEntry` 三处 + 手写 JSON 读写；引擎 `record_history` 从连接信息取 | `the_driver_id_round_trips`（含老 JSON 读回 `None`） |
>
> 口径修正（实测得出，写进代码注释）：**首次出现不产 `capability_change`** —— 那条
> `declare_sync` 的 `after_json` 已含全部能力位；否则首装会一个驱动写两行。
> 「幂等」也不再要求两条 `DeclarationSync` 完全相等（`recorded` 首跑与二跑按设计不同）。


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

- Q1 `project_id` 是否单列 → **已定案（D8a）：单列**（迁移 030 里就是 `project_id TEXT`）。
- Q2 `connection_audit`（A1）与本表是否合并成一张 `operations`（类型字段区分）——合并的好处是"一个入口看全部操作"，坏处是迁移历史计划。
- Q3 查询留痕的"强制记录 / 仅本地"（环境策略 ⑥）落到 `driver_operations` 还是继续走 `query_history`；建议**继续走 `query_history`**，本表只放状态变更类操作，避免一张表两种量级。

---

## 7. 落地进度

| 刀 | 内容 | 状态 |
| --- | --- | --- |
| D8a | 迁移 030 + store 读写 API + 声明同步/能力位留痕 + 保留策略 + 查询历史 `driver_id` | ✅ 已落地（见文首表） |
| D8b | 插件生命周期写入点：`install_from_path` / `enable_plugin` / `disable_plugin` / `uninstall_plugin` / `reload_plugins`（失败也要一行 `fail` + `reason`） | ✅ 已落地（见下） |
| D8c | 缺驱动被拒留痕（`action=missing_driver`，`reason` 指向「由插件 X 提供」） | ✅ 已落地（见下） |
| D8d | 查看入口：插件管理页「操作记录」栏、驱动能力 Tab「来源 + 最近同步」、历史面板「驱动」列与过滤 | ✅ 三段均已落地（见下） |
| D8e | `driver_operations` 导出 JSON（排障用，方案 §3.5） | 待做 |

**D8b 落地细节（2026-11-13）**

| 项 | 落点 |
| --- | --- |
| 留痕辅助 | `crates/plugin/src/operation_audit.rs`：`record(op)` 取 `global.db` 连接、校验表在不在、写入；**best-effort**——写失败只 `warn`，不把已成功的操作变失败。`record_result(...)` 把 `Ok/Err` 直接翻成 `result=ok/fail`（`Err` 的原因进 `reason`） |
| 安装 | `installer::install_from_path` 拆 `install_from_path_inner`：成功记 `install + ok`（`after_json` 带 code / version）；失败记 `install + fail`，标识用**包文件名**（失败时可能连清单都没解出来） |
| 启停 / 卸载 | `manager::{enable,disable,uninstall}_plugin` 各拆 `_inner`：成功与拒绝（被依赖、坏包、依赖没就绪）都留一行 |
| 重载 | `manager::reload_plugins`（拆 `_inner`）：`reason` 写「新增 x / 消失 x / 清单变化 x / 自动重启 x」的计数 |
| `actor` 口径 | 用户从界面点的（装/启/停/卸/重载）记 `ui`；热插拔扫描循环**不记**（每 tick 会刷噪声） |

**验证**：`rds-plugin` lib **247 passed**，其中
`operation_audit::tests::recorded_operations_land_in_the_table` 真起一次 `global.db`
（顺带把迁移 030 走一遍）后读回「启用 ok + 停用 fail 且原因可读」两行。

> ⚠️ 环境注记（已消解）：本刀验证时工作树里 `crates/plugin/src/checksums.rs` 有并行线
> **未完成**的改动（`walk_files` 改了签名、`checksums.rs:220` 的调用点未同步，整个 crate
> 编不过）。我**未改**那处代码，而是把它原样备份到 `.rds/tmp/d8b-checksums.wip.rs`
> （sha256 前后一致），临时还原 HEAD 版本跑编译与测试，跑完拷回。随后并行线自己把调用点
> 补完（`a4bc70d8`），在**真实工作树**上重跑 `rds-plugin` lib 已是 **248 passed**——
> 外包 `checksums.rs` 已不再是本刀的干扰项。

对应验收项：A-1（安装留痕）、A-2（安装失败留痕）、A-3（启停/卸载/重载留痕）由
D8b 的代码路径覆盖，自动化断言目前只有上面那条读回用例；A-5 由 D8c 覆盖；
A-7（历史面板驱动列/过滤）、A-8（来源 + 最近同步）已由 D8d 第 2、3 段闭。

**D8c 落地细节（2026-11-13）**

| 项 | 落点 |
| --- | --- |
| 留痕辅助 | `crates/engine/src/services/driver_audit.rs`：`record_missing_driver(driver_id, reason)`，与 D8a/D8b 同口径（best-effort、表不在只告警、`actor=runtime`） |
| 写入点 | `ConnectionManager::create_connection_with_registry`（新建/首次连接）与 `reconnect_connection`（打开已有连接、断线重连）。两处原来都是 `ok_or_else`，改成 `match` 后**先留痕再返回**原错误——错误本身一字不改，调用方行为不变 |
| `reason` | `missing_driver_reason(driver_id)`：点名驱动 id + 说清「内核只内置 mysql_native / postgres_native / sqlite / duckdb，该驱动若由插件提供请到插件管理安装并启用」 |
| 未做（取舍） | **没有**写出具体 provider 插件 id：引擎进程看不到插件声明，`drivers` 表也没有 provider 列（D4 的插件驱动行只带驱动自身信息）。要写「由 rds.driver.sqlx 提供」得让引擎去读插件清单——那是复制插件侧解析逻辑，不值当。D8d 的插件管理页本来就有清单，映射放那儿做 |
| 不记的地方 | `scan_and_diff_plugins` 之类的每 tick 循环、以及在一次请求里会重复命中的内部路径一律不记（噪声 > 价值） |

**验证**：`rds-engine` lib **220 passed**（+1：`driver_audit::tests::missing_driver_record_is_best_effort`）。

对应验收项：A-5（缺驱动被拒有留痕）代码路径覆盖；A-7、A-8（查看入口）仍未覆盖。

**D8d 第 1 段落地（2026-11-13）：插件管理页「操作记录」栏**

| 项 | 落点 |
| --- | --- |
| 数据 | `crates/workbench/src/panels/plugin_manager/audit.rs`：`read_operations(limit)` 读最近 `OPERATION_LOG_LIMIT = 50` 条，走 `global.db` 连接池（后台线程：优先借插件运行时的 tokio handle，退化 `acquire_sync`） |
| 状态 | `PluginManagerPanel` 加 `ops: Rc<RefCell<OperationLog>>` / `show_operations` / `ops_filter`；读盘在 `cx.background_executor()`，回主线程写状态再 `notify`——render 只读 |
| 交互 | `render.rs` 在驱动链路行下加一行：`操作记录（N）` 展开按钮 + 结果档 `TabBar`（全部 / 成功 / 被拒）+ 刷新按钮；展开后是五列表格（时间 / 动作 / 结果 / 对象 / 原因） |
| 文案 | `widgets.rs` 纯函数：`operation_action_label`（未知 action 原样露出，新动作不会被藏成空白）、`operation_target`（插件优先、否则「驱动 id」）、`operation_result_label`（ok→完成、其余→被拒）、`filtered_operations`（内存过滤） |
| 「不静默」 | 表不存在 → 显示「操作记录读取失败：…（迁移 030 未跑）」，**不**呈现成「0 条」；无记录时文案区分「还没有操作记录」与「当前筛选下没有记录」 |

**验证**：`rds-workbench` lib 全绿（`plugin_manager` 子集 21 passed，含新增 3 条纯函数用例）。

> 第 2、3 段见下：历史面板「驱动」列与过滤（`acb362ff`）、驱动能力 Tab 的「来源 + 最近同步」（`6329a472`）。

**D8d 第 2 段落地（2026-11-13）：历史面板「驱动」列与按驱动过滤（A-7）**

| 项 | 落点 |
| --- | --- |
| 筛选口径 | `crates/store/src/persistence/history_store.rs`：`HistoryFilter` 加 `driver_id` + `with_driver_id`；`matches` 里 `record.driver_id == Some(id)`——**老记录（无 driver_id）不冒充任一条** |
| 数据贯通 | `crates/editor/src/support/history.rs`：`HistoryQuery` 加 `driver_id`（透传进 `HistoryFilter`）；`HistoryItem` 加 `driver_id` 与 `driver_text`（`驱动 mysql_native`），`item_from` 填值 |
| 行内展示 | `crates/editor/src/view/history.rs` 的 `history_meta`：驱动文案排在库类型之后（`MYSQL · 驱动 mysql_native`） |
| 交互 | 筛选行加「驱动：全部 / <id>」下拉（`Button + dropdown_menu`，与既有「库：…」同款）；候选来自**已载入的行**（插件驱动是用户自己装的，硬编码清单必然过期）；`set_driver_filter` 换筛即回第一页并重查 |

**验证**：`rds-store` lib 19 passed（含新增 `history_filter_branches_by_driver_implementation`：
原生 / 插件 / 老记录三条互不冒充）、`rds-editor` lib **786 passed**（含新增
`driver_implementation_reaches_the_list_item`）。

**D8d 第 3 段落地（2026-11-13，`6329a472`）：驱动能力 Tab「来源 + 最近同步」**

| 项 | 落点 |
| --- | --- |
| 数据来源 | **不另建表**：`driver_operations` 的 `action=declare_sync` 行本来就记着「谁在什么时候写的这条声明」（`plugin_id` 空 = 内核内置，有值 = 该插件贡献） |
| 读法 | `crates/connection_dialog/src/services/data_source_service.rs::driver_sources()`：`action=declare_sync` / `limit=300`（`list_operations`）；读不到返回错误，由调用方降级为空表 |
| 归并 | `helpers/capability.rs::driver_sources_from_rows`：表按时间倒序，**先到先得**——每个驱动只留最新一条；没有 `driver_id` 的行跳过 |
| 文案 | `driver_source_text` 三档：「来源：插件 `<id>` · 最近同步 `<t>`」/「来源：内核内置 · 最近同步 `<t>`」/「来源：本次启动尚未同步（重开应用或刷新插件清单后可查）」——**没记录就不编来源** |
| 缓存与注入 | `state.rs` 字段 `driver_sources`，在 `refresh_meta` 里与驱动目录**同批**读（`meta_refreshed` 每次打开入口重置 → 每开一次对话框重读一次）；`driver_source(&str)` 访问器；`render.rs` 能力 Tab 在 hint 下、chips 上一行渲染，render 只读缓存不查库 |

**验证**：`rds-connection-dialog` lib **70 passed**（+2 纯函数用例：同一驱动多行只留最新 /
无 `driver_id` 的行不参与 / 空表不造来源；三档文案逐字对齐）；全仓
`cargo check --workspace --all-targets -j 2` 绿。

**顺带补回 D3 丢掉的真机覆盖（同一批提交后另作一刀）**

D3 删 sqlx 测试时，三个 engine 真机套件的 `use` 被重指到原生驱动，但**用例体没跟着补**，
于是 `MySqlNativeDatabase` / `PostgresNativeDatabase` 变成未用 import（`cargo check` 的
三条 `unused_imports` 警告就是这么来的）——MySQL / PostgreSQL 的覆盖**静默消失**。

| 套件 | 补回的用例 |
| --- | --- |
| `read_only_flag_from_real_servers.rs` | `postgres_native_reports_read_only`（`SELECT 1` + `SHOW search_path`）、`mysql_native_reports_read_only`（`SELECT 1` + `SHOW TABLES`） |
| `result_rows_reach_the_temp_table.rs` | `postgres_native_rows_reach_the_temp_table`、`mysql_native_rows_reach_the_temp_table`（3 行进表、3 行查回） |
| `zero_row_columns_reach_query_result.rs` | `postgres_native_zero_row_keeps_columns`、`mysql_native_zero_row_keeps_columns`（0 行仍返回 `["x","y"]`） |

文件头部的覆盖表同步改成「官方原生驱动」，并注明 sqlx 两条已随插件迁出内核、不在文件内验。

**验证（真机 `192.168.3.138`，`.rds/local/test-env.ps1`）**：三套件各 4 passed
（MySQL 3306 / PG 5432 走的都是内核原生驱动；SQLite / DuckDB 因未设路径按设计跳过），
`cargo check -p rds-engine --tests` 无警告。






