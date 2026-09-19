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

**2026-09-19 清理**：原先有**三套**「驱动类型」声明并存，现只留一套：

| 位置 | 性质 | 处置 |
| --- | --- | --- |
| `driver/registry/descriptors.rs`（`DriverKind` + `DriverDescriptor`） | 活：**声明本身**（注册表 key / 工厂 / 界面读的各列）+ `DriverKind` 枚举保留（前瞻） | 保留（升为权威） |
| `driver/metadata.rs`（`DriverType` + `DriverMetadata`，528 行） | **零引用**（v1 复制品，与 `descriptors.rs` 重复） | **已删除** |
| `driver/driver_config.rs`（`DriverConfig`/`DriverRegistryConfig` + 第二个同名 `BuiltinDriverDiscovery`） | **未在 `driver/mod.rs` 声明，根本不参与编译** | **已删除** |

驱动声明的**权威在代码**（2026-09-19 决策 ② 落地）：`registry/descriptors.rs` 声明
`config_schema` / `capabilities` / `supported_auth_types` / `is_file` / `default_port` / `url_template` /
`driver_properties`，启动时由 `driver/declaration.rs::sync_driver_declarations` **幂等 upsert** 进
`drivers` 表；界面与连接链路照旧读表（**读模型**，也让外部驱动有自己的落库位置）。
列归属：声明拥有上述列 + `name` / `driver_kind` / `version`；**库拥有** `enabled`（用户可关）、
`download_url` / `download_checksum`（外部驱动）、`driver_files`（本机安装状态）——upsert 不覆盖库侧列。
迁移 008/013/014/016/017 的种子降为**首装兜底**（表结构仍由迁移建），改声明不需要再写迁移。

---

## 2.1 连接串形态矩阵（谁解析这条 URL，就按谁的词汇写参数）

依据：2026-09-19 依赖源码实查（版本见根 `Cargo.toml`）。**驱动 id 不是数据库族 id**，
而解析连接串的是**具体客户端库**——同族两个实现的 TLS 参数词汇完全不同，
「按数据库族注入 SSL 参数」会直接弄坏 native 驱动。

| driver id | 客户端库 | 必需 scheme | URL 上的 TLS 参数 | 未知参数 | 证书与校验（谁落实） |
| --- | --- | --- | --- | --- | --- |
| `mysql` | sqlx 0.9 | `mysql://` | `ssl-mode` + `ssl-ca` / `ssl-cert` / `ssl-key` | **静默忽略** | 全部在 URL：CA / 客户端 PEM 证书 / 客户端私钥 / 五档模式 |
| `postgres` | sqlx 0.9 | `postgres://`（`postgresql://` 亦可） | `sslmode` + `sslrootcert` / `sslcert` / `sslkey` | **静默忽略** | 全部在 URL（同上） |
| `mysql_native` | mysql_async 0.37 | **仅 `mysql://`** | `require_ssl` / `verify_ca` / `verify_identity`（布尔） | **报错**（`UnknownParameter`） | CA → `SslOpts::with_root_certs`；客户端证书 → **只接受 PKCS#12**（`.p12`/`.pfx`，native-tls 后端限制）；PEM 两件套显式报错；verify 两档 → 两个 `danger_*` 开关 |
| `postgres_native` | tokio-postgres 0.7 | **仅 `postgres://`** / `postgresql://` | `sslmode=disable\|prefer\|require`（**无 verify 档**） | **报错**（`UnknownOption`） | CA → `Certificate::from_pem/from_der`；客户端证书 → `Identity::from_pkcs8`（PEM 即可）；`verify-ca` → 校链、`verify-full` → 链 + 主机名（均在驱动侧连接器） |
| `sqlite` / `duckdb` | rusqlite / duckdb-rs | 无（裸路径） | 无 TLS 语义 | — | — |

**驱动属性（`driver_properties`）同属“词汇”问题**：这张表同时是「哪些键写了会生效」的判据——
所以驱动声明里只写各库真认的键（文件型一个不写：路径由工厂取，查询串会被剥掉）。
旧种子那套 MySQL C-API 名（`connectTimeout` / `useCompression` / `characterEncoding`…）就是反例：
sqlx 静默忽略、mysql_async 报未知参数。接受键清单与测试见
`engine/src/driver/declaration.rs::tests::declared_property_keys_are_accepted_by_the_client_library`。

**源码证据**：`mysql_async-0.37.1/src/opts/mod.rs:1749`（scheme 校验）、`:2048`（未知参数报错）、
`:1999-2033`（TLS 三个布尔）；`tokio-postgres-0.7.18/src/config.rs:584`（sslmode 仅三档）、`:716`（未知键报错）；
`sqlx-mysql-0.9.0/src/options/parse.rs:50-79`、`sqlx-postgres-0.9.0/src/options/parse.rs:52-67`。

**真机验收（2026-09-19，端点见 `connection-user-guide.md` §9.0）**：

| 项 | 结果 |
| --- | --- |
| `{驱动 id}://…` 连接串（本文件 §2.1 第一行） | ✅ **MySQL / PG 双驱动真机连上并执行查询**（`official_driver_real.rs::official_drivers_connect_from_a_record_built_url`）——这正是修复前必失败的路径（`mysql_async` 报 `UnsupportedScheme`） |
| `mysql_native` + `ssl-mode=require`（即 `require_ssl=true&verify_ca=false`） | ✅ **加密连接成功**且可查询；`verify-full` 则**如预期失败**（schannel：证书链在不受信任的根证书中终止）——证明校验真的在执行（修前不会加密、也不会报） |
| `postgres_native` + `require` / `verify-*` | ⚠️ **真机 PG 未启用 TLS**（诊断：sqlx + `?sslmode=require` 也报 `server does not support TLS`）→ 该项在现有端点无法覆盖；**不是我们的缺陷**（有诊断钉住：`pg_server_tls_capability_diagnostic`） |
| Secret 族解析（§本文件第 ③ 项） | ✅ 真实驱动目录下 `mysql_native → MYSQL`、`postgres_native → POSTGRES` |
| 回归 | ✅ `editor_exec_real`（四库执行链 + 本地加速通道）与 `federation_sources`（`mysql_native` 联邦源）在改后仍全绿 |

**代码落点**：`connection::url_params::{SslMode, append_ssl_params, normalize_url_scheme}`
（模式 → 各库字面量的**唯一分派点**）、`connection::config::TlsRequest`
（URL 表达不了的那部分：证书路径 + 校验意图，随 `DriverConnectionConfig.tls` 进驱动）、
`engine::driver::factory::{mysql_native_url, postgres_native_url}`
（建连前把 scheme 从驱动 id 换成客户端库认的那份）、
`engine::driver::native::{mysql_native::mysql_async_ssl_opts, postgres_native::pg_tls_connector}`
（按请求构造 TLS 连接器）、`workbench::services::connection_service::{tls_request_of, apply_inline_ssl_override}`
（唯一派生点：网络档案 SSL 跳优先 → 「连接安全」内联覆盖）。

**两边必须成对**：`append_ssl_params` 写 URL（sqlx 读的那份 + native 的布尔档位），
`TlsRequest` 进驱动（证书与校验）；派生点同一个，不允许各写一套。
**做不到就报错，不静默降级**：典型是 `mysql_native` + PEM 证书+私钥（native-tls 后端只吃 PKCS#12）——
报可见错误并引导改用 sqlx 驱动，而不是默默丢掉证书。

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
| `postgres` | MB | MB（真层级） | MB | MB | MB | MB | MB | MB（含所属表） | list |
| `postgres_native` | MB | MB（真层级） | MB | MB | MB | MB | MB | MB（含所属表） | list |

¹ `has_schema_level() = false`：Catalog 即 schema（MySQL 的 database、SQLite / DuckDB 的单库），导航树跳过 Schema 层——`get_schemas` 返回空是**有意**的，不是没实现。
² DuckDB 索引从 `duckdb_indexes()` 表函数取，列名从 `expression` 解析。
³ 该库没有这类对象（DuckDB / SQLite 无存储过程与序列；MySQL 无序列）。

> **序列 / 触发器为什么要写进 `MetadataBrowser`**：`Database::list_*` 回退路径的前提是「浏览器层返回空」——
> 而 trait 默认实现恰好就是空，「未支持」与「真的没有」分不清。两个 PG 驱动此前一个靠回退、一个完全没有，
> 现在都在浏览器层给真实实现（2026-09-19 补齐）。触发器的所属表由 `NodeInfo::parent_name` 带到属性面板。

### 3.3 能力位（`DataSourceMeta`）

| 驱动 | 事务 | 流式 | Arrow | 联邦 | 并发写 | 内存库 |
| --- | --- | --- | --- | --- | --- | --- |
| `mysql` / `mysql_native` | ✅ | ✅ | ⛔ | ⛔ | ✅ | ⛔ |
| `postgres` / `postgres_native` | ✅ | ✅ | ⛔ | ⛔ | ✅ | ⛔ |
| `sqlite` | ✅ | ⛔ | ⛔ | ⛔ | ⛔ | ⛔ |
| `duckdb` | ✅ | ✅ | **✅** | **✅** | ✅ | ⛔ |

> `supports_arrow` 只有 DuckDB 为真：本仓的 Arrow 通路（结果集 → 分析引擎）读的是**查询结果**的 batch，不依赖该位；
> 该位目前的实际用途是**插件通信能力的声明**。

**2026-09-19 收敛：能力只有一本字典、且按驱动给**（`engine/src/driver/capability.rs`）：

| 面 | 现在 |
| --- | --- |
| 键的语义 | `CAPABILITY_DICTIONARY`（键 → 中文标签 + 可选运行时位 + 真机验收状态）**唯一一处**；对话框不再另存 12 键标签表 |
| 声明（哪个驱动有哪些键） | `drivers.capabilities`（读模型）：权威在代码（`registry/descriptors.rs` 的 `capability_keys()`），启动幂等 upsert 进表——键由 `capabilities` 显式声明 + 网络布尔位派生 |
| 运行时位 | `DataSourceMeta` **按驱动**给：`mysql()` 与 `mysql_native()`、`postgres()` 与 `postgres_native()` 各自一份（目前同值，单列一份是为让差异可表达） |
| 键 ⇄ 位一致性 | 单测 `capability::tests::declared_capabilities_and_runtime_bits_agree` **直接读代码声明**双向盯住（声明了键 ⇒ 位为 true；位为 true 且有键 ⇒ 必须声明） |
| 归属（驱动能力 / 应用级） | `CapabilitySpec::scope`：`export` / `mock` / `resource` 是**应用级**（与驱动无关，没有驱动声明它们），不进能力矩阵逐行对比，改由一句说明带出；单测盯住「应用级键不得被任何驱动声明」+「驱动声明的键必须在驱动能力字典里（否则矩阵漏行）」 |
| 没有界面的位 | `META_BITS_WITHOUT_UI_KEY`（`streaming` / `arrow` / `concurrent_write` / `in_memory`）+ 单测穷尽性检查：新增一个位不表态就红 |
| 门控（键真管事的处） | `federation` → `SqlService::{register_external_database, create_external_table}` 拒非联邦源；`transactions` → `EngineQueryRunner::supports_transactions()` **改读连接的实际 `supports_transaction`**（此前恒 `true`）。**其余 10 个键目前只展示、没有消费者**（2026-09-20 实查：`index_analysis` / `table_editor` / `sql_autocomplete` / `schema_browser` / `analytics` / `health_check` / `tree` / 三个网络键在 crates 内无读者）——要门控得先定「哪个键管哪个入口」，属产品拍板，不是接线活 |
| 验收标记 | 能力 Tab 行尾 `✓` = 已真机验收（文字另带可复现用例名）；键声明了但无真机证据的键不给 `✓`（D10） |
| 验收证据不烂掉 | `crates/engine/tests/acceptance_evidence_is_real.rs`：字典里每个已验收键引用的用例名必须在工作区真实存在（测试目标文件名或 fn 名）；引用的测试被改名 / 删除而忘了同步字典 → 红。写法约定写在 `Acceptance::verified` 的文档上（多个用 ` + ` 连，补充说明放全角括号） |
| 依赖源码引用不烂掉 | `crates/engine/tests/dependency_citations_match_lock.rs`：代码里形如 `sqlx-mysql-0.9.0/src/…` 的引用必须与 `Cargo.lock` 一致（扫 `crates/**`）；升依赖而没复读源码 → 红（带 `文件:行号` 与“复读结论”的提醒）。行号本身不校验（CI 上未必有源码） |

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
| **L1 官方** | MySQL | `mysql` | `ATTACH … (TYPE mysql, READ_ONLY)` | ✅ | ✅ INSTALL + LOAD 通过；**跨源真机验收**（见下） |
| **L1 官方** | PostgreSQL | `postgres` | 同上 | ✅ | ✅ |
| **L1 官方** | SQLite | `sqlite` | 同上 | ✅ | ✅ INSTALL + LOAD 通过；**跨源真机验收**（见下） |
| **L1 官方** | SQLite | `sqlite` | 同上 | ✅ | ✅ |
| **L1 官方** | 文件（CSV / Parquet / JSON / httpfs） | 内核自带 / `httpfs` | `read_*` 表函数 | 天然只读 | ✅ |
| **L2 社区** | **Oracle** | `oracle_scanner`（`INSTALL … FROM community`） | `ATTACH '<secret>' AS 别名 (TYPE oracle_scanner)`，**凭据只能走会话级 Secret**，表挂 `main` schema ⇒ **两段名** | ⛔ **扩展不接受 `READ_ONLY`** | ✅ **真机验收完成**（v0.2.2，2026-09-18，见 §2.1）；引擎侧写拒绝 + 编辑器闸门 + 只读账号三道兜底 |
| **L2 社区** | **SQL Server** | `mssql`（native TDS，**无需 ODBC / FreeTDS**） | 待验 | 待验 | ⚠️ INSTALL + LOAD 通过，**连通与下推未真机验收** |
| **L2 社区** | Firebird / Snowflake / BigQuery / Mongo … | 各自扩展 | 待验 | 待验 | ⚠️ 装得上，未验收 |
| **L3 桥接** | 任意（只要有自家驱动） | 不需要扩展 | 应用拉 Arrow → DuckDB 临时表 → 参与 join | 随 A | 🟡 **接口草案**（`federation/bridge.rs`） |

**联邦硬约束**（改这块前先读 `federation/README.md`）：一律只读；扩展显式管理（关掉 `autoinstall_known_extensions` /
`autoload_known_extensions`，否则 SQL 里一出现扩展函数名就**静默联网下载**）；主源语义（未限定名只在主源解析，同名表报错）；
资源边界（会话级 `memory_limit` / `temp_directory`）；状态如实（挂不上的源留清单 + 原话原因）。

**L1 跨源真机验收（2026-09-20，端点 `192.168.3.138` + `D:\FossilT\T.fossil`）**：
`crates/workbench/tests/federation_sources.rs`（`RDS_TEST_MYSQL_URL` + `RDS_TEST_SQLITE_PATH`，**两个变量都要设**，
只设一个时用例会静默跳过——只看“测试通过”会被跳过骗到）验了四条：

| 项 | 结果 |
| --- | --- |
| 跨源查询（两条标记过的连接，**应用不先建连**，由 DuckDB 自己 `ATTACH`） | ✅ `mysql_src.mysql.user JOIN sqlite_src.main.blob` 聚合回 1 行（count = 5）；结果区小字如实列出两个源 |
| 换主源（未限定名的解析者） | ✅ 切到 `sqlite_src` 后全限定名查询照常 |
| 重挂（新表可见） | ✅ 重新挂载 `mysql_src` → 354 张表 |
| 撤掉标记后如实拒绝 | ✅ “联邦查询至少需要两个源（现在只有 mysql_src）” |

这是在本轮驱动侧改动（声明单源 / 属性下发 / 连接串拼装）之后复跑的——即 §2.1 那条
“源组装时把驱动 id 归一成扫描器认的 scheme”（`accel::normalize_scheme`）仍然成立。

---

## 5. 空壳与死路（别当成方案用）

| 位置 | 看起来像 | 实际是 |
| --- | --- | --- |
| `engine/src/driver/jdbc/` | 「支持 Oracle / SQL Server / DB2」 | 4 个空 struct；`JdbcDriver` 无 `MetadataBrowser`；无 JNI；零实例化；发现器恒返回空 |
| `engine/src/driver/wasm/` | WASM 驱动宿主 | 同 JDBC 形态（`WasmDriver` 全 `NotSupported`） |
| `engine/src/driver/manager.rs` | 驱动生命周期管理（load / unload / status） | **零调用**（`DriverManager` / `init_driver_manager` / `DRIVER_MANAGER` 只有定义与重导出；运行时注册走 `DriverRegistry`）——留作 M9 插件驱动的接口预演，**不要当成现成能力用** |
| `engine/src/driver/loader.rs` 的 `DriverLoader` / `WasmDriverDiscovery` / `JdbcDriverDiscovery` | 多类型驱动发现 | **零调用**（唯一活的是 `BuiltinDriverDiscovery::builtin_factories()`）；且两个 discovery 的目录是 CWD 相对路径 + `~` 不展开 |
| `plugin/src/sidecar/driver.rs` | Sidecar 数据库驱动 | **已于 P0（2026-09-20）删除**：用的 `DriverFactory` 签名（`id/name/kind/default_port/create_pool`）与 v2（`descriptor/create`）不匹配，而且它的传输假设（HTTP + 单端口 + JSON 行）与 D5/D4（stdio 分帧 + Arrow）相反 —— 不是“补个声明”能救的。驱动桥在 P1 按三层对象模型**新建** |
| `plugin/src/{host,model,plugin_view}.rs` | 插件宿主 | 3 行空壳；`health_checker.rs` / `hot_reload_manager.rs` 为 **0 字节** |
| `plugin/src/federation/legacy.rs` | 联邦旧实现 | 标「已被取代」，待退役 |

---

## 6. 新驱动怎么接（两步）

1. **定类型 + 声明**：在 `registry/descriptors.rs` 写一个工厂函数（id / 显示名 / 默认端口 /
   是否需要库名 / 是否需要文件 / 是否支持 SSL / SSH / 代理 / 字段 / 能力键 / 属性默认值），
   并在 `loader.rs::BuiltinDriverDiscovery::builtin_factories()` 加一行。
   **就这两处**：启动时 `driver/declaration.rs` 会把声明 upsert 进 `drivers` 表
   （`config_schema` / `capabilities` / `supported_auth_types` / `is_file` / `default_port` / `url_template` / 属性）——
   **不要再手写种子迁移**（那是旧流程，见 §7 缺口 6）。
   注意三件事：
   - `target_database` 写**族 id**（`data_source_types.id`，如 `postgresql`），不是驱动 id；写错会让整行挂不上外键（批量里被跳过并告警）；
   - 能力键要么在 `capability.rs::CAPABILITY_DICTIONARY` 里，要么声明时就想清楚只给内部用——不在字典里的话界面只能原样显示键名（测试会红）；
   - `driver_properties` 只写**该客户端库真认的键**（§2.1 的「未知参数」列）；网络能力键不用手写，由 `supports_*` 布尔位派生。
2. **实现 trait**：按 §3.4 的面实现 `Database` + `MetadataBrowser`（**必做**）+ 需要的 `DbPool` / `Transaction`。
   内省 SQL 各库不同，但返回类型是同一套；表 / 视图 / 列 / 索引 / 约束照抄同族驱动（MySQL 抄 `mysql.rs`、PG 抄 `postgres.rs`）。
   **若新驱动的连接串语法与现有库不同**：在 §2.1 的连接串矩阵里补一行（scheme + TLS 参数词汇），
   并在 `connection::url_params::append_ssl_params` 里加一条分支——不要拿数据库族去猜。

**注册后的接线自动完成**：`AutoDriverRegistrar`（内置发现器）把工厂放进 `DriverRegistry`，
`MetadataService` 自动接上——**不需要改 `database` / `workbench` 一行**（唯一闸门在 `MetadataService`，导航树只认 `NavPath`）。

**认证与网络**（SSH 隧道 / SSL / 代理 / 凭据加密）不在驱动里，在 `connection` crate（M3）的协议链上；驱动只拿最终 URL。

---

## 7. 已知缺口（权威清单）

| # | 级别 | 缺口 | 影响 | 建议 |
| --- | --- | --- | --- | --- |
| 1 | 🟡 | **JDBC / ODBC / ADBC / HTTP / Python / JS 六种 `DriverKind` 无实现** | 长尾库（DB2 / 国产库）只能靠 L3 桥接，而 L3 仍需自家驱动 | 按 §1 判据决定：先推 L3 桥接，JDBC 留到有客户点名再做 |
| 2 | 🟡 | **L2 scanner 未真机验收**（SQL Server / Firebird / Snowflake / BigQuery / Mongo） | 「装得上」≠「连得上、推得下去」 | 有端点就补真机用例；界面按 D10 如实标注 |
| 3 | ⚪ | **`JdbcDriverDiscovery` 路径依赖 CWD**（`./jdbc-drivers`）+ `~` 不展开 | 空实现，暂无影响 | 随 P3-a（插件路径统一）一起定 |
| 4 | ⚪ | **`DriverKind` 里 7 个无实现的取值** | 读代码的人容易高估覆盖面 | 保留（前瞻），但**新文档不要再写「支持 JDBC」** |
| 5 | ⚪ | **DuckDB / SQLite 无 `get_routine_source`** | 无影响（这两库没有存储过程） | 不做 |
| 6 | ✅ | ~~**驱动声明两处写**~~（**已处置 2026-09-19**）：权威在代码（`registry/descriptors.rs`）+ 启动幂等 upsert（`driver/declaration.rs`）；迁移种子降为首装兜底 | `loader.rs` 里「加驱动只改一行」的承诺现在成立（descriptors 一处 + loader 一行）；声明不会再与表漂 | 已落地：列归属写进 `declaration.rs` 头注（声明拥有 name/type_id/kind/is_file/port/url_template/version/config_schema/auth/capabilities/属性默认值；库拥有 `enabled` / `download_*` / `driver_files`），新增驱动不再写种子迁移（§6） |
| 7 | ✅ | ~~**Official 驱动的 TLS 能力边界**~~（**已补齐 2026-09-19**）：新增结构化 `TlsRequest`（`DriverConnectionConfig.tls`）+ 驱动侧构造器——`postgres_native` 支持 CA / PEM 客户端证书 / `verify-ca` / `verify-full`；`mysql_native` 支持 CA / verify 两档，**客户端证书只接受 PKCS#12**（native-tls 后端限制，PEM 两件套报可见错误并引导用 sqlx） | 用户选 Official 驱动也能真要证书；只剩 MySQL 客户端的存档格式限制 | **MySQL 侧已真机验收**：`require` 加密成功 + `verify-full` 如预期失败；**PG 侧卡在端点**（服务端未启 TLS，已诊断）；若 MySQL 官方驱动后续支持 PEM（或换 rustls 后端），把 `mysql_async_ssl_opts` 的分支扩开即可（唯一改动点，有单测钉住） |
| 8 | ✅ | ~~**`postgres_native` 的 TLS 连接器无条件 `danger_accept_invalid_certs(true)`**~~（**已修 2026-09-19**）：校验策略改为由请求决定（`pg_tls_policy`）——无请求 = 历史行为（不校验），`verify-ca`/`verify-full` = 真校验（链 / 链+主机名） | 「填了 verify 却明文信任」的静默降级被消除 | 历史默认（无请求时不校验）有意保留：改成默认校验会让内网自签用户集体连不上，属产品决策 |
| 9 | ✅ | ~~**sqlx 静默忽略未知 URL 参数**（§2.1），而 `driver_properties` 的旧种子值是**MySQL C-API 名**（`connectTimeout` / `useCompression` / `characterEncoding`…）~~ | 驱动属性页里的这些键**写了也不生效**（不报错、无提示）；对 Official 驱动则是**直接报错** | **已收口（2026-09-19，三阶段）**：① 声明里只写各库真认的键（`mysql` 空、`postgres`/`postgres_native` 写 `application_name`、`mysql_native` 写 `max_allowed_packet`）；② 新增**属性规格** `engine/src/driver/property_spec.rs`（每驱动：键清单 + 中文标签 + 未知键的效果），属性页逐行标**去向**并列出常用键，未知参数会报错的驱动标危险色；③ **文件型属性真下发**：SQLite 的 `journal_mode` / `synchronous` / `busy_timeout` / `foreign_keys` / `cache_size` / `temp_store` / `mode`（PRAGMA + 开库标志）与 DuckDB 的 `access_mode` / `threads` / `memory_limit` / `temp_directory` / `max_temp_directory_size` / `preserve_insertion_order`（开库配置 + `SET`）由驱动侧在**开库时**应用，取值白名单校验 + **应用后读回比对**（做不到就报错，不静默降级）；旧驼峰名作为别名仍能生效；④ 顺手修掉根因 bug：`append_query_params` 硬拼 `?`（已有查询串时会把已有参数并进上一个键的值 → `unknown value`）· **剩余（阶段 4，未做）**：把属性页手填的**任意**键做“规格化下发”（当前不认的键只记 warn + 界面告知，不会拿用户输入去拼 SQL）；文件型属性的**声明默认值**有意不设（`WAL` / `foreign_keys=ON` 会改变用户库的行为，属产品决策） |
| 10 | ✅ | ~~**导航类型显示硬编码**~~（**已处置 2026-09-19**）：`driver_catalog::DriverMeta` 增 `type_name` / `type_category`（同一次只读扫描带出 `data_source_types`，**不按 `enabled` 过滤**——已保存的连接可能引用已禁用类型）；`nav_view::{nav_type_label, nav_type_short_label}` 改目录优先、内置表降为兜底（新增库族不用改 UI）；属性面板「数据库类型」行也改显目录名（`panels/editor.rs`） | 同一库在对话框 / 导航 / 属性面板三处不再出现两套名字 | **保留**：徽标**形状 + 2 字母**仍为硬编码映射——那是原型 §2.3 的有意设计（「字母是权威识别，形状是冗余强化」），不是遗漏；后续若要接类型目录的 emoji 图标，属产品决策 |
| 11 | 🟡→⚪ | **`driver` 与 `driver_id` 双列**（global `global_connections` / project `connections`）：两条写入路径写同一个值（驱动 id），而**读路径走的是旧的 `driver`** | 一列一个概念存两份，改一处不知另一处是否也该改 | **已登记待办（需拍板）**：建议分三步收敛——① 读改 `COALESCE(NULLIF(driver_id,''), driver)`（老库兼容，无迁移）；② 新写入只写 `driver_id`；③ 列永不删，文档标 legacy。**字段改名**（`db_type` → `driver_id`）与列收口同批做（v2 无 TS 绑定消费者，属编译器兜底的机械改）；待并发会话落地后再动，避免合并冲突 |
| 12 | ✅ | ~~**能力/属性声明的单源未定**~~（**决策 ②/③ 已落地 2026-09-19**）：① 代码声明为准 + 启动幂等 upsert（`driver/declaration.rs`，启动失败仅告警）；② 属性键按各库真认的名字重写（§7 #9）；③ 网络能力键（`ssh_tunnel` / `ssl_tls` / `proxy`）进入能力字典，并由 `supports_*` 布尔位**派生**（不再手写第三份）；④ 能力键 ↔ 运行时位的双向一致由 `capability::tests::declared_capabilities_and_runtime_bits_agree` 直接读代码声明盯住 | 改能力 / 属性只需改 `descriptors.rs`；「声明了但没证据」仍如实展示（`✓` 只给有真机用例的键） | 已落地；剩余待办是 §7 #9 的阶段 2（属性规格化）与「用能力键门控 UI」（当前只展示，不做门控——有意，需产品拍板） |

---

## 8. 实现位置映射

| 内容 | 落点 |
| --- | --- |
| 两层 trait 与结构对象 | `crates/engine/src/driver/traits.rs`（`Database` / `MetadataBrowser` / `NodeInfo` / `ColumnDetail` / `IndexDetail` / `ConstraintDetail` / `NodeDetail`） |
| 原生驱动 | `crates/engine/src/driver/native/{duckdb,sqlite,mysql,mysql_native,postgres,postgres_native}.rs` |
| 驱动注册与发现 | `crates/engine/src/driver/{registry/,loader.rs,auto_register.rs,missing_driver.rs}` |
| 驱动声明（**权威在代码**） | `crates/engine/src/driver/registry/descriptors.rs`（声明）+ `crates/engine/src/driver/declaration.rs`（启动幂等 upsert） |
| 驱动目录（读模型） | `drivers` 表：读侧 `engine::persistence::{driver_store, driver_catalog}`；表结构与首装兜底种子 `engine/migrations/global/{008,013,014,016,017}_*.sql`（改声明**不要**改这里） |
| 能力字典（键 ⇄ 运行时位 ⇄ 验收） | `crates/engine/src/driver/capability.rs`（`CAPABILITY_DICTIONARY` 15 键 / `MetaBit` / `Acceptance` / `META_BITS_WITHOUT_UI_KEY`）；消费：`connection_dialog::{helpers::capability_rows, render.rs}`、`services::editor_exec::supports_transactions` |
| 运行时能力位（按驱动） | `crates/engine/src/driver/traits.rs`（`DataSourceMeta::{mysql, mysql_native, postgres, postgres_native, sqlite, duckdb}`）+ 各驱动 `meta()` |
| 驱动 id → 数据库族 | `engine::persistence::driver_store::get_type_id`（SQL） · `driver_catalog::type_id_of`（只读入口） |
| 属性规格（键 → 去向） | `crates/engine/src/driver/property_spec.rs`（`verdict` / `known_keys` / `accepts`；依据 = 各客户端库源码，行号引用写在模块头）；消费：对话框属性页（`connection_dialog::{helpers::property_note, render.rs}`）、声明自检（`driver/declaration.rs` 的接受键测试） |
| 驱动侧落实属性（文件型） | `engine/src/driver/native/sqlite.rs`（`plan_connection` / `apply_pragmas`：PRAGMA + 开库标志，白名单 + 读回比对） · `native/duckdb.rs`（`plan_connection` / `apply_settings`：`access_mode` 开库配置 + `SET`，顺序在 `DuckDBManager::configure_connection` 之后→用户赢）；接线：`driver/factory.rs` 的两个文件型工厂 + 接线回归测试 |
| 连接串拼装（属性 / 参数的唯一出口） | `engine/src/driver/registry/config.rs::DriverConnectionConfig::{to_url, append_query_params}`（**分隔符按有无查询串选 `?` / `&`**，有单测钉住） |
| 连接串 scheme 归一 | `connection::url_params::normalize_url_scheme`；建连侧 `engine::driver::factory::{mysql_native_url, postgres_native_url}` |
| SSL 参数分派（驱动词汇） | `connection::url_params::{SslMode, append_ssl_params}`；「连接安全」表单 → URL 在 `workbench::services::connection_service::apply_inline_ssl_override` |
| 结构化 TLS 请求（证书 / 校验） | `connection::config::TlsRequest` → `engine::driver::registry::config::DriverConnectionConfig.tls`（`with_tls`）；派生点 `workbench::services::connection_service::{tls_request_of, profile_ssl_config}` |
| 驱动侧 TLS 连接器 | `engine::driver::native::mysql_native::{mysql_async_ssl_opts, is_pkcs12_archive}` · `postgres_native::{pg_tls_connector, pg_tls_policy}` |
| DuckDB Secret 类型 | `workbench::services::secret_integration::{secret_type_of, db_type_to_secret_type}` |
| 元数据唯一闸门 | `crates/database/src/metadata_service.rs`（`MetadataBrowser` 优先 → `Database::list_*` 回退，**纯转发**） |
| 空壳驱动 | `crates/engine/src/driver/{jdbc,wasm}/`（见 §5） |
| 联邦三层 | `crates/engine/src/duckdb/{accel.rs,federation/}`；设计见 `federation/federation-architecture.md` |
| 扩展探针 | `crates/engine/tests/{duckdb_extensions_probe.rs,federation_credentials_probe.rs}` |
| **Official 驱动 + TLS 真机验收** | `crates/workbench/tests/official_driver_real.rs`（环境变量 `RDS_TEST_MYSQL_URL` / `RDS_TEST_PG_URL`；含 PG 服务端 TLS 能力诊断） |
| Oracle 真机验收 | `crates/workbench/tests/oracle_federation.rs` |
