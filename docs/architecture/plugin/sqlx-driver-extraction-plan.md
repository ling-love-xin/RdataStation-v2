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

**必做**：按新驱动重新推导——`mysql_async` 与 `tokio-postgres` 的默认 TLS 行为要分别确认后决定是否补等价逻辑，并把 `settings/src/registry/entries/connection.rs:47` 里那条 `consumer` 文案同步改掉（它指向的函数已不再消费该开关）。

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
| D2 | 新建 `tools/plugin-drivers/sqlx-rust`（sidecar 二进制）：mysql + postgres 两个驱动实现，复用 D1 | D1 | 代码已落地（§7.2，含协议冒烟）；真机 3000 行 + 取消 + 事务待端点确认 |
| D3 | 删内核 sqlx 实现 + `engine` 的 sqlx 依赖与 `pg_probe` + 两个 pool | D2 | `cargo tree -p <app>` 无 sqlx；老连接走插件可用 |
| D4 | 迁移（`drivers` 两行处置）+ 连接 id 兼容 + 默认驱动 + 缺失驱动文案 | D3 | 老项目连接列表无孤儿行 |
| D5 | `apply_lan_tls_default` 重新推导 + settings 文案修正 | D3 | TLS 真机两档 |
| D6 | 测试重指：`type_decode_fidelity`（sqlx 数值口径 `1234.5600` 迁插件）、`mysql_rsa_auth_probe`、`error_location` 6 驱动 → 4 驱动 | D3 | 全部用例有归属，无静默删除 |

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

### 7.2 D2 落地记录（代码已落地，真机验收待端点）

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
| `cargo test`（`sqlx-rust`） | 8/8 通过（DSN 拼装 / 转义 / ssl-mode 归一 / 脱敏 / 超时默认 / 两套类型映射 / `returns_rows`） |
| 协议冒烟（`.rds/tmp/sqlx_smoke.py`，不连库） | `initialize` 回 `["mysql","postgres"]`；两个 `session.open` 连不上时回 `-32009` 且 URL 已脱敏；未知 driver 回 `-32001`；`shutdown` 正常退出 |

**没做 / 待做（如实记录）**

1. **真机验收未做**：本机是否有可连的 MySQL / PG 端点还没确认（真机口径 = 真查询 3000 行 + 取消 + 事务）。
2. 插件里的「单元格→文本」与内核 `sqlx_cell_as_text!` **临时重复**，D3 删内核 sqlx 实现时内核那份随之消失，不会长期并存。
3. `get_table_ddl` / `list_indexes` / `list_constraints` 在协议与线格式里都没有位置（R1），对应属性分区置灰。
4. 端口/加密默认值不自己发明：TLS 决策仍归宿主（`append_ssl_params` / `apply_lan_tls_default`，D5），插件只用拿到的 URL 与 `options`。

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
