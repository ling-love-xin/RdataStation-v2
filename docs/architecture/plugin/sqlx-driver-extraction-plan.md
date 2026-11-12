# sqlx 驱动抽为插件（方案 · 待评审）

> **一句话**：把 MySQL / PostgreSQL 的 **sqlx 实现**（驱动 id `mysql` / `postgres`）从主程序剥离，交给插件（sidecar 二进制）提供；内核只保留每族一条官方原生实现（`mysql_native` / `postgres_native` / `sqlite` / `duckdb`）。
>
> **前置**：`driver-capability-gating-plan.md`（能力置灰底座）必须先落，否则插件驱动的能力缺口会静默成「索引 (0)」。
>
> **状态**：方案稿，待评审；**不改代码**。

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
| **A（推荐）** | 新建 workspace 成员 `crates/driver-sql`：只放**纯字符串 / 纯函数**（`PG_LIST_*` / `MY_*` SQL 常量、`columns_from_detail_rows`、`create_table_ddl` 复用、`pg_wire` 的 wire 解码核心）。内核 4 驱动与插件二进制都 path 依赖它 | 一次搬家；插件跨 workspace path 依赖需实测 cargo 是否接受（**未验证**） |
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
| D1 | 抽 `crates/driver-sql`（纯 SQL 常量 + wire 核心），内核 4 驱动改依赖 | D0 | 全仓 `cargo test-all` 不降 |
| D2 | 新建 `tools/plugin-drivers/sqlx-rust`（sidecar 二进制）：mysql + postgres 两个驱动实现，复用 D1 | D1 | 真机查询 3000 行 + 取消 + 事务 |
| D3 | 删内核 sqlx 实现 + `engine` 的 sqlx 依赖与 `pg_probe` + 两个 pool | D2 | `cargo tree -p <app>` 无 sqlx；老连接走插件可用 |
| D4 | 迁移（`drivers` 两行处置）+ 连接 id 兼容 + 默认驱动 + 缺失驱动文案 | D3 | 老项目连接列表无孤儿行 |
| D5 | `apply_lan_tls_default` 重新推导 + settings 文案修正 | D3 | TLS 真机两档 |
| D6 | 测试重指：`type_decode_fidelity`（sqlx 数值口径 `1234.5600` 迁插件）、`mysql_rsa_auth_probe`、`error_location` 6 驱动 → 4 驱动 | D3 | 全部用例有归属，无静默删除 |

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
