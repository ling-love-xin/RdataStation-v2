# 联邦查询模块 · 开发方案（第一期 / 第二期 / 第三期）

> 状态：**第一期（多源挂载）执行路径已通；第三期 Oracle 真机验收已完成（2026-09-18）**——
> 编辑器里选「执行位置：联邦」能跑跨源 join（L1 源），Oracle（L2）的挂载形态已知（架构 §2.1），
> 等 T3.2 接进会话。剩余：源清单浮层与「用作联邦源」标记存储（T1.2 / T1.6）、
> L2 挂载路径（T3.2）。
> 关联：`README.md`（入口与硬约束）、`federation-prototype-design.md`（长什么样）、
> `federation-architecture.md`（为什么这样设计，§8 已知问题为权威）。
> **Oracle / SQL Server 的真机验收**：Oracle **已完成**（`oracle_probe.rs`，台账见架构 §2.1）；
> SQL Server 等用户通知（驱动 / 端点）。

---

## 0. 进度记录（最近在前）

| 日期 | 内容 | 状态 |
| --- | --- | --- |
| 2026-09-18（第一期收尾前半 —— **源登记收口**） | **源清单不再依赖“已连接 + 开开关”，而是「标记过的连接」+「从记录组装」**：① **口径改定（D15）**：原计划的“新增一列「用作联邦源」”**取消**——连接上那个 `use_duckdb_fed` 开关的语义就是“**允许 DuckDB 直连本连接**”（对话框文案原本就写着“联邦查询直连源库”），且**项目侧表里也有这一列**；再开一列只会让人猜“两个开关差在哪”。对话框那个分组改名「DuckDB 直连（本地加速 / 用作联邦源）」、**所有类型都摆**（文件库也要能当联邦源），文案同步。② **组装改成从记录出发（D16）**：`federated_plan` 先读 `DataSourceService::list()`（全局表，**不要求已连接**，连接串现拼 + 解密口令），再用连接管理器里已建连的（含项目作用域 `P_`）补充，同一个 conn_id 只算一次；纯函数 `plan_from_records(records, notes, owner)` 可逐条断言，并新增一例“同一个连接在两张表里只算一次”。**为什么必须这样**：源是 DuckDB 自己 `ATTACH` 的，不需要应用先建连——这正是**没有原生驱动**的库（Oracle）能参与的前提。③ **真机当场抓到两个真缺陷并修掉**：`build_connection_url` 用**驱动 id** 当 URL scheme（`mysql_native://…`）——DuckDB 的 scanner 只认 `mysql://`，报 `Invalid dsn … expected key=value pairs`；新增 `accel::scheme()` + `normalize_scheme()`（交给 DuckDB 前把 scheme 换成扫描器那份，查询串原样保留），accel / 联邦两条路都过它。④ **门控同步**：联邦档 = 两个以上**开了开关且驱动可挂**的连接（不再要求已连接）；开了开关但驱动不行（T3.2 之前的 Oracle）在原因里点名。⑤ **新增真机集成测试 `tests/federation_sources.rs`**（`runner_for_test()` 小口子：窗口层之下直接驱动执行器）：两条标记连接（MySQL `mysql_native` + SQLite）**全程不建原生连接** → 跨源 join 出结果（5 行）+ 结果区小字点名两个源；撤掉一条标记 → 入口就拒且说清差多少。**验证**：engine 单测 425 → **427**（scheme 两例 + 联邦网络源组装一例）· workbench 单测 90 → **92**（组装三例新增 / 门控两例改写）· 真机 `federation_sources` ✅（workbench 全量 212 → 215 项全绿）。⬜ 余：源清单浮层（T1.6）· L2 挂载路径（T3.2）。 | `workbench/src/services/{editor_exec,editor_channels}.rs` + `workbench/src/components/connection_dialog/render.rs` + `engine/src/duckdb/{accel.rs,federation/registry.rs}` + `workbench/tests/federation_sources.rs`（新） | 单测 428 / 92（✅）· 真机：标记→跨源（✅）· 撤标记→拒绝（✅） |
| 2026-09-18（第三期前半 —— **Oracle 真机验收**） | **L2 社区 scanner 的第一份真机台账**（探针 `crates/engine/tests/oracle_probe.rs`，实例 `192.168.3.138:1521` / service `XEPDB1` / 普通开发账号）：① **装 + 载** ✅ `INSTALL oracle_scanner FROM community`（0.2.2）。② **凭据只能走 Secret**：`oracle_query('<secret>', …)` 与 `ATTACH '<secret>'` 的第一参数都是 **secret 名**（不是连接串）——`CREATE SECRET (TYPE ORACLE, HOST/PORT/USER/PASSWORD/SERVICE_NAME)`，**会话级即可**（不落盘）；`CONNECTION_STRING` 这种参数名不认。服务名写成 `XE` 报 `ORA-01017`——**是 XEPDB1**。③ **两条读数据的路都通**：目录形态 `ATTACH '<secret>' AS ora (TYPE oracle_scanner)`（表挂在 **`main`** schema → **两段名 `ora.<表>`**；`duckdb_tables()` 看得到表与列；表清单在 ATTACH 时定型，且只列**当前账号自己的**表——空 schema 时目录看着是空的）与表函数 `oracle_query('<secret>', '<Oracle SQL>')`（能读 SYS 视图）。④ **只读：不支持**（原话 *Oracle ATTACH does not accept option 'read_only' yet*）——架构 §8 #1 就此结案：**L2 的引擎侧写保护不成立**（靠编辑器闸门 + 只读账号）。⑤ 写 / DDL：`oracle_execute` **只接受 INSERT/UPDATE/DELETE**；DDL 要走 `oracle_call_auto(secret, 'DBMS_UTILITY.EXEC_DDL_STATEMENT', ['…'])`。⑥ `oracle_scan_parallel(secret, '<表>', '<分片键列>', shards := N)` **需要额外权限**（`SYS.DBMS_FLASHBACK` 的 EXECUTE + 表上的 FLASHBACK）——普通账号跑不了，默认走 `oracle_query` / 目录。⑦ **过滤不推**（列投影会推）：`FILTER` 在 `ORACLE_QUERY` / `ORACLE_ATTACHED_SCAN` 之上，要快得把谓词写进 Oracle SQL。⑧ 类型：无精度 `NUMBER` → **VARCHAR**，`NUMBER(p,s)` → `DECIMAL(p,s)`。探针自身：建一张 `RDS_PROBE_ORDERS` 并**在断言之前**删除（失败也不留垃圾）。 | `crates/engine/tests/oracle_probe.rs`（新） | 真机：装/载 ✅ · Secret 形态 ✅ · 目录 + 两段名 ✅ · 跨源（Oracle × SQLite）✅ · 只读不可用（已确认） |
| 2026-09-18（第一期后半 —— 执行路径通到编辑器） | **联邦档从占位变成能用**：① 引擎侧 `session.rs` 加**进程内会话缓存**（`ensure_session` / `session_for` / `snapshot_for` / `cancel` / `drop_session` / `drop_all` / `refresh_source` / `refresh_all` / `set_primary`）：会话按**会话主人**（本文档绑定的连接）缓存，命中条件是 `source_fingerprint`（只含源清单：conn_id / 别名 / 种类 / 连接串，排序后拼接）；**换主源不重建**（只是会话上的 `USE`，本地临时对象不丢），**源清单变了才重建**（日志一条）；源清单为空 / 少于两个 → 入口就拒（可读原因）。② **凭据路径纠正（真机实测驱动）**：DuckDB 1.5.5 的 **mysql / postgres 扫描器都不认 Secret**（会话级 / 持久化 / 带 scope / `ATTACH ''` 全试过，见新探针 `tests/federation_credentials_probe.rs`），而脱敏 URL（`user:******@`）只会认证失败——所以挂载改用**运行时连接串**（`DriverConnectionConfig.url_override`，含凭据）；代价是文本脱敏：新增 `accel::scrub_credentials`（整串替成脱敏版 + `user:pass` 片段替换），错误 / 挂载失败原因 / 历史统一过它（探针里验证：真机失败信息里口令被抹成 `******`，原因保留）。顺手两处：`configure_connection` 补 `SET secret_directory`（与 `{system}/secrets` 对齐：Secret 注册与读取终于同一目录；`get_secrets_dir()` 成为唯一权威）；`connection::secret::register` 按类型选「用户名」参数名（**MYSQL 只认 `USER`**，写 `USERNAME` 直接 Binder Error——之前 MySQL 的 Secret 注册一直静默失败）。③ **工作台执行路径**（`services/editor_exec.rs`）：`federated_plan` 组装源清单（纯函数 `plan_from_infos`，宿主组装：已连接 + 开「本地加速」+ 驱动可挂；别名 `sanitize_alias` + `unique_alias`，按连接 id 排序保证稳定；主源 = 本文档的连接）、`run_on_federation` / `fetch_next_on_federation` / `run_rewritten_on_federation`（与另两档同一套分段与改写形状），结果区一行小字说清“挂了哪些源 + 跨源写 `别名.schema.表`”（T1.6 的源清单上线后收进浮层），失败时**点名源**（错误里提到没挂上的别名就缀上它的原因）。④ **门控真值**（`editor_channels.rs`）：联邦档 = **≥ 2 个**已连接 + 开开关 + 驱动可挂的连接（联邦与加速的区别就是“跨源”），不可用时把“还差什么”说出来（没开开关 / 名字+还没连上 / 驱动不支持）。⑤ **历史带参与源**：`history_store` 加 `sources` 字段（逗号分隔别名，老记录读回 `None`），历史面板拼出 `MYSQL·联邦 · 源 mysql_src, pg_warehouse`。**验证**：engine 单测 419 → 425 · editor 333 → 334 · workbench 212 项全绿（含新 `plan_from_infos` 四例与门控两例）· 真机：`federation_probe`（MySQL 354 表 + SQLite 26 表跨源）· `duckdb_accel_probe` 3 项（MySQL / PG / SQLite）· `federation_credentials_probe`（新）。⬜ 余：源清单浮层（`源清单 ▾`）·「用作联邦源」标记存储（现按“已连接 + 本地加速开关”组装，见 T1.2）· 扫描量可见性。 | `engine/src/duckdb/federation/session.rs` + `engine/src/duckdb/{accel,manager}.rs` + `workbench/src/services/{editor_exec,editor_channels}.rs` + `engine/tests/federation_credentials_probe.rs`（新） | 真机：跨源 ✅ · 凭据/脱敏 ✅ · 单测 425/334/212 ✅ |
| 2026-09-18（第一期前半 —— 多源会话落地） | **联邦能真的挂多个源并跨源查询了（引擎侧）**：① `registry.rs`——`FederatedSource`（连接 id + **别名** + 种类 + 连接串，**宿主组装**：引擎不读连接库、不解密口令）；别名三件套纯函数：`sanitize_alias`（展示名 → 可用标识符，撞保留字缀 `_src`）· `unique_alias`（`orders` → `orders_2`）· `validate_alias`（首字符 / 字符集 / **保留名**——`left` / `order` / `table` 这类 SQL 关键字与 `memory` / `rds_src` 一起挡，**真机踩到过** `ATTACH … AS left` 直接语法错）；`MountState` / `MountedSource` / `SessionSnapshot`（界面读的内存快照，`ready_count()` 收口）。② `session.rs`——`FederatedSession::{open,run,refresh,set_primary,snapshot,interrupt}`：逐源 `ATTACH … (READ_ONLY)`（**坏源不阻断**，原话留在快照里）· 主源 `USE`（请求的主源不可用 → **回退到第一个可用源 + `primary_note`**，不悄悄换）· 按源 `DETACH`+`ATTACH` 重挂（重挂后主源 `USE` 回去）· 取消（`InterruptHandle` + `running` 标记）。③ `accel.rs` 抽出三个复用点：`mount_lock()`（装扩展 + 一批 ATTACH 串行）· `run_sql_on()`（**读写分流与 Arrow 转换与加速档同一份**，同一句在两个档上形状一致）· `AccelKind::{extension,attach_type}` / `normalize_file_path` / `quote_literal` 转 `pub(crate)`。④ **真机探针 `tests/federation_probe.rs`**（新）：MySQL + SQLite 同会话（**354 / 26 张表**）· **一条 SQL 跨两源**（`mysql_src.mall_business.product_category` + `sqlite_src.main.blob`）· 未限定名走主源 · 坏源带原因保留 · 写源库被拒 · 本地临时对象允许 · 主源可切换。**验证**：engine 单测 **410 → 419**（registry 4 项 + session 5 项）；**顺手记下一条测试限制**：DuckDB 文件在同一进程只允许一个连接（`File is already open in …`），所以“源库新建表 → 重挂可见”在单进程单测里验不了，由真机探针（MySQL / SQLite 源）覆盖。⬜ 余（第一期后半）：源登记的**存储**（连接表加“用作联邦源”标记）· 执行路径分流（`QueryRunner` 的 federated 分支 + 历史带参与源）· 扩展状态门控 · 源清单 UI。 | `engine/src/duckdb/federation/{registry,session}.rs` + `engine/src/duckdb/accel.rs` + `engine/tests/federation_probe.rs`（新） | 真机：MySQL + SQLite 跨源（✅）· 单测 419（✅） |
| 2026-09-18（设计定稿 + 目录落位） | **联邦有了自己的目录与四份文档**：① **选型实测**（`crates/engine/tests/duckdb_extensions_probe.rs`，提交 `ad35b21`）——官方 `mysql`/`postgres`/`sqlite`/`parquet`/`json`/`httpfs` 与社区 `mssql`/`oracle_scanner`/`adbc`/`adbc_scanner`/`firebird` 在**动态链接**的 libduckdb 上全部 INSTALL + LOAD 成功；`SET extension_directory` 生效（扩展落 `<目录>/v1.5.5/`，可离线预置）；**默认 `allow_community_extensions` / `autoinstall_known_extensions` / `autoload_known_extensions` 全为 `true`**——SQL 里一出现扩展函数名就**静默联网下载**，产品侧必须显式接管。② **目录落位**：`crates/engine/src/duckdb/federation/`——旧 `federation.rs` 迁为 `legacy.rs`（顶部标明“已被取代”的原因，11 项测试全绿），新增 `mod.rs`（五条硬约束）+ `registry.rs` / `session.rs` / `bridge.rs`（职责与接口草案，**尚未实现**）。③ **四份文档**（本目录）。④ **选型结论**：L1 官方 scanner / L2 社区 scanner 为骨干，**L3 桥接降为兜底**（Oracle / SQL Server 都有社区 scanner，实测可装可用）；**ODBC 不做**（DuckDB 1.5.5 没有 ODBC 扫描器；自写桥等于重做 L3）；**ADBC 留作可选层**（有现成扩展，但要求用户侧有 ADBC 驱动）。 | ✅ 目录与文档完成 · 第一期未开工 |

---

## 1. 现状结论（盘点摘要）

| 层 | 状态 | 证据 |
| --- | --- | --- |
| 源会话基建（单源） | ✅ 已有 | `engine/src/duckdb/accel.rs`：按源缓存专用连接 · `ATTACH … (READ_ONLY)` · `USE` 解析表名 · `MOUNT_LOCK` · 旁路回执 |
| 扩展安装与状态 | ✅ 已有 | `engine/src/duckdb/extensions.rs`；选型实测见 `tests/duckdb_extensions_probe.rs` |
| 临时表生命周期 | ✅ 已有 | `engine/src/duckdb/{analysis.rs, temp_table.rs}` |
| 多源挂载 | ✅ 引擎侧已落地 | `federation/{registry,session}.rs`：多源 `ATTACH … (READ_ONLY)` + 主源 + 按源重挂 + 坏源不阻断；真机探针 `tests/federation_probe.rs`（MySQL + SQLite 同会话跨源） |
| 源登记（“哪些连接用作联邦源”） | 🟡 部分 | **本期口径**：已连接 + 开「本地加速」（`use_duckdb_fed`）+ 驱动可挂的连接（宿主组装，`editor_exec::plan_from_infos`）；⬜ 独立的「用作联邦源」标记与存储（T1.2） |
| 联邦档的执行路径 | ✅ 已接 | `editor_exec.rs` 联邦分支（组装 → 会话 → 结果 + 历史带参与源）；门控在 `editor_channels.rs` |
| L3 桥接 | ⛔ 无 | 驱动与 Arrow 批已有；缺“下推 + 物化 + 上限”的编排 |

---

## 2. 分期与任务表

### 第一期 — 多源挂载（可真机验收：MySQL / PG / SQLite / DuckDB）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| T1.1 | **目录收拢**（已完成） | `engine/src/duckdb/federation/{mod,legacy}.rs` | legacy 测试全绿（✅ 11 项） |
| T1.2 | **源登记**：复用连接上的「DuckDB 直连」开关（原计划的“另加一列”已取消，理由见架构 D15）；`SourceRegistry` 快照（别名 / 状态 / 表数 / 刷新时间）+ 别名重名检测 | `.../federation/registry.rs`（✅ 形状已落）+ `workbench/src/services/editor_exec.rs`（✅ `plan_from_records`：从记录组装，不要求已连接） | ✅ 单测：快照是内存读 · 别名重名给唯一名 · 组装（含重复 conn_id 只算一次）· 真机 `tests/federation_sources.rs`（跨源 + 撤标记拒绝） |
| T1.3 | **多源会话**：`FederatedSession::open/run/refresh/set_primary`——逐源 `ATTACH … (READ_ONLY)`，**失败不中断**（记原因继续）；主源 `USE`；按源 `DETACH`+`ATTACH`；会话资源边界（`memory_limit` / `temp_directory` / `max_temp_directory_size`，统一过 `configure_connection`） | `.../federation/session.rs`（✅ 已落，复用 `accel` 的 `mount_lock` / `run_sql_on`；**加进程内会话缓存**：指纹命中就复用） | ✅ 真机：MySQL + SQLite 同会话 + 一条 SQL 跨两源 · 坏源带原因保留 · 写源库被拒 · 主源可切（`tests/federation_probe.rs`）；单测：复用 / 换主源不重建 / 源清单变则重建（✅） |
| T1.4 | **主源与表名解析**：未限定名只在主源；两源同名 → 报错（错误卡片给候选，见原型 §3） | `.../federation/session.rs` + `crates/editor/src/execution.rs`（错误解析） | 真机：同名表场景报错且信息可读；限定名可跨源 join（🟡 错误卡片给候选未做，报错本身由 DuckDB 给） |
| T1.5 | **执行路径**：`QueryRunner` 的 federated 分支在联邦会话上跑（复用 `run`/`run_filtered` 的分流形状）；历史带**参与源清单** | `crates/workbench/src/services/editor_exec.rs` + `engine/src/persistence/history_store.rs`（✅ 本期） | ✅ 单测：源清单组装 / 不够源时拒 / 门控原因；历史 `sources` 字段往返（✅）；真机：`tests/federation_sources.rs`（标记 → 跨源 → 撤标记拒绝） |
| T1.5b | **扩展状态门控**（**取代原先“建 `engine_extensions` 登记表”的想法**）：扩展“装没装”的**真值在 DuckDB 自己身上**（`duckdb_extensions()` 的 `installed` / `loaded`）——再建一张镜像表只会产生“表说已装、实际没有”的偏差（单一权威）；我们只保留**动作与失败原因**（进程内；跨重启重试本来就合理，环境可能变了）+ 官方 / 社区清单（常量） | `.../federation/session.rs` + `duckdb/{extensions.rs, accel.rs}` | 单测：未装扩展时如实报“扩展 X 未安装”，**不静默联网**；真机：装成功后状态翻转（`duckdb_extensions()` 是真值）（✅ 选型探针已覆盖“装/晒”；⬜ 会话侧的装机进度与动作未做） |
| T1.6 | **门控真值 + 源清单 UI**：`editor_channels.rs` 的联邦门控改为真值（≥ 2 个已连接且可挂的源）；工具栏「源清单 ▾」浮层（失败行带原因、按源重挂、设为主源） | `crates/workbench/src/services/editor_channels.rs`（✅ 门控真值）+ `crates/editor/src/view/host.rs` + 新视图文件（⬜ 浮层未做，登记进 `ui_contract` 两份清单） | ✅ 窗口/单测：无源 / 只有自己 / 别人没连上 三种状态各自给原因 · 两个源就绪则可选；⬜ 源清单浮层与状态栏主源显示 |

> **L2 源的例外**（架构 D14 / §2.1）：门控与挂载对 Oracle 这类 L2 源要额外看两件事——
> 账号有没有权限（建 Secret 与读目标表）、以及“不能带 `READ_ONLY`”带来的写保护缺口。

**第一期验收口径**：编辑器里能把 MySQL 与 SQLite（或 PG）两个源一起挂上，跑一条**跨源 join**，
结果落结果集（徽标 `联邦`）、历史带参与源；其中一个源挂不上时其余照常可用。（✅ 引擎侧真机已验；
编辑器内的端到端待「源清单」面板与窗口级用例补齐——现在靠源清单组装/门控/历史的单测 + 引擎真机探针覆盖。）

### 第二期 — L3 桥接（拉数 → DuckDB 临时表）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| T2.1 | **条件下推**：`pushdown_predicates` + `sql/filter.rs` 经验；下推不了要明说 | `.../federation/bridge.rs` + `engine/src/sql/` | 单测：能推的推下去（改写后 SQL 可断言）· 不能推的如实返回“全表拉取” |
| T2.2 | **物化**：驱动 Arrow 批 → `analysis.rs` 登记的临时表；分页用 `execute_segment`（可中断）；行数上限 | 同上 | 真机：sqlite 小表 → 临时表 → 与 MySQL 表 join；中断能停 |
| T2.3 | **标注**：结果区标“外部源 X · 已拉 N 行 · 拉取于 HH:MM”（与 L1/L2 的实时读区分） | `crates/editor/src/view/host.rs` | 窗口测试：文案随数据源与截断状态变化 |
| T2.4 | 与 1c 的临时对象共享验证（若 1c 已开工） | —— | —— |

### 第三期 — L2 社区 scanner（**Oracle 真机验收已完成，2026-09-18**）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| T3.1 | **扩展显式管理**：按源装扩展（显式动作 + 进度 + 失败原话）；`extension_directory` 钉应用目录 | `.../federation/session.rs` + `duckdb/extensions.rs` | 单测 + 真机：未装扩展时该源报“扩展未安装”而不是静默联网（✅ 目录已生效；⬜ 会话侧的“社区扩展要 `FROM community`”未接） |
| T3.2 | **L2 挂载路径（D14）**：会话内建**会话级** Secret（`TYPE ORACLE` + HOST/PORT/USER/PASSWORD/SERVICE_NAME）→ `ATTACH '<secret>' AS <别名> (TYPE oracle_scanner)`（**不带 `READ_ONLY`**）；引擎侧补一道“写 L2 源就被拒”的判定；提示语按源类型给两段 / 三段名 | `.../federation/{session,registry}.rs` + `crates/workbench/src/services/editor_exec.rs` | ⬜ 待写（真机验收口径已就绪：`tests/oracle_probe.rs` 的台账 + 把 Oracle 接进 `federation_probe`） |
| T3.3 | **真机验收（Oracle ✅ / SQL Server 待用户通知）** | —— | ✅ Oracle（`192.168.3.138:1521` / `XEPDB1`）：装/载 · Secret · 目录两段名 · 表函数 · 跨源 × SQLite · 只读不可用（台账见架构 §2.1）；⏳ SQL Server 待通知 |

### 之后

- **ADBC**（`adbc` / `adbc_scanner`）：只在“两种 scanner 都没有、但有 ADBC 驱动”时接；
- 扫描行数可见性（架构 §8 #3）；
- 扩展离线预置包（与内核升级流程绑定，架构 §8 #5）。

---

## 3. 测试场景（清单，随各期落地）

1. 四类源互挂：mysql / postgres / sqlite / duckdb 文件同会话共存；
2. 跨源 join（两源 + 三源各一次）；
3. 同名表未限定名 → 报错并给候选（D3）；
4. 单源挂载失败 → 其余可用 + 失败原因入清单（D6）；
5. 按源重挂 → 源库新建表可见（与加速档同语义）；
6. 写源库对象 → 被拒（引擎侧 + 编辑器侧双保险）；
7. 取消：联邦档慢查询 → 中断 → DuckDB 与源库都停（架构 §8 #8）；
8. 资源上限：大 join 触发 `memory_limit` → 报可读错误而不是崩；
9. L3：条件下推命中 / 不命中两种路径 + 行数上限截断标注；
10. 历史带参与源清单；
11. 门控：无源 / 有源 / 扩展未装 三种状态下的「执行位置」表现。

## 4. 风险与对策

| 风险 | 影响 | 对策 |
| --- | --- | --- |
| 跨源 join 吃内存 / 写满系统盘 | 桌面应用最痛的两种失败 | T1.3 就设资源边界（D5）；错误可读并给建议 |
| 社区 scanner 成熟度不由 DuckDB 官方保证 | 用户遇到怪问题时难定位 | 界面标“社区扩展”；逐库真机验收（第三期）；错误归属点名源 |
| 扩展与内核版本绑定（升级要重下） | 离线环境升级断链 | 预置包跟内核版本（架构 §8 #5） |
| 与 1c 的连接归属没定 | 临时对象共享出问题 | 第一期按“每文档一条”落地并留切换点（D2 / §8 #9） |
| 两套实现并存（legacy 与新 session） | 误用旧路径 | legacy 顶部已标“已被取代”；覆盖后退役（§8 #10） |

## 5. 实现位置映射

见 `federation-architecture.md` §9（唯一权威）；代码改动需同步更新那张表。
