# 驱动与插件：现状梳理

> 目的：把「驱动设计」「插件设计」两条线还原成可核对的现状，标清已落地、半落地、只有表没有写入点的部分。
> 事实来源：`feat/federation-entry` 当前工作区代码 + `global.db` / `project.db` 真机数据 + `.rds/logs` 运行日志。
> 本文只描述现状，不含方案取舍；驱动 id 的具体改法见同目录 `driver-id-design.md`。

## 零 一句话结论

驱动侧的设计是**完整**的：族（`target_database`）与实现（`id`）从一开始就是两个字段，
内置 4 个驱动全部显式声明了族。
断在**插件侧**：`ContributesDriver` 没有「族」这个输入，于是宿主把族硬赋成插件 id，
现在能跑，是因为三个已装插件的 id 恰好等于族名。

## 一 驱动设计：五层

### 1.1 族目录（有限集合）

表 `data_source_types`，真机 **10 行**：

| 族 id | 名称 | 分类 | enabled |
| --- | --- | --- | --- |
| clickhouse | ClickHouse | analytics | 1 |
| duckdb | DuckDB | analytics | 1 |
| mariadb | MariaDB | relational | 1 |
| mongodb | MongoDB | nosql | 0 |
| mssql | SQL Server | relational | 1 |
| mysql | MySQL | relational | 1 |
| oracle | Oracle | relational | 1 |
| postgresql | PostgreSQL | relational | 1 |
| redis | Redis | nosql | 0 |
| sqlite | SQLite | file-based | 1 |

族的三个真实消费方：

- 品牌图标 `db_icon_of(type_id, category)`（`crates/workbench_shell/src/db_icons.rs:81`），图标文件 `icons/db/<type_id>.svg`
- DuckDB 密钥类型（`crates/connection_dialog/src/services/secret_integration.rs:110`）
- **按族列驱动** `list_drivers_by_type(type_id)`（`crates/connection_dialog/src/services/data_source_service.rs:154`）—— 这就是「同族多实现」在 UI 上成立的基础

族解析的唯一入口是 `store::persistence::driver_catalog::type_id_of(driver_id)`（`crates/store/src/persistence/driver_catalog.rs:106`），
从 `drivers` 表按驱动 id 反查 `type_id`。注释写明：**「插件驱动的 id 与库族无关，不要按字符串前缀猜」**。

### 1.2 驱动声明（`DriverDescriptor`）

`crates/driver/src/registry/descriptors.rs:149`，20 余个字段，关键的是两个：

```rust
pub struct DriverDescriptor {
    pub id: String,                       // 注册键，进 DriverRegistry
    pub target_database: Option<String>,  // 族；注释原文：非 Native 驱动标记连接的数据库类型，如 JDBC 驱动可为 "mysql"
    pub driver_kind: DriverKind,          // Native / JDBC / ODBC / WASM / Sidecar
    pub category: String,                 // relational / file-based / nosql / analytics
    // …default_port / url_template / fields / capabilities / supported_auth_types…
}
```

**设计意图在这里是清楚的**：`id` 与 `target_database` 分开，同一族可以有多个 `id`。

### 1.3 内置清单（4 个，全部显式声明族）

| id | 名称 | 族 | 位置 |
| --- | --- | --- | --- |
| `sqlite` | SQLite (rusqlite) | `sqlite` | `descriptors.rs:404` |
| `duckdb` | DuckDB (duckdb-rs) | `duckdb` | `descriptors.rs:452` |
| `mysql_native` | MySQL (Official) | `mysql` | `descriptors.rs:516` |
| `postgres_native` | PostgreSQL (Official) | `postgresql` | `descriptors.rs:596` |

**4/4 都写了 `.with_target_database(...)`。** 这是「族/实现分离」在设计上成立的直接证据。

### 1.4 注册表（进程内，只按 id 索引）

`crates/driver/src/registry/mod.rs`：

```rust
static DRIVER_REGISTRY: OnceLock<RwLock<HashMap<String, Arc<dyn DriverFactory>>>
```

- `register_by_factory(id, factory)`（`:121`）就是 `reg.insert(id, factory)`
- `get(id)`（`:143`）按 id 精确匹配，无别名、无族回退
- **没有族索引**：拿不到 `get_by_family("mysql")`

### 1.5 建连与落库

- 生产入口 `ConnectionService::connect_with_type`；配置由 `DriverConnectionConfig::new(input.db_type)`（`crates/connection_dialog/src/services/connection_service/runtime.rs:900`，重连在 `:473`）
- 取工厂：`DriverRegistry::get(config.driver)`；失败即 `DriverNotFound`（`crates/engine/src/connection_manager.rs:201`）
- 测试/工具入口 `create_connection_with_registry`（`connection_manager.rs:183`）

`drivers` 表真机 **7 行**：4 个 native（2026-09-10 建）+ 3 个 sidecar（2026-11-15 由插件行同步写入）：

| id | 族 | 名称 | kind |
| --- | --- | --- | --- |
| duckdb | duckdb | DuckDB (duckdb-rs) | native |
| mssql | mssql | SQL Server | sidecar |
| mysql_native | mysql | MySQL (Official) | native |
| oracle | oracle | Oracle Database | sidecar |
| postgres_native | postgresql | PostgreSQL (Official) | native |
| postgresql | postgresql | PostgreSQL | sidecar |
| sqlite | sqlite | SQLite (rusqlite) | native |

**注意 `postgresql` 行**：它是 `rds.driver.postgres-py` 声明的驱动，族恰好等于 `postgresql`——对；但它的 id 也是 `postgresql`，与族同名，这正是巧合的特征。

## 二 插件设计：六层

### 2.1 清单（`PluginManifest`，8 段）

`crates/plugin/src/manifest.rs:9`：

| 段 | 内容 |
| --- | --- |
| `[plugin]` | id / name / version / publisher / description / icon |
| `[capabilities]` | 能力面（默认拒绝） |
| `[permissions]` | 权限，四轨 |
| `[[contributes]]` | 贡献点：`drivers` / 未来别的命名空间 |
| `[dependencies]` | 依赖（`id@version`） |
| `[backend]` | **怎么跑**：`kind`（sidecar/wasm/frontend）/ `executable` / `transport` / `runtime` / `interpreter_args` / `platforms` / `max_instances` |
| `api` | 能力面命名空间（如 `"database"`），只认小写字母/数字/连字符 |
| `methods` | sidecar 轨声明的 RPC 方法清单，拼错即安装期拒绝 |

### 2.2 形态三种

| 形态 | 运行时 | 当前状态 |
| --- | --- | --- |
| `sidecar` | 子进程 + stdio 帧（`[u32 total_len][u8 kind][payload]`），JSON-RPC 2.0 + Arrow IPC | 已落地，3 个已装插件全走这条 |
| `wasm` | Extism 进程内沙箱 | 已落地，但 **host function 面一个都不提供**（按 P3 决策待重建） |
| `frontend` | 进程内，无独立进程 | 语义仍不太符（历史欠债） |

### 2.3 生命周期（`LoadStatus` 七态）

`crates/plugin/src/manager.rs:274`：`Unloaded` / `Loading` / `Loaded` / `Activating` / `Active` / `LoadFailed(String)` / `ActivationFailed(String)`

五个动作：

| 动作 | 入口 | 落点 |
| --- | --- | --- |
| 发现 | `scan_and_diff_plugins`（`:1567`） | 目录扫描 vs 快照 diff |
| 加载 | `load_plugin_from_dir`（`:778`） | 校验 id / 内置碰撞 / 目录名一致 / 完整性 / 贡献点冲突 → 写 `plugin_index` + `loaded_plugins` |
| 激活 | `activate_plugin`（`:1098`） | 建进程、`register_sidecar_drivers`（`sidecar/factory.rs:296`）、emit `activated`（`:1011`） |
| 停用 | `deactivate_plugin`（`:1109`） | 停进程、unregister、emit `deactivated`（`:1163`） |
| 卸载 | `unload_plugin`（`:1292`） | 删 index + loaded + 目录 |

### 2.4 六处事实源（同一条状态的六个副本）

| 事实源 | 载体 | 谁写 |
| --- | --- | --- |
| 磁盘 | `.rds/plugins/<id>/plugin.toml` | 安装器 / 用户拷贝 |
| 插件登记 | `global.db` `plugins`（3 行） | `register_manifest` |
| 驱动声明 | `global.db` `drivers`（7 行，含 3 行 sidecar） | `resync_driver_rows` |
| 运行时工厂 | `DriverRegistry`（内存） | 仅在**激活**时 `register_sidecar_drivers` |
| 形态索引 | `plugin_index`（内存 `HashMap<id, PluginKind>`） | `load_plugin_from_dir` / `load_plugin` |
| 已加载清单 | `loaded_plugins`（内存 `HashMap<id, LoadedPlugin>`） | `load_plugin_from_dir` |

**关键不对称**：`drivers` 表有行 ≠ `DriverRegistry` 有实现。停用插件会把工厂从注册表移除，但 `drivers` 表的行保留（只翻 `enabled`）。

### 2.5 热插拔

- `RUNTIME_TICK = 5s`（`crates/plugin/src/sidecar/runtime.rs:39`），`tick_once`（`:59`）先扫插件目录
- 四类 diff 分级：`added` 自动 load + activate（`manager.rs:1648` / `:1724`）；`reloaded`（载荷变）自动重启；`changed`（清单变）**只置 `needs_reload`，不热生效**；`removed` 自动 unload（`:1740`）
- 已实测：`.rds/logs/app.2026-11-10` 有一段 `插件热插拔扫描：目录有变化 added=["rds.driver.oracle-rust", "rds.driver.mssql-rust", "rds.driver.postgres-py"]`，说明机制真跑过

### 2.6 UI 落点

- 中间栏「插件管理」与左 Dock「插件」**共用同一个 `PluginPanel` 实体**：`plugin_manager/mod.rs:84` 构造，`:106` 的 `snapshot()` 委托 `panel.plugins_snapshot()`
- 面板取数：`panel.rs:117 refresh_plugins` → `get_plugin_manager()` → `list_plugins()`
- 刷新触发：`new` 里拉一次（`:50`）+ 订阅插件事件（`:51`）；事件白名单 `pure.rs:67` 只排除 `Custom`
- `list_plugins`（`manager.rs:2087`）遍历 `plugin_index`，**只 push `loaded_plugins` 里也有的**（`:2148-2191`），再补 `failed_loads` 里没加载成功的行

## 三 两者的接缝：四处，全部断在「族」上

| 位置 | 今天的行为 | 应有行为 |
| --- | --- | --- |
| `manifest.rs:467` `ContributesDriver` | **没有 `type_id` 字段** | 有可选 `type_id`，省略回落为 id |
| `driver_decl.rs:175` `derive_row` | `type_id: driver.id.clone()` | 取清单声明的族 |
| `sidecar/factory.rs:342` `descriptor_for` | `new_external(driver.id, …, driver.id)` | 族参数取清单声明的族 |
| `driver_decl.rs:217` `ensure_plugin_family` | 族不存在就 `OR IGNORE` 建行 | 保留，但此后只会收到真实族名 |

**后果**（真机可验证）：装 `rds.driver.sqlx`（声明 `mysql` + `postgres`）会让族目录多出一行 `postgres`，
与 `postgresql` 并存；而 `rds.driver.oracle`（JDBC）与 `oracle-rust` 撞同一个 id `oracle`，被贡献点冲突拒绝。

**2026-11-15 更新**：上表四处**均已修复**（P1~P4 落地）——`ContributesDriver` 有了 `type_id`，
两个落点都走 `effective_type_id()`，`ensure_plugin_family` 保留并在建族时留痕。

第五处接缝当时没列出来：**归属**。`drivers` 表只有 `driver_kind` 区分内置 / 插件，
「哪一行属于哪个插件」只存在于运行时索引或 `plugins.manifest_json`（反查要解析 JSON，没人这么做）。
迁移 032 补了 `plugin_code`：

- `NULL` = 内置（`declaration.rs` 播种，永不写这一列）；非空 = 插件 code（`plugins.code`）
- 写入点唯一：`upsert_plugin_driver_row`；`derive_row` 填 `manifest.plugin.id`
- 对话框驱动下拉显示 `短名 · 内置` / `短名 · 插件短码`（`Official · 内置` vs `PostgreSQL · postgres-py`）

效果：`postgresql` 族下插件版与内置版从「两个看不懂的名字」变成可区分的两项。

插件卡片也标同一个短码（`Oracle Database (Rust) · oracle-rust`），详情面板仍给完整驱动 id
（`oracle`）；短码实现只有一份——`shared::utils::string::plugin_short_code`，
插件页与连接对话框都调它，不允许各自实现。

## 四 项目侧：表在，写入点没有

| 表 | 行数 | 说明 |
| --- | --- | --- |
| `project_used_plugins` | 0 | 「项目引用某个全局插件」 |
| `project_plugin_config` | 0 | 项目级插件配置 |
| `project_drivers` | 0 | 项目级驱动覆盖 |

三张表结构齐备、**零写入点**。所以「插件归全局、项目只引用」目前只是库结构约定，
UI 上没有任何入口，也没有任何代码往里写。

## 五 进度盘点

### 已经能跑

- sidecar 协议（帧 / JSON-RPC / Arrow、五条硬约定、六条一致性测试）
- 安装 / 校验 / 目录布局（`checksums.json`、`layout::validate_dir_name`）
- 生命周期五动作 + 状态持久化
- 依赖解析与反向依赖（卸载被依赖插件会拒）
- 热插拔四类 diff
- 驱动行同步（插件声明 → `drivers` 表）
- 三个 Rust / Python 驱动插件真机可用（mssql / oracle / postgresql）
- 清单 `type_id`：族与实现分层，取值收敛到 `ContributesDriver::effective_type_id()`（P1/P2 已落地）
- 建连族回退：实现被停用/卸载时由同族内置实现接住，5 处接线（P3 已落地，**真机未验**）
- 驱动归属：`drivers.plugin_code`（迁移 032）标出「哪个插件声明了它」，对话框下拉按来源区分（已落地，**真机未验**）
- 插件详情「入口」：从清单 `[backend].executable` → `capabilities.wasm.entry` → `capabilities.frontend.entry` 取值；此前四处构造 `PluginInfo` 全写死空串，界面永远显示 `—`（已落地，**真机未验**）

### 半落地

| 项 | 状态 |
| --- | --- |
| 事件总线 | 9 个 emit 函数齐备，但 `emit_plugin_loaded` **零调用点**；`activated` 只在 `activate_plugin` 里发 |
| 插件面板 | 代码链路无「永空」点，但 10:47 截图实测全 0；三种可能均需一次运行日志区分 |
| 驱动「族」 | 内置侧 4/4 正确；插件侧原先 0/3 显式（靠 id 巧合）——**已补 `type_id`，并加了按族回退的建连路径** |
| 缓存身份指纹 | 已按「族」归一；新实现走注入的注册表解析器，历史映射永不被改写（P4 已落地） |

### 只有结构、没有链路

- 项目级插件引用（三表全 0 行）
- `project_db` 每次 `toggle_nav_node` 重建（含 15 条 migration）
- DuckDB 扩展管理（`duckdb_extensions` / `extension_operations` 均 0 行）
- `verify.rs`（未编译验证、界面无「校验」按钮）
- PG 列解码缺口（`postgres_native.rs:435-459`）

### 明确未做

- 清单 `type_id` + 建连族回退（`driver-id-design.md` 的 P1~P3）
- 面板重建后的重新订阅
- M9 一批口径问题：`PluginKind::Frontend` 语义、停用被依赖插件、三个错误码差异、批量停用、`PluginInfo.is_enabled` 是否入库

## 六 动代码的优先级

| 序 | 改动 | 为什么先做 | 风险 |
| --- | --- | --- | --- |
| 1 | `ContributesDriver` 加 `type_id`，两处落点改用它 | 纯增量、向后兼容（不写即回落为 id），现有 3 包行为零变化 | 低，可用「`drivers` 表 7 行逐字段不变」验证 |
| 2 | 建连三级回退（实现 → 族 → 内置） | 根治「插件没装就打不开库」，不依赖改数据 | 中，需要单测覆盖四种组合 |
| 3 | 面板订阅/回写加固 | F3 的唯一收口 | 低，但要先有一次运行日志确认方向 |
| 4 | 项目级插件引用链路 | 让「项目只引用」从表结构变成真实功能 | 中，涉及项目库写入 |

前两项不依赖运行期日志，可以立刻动；第三项需要一次启动取证；第四项要等前三项稳定。

## 七 代码索引

| 主题 | 文件 |
| --- | --- |
| 驱动声明 / 内置清单 | `crates/driver/src/registry/descriptors.rs` |
| 驱动注册表 | `crates/driver/src/registry/mod.rs` |
| 驱动落库 / 族校验 | `crates/driver/src/declaration.rs` |
| 族解析入口 | `crates/store/src/persistence/driver_catalog.rs` |
| 插件清单 | `crates/plugin/src/manifest.rs` |
| 插件生命周期 | `crates/plugin/src/manager.rs` |
| 插件驱动声明 | `crates/plugin/src/driver_decl.rs` |
| sidecar 工厂注册 | `crates/plugin/src/sidecar/factory.rs` |
| 热插拔 | `crates/plugin/src/sidecar/runtime.rs` |
| 面板 | `crates/workbench/src/panels/left_tabs/plugin_tab/` |
| 管理页 | `crates/workbench/src/panels/plugin_manager/` |
