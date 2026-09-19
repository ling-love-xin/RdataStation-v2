# 驱动能力矩阵（谁连得上、看得见什么）

> **定位**：一份**横向台账**，回答三个问题——「支持哪个库」「某库的某个能力为什么没有」「新驱动怎么接」。
> 与 `core-design-current.md` §5（驱动与元数据访问，讲**结构**）互补：这里只谈**覆盖面与边界**。
>
> **依据口径**：全部条目来自 2026-09-19 的代码实查（`crates/engine/src/driver/`）与既有探针台账
> （`crates/engine/tests/duckdb_extensions_probe.rs`、`federation_credentials_probe.rs`）。
> 标 ⚠️ 的是「装得上 / 写得有，但**未真机验收**」——按本仓规矩（`federation-architecture.md` D10），
> **未真机验收不算可用**。

---

## 1. 一个库进本仓的三条路

| 路径 | 谁建立连接 | 能做到什么 | 写操作 | 现状 |
| --- | --- | --- | --- | --- |
| **A 原生驱动** | 应用自己（sqlx / rusqlite / duckdb-rs / mysql_async / tokio-postgres） | 完整：查询、事务、元数据内省、结果集进 DuckDB 二次分析 | ✅ 源库档可写 | 6 个驱动活（§3） |
| **B 联邦 scanner** | **DuckDB 自己**（`ATTACH … (TYPE …)`） | 只读跨源查询、跨源 join；**应用不需要该库的驱动** | ⛔ 一律只读（L2 连只读选项都没有） | L1 产品级、L2 Oracle 已真机验收（§4） |
| **C L3 桥接** | 应用（自家驱动拉 Arrow → DuckDB 临时表） | 兜底：两种 scanner 都没有的库（国产库 / 私有协议 / 受限环境） | 语义未定，随 A | 🟡 **仅接口草案**（`federation/bridge.rs`，第二期） |

**判据（选哪条）**：

- 需要**写**、**事务**、或库在 L1 → **A**；
- 只需要**读**、且要**跨库 join** → **B**（B 的前提是那颗库有 scanner，见 §4）；
- 库**没有 scanner、也没有原生驱动**（DB2 / 国产库）→ 只有 **C**，而 C 仍需要一个「能连它的自家驱动」——
  这正是 JDBC / ODBC 唯一真正要解决的场景，**不是** Oracle / SQL Server（那两个 B 已经覆盖）。

---

## 2. 驱动工厂清单（`DriverKind` 枚举 vs 现实）

`DriverKind` 有 8 个取值，但**只有 1 个有实现**：

| `DriverKind` | 实现 | 现状 |
| --- | --- | --- |
| `Native` | `driver/native/*` | ✅ 6 个驱动活（下表） |
| `Jdbc` | `driver/jdbc/*` | ❌ **空壳**：`JdbcDriver` 实现了 `Database` 但全部返回 `NotSupported` / 空 `Vec`；**无 `MetadataBrowser`**（导航树必然为空）；**无 `jni` 依赖**（`jvm_manager.rs` 9 行占位）；`JdbcDriverDiscovery::load_drivers()` 恒返回空 `Vec`。v1 即如此，v2 整份复制 |
| `Odbc` / `Adbc` | 无 | 枚举值，无代码 |
| `Wasm` | `driver/wasm/*` | ❌ 空壳（同 JDBC 形态） |
| `Http` / `Python` / `Js` | 无 | 枚举值，无代码 |

> 历史口径更正：`v1/README.md` 曾写 `[x] Sidecar adapter — JVM bridge (JDBC)`「已实现」，而 v1 的 `jdbc/` 与 v2 逐字相同（都是空壳）。
> v2 文档没有沿用这个宣称（`runtime/data-paths.md` §9.3 明确记为「空实现」）。

---

## 3. 原生驱动矩阵

### 3.1 引擎与实现

| 驱动 id | 库 | 客户端 | 备注 |
| --- | --- | --- | --- |
| `duckdb` | DuckDB | `duckdb-rs`（动态链接，见 `dependencies/duckdb-linking.md`） | 分析引擎本体，也是联邦宿主 |
| `sqlite` | SQLite | `rusqlite` | 单文件库 |
| `postgres` | PostgreSQL | `sqlx::Pool<Postgres>` | |
| `postgres_native` | PostgreSQL | `tokio-postgres` | 与上者并存（协议栈不同） |
| `mysql` | MySQL / MariaDB | `sqlx::Pool<MySql>` | |
| `mysql_native` | MySQL / MariaDB | `mysql_async` | 与上者并存 |

### 3.2 元数据内省覆盖（`MetadataBrowser`）

「MB」= 由 `MetadataBrowser` 提供；「list」= 只有 `Database::list_*` 回退实现；「—」= 无（该库没有这类对象，或未实现）。

| 驱动 | catalogs | schemas | tables | 列详情 | 索引 | 约束 | 序列 | 触发器 | 例程源码 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `duckdb` | MB | MB（空¹） | MB | MB | MB² | MB | —³ | —³ | —³ |
| `sqlite` | MB | MB（空¹） | MB | MB | MB | MB | —³ | —³ | —³ |
| `mysql` | MB | MB（空¹） | MB | MB | MB | MB | —³ | —³ | list |
| `mysql_native` | MB | MB（空¹） | MB | MB | MB | MB | —³ | —³ | list |
| `postgres` | MB | MB（真层级） | MB | MB | MB | MB | **list** | **list（含所属表）** | list |
| `postgres_native` | MB | MB（真层级） | MB | MB | MB | MB | ⚠️ **缺** | ⚠️ **缺** | list |

¹ `has_schema_level() = false`：Catalog 即 schema（MySQL 的 database、SQLite / DuckDB 的单库），导航树跳过 Schema 层——`get_schemas` 返回空是**有意**的，不是没实现。
² DuckDB 索引从 `duckdb_indexes()` 表函数取，列名从 `expression` 解析。
³ 该库没有这类对象（DuckDB / SQLite 无存储过程与序列；MySQL 无序列）。

> **已知不一致（待补）**：`postgres` 的序列与触发器走 `Database::list_*` 回退（因为它没覆盖 `get_sequences` / `get_triggers`，
> 而 trait 默认返回空 ⇒ `MetadataService` 会回退），`postgres_native` **两个都没有** ⇒ 用原生驱动连 PG 时「序列 / 触发器」文件夹恒空。
> 触发器的所属表已由 `NodeInfo::parent_name` 带到属性面板（2026-09-19 接通）。

### 3.3 能力位（`DataSourceMeta`）

| 驱动 | 事务 | 流式 | Arrow | 联邦 | 并发写 | 内存库 |
| --- | --- | --- | --- | --- | --- | --- |
| `mysql` / `mysql_native` | ✅ | ✅ | ⛔ | ⛔ | ✅ | ⛔ |
| `postgres` / `postgres_native` | ✅ | ✅ | ⛔ | ⛔ | ✅ | ⛔ |
| `sqlite` | ✅ | ⛔ | ⛔ | ⛔ | ⛔ | ⛔ |
| `duckdb` | ✅ | ✅ | **✅** | **✅** | ✅ | ⛔ |

> `supports_arrow` 只有 DuckDB 为真：本仓的 Arrow 通路（结果集 → 分析引擎）读的是**查询结果**的 batch，不依赖该位；
> 该位目前的实际用途是**插件通信能力的声明**。

### 3.4 trait 实现面（补一个驱动要写什么）

| trait | 必需性 | 方法数 | 说明 |
| --- | --- | --- | --- |
| `Database` | **必需** | 4 必需 + 其余有默认实现 | `query` / `query_with_cancel` / `begin_transaction` / `meta`；`list_*` 与 `get_routine_source` 有默认（空 / `None`） |
| `MetadataBrowser` | 强烈建议 | 4 必需（`get_catalogs` / `get_schemas` / `get_tables` / `get_table_detail`）+ 4 可选 | **不实现它，导航树拿不到任何对象**——`MetadataService` 回退到 `list_*`，而只实现 `Database` 的桥接驱动那里通常是空的 |
| `DbPool` | 池化时必需 | 4 | `acquire` / `close` / `is_closed` / `status` |
| `Transaction` | 支持事务时必需 | 3 | `query` / `commit` / `rollback` |

**关键**：`Database::list_*` 与 `MetadataBrowser::get_*` 现在返回**同一套类型**（`NodeInfo` / `ColumnDetail` / `IndexDetail` / `ConstraintDetail`），
实现了浏览器的驱动直接转发（`list_tables` → `get_tables`），**不再需要写一遍降级映射**（2026-09-19 统一，此前 5 个驱动共约 120 行空转）。

---

## 4. 联邦 scanner 矩阵（DuckDB 连）

依据：`crates/engine/tests/duckdb_extensions_probe.rs` 与 `federation-architecture.md`。

| 层 | 库 | 扩展 | 挂载形态 | 只读 | 验收状态 |
| --- | --- | --- | --- | --- | --- |
| **L1 官方** | MySQL | `mysql` | `ATTACH … (TYPE mysql, READ_ONLY)` | ✅ | ✅ INSTALL + LOAD 通过 |
| **L1 官方** | PostgreSQL | `postgres` | 同上 | ✅ | ✅ |
| **L1 官方** | SQLite | `sqlite` | 同上 | ✅ | ✅ |
| **L1 官方** | 文件（CSV / Parquet / JSON / httpfs） | 内核自带 / `httpfs` | `read_*` 表函数 | 天然只读 | ✅ |
| **L2 社区** | **Oracle** | `oracle_scanner`（`INSTALL … FROM community`） | `ATTACH '<secret>' AS 别名 (TYPE oracle_scanner)`，**凭据只能走会话级 Secret**，表挂 `main` schema ⇒ **两段名** | ⛔ **扩展不接受 `READ_ONLY`** | ✅ **真机验收完成**（v0.2.2，2026-09-18，见 §2.1）；引擎侧写拒绝 + 编辑器闸门 + 只读账号三道兜底 |
| **L2 社区** | **SQL Server** | `mssql`（native TDS，**无需 ODBC / FreeTDS**） | 待验 | 待验 | ⚠️ INSTALL + LOAD 通过，**连通与下推未真机验收** |
| **L2 社区** | Firebird / Snowflake / BigQuery / Mongo … | 各自扩展 | 待验 | 待验 | ⚠️ 装得上，未验收 |
| **L3 桥接** | 任意（只要有自家驱动） | 不需要扩展 | 应用拉 Arrow → DuckDB 临时表 → 参与 join | 随 A | 🟡 **接口草案**（`federation/bridge.rs`） |

**联邦硬约束**（改这块前先读 `federation/README.md`）：一律只读；扩展显式管理（关掉 `autoinstall_known_extensions` /
`autoload_known_extensions`，否则 SQL 里一出现扩展函数名就**静默联网下载**）；主源语义（未限定名只在主源解析，同名表报错）；
资源边界（会话级 `memory_limit` / `temp_directory`）；状态如实（挂不上的源留清单 + 原话原因）。

---

## 5. 空壳与死路（别当成方案用）

| 位置 | 看起来像 | 实际是 |
| --- | --- | --- |
| `engine/src/driver/jdbc/` | 「支持 Oracle / SQL Server / DB2」 | 4 个空 struct；`JdbcDriver` 无 `MetadataBrowser`；无 JNI；零实例化；发现器恒返回空 |
| `engine/src/driver/wasm/` | WASM 驱动宿主 | 同 JDBC 形态（`WasmDriver` 全 `NotSupported`） |
| `plugin/src/sidecar/driver.rs` | Sidecar 数据库驱动 | **v1 死代码（未编译）**：用的 `DriverFactory` 签名（`id/name/kind/default_port/create_pool`）与 v2（`descriptor/create`）不匹配 |
| `plugin/src/{host,model,plugin_view}.rs` | 插件宿主 | 3 行空壳；`health_checker.rs` / `hot_reload_manager.rs` 为 **0 字节** |
| `plugin/src/federation/legacy.rs` | 联邦旧实现 | 标「已被取代」，待退役 |

---

## 6. 新驱动怎么接（三步）

1. **定类型**：`DriverDescriptor`（`registry/descriptors.rs`）描述 id / 显示名 / 默认端口 / 是否需要库名 / 是否需要文件 / 是否支持 SSL / SSH。
2. **实现 trait**：按 §3.4 的面实现 `Database` + `MetadataBrowser`（**必做**）+ 需要的 `DbPool` / `Transaction`。
   内省 SQL 各库不同，但返回类型是同一套；表 / 视图 / 列 / 索引 / 约束照抄同族驱动（MySQL 抄 `mysql.rs`、PG 抄 `postgres.rs`）。
3. **注册**：`registry/factory.rs` 的工厂 + `auto_register.rs`（内置驱动发现器），随后 `MetadataService` 自动接上——
   **不需要改 `database` / `workbench` 一行**（唯一闸门在 `MetadataService`，导航树只认 `NavPath`）。

**认证与网络**（SSH 隧道 / SSL / 代理 / 凭据加密）不在驱动里，在 `connection` crate（M3）的协议链上；驱动只拿最终 URL。

---

## 7. 已知缺口（权威清单）

| # | 级别 | 缺口 | 影响 | 建议 |
| --- | --- | --- | --- | --- |
| 1 | 🟡 | **JDBC / ODBC / ADBC / HTTP / Python / JS 六种 `DriverKind` 无实现** | 长尾库（DB2 / 国产库）只能靠 L3 桥接，而 L3 仍需自家驱动 | 按 §1 判据决定：先推 L3 桥接，JDBC 留到有客户点名再做 |
| 2 | 🟡 | **`postgres_native` 缺序列 / 触发器**，`postgres` / `postgres_native` 缺 `get_sequences` / `get_triggers`（走 list 回退） | 原生驱动连 PG 时这两类文件夹恒空 | 补齐两处：把 `list_*` 的实现上移为 `get_*`（类型已统一，是纯搬运） |
| 3 | 🟡 | **L2 scanner 未真机验收**（SQL Server / Firebird / Snowflake / BigQuery / Mongo） | 「装得上」≠「连得上、推得下去」 | 有端点就补真机用例；界面按 D10 如实标注 |
| 4 | ⚪ | **`JdbcDriverDiscovery` 路径依赖 CWD**（`./jdbc-drivers`）+ `~` 不展开 | 空实现，暂无影响 | 随 P3-a（插件路径统一）一起定 |
| 5 | ⚪ | **`DriverKind` 里 7 个无实现的取值** | 读代码的人容易高估覆盖面 | 保留（前瞻），但**新文档不要再写「支持 JDBC」** |
| 6 | ⚪ | **DuckDB / SQLite 无 `get_routine_source`** | 无影响（这两库没有存储过程） | 不做 |

---

## 8. 实现位置映射

| 内容 | 落点 |
| --- | --- |
| 两层 trait 与结构对象 | `crates/engine/src/driver/traits.rs`（`Database` / `MetadataBrowser` / `NodeInfo` / `ColumnDetail` / `IndexDetail` / `ConstraintDetail` / `NodeDetail`） |
| 原生驱动 | `crates/engine/src/driver/native/{duckdb,sqlite,mysql,mysql_native,postgres,postgres_native}.rs` |
| 驱动注册与发现 | `crates/engine/src/driver/{registry/,loader.rs,auto_register.rs,missing_driver.rs}` |
| 元数据唯一闸门 | `crates/database/src/metadata_service.rs`（`MetadataBrowser` 优先 → `Database::list_*` 回退，**纯转发**） |
| 空壳驱动 | `crates/engine/src/driver/{jdbc,wasm}/`（见 §5） |
| 联邦三层 | `crates/engine/src/duckdb/{accel.rs,federation/}`；设计见 `federation/federation-architecture.md` |
| 扩展探针 | `crates/engine/tests/{duckdb_extensions_probe.rs,federation_credentials_probe.rs}` |
| Oracle 真机验收 | `crates/workbench/tests/oracle_federation.rs` |
