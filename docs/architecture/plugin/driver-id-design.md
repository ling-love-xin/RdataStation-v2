# 驱动 id 分层：族 / 实现 / 连接引用

> 状态：设计稿，未实施。所有行号取自 `feat/federation-entry` 当前工作区。
> 目标：让「同一个数据库族可以有多套驱动实现」这件事在清单、落库、注册、建连四处都成立，
> 且插件缺失时**不再变成"打不开库"**。

## 一 结论

1. **族（`type_id`）是有限目录，实现（`id`）是任意字符串。** 现在插件清单只能声明 `id`，
   于是宿主动手把「族」直接赋成 `id`——这是**巧合成立**，不是设计。
2. 后果一：同族两套实现必然抢同一个注册键（`oracle` 的 Rust 版与 JDBC 版）。
3. 后果二：插件 id 若不巧等于族名，会**新建一个错误的族**（`postgres` vs `postgresql`）。
4. 解法不是改名，是**清单补 `type_id`** + **连接存「族 + 实现」** + **建连回退**。

## 二 现状：三个概念，只有两个值

| 概念 | 载体 | 语义 | 现状 |
| --- | --- | --- | --- |
| 族 | `data_source_types.id` / `drivers.type_id` | 有限的数据库种类目录，UI 图标、密钥类型、分组都按它 | **预置 10 行**，不可自由造 |
| 实现 | `drivers.id` / `DriverRegistry` 的 key | 具体驱动工厂，用户在下拉里选的那个 | 内置 4 + 插件若干 |
| 连接引用 | `connections.driver` + `connections.driver_id` | 这条连接用谁连 | 两列都在，但语义重叠 |

内置侧是干净的：`DriverDescriptor::new("mysql_native", "MySQL (Official)").with_target_database("mysql")`
—— 注册键 `mysql_native`、族 `mysql`，两者显式分开。

插件侧被压成了一个值：

| 环节 | 代码 | 实际行为 |
| --- | --- | --- |
| 清单结构 | `crates/plugin/src/manifest.rs:467` | `ContributesDriver` **只有 `id`**，没有 `type_id` |
| 落库 | `crates/plugin/src/driver_decl.rs:175` | `type_id: driver.id.clone()` —— 族直接取 id |
| 注册 | `crates/plugin/src/sidecar/factory.rs:342` | `new_external(driver.id, …, driver.id)` —— 两个参数同一个值 |
| 兜底 | `crates/plugin/src/driver_decl.rs:217` | `ensure_plugin_family(conn, &row.type_id, …)`，族不存在就 `OR IGNORE` 建一行 |

`derive_row` 的注释（`driver_decl.rs:163-165`）自己写明了这是**妥协**：

> `type_id`（数据库族）取驱动 id：常见族（oracle / postgresql…）内置声明已播种，
> `ensure_plugin_family` 的 OR IGNORE 不会动它们；生僻族按驱动 id 建行

## 三 巧合成立的实证

族目录（`data_source_types`）实际 10 行：
`clickhouse` / `duckdb` / `mariadb` / `mongodb` / `mssql` / `mysql` / `oracle` / `postgresql` / `redis` / `sqlite`。
**没有 `postgres`。**

| 插件 | 驱动 id | 派生族 | 族存在 | 结果 |
| --- | --- | --- | --- | --- |
| `rds.driver.mssql-rust` | `mssql` | `mssql` | 是 | 巧合正确 |
| `rds.driver.oracle-rust` | `oracle` | `oracle` | 是 | 巧合正确 |
| `rds.driver.postgres-py` | `postgresql` | `postgresql` | 是 | 巧合正确 |
| `rds.driver.sqlx`（未装） | `postgres` | `postgres` | **否** | **会凭空建出一个 `postgres` 族**，与 `postgresql` 并存 |
| `rds.driver.oracle`（未装） | `oracle` | `oracle` | 是 | 族对，但与 rust 版**抢同一个注册键**，被贡献点冲突拒绝 |

前三个能跑，是因为作者恰好把 id 写成族名。第四个只要装上就会污染族目录。

## 四 设计

### 4.1 清单新增 `type_id`（向后兼容）

```toml
[[contributes.drivers]]
id = "oracle_rust"        # 全局唯一注册键，进 DriverRegistry
type_id = "oracle"        # 族，可与他人共享；省略时回落为 id
display_name = "Oracle (Rust 原生)"
```

`ContributesDriver` 加一个 `#[serde(default)] pub type_id: Option<String>`。省略即回落，
现有 3 个包**一行不用改**，行为不变。

### 4.2 两个落点都改用它

- `derive_row`（`driver_decl.rs:175`）：`type_id: driver.type_id.clone().unwrap_or_else(|| driver.id.clone())`
- `descriptor_for`（`factory.rs:342`）：同样取 `type_id` 作为 `target_db` 参数

`ensure_plugin_family` 保留（生僻族仍要能建），但它现在只会收到**真实族名**，
不会因为作者命名随意而造出脏族。

### 4.3 连接引用保持二元组

`connections` 已有 `driver` 与 `driver_id` 两列，不改结构，只改**语义约定**：

- `driver` 存**族**（`mysql` / `postgresql` / `oracle` …）
- `driver_id` 存**实现**（`mysql_native` / `postgres` / `oracle_rust` …）

建连查 registry 用 `driver_id`；族只用于回退与展示。

### 4.4 建连回退链（关键）

现在：`DriverConnectionConfig::new(db_type)`（`crates/connection_dialog/src/services/connection_service/runtime.rs:900`）
→ `DriverRegistry::get(config.driver)`（`registry/mod.rs:143`，按 id 精确匹配）
→ 查不到就 `DriverNotFound`（`crates/engine/src/connection_manager.rs:201`）。

改成三级：

```
1. DriverRegistry::get(实现 id)                 ← 用户明确选的那个
2. 失败 → type_id_of(实现 id) 得族              ← store/persistence/driver_catalog.rs:106
     → get_drivers_by_type(族) 取候选             ← 已存在，data_source_service.rs:154 在用
     → 候选按 (内置优先, 名称) 排序后逐个 get      ← drivers.driver_kind / is_builtin
3. 全失败 → DriverNotFound（此时才该报错）
```

**这条链不需要给 registry 加任何索引**：`DriverRegistry` 是纯 `HashMap<id, factory>`
（`registry/mod.rs:122`），按族反查直接查 `drivers` 表即可，候选 id 再去 `get`。

**这是「mysql 驱动没注册，打不开库」的根治点**：插件没装 / 被停用时，
同族的**内置实现**会接住，连接照常能开，而不是抛错。
之前把连接记录改成 `mysql_native` 是改数据绕过，这不是修链路。

### 4.5 族内排序只影响展示

同族多实现并存不是冲突，是设计意图。下拉里内置在前、插件在后，
用户选谁就存谁——**默认由排序体现，不由数据决定**。

### 4.6 归属列：从「能连上」到「看得出是谁给的」

族归一解决了「换驱动打不开库」，但没解决「两个实现看起来是一个」。
`postgresql` 族下同时有插件版 `PostgreSQL` 与内置版 `PostgreSQL (Official)`，
短名分别是 `PostgreSQL` 与 `Official`——单独看都干净，但**看不出哪个来自插件**。

`drivers` 表此前只有 `driver_kind`（`native` / `sidecar`）区分内置与插件，
「哪一行属于哪个插件」只存在于运行时索引或 `plugins.manifest_json` 里（反查要解析 JSON，
且没有任何界面这样做）。

迁移 032 加 `plugin_code`：

- `NULL` = 内置（`declaration.rs` 播种，内置 upsert 不碰这一列）
- 非空 = 插件 code（`plugins.code` = 清单 `[plugin] id`，如 `rds.driver.postgres-py`）

写入点唯一（`plugin_driver_store::upsert_plugin_driver_row`）；`derive_row` 填
`manifest.plugin.id`。读侧 `Driver` 加 `plugin_code: Option<String>`，下拉标签变为
`短名 · 内置` / `短名 · 插件短码`（`plugin_short_code` 去掉 `rds.driver.` 前缀，
完整 code 留给详情面板）。

回读兼容逐级回退：菜单标签 → 驱动 id → 完整名 → 短名（旧草稿存的是短名，仍能定位）。

**插件页也标同一个短码**：卡片上的驱动标签从只有 `display_name` 变成
`display_name · 短码`（`Oracle Database (Rust) · oracle-rust`），与下拉标签的后半段一致；
详情面板继续给完整 id（`oracle`）。短码的唯一实现落在
`shared::utils::string::plugin_short_code`——连接对话框与插件页都调它，两边各写一份就会
再次「各是各的」。

## 五 迁移路径

四阶段，每阶段可独立验证、可独立回滚。

| 阶段 | 改动 | 状态 |
| --- | --- | --- |
| P1 清单字段 | `ContributesDriver` 加 `type_id`；两处落点改用它 | **已落地** |
| P2 族归一 | 族取值收敛到 `effective_type_id()`（`trim` + 小写）；建族时留痕 | **已落地**（语义与初稿不同，见下） |
| P3 回退链 | 建连按「实现 id → 族」两级解析（`DriverRegistry::resolve_family`），5 处接线 | **已落地** |
| P4 族解析兜底 | `driver_family` 改三级：历史表 → 注入的注册表解析器 → 实现 id | **已落地**（实现与初稿不同，见下） |
| P5 归属列 | `drivers` 加 `plugin_code`（迁移 032）；下拉标来源 | **已落地** |

**P2 的语义修正**：初稿写的是「`manifest.validate` 拒绝声明了不存在族的清单」，**这条作废**，两个原因：

1. **与产品约束冲突**——社区扩展必须允许自带新族，硬拒会让它装不上。
2. **技术上做不到**——`manifest.validate()` 是纯函数、不读库，校验期判定不了族是否存在；
   把库读进校验会引入时序依赖（加载期全局库可能还没初始化）。

实际实现是：`type_id` 声明了就用（`trim` + 转小写，避免同一族在库里出现两个 id），
没声明才回落为 `id`；族不存在时 `ensure_plugin_family` 的 `INSERT OR IGNORE` 建一行，
并记一条 `插件声明了新数据库族，已在族目录登记`。**允许建族，但要让它可见。**

**P3 的实现也与初稿不同**：初稿写「`runtime.rs:900` 与 `:473` 三级回退」，实际不需要读库——
`DriverDescriptor::target_database` 本身就是族的权威字段，注册表里每个 factory 都能给出，
所以回退是**纯内存遍历**：先 `get(id)` 直查，再在同族实现里按 id 字典序取第一个。
`DriverRegistry::get` 的精确语义保持不变（`is_registered` 与 UI 判断依赖它），
只有建连路径走 `resolve_family`。

**P4 的实现**：初稿写「复用 `type_id_of` 的结论」，**不能这么做**——`type_id_of` 读 `global.db`
（磁盘 I/O），而指纹是热路径；更关键的是它返回 `postgresql`，会把 `postgres_native` 的指纹从
`postgres` 改成 `postgresql`，等于把刚修好的缓存再作废一次。

实际实现是三级（`resolve_family_with`）：

1. **历史表优先**：`mysql`/`mysql_native` → `mysql`，`postgres`/`postgres_native`/`postgresql` → `postgres`。
   这些结果已固化在磁盘指纹里，**永不改动**；
2. **注入的解析器**：宿主启动时 `install_family_resolver` 注入「实现 id → `target_database`」
   （内存查驱动注册表，不读盘）。只对**表里没有的新实现**生效——如同族的 SQLx 版因此与旧实现
   共享同一份 L2 缓存，这正是「换驱动就退回实时内省」的最后一环；
3. **兜底**：实现 id 规范化后原样返回（`sqlite`/`duckdb`/`mssql`/`oracle` 属此类）。

注入缺失（单测进程、早期启动）时行为与今天完全一致，不会退化。

**注意不一致点**：`type_id_of("postgres_native")` 返回 `postgresql`（取自 `drivers.type_id`），
而 `driver_family("postgres_native")` 返回 `postgres`（与老连接的 `driver` 列对齐）。
两者**故意不同**——指纹要跟着历史数据走，不能跟着族目录走。P4 替换时必须保住这个语义，
否则会把已经修好的缓存再一次作废。

## 六 影响面

| 文件 | 改动 |
| --- | --- |
| `crates/plugin/src/manifest.rs` | `ContributesDriver` 加字段 + 校验 |
| `crates/plugin/src/driver_decl.rs` | `derive_row` 取族方式 |
| `crates/plugin/src/sidecar/factory.rs` | `descriptor_for` 取族方式 |
| `crates/driver/src/registry/mod.rs` | `resolve_family` / `first_in_family`（族回退的唯一实现） |
| `crates/driver/src/router.rs` | 生产建连入口（`runtime.rs:1014` 调它）改用 `resolve_family` |
| `crates/engine/src/services/connection_probe.rs` | 「测试连接」改用 `resolve_family`（与真连同一条口径） |
| `crates/engine/src/connection_manager.rs` | 建连 / 跨库浏览 / 重连三处改用 `resolve_family` |
| `crates/store/src/persistence/metadata_identity.rs` | P4 才动，且必须保住指纹语义 |
| `tools/plugin-drivers/sqlx-rust-plugin/plugin.toml` | 补 `type_id = "postgresql"`（`mysql` 那项保持 `mysql`） |
| `crates/store/migrations/global/032_add_driver_plugin_code.sql` | 新增：`ALTER TABLE drivers ADD COLUMN plugin_code`（NULL = 内置） |
| `crates/store/src/persistence/plugin_driver_store.rs` | `PluginDriverRow` 加 `plugin_code`，upsert 写该列并随插件启停一起更新 |
| `crates/store/src/persistence/driver_store.rs` | `Driver` 加 `plugin_code`；三条查询（单查 / 全量 / 按类型）都带出新列 |
| `crates/plugin/src/driver_decl.rs` | `derive_row` 填 `manifest.plugin.id`（= 插件 code） |
| `crates/connection_dialog/.../helpers/mod.rs` | `driver_menu_label` / `plugin_short_code`（下拉来源标签） |
| `crates/connection_dialog/.../state.rs` | 下拉选项、默认选中、编辑回读三处改用来源标签 |
| `crates/connection_dialog/.../helpers/capability.rs` | `find_driver_by_value` 优先按菜单标签匹配，再回落 id / 全名 / 短名 |

## 七 不在本次范围

- 不改 `DriverRegistry` 的内部结构（不加族索引）
- 不改 `connections` 表结构（两列已够）
- 不碰插件面板刷新链路（F3 是独立问题）
- 不做「族内用户偏好记忆」（记住上次选的实现）——需要单独设计存储位

## 八 验证口径

**已执行**（命令与结果都是实测，不是预期值）：

| 项 | 命令 | 结果 |
| --- | --- | --- |
| 编译 | `cargo check -j 1 -p rds-plugin -p rds-store` | EXIT=0 |
| 编译 | `cargo check -j 1 -p rds-driver -p rds-engine` | EXIT=0 |
| P1 语义 | `cargo test -p rds-plugin --lib driver_decl` | 8 passed / 0 failed |
| P1+ 回归 | `cargo test -p rds-plugin --lib` | **254 passed / 0 failed** |
| P2 归一 | 同上，`declared_type_id_is_normalised_before_it_becomes_a_family_id` | passed |
| P3 回退 | `cargo test -p rds-driver --lib` | **95 passed / 0 failed** |
| P3 建连回归 | `cargo test -p rds-engine --lib` | **236 passed / 0 failed** |
| P2 建族 | `cargo test -p rds-store --lib plugin_driver_store` | 4 passed / 0 failed |
| P4 族解析 | `cargo test -p rds-store --lib metadata_identity` | **24 passed / 0 failed**（含 2 条新增） |
| P4 注入点 | `cargo check -j 1 -p rds-app` | EXIT=0 |

**未验证**（都需要真机启动，本轮没跑）：

1. `drivers` 表 7 行在 P1 后**逐字段不变**。当前只能给逻辑保证：3 个包都没写 `type_id`，
   `effective_type_id()` 回落到 `id`，与改动前取值相同。
2. 停用 `rds.driver.postgres-py` 后，`driver_id = postgresql` 的连接仍能连上（回退到
   `postgres_native`），日志出现 `驱动未注册，已按同族回退`。这是 P3 的唯一真机证据口径。
3. 装 `rds.driver.sqlx` 后族目录的变化：若清单不补 `type_id`，会新建 `postgres` 族并留痕
   （P2 之后这是**允许**的行为，不再是"脏族"）。

**已知边界**：回退是「按族名找实现」，不是「按别名找族」。历史值 `driver = postgres`
（族目录里只有 `postgresql`）仍会失败。要覆盖得加别名映射，那是另一个决定。

## 九 实施记录（全部未提交）

| 文件 | 改动 |
| --- | --- |
| `crates/plugin/src/manifest.rs` | `ContributesDriver` 加 `type_id` + `effective_type_id()` |
| `crates/plugin/src/driver_decl.rs` | `derive_row` 取族；建族留痕；2 条单测 |
| `crates/plugin/src/sidecar/factory.rs` | `descriptor_for` 取族 |
| `crates/store/src/persistence/plugin_driver_store.rs` | `ensure_plugin_family` 返回「是否真的新建」 |
| `crates/driver/src/registry/mod.rs` | `resolve_family` + `first_in_family` + 单测 |
| `crates/driver/src/router.rs` | 建连改用 `resolve_family` + 回退日志 |
| `crates/engine/src/services/connection_probe.rs` | 「测试连接」改用 `resolve_family` |
| `crates/engine/src/connection_manager.rs` | 建连 / 跨库浏览 / 重连三处改用 `resolve_family`；回退成功不再记「缺驱动」审计 |
| `crates/store/src/persistence/metadata_identity.rs` | `driver_family` 改三级；`install_family_resolver` 注入位；2 条单测 |
| `crates/app/src/main.rs` | 启动时注入族解析器（内存查注册表，不读盘） |
