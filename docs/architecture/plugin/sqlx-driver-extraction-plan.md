# sqlx 驱动抽为插件（方案 · 待评审）

> **一句话**：把 MySQL / PostgreSQL 的 **sqlx 实现**（驱动 id `mysql` / `postgres`）从主程序剥离，交给插件（sidecar 二进制）提供；内核只保留每族一条官方原生实现（`mysql_native` / `postgres_native` / `sqlite` / `duckdb`）。
>
> **前置**：`driver-capability-gating-plan.md`（能力置灰底座）必须先落，否则插件驱动的能力缺口会静默成「索引 (0)」。
>
> **状态**：方案稿；D0（置灰底座）与 **D1（抽 `crates/driver-sql`）已落地**，D2 起未开始。分刀与落地记录见 §7。

---

## 1. 目标 / 非目标

**目标**

| # | 目标 |
| --- | --- |
| 1 | 主程序依赖树移除 `sqlx` / `sqlx-mysql` / `sqlx-postgres`（连带 `bigdecimal`），只链 `mysql_async` / `rusqlite` / `duckdb` / `tokio-postgres` |
| 2 | `mysql` 与 `postgres` 两个实现改由插件提供，走**现有** sidecar 通道（不新造第二套通道） |
| 3 | 迁移后老连接仍可用（或给出明确、可操作的去向） |
| 4 | 插件如实声明能力位，缺的能力按 `driver-capability-gating-plan.md` 置灰 |

**非目标**

- SQL 语言服务（sqlglot）、导航方言（`database/src/nav/dialect`）、DuckDB 分析 / 联邦加速（`engine/src/duckdb`）**不动**——它们按**族**归一，与具体驱动实现无关。
- 不改造插件协议为"进程内 dylib 驱动"；插件仍是有独立 `[workspace]` 的 sidecar 二进制。
- 不顺手改原生驱动的行为。

---

## 2. 现状（代码实证）

### 2.1 六个内置实现 = 4 族 × 1~2 实现

`driver/src/loader.rs::BuiltinDriverDiscovery::builtin_factories()`：

| 驱动 id | 客户端库 | 处置 |
| --- | --- | --- |
| `mysql` | `sqlx::MySql` | **抽出** |
| `mysql_native` | `mysql_async` | 保留 |
| `postgres` | `sqlx::Postgres` | **抽出** |
| `postgres_native` | `tokio-postgres` | 保留 |
| `sqlite` | `rusqlite` | 保留 |
| `duckdb` | `duckdb-rs` | 保留 |

### 2.2 sqlx 的足迹（比想象窄，但有几处硬耦合）

| 位置 | 内容 | 处理难度 |
| --- | --- | --- |
| `driver/src/native/mysql.rs`（79 KB） | sqlx MySQL 驱动全量（Database + MetadataBrowser） | 需按协议重写 |
| `driver/src/native/postgres.rs`（80 KB） | sqlx PG 驱动全量 | 同上 |
| `driver/src/native/{mysql,postgres}_pool.rs`（3.3 + 3.7 KB） | 池实现 | 同上 |
| `driver/src/utils.rs`（65 KB） | `PG_LIST_*` / `MY_*` 元数据 SQL、`columns_from_detail_rows`、`sqlx_cell_as_text!` 宏 | **同族两个驱动共用**，见 §3 |
| `driver/src/native/pg_wire.rs`（21 KB） | 自写 PG wire 解码，**同时供 tokio-postgres 与 sqlx 使用**（`Type`/`Decode` 两套 impl） | 必须切分，见 §3 |
| `driver/src/error_location.rs`（26 KB） | `postgres_from_sqlx` 走 `PgDatabaseError`；另有文本解析（与驱动无关） | sqlx 部分随驱动走 |
| `driver/src/registry/{descriptors,property_spec,capability}.rs` | 两个驱动 id 的声明、属性默认、能力位 | 删条目 + 迁移 |
| `engine/src/bin/pg_probe.rs` | 唯一在 engine 里用 sqlx 的生产代码 | 迁到插件工具或删 |
| `engine/tests/{mysql_rsa_auth_probe,type_decode_fidelity,...}` | 验证 sqlx 行为的真机用例 | 迁到插件项目 |
| `connection_dialog/.../runtime.rs:963` | `apply_lan_tls_default` 写死 `matches!(db_type, "mysql" \| "postgres")` | 必改，见 §4.5 |

**可直接拿走的收益**：`engine/Cargo.toml:35-37` 那条注释——sqlx 的 feature 会弱引用 `sqlx-sqlite`，与 `rusqlite` 的 `libsqlite3-sys`（`links = "sqlite3"`）冲突，所以 sqlx 一直不敢开 sqlite 相关 feature。抽走后这条约束在主程序里消失（插件是独立 workspace，`tools/plugin-drivers/*/Cargo.toml` 均写了空 `[workspace]`，互不干扰）。

---

## 3. 共享代码怎么切（本方案最关键的一刀）

`utils.rs` 与 `pg_wire.rs` 都是**"一份逻辑喂两个实现"**的设计，直接搬家会让插件抄一份、日后漂移。三种做法（**待评审选一个**）：

| 方案 | 做法 | 代价 |
| --- | --- | --- |
| **A（推荐，已选）** | 新建 workspace 成员 `crates/driver-sql`：只放**纯字符串 / 纯函数**（`PG_LIST_*` / `MY_*` SQL 常量、`pg_wire` 的 wire 解码核心、枚举码→文案）。内核 4 驱动与插件二进制都 path 依赖它 | 一次搬家；插件跨 workspace path 依赖**已实测 cargo 接受**（见 §7.1） |
| B | 复制一份进插件仓库 | 零结构改动，但两份元数据 SQL 必然漂移（本仓 2026-09-21 刚收敛过） |
| C | 插件只拿 `rust-core`，SQL 常量由插件自己写 | 同上，且更散 |

`pg_wire` 的特殊性：它的 `Type`/`Decode` impl 一半是 `tokio-postgres`、一半是 `sqlx`。切法 = 内核保留 tokio 侧 impl 与 wire 解析核心；sqlx 侧 impl 迁进插件（对 `crates/driver-sql` 复用核心）。

`error_location`：`postgres_from_sqlx`（协议字段）随驱动走；文本解析（MySQL/PG 消息解析）留内核——它是**驱动无关**的。

---

## 4. 迁移与兼容

### 4.1 驱动 id：沿用还是改名

| 选项 | 优点 | 缺点 |
| --- | --- | --- |
| **A（推荐）沿用 `mysql` / `postgres`** | 老连接（`global_connections.driver`、项目连接）零迁移；`family_of` / `sql_gen` / `secret_type_of` 全部照旧命中 | 依赖"先卸内置、插件补位"，插件未装时这两个 id 会变成"缺失驱动" |
| B 改名 `sqlx-mysql` / `sqlx-postgres` | 语义清晰、可与内置共存 | 必须数据迁移所有连接行 + `family_profile` 别名表 + `url_params` 分派 + 测试 |

选 A 时插件清单写法（`plugin.toml` 片段，结构对齐 `tools/plugin-drivers/mssql-rust-plugin/plugin.toml`）：

```toml
[plugin]
id = "rds.driver.sqlx"

[backend]
kind = "sidecar"
runtime = "rust"
executable = "bin/agent"
transport = "framed"
protocol = "rds-driver/1"
max_instances = 1

[permissions]
sidecar = ["spawn.child_process", "net.connect", "fs.read_plugin_data", "credentials.database"]
driver = ["driver.query", "driver.metadata", "driver.transaction", "driver.cancel"]

[capabilities.driver]
concurrency = "parallel"   # sqlx 是异步库，可并行会话

[[contributes.drivers]]
id = "mysql"
display_name = "MySQL (sqlx)"
features = ["tree", "health_check", "transactions", "sql_autocomplete",
            "meta_index_detail", "meta_constraint_detail", "meta_ddl_source", "meta_fk_target"]

[[contributes.drivers]]
id = "postgres"
display_name = "PostgreSQL (sqlx)"
features = ["tree", "health_check", "transactions", "sql_autocomplete", "schema_browser",
            "meta_index_detail", "meta_constraint_detail", "meta_fk_target"]
# 注意：sidecar 协议当前给不出 → 这里**不声明** meta_ddl_source（PG 无源版 DDL 本来就是合成）
```

> 上述 `features` 是**初稿**：以 §5 的实际实现结果为准，声明不到位的位在 UI 上会灰——这正是本方案要的诚实度。

### 4.2 `drivers` 表里旧行

`declaration.rs::sync_driver_declarations` 是**幂等 upsert**，不删旧行。删掉代码里的两个描述符后，`drivers` 表仍留着 `mysql` / `postgres` 两行（`enabled = 1`），连接对话框会显示两个点不动的驱动。

**必须**：新增迁移（下一号）把这两行置 `enabled = 0` 或删除，并在 `data_source_types` 保留 `mysql` / `postgresql` 族行（族是驱动行的外键）。

### 4.3 插件未安装时的表现

`MissingDriver`（`driver/src/missing_driver.rs`）已经存在。要补的是一条**可操作**文案：告诉用户"该驱动由插件 `<id>` 提供，去插件管理页安装"，而不是冷冰冰的"驱动缺失"。

### 4.4 默认驱动会换人

连接对话框默认取该类型首个启用驱动。删掉 `mysql` / `postgres` 后默认变 `mysql_native` / `postgres_native`。这是**用户可见行为变化**，必须在发布说明里写明；两者 TLS 词汇完全不同：

- `mysql_native`：`require_ssl` / `verify_ca` / `verify_identity` 三个布尔，客户端证书**只吃 PKCS#12**；
- `postgres_native`：`disable/prefer/require` 三档，证书在连接器里拼；
- sqlx：五档 `ssl-mode` + CA / 客户端证书 / 私钥 PEM 三件套。

### 4.5 `apply_lan_tls_default` 变成死代码

`connection_dialog/src/services/connection_service/runtime.rs:958-985` 硬编码 `matches!(db_type, "mysql" | "postgres")`（规避 sqlx 默认 `sslmode=prefer` 在 LAN 目标的握手卡顿）。删驱动后这段永不生效。

**必做**：按新驱动重新推导——`mysql_async` 与 `tokio-postgres` 的默认 TLS 行为要分别确认后决定
是否补等价逻辑，并把 `settings/src/registry/entries/connection.rs:47` 里那条 `consumer` 文案同步改掉
（它指向的函数已不再消费该开关）。

→ **已处置（D5，见 §7.5）**：`tokio-postgres` 默认 `Prefer`，所以这段对 `postgres_native` **不是**
死代码；`mysql_async` 默认不加密且不认 `ssl-mode`，故从目标集合里排除。

### 4.6 族归一表

`database/src/nav/family_profile.rs:378/405/493-494` 的别名表已含 `postgres` / `pg` / `postgres_native` / `mysql_native`。沿用 id A 方案时**零改动**；改名 B 方案要补别名。

---

## 5. 插件侧要补什么（协议缺口）

现状（`plugin/src/sidecar/driver.rs` 逐行核实）：

| 能力 | sidecar 现状 |
| --- | --- |
| `query` / `fetch` / `cancel` / 事务 begin-commit-rollback | ✅（真取消，Arrow 批过进程边界） |
| catalogs / schemas / objects / object_detail / routine_source | ✅ |
| `list_columns` | ✅（列名/类型/可空/PK/默认/注释） |
| `list_indexes` / `list_constraints` | ❌ 明确 `Err`（"协议面还没定"） |
| `get_table_ddl` | ❌ 无方法 |
| 列 `references` / `ordinal` / `is_identity` / `collation` | ❌ 线格式无字段 |

**两条路（与置灰方案联动，二选一）**：

- **R1（推荐，先做）**：不扩协议，按 §4 如实声明能力位 → 索引/约束/DDL 源版在这些插件驱动上**置灰**。代价：用户装回 sqlx 插件后，属性面板比今天的 sqlx 驱动弱（今天内核 sqlx 版三项都全）。
- **R2（可选，后做）**：扩协议加 `meta.indexes` / `meta.constraints` / `meta.table_ddl` 三方法（同时惠及 oracle/mssql 插件）。收益：能力对齐今天；代价：协议版本 + `rust-core` + 内核桥 + 每个插件驱动实现。

> **建议顺序**：本轮 R1 落地（保证抽出不被能力缺口卡住），R2 单独立项。

---

## 6. 留痕

抽出过程与日常运行都要留痕，见 `driver-operation-audit-plan.md`。本方案涉及的具体记录点：

1. 插件**安装 / 启用 / 停用 / 卸载 / 重载** → `driver_operations` 一条；
2. 驱动声明**来源变更**（内置 → 插件）→ 记录 before/after 的 `provider_kind`；
3. 老连接在插件未装时被打开 → 记录一次"缺失驱动"事件（含连接 id + 期望驱动 id），便于用户事后追查。

---

## 7. 分刀计划（每刀一 commit，各自可回滚）

| 刀 | 内容 | 前置 | 验收 |
| --- | --- | --- | --- |
| D0 | 方案评审 + 三态/置灰底座落地 | — | 置灰方案 §7 全绿 |
| D1 | 抽 `crates/driver-sql`（纯 SQL 常量 + wire 核心），内核 4 驱动改依赖 | D0 | 全仓 `cargo test-all` 不降 ✅ 已落地（§7.1） |
| D2 | 新建 `tools/plugin-drivers/sqlx-rust`（sidecar 二进制）：mysql + postgres 两个驱动实现，复用 D1 | D1 | ✅ 代码 + 插件包落地（§7.2）；真机 2026-11-12 两驱动各 12 项全绿（3000 行 / 分页 / 事务 / 元数据 / sqlstate / 干净退出） |
| D3 | 删内核 sqlx 实现 + `engine` 的 sqlx 依赖与 `pg_probe` + 两个 pool | D2 | `cargo tree -p <app>` 无 sqlx；老连接走插件可用 |
| D4 | 迁移（`drivers` 两行处置）+ 连接 id 兼容 + 默认驱动 + 缺失驱动文案 | D3 | 老项目连接列表无孤儿行 ✅ 已落地（§7.4） |
| D5 | `apply_lan_tls_default` 重新推导 + settings 文案修正 | D3 | TLS 真机两档 → 代码 + 单测落地（§7.5），真机档因磁盘满待补 |
| D6 | 测试重指：`type_decode_fidelity`（sqlx 数值口径 `1234.5600` 迁插件）、`mysql_rsa_auth_probe`、`error_location` 6 驱动 → 4 驱动 | D3 | 全部用例有归属，无静默删除 ✅ 已落地（§7.6），并挖出 D7 缺陷 |
| D7 | `mysql_native` 会话级 schema 失效（池换连接 / 引擎虚拟键静默跳过 / 解绑残留，三处根因） | D3/D6 | `editor_exec_real` T2c-1 MySQL + PG 双绿 ✅ 已落地（§7.7） |

### 7.1 D1 落地记录（已完成）

**新 crate**：`crates/driver-sql`（包名 `rds-driver-sql`，**零第三方依赖**，连 `shared` 都不引 —— 这样插件二进制 path 依赖它不会把内核依赖树拖进去）。

| 模块 | 内容 |
| --- | --- |
| `meta_sql` | 10 个元数据 SQL 常量：4 个 `*_TABLE_DETAIL_SQL`（PG / MySQL / DuckDB，共用一份列序契约文档）、`PG_LIST_CATALOGS/SCHEMAS/TABLES SQL`、`PG_CURRENT_DATABASE_SQL`、`PG_LIST_INDEXES_SQL`、`PG_LIST_CONSTRAINTS_SQL`、`MY_LIST_CONSTRAINTS_SQL` |
| `wire` | PG 二进制 wire → 文本的 6 个纯函数（`numeric_to_text` / `interval_to_text` / `interval_parts_to_text` / `array_literal` / `inet_to_text` / `point_to_text`）+ 原 5 个单测（整块搬来，不是重写） |
| `codes` | `split_csv` / `pg_constraint_kind` / `pg_fk_action`（枚举码→文案）+ 3 个新单测（含 `pg_fk_action(' ') == None` 这条口径） |

**内核只做 re-export，调用点零改动**：`crates/driver/src/utils.rs` 用 `pub use rds_driver_sql::{codes, meta_sql}` 重新导出常量与三个函数；`crates/driver/src/native/pg_wire.rs` 保留 OID 常量与 `pg_wire_type!` 生成的 `PgNumeric/PgInterval/PgInet/PgPoint`（`FromSql` + `sqlx::Type`/`Decode`），六个纯函数改为 re-export。既有 `utils::PG_LIST_TABLES_SQL` / `pg_wire::numeric_to_text` 等引用**一行都不用改**。

**有意留在内核的**（判定标准：插件二进制用得上吗？用不上就不搬）：

| 留着的东西 | 原因 |
| --- | --- |
| `columns_from_detail_rows` | 吃 `QueryResult` + `crate::ColumnDetail`（驱动模型）。等 D2 真做插件时再看是否要把「列序契约 → 结构体」也提纯 |
| `batch_to_string_rows` / `create_statement` | 吃 `shared::models::QueryResult` + arrow；**插件走 Arrow batch，根本不用 `QueryResult`**，搬过去是给插件加无用依赖 |
| `sqlx_cell_as_text!` / `sqlx_value_is_null` | 吃 `sqlx::Row`，随 sqlx 实现一起走（D3 删） |
| `build_connection_url` / `with_database_in_url` / `database_from_url` | 依赖 `connection::url`（`encode_userinfo`）。插件按自己的配置拼 URL，不需要内核那套 |
| `DUCK_TABLE_DETAIL_SQL` 的使用方（duckdb native） | SQL 文本已进 `driver-sql`，但执行/映射留在内核 |

**§3 的「未验证」风险已实测解除**：在 `.rds/tmp/pathdep-probe` 建了一个自带空 `[workspace]` 的独立 crate，path 依赖 `crates/driver-sql`（主 workspace 成员），`cargo run` 正常输出 `620 0`（读到 `PG_LIST_INDEXES_SQL` 长度与 `numeric_to_text` 结果）。说明**插件可以按 §4.1 的 `plugin.toml` 形态直接 path 依赖 `crates/driver-sql`**，不需要把 SQL 复制进插件。

**验证**：`cargo test -p rds-driver-sql` 8/8 通过；`cargo test -p rds-driver --lib` 97 passed / 22 ignored（忽略数与搬前一致，端点用例）；`cargo check --workspace`、`cargo test-all` 通过。

**没做的事（如实记录）**：`sql_gen::create_table_ddl`（PG 的 DDL 合成）**没搬**——它吃 `TableInfo` 等驱动模型，且插件侧 DDL 合成属于 D2 的活；D1 只搬「字符串与纯函数」，避免为了搬而提前重构模型。

### 7.2 D2 落地记录（已完成，含真机验收）

**① 协议内核加多驱动支持**（`tools/plugin-drivers/rust-core`）

一个插件进程供两个 driver id，需要把 `Driver` 的 `type Session` 擦掉：

| 改动 | 内容 |
| --- | --- |
| `AnySession` | `Box<dyn Any + Send>`，多驱动进程按 driver id 各存各的会话类型 |
| `SidecarDriver` | 对象安全版 `Driver`（`BoxFuture` 方法），`impl<D: Driver> SidecarDriver for D`，业务侧不用手写 |
| `as_sidecar` / `serve_many` | `serve_many(Vec<Box<dyn SidecarDriver>>, …)`；`serve` 保留原签名，内部就是「一个驱动」的特例 —— 会话管理代码只有一份 |
| 启动期校验 | 空驱动集、同 id 撞车都在起进程时拒绝（路由必须唯一） |
| `SessionEntry.driver` | 会话记住自己属于第几个驱动，`routed()` 不靠猜 |

**一个设计坑（已解）**：`send_result` / `send_error` 若挂成 `Server` 的 `&self` 方法，`&Server`
会跨 await 存活 → 编译器要求 `Server: Sync` → 进而逼所有驱动的 `Session: Sync`（Oracle /
MSSQL 的会话未必满足）。改成**自由函数**后 `Server` 只需 `Send`，`Session` 约束不变（仍只 `Send`）。

**② 新插件 crate**：`tools/plugin-drivers/sqlx-rust`（包名 `rds-driver-sqlx`，自带空 `[workspace]`）

| 文件 | 内容 |
| --- | --- |
| `src/main.rs` | `serve_many(vec![as_sidecar(MysqlDriver), as_sidecar(PostgresDriver)], stdin, stdout)` |
| `src/config.rs` | 宿主 `DriverConnectionConfig` JSON → DSN：`url_override` 优先、`options` 补缺（同名不重复追加）、userinfo/path 按段转义、PG `ssl-mode` 归一为 sqlx 认的小写枚举；错误消息里的口令一律 `***` |
| `src/values.rs` | 单元格→文本（沿用内核 `sqlx_cell_as_text!` 同一条试探链）、类型名→`Canonical`、`returns_rows` 入口分流、`map_error`（带 sqlstate） |
| `src/mysql.rs` / `src/pg.rs` | 两个 `Driver` 实现：单连接会话、查询超时（默认 15s）/ 建连超时（默认 10s）、`BEGIN/COMMIT/ROLLBACK`、catalogs/schemas/objects/object_detail |

**关键取舍（都写进了代码注释）**

| 取舍 | 原因 |
| --- | --- |
| 时间列按 `Page` 能解析的写法输出（`%Y-%m-%dT%H:%M:%S%.6fZ` 等） | 内核那份进 Arrow 不经 `parse_timestamp_micros`，写 `+00` 也行；插件走 `Page`，写成 `+00` 会被判 NULL —— 那是另一种「时间列静默变 NULL」 |
| `DECIMAL` / `NUMERIC` / `MONEY` → `Canonical::Text` | 协议白名单里没有 Decimal 物理类型；映射 `Float` 会把 `numeric(38,10)` 静默舍成 f64。文本保住每一位，等 R2 加 `DECIMAL` canonical 再改回数值 |
| 解不出的单元格写 `<无法解码：类型>` 而不是 NULL | 「库里有值、界面空白」与真 NULL 不可区分是最难查的形态 |
| `has_more` 显式置位（多取一行判断） | 截断不能让行凭空消失（否则看着像「查询只返回了 1000 行」） |
| 能力面只声明协议真能做到的 | 索引/约束明细、DDL 原文、触发器、例程源码、序列在 sidecar 协议里**没有通道**（`rust-core` 直接回「不支持」）；声明了就是骗宿主，属性分区会「有内容但永远是 0」 |
| `cancel = false` | 协议是「置标记 + 查完再看」，不能真中断在飞查询（R1 不扩协议） |

**插件包**：`tools/plugin-drivers/sqlx-rust-plugin/`（`plugin.toml` id `rds.driver.sqlx`，两条
`[[contributes.drivers]]` = `mysql` / `postgres`，驱动 id 沿用内核零迁移；`build.ps1` 产
`bin/win-x64/agent.exe`，二进制不入库）。

**验证**

| 项 | 结果 |
| --- | --- |
| `cargo test`（`rust-core`） | 9/9 通过（含新增 `two_drivers_in_one_process_route_by_driver_id`：两个不同会话类型的驱动，`initialize` 报两个 id、`driver.describe` 各报各的、`query.execute` 各出各的数据、未知 id 回 `-32001`） |
| `cargo check`（`mssql-rust` / `oracle-rust`） | 均 `Finished` —— SDK 改动**没有**打断既有插件 |
| `cargo test`（`sqlx-rust`） | 9/9 通过（DSN 拼装 / 转义 / ssl-mode 归一 / 脱敏 / 超时默认 / 两套类型映射 / `returns_rows` / 非 UTF-8 错误提示） |
| 协议冒烟（`.rds/tmp/sqlx_smoke.py`，不连库） | `initialize` 回 `["mysql","postgres"]`；两个 `session.open` 连不上时回 `-32009` 且 URL 已脱敏；未知 driver 回 `-32001`；`shutdown` 正常退出 |

**真机验收（2026-11-12，全绿）**

端点与凭据来自仓库里的测试说明（`docs/architecture/editor/editor-dev-plan.md:363-364`、
`docs/architecture/insight/insight-dev-plan.md:1404-1405`、
`crates/workbench/tests/oracle_federation.rs:7`）：`192.168.3.138` 的
**MySQL 9.7.2**（`root`，库 `mysql`）与 **PostgreSQL 18.6 x86_64-windows**（`postgres`，库 `postgres`，非 TLS 可连）。
脚本 `.rds/tmp/sqlx_real_accept.py`（自建帧客户端；按 `result.attachments[].frames` 把 Arrow 附件收干净，
否则下一轮读帧就错位）。

| 用例 | mysql | postgres |
| --- | --- | --- |
| `initialize` 回两个 driver id | ✅ | ✅ |
| `session.open` + `driver.describe` | ✅ `9.7.2` | ✅ `18.6` |
| 3000 行整取（`row_count=3000`、`has_more=false`、列 `id,name`） | ✅ | ✅ |
| 分页 1000 行（`has_more=true`） | ✅ | ✅ |
| `tx.begin` → `insert`(affected=1) → `rollback` → 计数 `0` | ✅ | ✅ |
| `tx.begin` → `insert` → `commit` → 计数 `1` | ✅ | ✅ |
| `meta.catalogs` | ✅（业务库 `mall_business`） | ✅ |
| `meta.schemas` | —（MySQL 无 schema 层，如实回空） | ✅ |
| `meta.objects` / `meta.object_detail` 首列带 `canonical` | ✅ | ✅ |
| 错误 SQL → `-32003` + `sqlstate` | ✅ | ✅ |
| `cancel` | 只置标记（如实声明 `false`） | 同 |
| `session.close` → `shutdown` 干净退出 | ✅ 退出码 0 | ✅ 退出码 0 |

**首跑挖出的三个缺陷（本刀一起修）**

1. **整数整列解不出来** —— `values::cell_as_text!` 的试探链只有
   `String / BigDecimal / 时间族 / Uuid / Json / Vec<u8>`，**没有整数与浮点分支**：
   MySQL `COUNT(*)`（BIGINT UNSIGNED）解不出来，出口层只能写 `<无法解码：BIGINT>`，
   界面看到的是「库里有值、显示不懂」。**内核 `driver/src/utils.rs` 那份同样缺**（D3 时一并处理）。
   修法：补 `i64/i32/i16/i8 · f64/f32 · bool`；`u64` 单独给 MySQL 开前置分支
   （`mysql_cell_as_text!`）—— `sqlx-postgres` 没有 `u64/u32/u16/u8` 的 `Type` 实现，
   写进公共链会直接编译失败（`trait bound u64: sqlx::Type<Postgres> is not satisfied`）。
2. **MySQL 事务控制语句静默失败** —— `START TRANSACTION / COMMIT / ROLLBACK` 用
   `sqlx::query()` 走的是**预处理语句**通道，MySQL 对这三条直接回 `ER_UNSUPPORTED_PS`（1295）。
   首跑表现：「回滚之后数据还在（计数 1）」「提交之后变 2」—— 因为 `tx.begin` 失败没人看，
   每条语句各自自动提交。修法：事务语句与**无参数**的 DDL/工具语句改走简易协议
   `sqlx::raw_sql(...)`（动态 SQL 仍需 `AssertSqlSafe`：`raw_sql` 的 `SqlSafeStr` 也要求 `'static`）。
   PG 侧同步走同一条通道，避免「两个驱动行为不同」。
3. **关闭路径无界等待** —— `session.close` / `shutdown` 之后进程 10 秒不退（宿主只能杀进程）。
   修法：两个驱动的 `close` 套 5 秒 `tokio::time::timeout`，等不到就放弃这条连接
   （进程退出时内核会回收 socket，不值得为「优雅关闭」把进程卡死）。

验收脚本自身也修了两处（否则结论会被掩盖）：每步**当场**打印 PASS/FAIL、`close()` 失败时
`kill()` 兜底 —— 首跑就是收尾超时把 PG 的整段结论吞掉了；事务/取消每步的返回值单独断言，
不再「调了就算过」。

**端点探测的残留记录（更早的一轮，用 Oracle 那套账号试的）**：MySQL 回
`1045 Access denied for user 'devuser'`（通路与协议对、账号不对）；PG 非 TLS 被服务端 RST，
加 `sslmode=require` 后进到认证阶段、但服务端 `lc_messages` 不是 UTF-8，sqlx 只能报
`Postgres returned a non-UTF-8 string`。后者促成了 `values::friendly_message` 那条「往哪看」的提示
（用例 `non_utf8_server_error_gets_a_hint_instead_of_a_bare_protocol_complaint` 钉住）。

**没做 / 待做（如实记录）**

1. ~~**真机验收未做**~~ → **已完成**（见上面「真机验收」表，MySQL / PG 各 12 项全绿）。
   留一句口径提醒：`cancel` 只能如实声明 `false`（串行分发到不了在飞查询），别指望它「取消」。
2. 插件里的「单元格→文本」与内核 `sqlx_cell_as_text!` **临时重复**，D3 删内核 sqlx 实现时内核那份随之消失，不会长期并存。
3. `get_table_ddl` / `list_indexes` / `list_constraints` 在协议与线格式里都没有位置（R1），对应属性分区置灰。
4. 端口/加密默认值不自己发明：TLS 决策仍归宿主（`append_ssl_params` / `apply_lan_tls_default`，D5），插件只用拿到的 URL 与 `options`。

---

### 7.3 D3 落地记录（代码已剥离，编译全绿；真机复验待做）

**删了什么（逐条可对账）**：机械删除由 `.rds/tmp/d3_strip.py` 执行（锚点唯一才动手，每步打印删行数），
不是手撕 diff。

| 位置 | 删掉的内容 |
| --- | --- |
| `driver/src/native/{mysql,postgres,mysql_pool,postgres_pool}.rs` | 4 个文件整体（81 KB + 82 KB + 3.4 KB + 3.8 KB） |
| `driver/src/registry/descriptors.rs` | `mysql_driver()` + `postgres_driver()` 两个工厂，203 行；原位留一段说明指向插件清单 |
| `driver/src/factory.rs` | `MySqlDriverFactory` + `PostgresDriverFactory` 两个 impl 块，150 行（连带 3 条只为它们存在的 `use`） |
| `driver/src/utils.rs` | `sqlx_cell_as_text!` 宏 + `sqlx_value_is_null`，78 行 |
| `driver/src/error_location.rs` | `postgres_from_sqlx`，8 行（`postgres_from_tokio` 保留） |
| `driver/src/native/pg_wire.rs` | `impl sqlx::Type` + `impl sqlx::Decode`，24 行；只留 `tokio_postgres::types::FromSql` 与 re-export |
| `engine/src/bin/pg_probe.rs`、`engine/tests/mysql_rsa_auth_probe.rs` | 两个文件整体 —— 前者是 sqlx 专用的临时探针（其端点已登记进 `.rds/local/`），后者的主题（MySQL 8+ 明文信道 RSA 认证）已由插件（`mysql-rsa` feature）+ D2 真机验收覆盖 |
| `engine/tests/{read_only_flag_from_real_servers,zero_row_columns_reach_query_result,result_rows_reach_the_temp_table,type_decode_fidelity,error_location_from_real_errors}.rs` | 各删 2 条 `use` + 2 条 sqlx 用例（6 驱动 → 4 驱动），共 10 条用例 |

**顺手必须改的配套**（不改就编不过，逐条都已改）：

1. `native/mod.rs` 去掉 4 行 `mod`；
2. `lib.rs` / `registry/mod.rs` 的导出各去掉两个工厂/两个描述符工厂；`registry/mod.rs` 的
   `DriverFactory` 文档示例从 `mysql_driver()` + `MySqlDatabase` 换成 `mysql_native_driver()` +
   `MySqlNativeDatabase`（示例不能指向不存在的实现）；
3. `loader.rs::builtin_factories()` 从 6 工厂降到 4，文档表同样补一句「`mysql`/`postgres` 由插件贡献」；
4. `driver/Cargo.toml` 删 `sqlx` / `sqlx-postgres` / `sqlx-mysql` / `bigdecimal` / `num-traits`；
   `engine/Cargo.toml` 删 `sqlx` / `sqlx-postgres` / `sqlx-mysql`（含「只为合并 features」那两条）；
   **根 `Cargo.toml` 也删**了 `sqlx` / `sqlx-postgres` / `sqlx-mysql` 三条 workspace 定义 ——
   留着它们等于把剥离掉的内核又长回来（插件有自己的 Cargo.toml 与 Cargo.lock，版本钉在那边）；
5. 三处测试断言随事实改：
   * `capability.rs::metadata_plane_diverges_inside_a_family` → 改名
     `native_drivers_declare_only_what_they_implement`：同族两个实现已经不在内核，
     比较对象消失；改成守「官方实现只声明它真做了的」（`mysql_native` 无索引明细、
     `postgres_native` 无源版 DDL），同族分叉的等价断言落到插件侧 `driver.describe`；
   * `capability.rs::declared_capabilities_and_runtime_bits_agree` 的 `>= 6` → `>= 4`；
   * `capability.rs::network_keys_are_derived_from_the_booleans`、
     `declaration.rs::{every_declaration_targets_a_seeded_family,config_schema_carries_what_the_view_reads}`
     里的 `mysql_driver()` 换成 `mysql_native_driver()`；后者原来断言 MySQL 的
     `ssl-mode=VERIFY_IDENTITY` 下拉，现在带 `Select` 能力的内置驱动只剩 sqlite 的
     「打开模式」，断言改到 `rwc`。

**一处守卫的外延（需要评审确认）**：`dependency_citations_match_lock.rs` 原来只读根
`Cargo.lock`，而 `crates/` 里仍有按 sqlx 源码立论的结论（`property_spec` 的「静默忽略未知键」、
`url_params` 的 TLS 参数词汇）。sqlx 从根锁里消失后这些引用会变成「lock 里没有这个 crate」。
本刀的处理是**把插件 lock 一起读**（`tools/plugin-drivers/*/Cargo.lock`，根锁优先），
引用照旧钉版本、钉的是「真正使用它的那份锁」；扫描根仍只扫 `crates/**`（`tools/` 侧尚未纳入，
见 §8 那条待评审项）。

**验证（2026-11-12）**：

| 命令 | 结果 |
| --- | --- |
| `cargo check -p rds-driver --all-targets` | ✅ 通过（无 warning） |
| `cargo check -p rds-engine --all-targets` | ✅ 通过 |
| `cargo check --workspace --all-targets -j 2` | ✅ 通过（`-j 2` 是必须的：默认并发在本机 32 GB 内存下会 OOM，见下面「环境坑」） |
| `cargo tree -p rds-driver` / `cargo tree -p rds-app` | ✅ 无 `sqlx` / `sqlx-mysql` / `sqlx-postgres`（全 workspace `Cargo.lock` 里 sqlx 家族 0 条，连带掉 233 行：crc / dotenvy / etcetera / futures-intrusive / hashlink / tokio-stream 等只为 sqlx 存在的包） |
| `cargo test -p rds-driver --lib` | ✅ 90 passed / 0 failed / 8 ignored（含改过的 `capability` 三条、`declaration` 两条、`auto_register`、`property_spec`） |
| `cargo test -p rds-engine --test dependency_citations_match_lock --test acceptance_evidence_is_real` | ✅ 1 passed / 2 passed |

**环境坑（记一笔）**：`cargo check --workspace --all-targets` 默认并发在本机直接
`memory allocation of 2097152 bytes failed` + `StackOverflowException`（rustc 进程 OOM，
退出码 `-1073741571`）。加 `-j 2` 即通过 —— 与 `.cargo/config.toml` 的 `test-all`（`-j 2`）
同一个原因，全仓编译验证请一律带 `-j 2`。

**D3 没做的（如实记录）**：

1. **真机复验未做**：本刀只证明「编译与单测」，还没在 192.168.3.138 上重跑
   `official_driver_real` / `editor_exec_real`（验收口径 S-3「老连接走插件仍可用」要等
   D4 的 `drivers` 行处置之后才有完整链路）；
2. `drivers` 表里 `mysql` / `postgres` 两行的处置、默认驱动换人、缺失驱动文案 → D4；
3. `apply_lan_tls_default` 的重推与 settings 文案 → D5；
4. `type_decode_fidelity` 的 `1234.5600` 尾零口径断言迁到插件侧、`error_location` 由 6 驱动
   收缩后补齐 → D6（本刀只删了 sqlx 那一半，没有新增插件侧断言）。

---

### 7.4 D4 落地记录（迁移 + 默认驱动 + 缺失驱动文案）

**1) `drivers` 两行处置 —— 删除，不是置灰**（迁移 `029_retire_kernel_sqlx_drivers.sql`）：

```sql
DELETE FROM drivers WHERE id IN ('mysql','postgres') AND driver_kind = 'native';
```

* 为什么删：插件激活时会按同 id 重新 upsert（`driver_kind='sidecar'`、`enabled` 跟随插件启停），
  删掉不会丢东西；留 `enabled=0` 反而会留一个「已停用」的幽灵实现，且 `enabled` 的两套语义
  （归用户 / 归插件）会打架。
* 为什么带 `driver_kind='native'`：插件已装过的机器上这两行可能已被 upsert 成 `sidecar`，
  那时它是**活的插件行**，删它等于抹掉用户的驱动配置。
* `data_source_types` 的 `mysql` / `postgresql` 族行**保留**（驱动行外键 + 类型下拉的唯一来源）。

**2) 默认驱动换人（用户可见行为变化）**：连接对话框默认取该类型下第一个启用驱动
（`enabled_drivers_of_type(...).first()`），删掉 `mysql` 后 MySQL 族默认变成 `mysql_native`，
`postgres` → `postgres_native`。TLS 词汇随之变化（见 §4.4），**发布说明必须写这一条**。

**3) 项目种子同步收缩**：`project_connection_store::seed_default_drivers` 从 6 个 id 降到 4 个
（`mysql_native` / `postgres_native` / `sqlite` / `duckdb`）。老项目里已存在的 `mysql` /
`postgres` 两行留着不动 —— 插件装好后行会回来，那两行正好是「本项目允许该插件驱动」的开关。

**4) 缺失驱动文案**：

* 测试连接失败提示（`connection_dialog::test_failure_message`）对 `DriverNotFound` 现在会看
  驱动 id：`mysql` / `postgres` 直接点明「由插件 `rds.driver.sqlx` 提供，到「插件管理」安装启用；
  不想装就改选同族官方实现」，其余 id 给通用去向（插件管理页）。
* 顺手修一个 D3 引出的假警报：`project::store::check_project_missing_drivers` 原来把
  `driver_kind != 'native'` 一律当「驱动文件没装」去查 `driver_files`，而插件行（`sidecar`）
  永远查不到 —— 结果是每个项目自检都报「缺失驱动 MySQL (sqlx)，下载地址为空」。现在跳过
  `sidecar` 行：插件驱动的存亡由插件管理负责，卸载时行会被 prune。

**验证**：`cargo check -p rds-store -p rds-project -p rds-connection_dialog --all-targets`、
`cargo test -p rds-store --lib`、`cargo test -p rds-connection_dialog --lib`、全仓
`cargo check --workspace --all-targets -j 2` 均为绿；真机复验（装/不装插件两态）与 D5 的
`apply_lan_tls_default` 一起做。

**D4 没做的**：真机两态复验（本机 `192.168.3.138` 上装插件后跑 `official_driver_real` /
`editor_exec_real`）；`global_connections` 里 `driver_id='mysql'` 的老连接没有做批量迁移
（按 §4.1 的 id 沿用方案，插件装上即可用，不需要改数据）。

### 7.5 D5 落地记录（`apply_lan_tls_default` 按真实默认值重推）

**1) 先取证，再改代码**（全部读本地 registry 源码，不靠记忆）：

| 客户端库 | 默认 | 证据 |
| --- | --- | --- |
| `tokio-postgres 0.7.18`（`postgres_native`） | `SslMode::Prefer`（先试 TLS，服务端不支持再回落） | `src/config.rs:255` `ssl_mode: SslMode::Prefer`；`:344` 文档 `Defaults to prefer` |
| 同上，URL 参数 | 只认小写 `sslmode=disable\|prefer\|require` | `src/config.rs:584-590`（其它值报 `InvalidValue("sslmode")`） |
| `mysql_async 0.37.1`（`mysql_native`） | `ssl_opts: None`（= 不加密） | `src/opts/mod.rs:1206` |
| 同上，URL 参数 | 只认 `require_ssl` / `verify_ca` / `verify_identity` / `built_in_roots` 等，**未知参数直接报错** | `:1999` `require_ssl`；`:2048` `else { return Err(UrlError::UnknownParameter) }` |
| `sqlx 0.9`（插件 `mysql` / `postgres`） | `sslmode=prefer`（MySQL 用 `ssl-mode`，PG 用 `sslmode`），读 URL | 插件侧 `sqlx-rust/src/config.rs` 把宿主 URL 原样交给 sqlx |

**2) 结论：目标集合不是「数据库族」，而是「客户端库默认先试 TLS 的那些驱动」**

| 驱动 | 默认 | 本刀处置 |
| --- | --- | --- |
| `postgres_native` | prefer | **新增**：LAN 直连追加 `sslmode=disable`（此前它被单测明确断言「不动」，是错的） |
| `postgres` / `mysql`（插件 sqlx） | prefer | 保留原行为（`sslmode=disable` / `ssl-mode=DISABLED`） |
| `mysql_native` | 明文 | **排除**：既不需要关（本来就不加密），也**不能**加 —— mysql_async 对未知 URL 参数返回 `UnknownParameter`，加 `ssl-mode=DISABLED` 等于把连接弄坏 |

`runtime.rs::apply_lan_tls_default` 里的 `matches!` 从 `("mysql" | "postgres")` 改成
`("mysql" | "postgres" | "postgres_native")`，参数键按驱动分派（`mysql` → `ssl-mode`，
其余 → `sslmode`），注释里附上上面这张证据表与两个「为什么」。

**3) 单测随事实改写**：`lan_tls_default_only_touches_sqlx_direct_lan` →
`lan_tls_default_follows_which_drivers_actually_prefer_tls`。原来断言「`postgres_native` 不被改动」
的那条反过来了，并补三条反向用例：`mysql_native` **必须不被改动**（附 `UnknownParameter` 理由）、
公网地址不动、`sqlite` 不动。

**4) settings 三处文案同步**：

* `settings/src/registry/entries/connection.rs`：`hint` 从「规避 sqlx 默认 prefer 的握手卡顿」
  改成「sqlx 与 tokio-postgres 默认都先试 TLS……mysql_async（MySQL Official）本就不加密、
  不受此开关影响」；`consumer` 从已迁走的 `workbench/src/services/connection_service.rs`
  改成真实的 `connection_dialog/src/services/connection_service/runtime.rs::apply_lan_tls_default`。
* `settings/src/model/connection.rs`：`lan_disable_tls` 的字段文档同口径改写，点明它管哪三条驱动、
  为什么不管 `mysql_native`。

**验证**：`cargo test -p rds-connection-dialog -p rds-settings --lib` 全绿（68 / 36）；
`cargo check -p rds-connection-dialog -p rds-settings --all-targets` 无 warning。
真机档（`official_driver_real` 走 `ConnectRequest.network_method=None`，正好踩到
`apply_lan_tls_default`）本次**未跑成**：编译 `rds-workbench` 测试目标时磁盘写满
（`failed to build archive ... os error 112`，D 盘仅剩 1.5 GB），清理 `target/debug/incremental`
（31.7 GB 纯增量缓存）的操作在等你确认，因此「`postgres_native` + 追加 `sslmode=disable` 真机可连」
这条还是待验状态，留给 D6 与本机清理一起做。

**D5 没做的**：`mysql_native` 收到结构化 `TlsRequest{mode: Disable}` 时仍会
`builder.ssl_opts(Some(SslOpts))`（「关 TLS」可能实际尝试 TLS，`native/mysql_native.rs:119-137`，
单测 `tls_request_becomes_ssl_opts` 甚至断言「空请求也可构造」）—— 这一条与 `lan_disable_tls`
无关（后者走 URL、且已把 `mysql_native` 排除），属**另一个独立缺陷**，未纳入本刀，需单独立项。

### 7.6 D6 落地记录（测试重指 + 真机复验，顺带挖出一个真缺陷）

**1) `official_driver_real` 两处**（D3 后它们还在按「内核有 sqlx」写）：

* `declared_driver_properties_..._rejected` 的第 ③ 步拿同族 sqlx 驱动做「未知键被静默忽略」的
  对照。现在先用 `driver_is_registered(id)` 查注册表：插件没装（内核只有 4 条官方驱动）→
  `⏭️ 如实跳过`，不假装通过，也不把「内核本不该注册它」判成失败；装了 → 照旧跑对照。
* `pg_server_tls_capability_diagnostic` 的判据原来是「sqlx `?sslmode=require`」，同样没了。
  改成选判据：插件在 → `postgres`（无争议）；插件不在 → `postgres_native`，且在输出里
  **标注结论有歧义**（失败可能是服务端没开 TLS，也可能是我方连接器）。判据驱动自己连不上时
  记「未覆盖」而不是判红——诊断项不该变成新的失败源。

**2) 尾零口径迁到插件**：内核 `type_decode_fidelity` 里那条
`sqlx_postgres_decodes_numeric_temporal_json_and_uuid` 已随实现删除，它钉的
`numeric(p,4)` → `1234.5600` 现在由插件单测
`values::tests::numeric_keeps_trailing_zeros_as_text` 承接（`BigDecimal::to_string()` 必须原样
带出列精度，不能抹成 `1234.56`、也不能折成浮点）。插件单测 10/10。

**3) 真机复验结果（2026-11-13，`192.168.3.138`）**

| 套件 | 结果 |
| --- | --- |
| `official_driver_real` | 5/5 ✅ —— `mysql_native` / `postgres_native` 驱动 id 当 scheme 可连、声明属性下发后可用、`require` 加密可用（MySQL）/ 不可用（PG 服务端未开 TLS，如实记录）、`verify-full` 如实失败、UI 旧初值被拒 |
| `connection_type_driver` / `connection_template` / `connection_render_matrix` | 9 / 2 / 6 ✅ |
| engine 5 套（`error_location` / `read_only_flag` / `zero_row_columns` / `result_rows` / `type_decode_fidelity`） | 4 / 2 / 2 / 2 / 2 ✅（SQLite / DuckDB 未设路径，按设计跳过） |
| `editor_exec_real` | ❌ **1 红**，见下 |

**4) D6 挖出的真缺陷（→ D7）**：`editor_exec_real` 的 T2c-1「切 schema 真影响执行」在
`mysql_native` 上**连续 3 次稳定失败**：切到库 `rds_schema_probe_<pid>` 后不写限定名的
`SELECT count(*) FROM rds_inside_<pid>` 报
`Table 'mysql.rds_inside_<pid>' doesn't exist` —— `USE` 没生效。

根因（代码实证）：D3 删掉的 sqlx MySQL 驱动是**单连接直连**模型（
`conn: Arc<Mutex<Option<...>>>`，其注释明写「池化设计在 PG 真机上反复撞空闲连接被网络层静默
丢弃 / 验证挂 TCP 半开」，用户已拍板摒弃池）；而 `mysql_native` 用 `mysql_async::Pool`
（max 10 / min 0），`db.query("USE ...")` 只落在池里某一条连接上，紧随其后的用户语句从池里
取到的可能是另一条 —— 会话级 `USE` 本来就不跟着语句走。PG 侧没事，因为 `postgres_native`
是 `Arc<Mutex<Client>>` 单连接。

这不是测试写错，是**行为回退**（DBeaver 的 Main 连接也是单连接模型）：单独立一刀 D7 修，
不在 D6 里混改驱动。

**D6 没做的**：`mysql_rsa_auth_probe`（D3 已删文件）的主题由插件 `sqlx` 的 `mysql-rsa`
feature（`sqlx-rust/Cargo.toml:25`）+ D2 真机 12 项 × 2 承接，没有在插件侧新写一个等价探针。

### 7.7 D7 落地记录（`mysql_native` 会话级 schema 失效：三处根因 + 真机转绿）

**1) 根因不是一个，是三个（真机取证后修正 D6 的单因判断）**

| # | 根因 | 证据 |
| --- | --- | --- |
| R-1 | `mysql_native` 是**池**（`pool.get_conn()` 每次可能换连接），而 `USE` 是**连接级**状态 | 真机：切库后未限定名报 `Table 'mysql.t' doesn't exist`；`show processlist` 上 10 条连接共用，`USE` 只落在其中一条 |
| R-2 | 引擎拿**虚拟键** `"active"` 去查 `db_type`，查不到 → `unwrap_or_default()` 得空串 → 方言判不出 → **静默跳过上下文** | `sql_service.rs` 里 `db_type_of(&conn_key)` 的 key 可能是 `DEFAULT_CONN_KEY`；真连接 id 是 `conn_key_for(&conn_id)` 给的 `tx_key`（那行原本在 schema 块之后） |
| R-3 | **解绑语义缺失**：`SET search_path` / `USE` 都会残留，引擎在 `schema = None` 时什么都不发 | 真机：`set_schema(None)` 后未限定名仍命中所选 schema（本该报「表不存在」）；PG 侧同样中招 → **不是驱动单侧问题** |

R-2 是 PG 也失败的原因（同一条链路），也是用户最反感的「静默降级」形态：判不出方言就
当没这回事。D7 把它改成 `tracing::warn!`（带 conn key 与 db_type），不再无声无息。

**2) 驱动侧（`crates/driver/src/native/mysql_native.rs`）——选「会话重放」而不是重写单连接**

失败 SQL 只有 `USE` 会撞 1295：`execute_writing` 走 `exec_drop`（**prepared**），MySQL 对
prepared 里的 `USE` 直接回 `ERROR 1295: This command is not supported in the prepared
statement protocol yet`（D6 阶段没暴露是因为上下文压根没发出去）。

- 结构体加 `session_sql: Arc<Mutex<Option<String>>>`（最后一次成功的 `USE` 原文）；
- `replay_session_sql(...)`：**每次拿到连接先补上**这条会话状态（池换连接也不丢）；
  `query` / `query_with_params` / `query_with_cancel` / `begin_transaction` 四处取连接后调用；
- `run_session_statement(...)`：`USE` 走**文本协议**（`query_drop`）并记成会话状态，
  影响行数如实报 0；三条执行路径（含事务内 `MySqlTransaction::query`）都短路到它；
- 事务结构体加同一个 `Arc`，事务内发的 `USE` 提交后主连接也沿用；
- 删掉 D3 后无人使用的 `from_pool` / `from_pool_with_config`。

为什么没改成「单连接直连」：那要重写事务/取消/超时三条路径（1300 行的驱动），风险与收益
不成比例；会话重放把「池 + 会话状态」这对矛盾用 1 次额外往返解掉，行为与单连接一致。

**3) 引擎侧（`crates/engine/src/services/sql_service.rs`）**

- 方言识别改用**真实连接 id**（`tx_key` 前移），不再拿 `"active"` 去查；
- 判不出方言 / 查不到连接信息时 `tracing::warn!`（保留「不注入」的结果，但不再无声）；
- `SqlService` 新增 `session_schemas: Mutex<HashMap<String, String>>`：记每条连接**已应用**的
  会话 schema；`schema = None` 且曾经绑过时发一句**恢复默认**的语句
  （`utils::default_schema_reset_sql`：PG `RESET search_path`；MySQL 从 URL 库段拼
  `` USE `库名` ``；DuckDB/SQLite 不注入）；
- 上下文**真落上了**才记账（失败不记，下次还会再发）。

**4) 测试重指（`crates/workbench/tests/editor_exec_real.rs`）**

`TARGETS` 里 `mysql` / `postgres` 两条 sqlx 目标由插件贡献，插件没装时不再记成
「连不上」（那是把「没装」误报成故障）——新增 `driver_is_registered(id)`，未注册就
`⏭️ 驱动未注册（插件没装），如实跳过`。

**5) 真机复验（2026-11-13，`192.168.3.138`）**

| 用例 | 结果 |
| --- | --- |
| `editor_exec_real` 整支（MySQL + PG 真机） | **3 passed / 0 failed** |
| MySQL `mysql_native` 14 项（查询/批量/中断/事务/错误定位/执行计划/分段×2/影响行数/下发×2/历史/超时/切 schema） | 全 ✅ |
| PG `postgres_native` 同组 | 全 ✅（含「切 schema 真影响执行」） |
| `mysql` / `postgres`（sqlx） | ⏭️ 如实跳过（插件未装） |
| `rds-driver` lib 单测 | **92 passed**（含新增 `default_schema_reset_maps_per_flavor`、`session_schema_statements_are_recognized`） |
| 全仓 `cargo check --workspace --all-targets -j 2` | 绿 |
| `rds-engine` lib + `transaction_affinity` | **219 + 12 passed** |

顺带修掉 D4 留下的一条**陈旧断言**：`global_init.rs` 的启动接线回归里
`sync.written >= 6` 还是剥离前的口径（D4 之后内核只剩 4 条），会让这条本该守「启动调到
同步」的用例变成「数驱动」。改为 `>= 4` 并注明缘由。

**D7 没做的**：`mysql_native` 收到 `TlsRequest{mode: Disable}` 仍构造 `SslOpts`（等于
「关 TLS 却尝试 TLS」）这个独立缺陷不在本刀（不改原生驱动既有行为），需单独立项；
「装插件 / 不装插件」两态真机复验（老连接 `driver_id='mysql'` 的两种行为）仍需插件包
`bin/win-x64/agent.exe` 构建后才能跑。

### 7.8 D8 后的插件侧实证（2026-11-13，插件包已构建）

**做了什么**：`tools/plugin-drivers/sqlx-rust-plugin/build.ps1` 在本机跑不通（pwsh 7 下
解析报「字符串缺少终止符」，脚本内容被按 GBK 解码），改用等价命令直接编：
`$env:CARGO_TARGET_DIR=.rds\target-gf; cargo build --manifest-path tools/plugin-drivers/sqlx-rust/Cargo.toml`
→ `.rds\target-gf\debug\rds-sqlx-sidecar.exe`（9.7 MB）→ 复制为
`tools/plugin-drivers/sqlx-rust-plugin/bin/win-x64/agent.exe`（`bin/` 已 gitignore）。

**跑了什么**：`crates/plugin/tests/sidecar_conformance.rs` 换靶子（`RDS_SIDECAR_BIN` = 上面那个 exe），
两驱动各跑一支真机一致性（`192.168.3.138`）：

| 靶子 | 结果 | 关键事实 |
| --- | --- | --- |
| `mysql`（3306，库 `mysql`） | **6 passed** | `driver.describe` 能力 6 项、服务端版本 `9.7.2`；3000 行 → Arrow 1 批；`meta.objects` 7 个；stdin 关掉后自退（`exit_code=0`）；`cancel` 未声明 → 该条按设计跳过 |
| `postgres`（5432，catalog `warehouse_supply_chain`） | **6 passed** | 能力 7 项、服务端 `PostgreSQL 18.6`；catalog 2 个 / schema 4 个 / 对象 27 个；`dbeaver_pg_render_test` 表 34 列（首列 `id integer`）；3000 行 → Arrow 1 批 |

**顺带修掉验收工具的一处假绿（重要）**：`sidecar_conformance.rs` 判「有没有 schema 层」用的是
`descriptor.supports("schemas")`——那是**本仓 fixture 的词汇**；真实驱动插件报的是内核能力字典的
`schema_browser`。于是 PG 永远走「catalog 当 schema」的单层分支，对象 0 个，`object_detail` /
`routine_source` 两项又被「空库合法」放过去，**schema 层从未被验**。改为两个键都认之后，PG 立刻
走到 `public` 等 4 个 schema 并验到 27 个对象与 34 列（上面那行就是这么来的）；fixture 与 MySQL
（不声明 schema 浏览，本来就没有 schema 层）结论不变。

**仍未做**：应用内「装插件 / 不装插件」两态（S-3）——需要 GUI 安装插件后拿老连接
（`driver_id='mysql'`）真连一次，属界面级验收。

---

## 8. 验收清单

| # | 断言 | 落点 |
| --- | --- | --- |
| S-1 | 主程序 `cargo tree` 无 `sqlx*` / `bigdecimal` | CI 脚本 / 本地 `cargo tree` |
| S-2 | 未安装插件时，`mysql` / `postgres` 连接给出「由插件提供」的可操作提示 | `missing_driver` 用例 |
| S-3 | 安装插件后，老连接直接可用（URL/属性原样） | 真机 |
| S-4 | 插件驱动如实置灰索引/约束/DDL 源版（若选 R1） | 置灰方案 G-2 |
| S-5 | 内核 4 驱动能力不回退（索引/约束/DDL/事务/取消） | `official_driver_real` 扩 |
| S-6 | `apply_lan_tls_default` 无死代码、开关有真实消费者 | 单测 + 设置项契约 |

**两条守卫要同步**：

1. `engine/tests/acceptance_evidence_is_real.rs` —— 能力字典引用的用例名被删会红，这是**有意的信号**，按 D6 逐个重指；
2. `engine/tests/dependency_citations_match_lock.rs` **只扫 `crates/**`** —— sqlx 源码引用搬到 `tools/` 之后这条守卫**看不见它们**（盲区）。对策：扩扫描根，或把需要被守卫的引用留在 `crates/`（**待评审**）。

---

## 9. 风险

| # | 风险 | 缓解 |
| --- | --- | --- |
| 1 | 插件驱动性能（每会话过进程边界；PG 跨库浏览会开额外会话） | `concurrency = parallel` + `max_instances` 实测；必要时提高实例数 |
| 2 | 老连接一夜之间"驱动缺失" | 插件**随安装包预装**（建议）；迁移只对"没装插件"的机器提示 |
| 3 | 元数据 SQL 两份漂移 | 选 §3 方案 A（共享 crate） |
| 4 | 数值/时间类型口径变化（`1234.5600` → `1234.56`） | D6 把口径断言迁到插件项目；发布说明写明 |
| 5 | 插件崩溃影响面 | 已有 supervisor：崩溃**不静默重连**，需手动重启（既有设计，不新增） |
