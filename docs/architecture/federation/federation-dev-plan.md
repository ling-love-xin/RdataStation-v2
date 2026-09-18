# 联邦查询模块 · 开发方案（第一期 / 第二期 / 第三期）

> 状态：**第一期（T1.1～T1.6）已完成（2026-09-18）；第三期 T3.2（L2 挂载路径）也已完成（同日）**——
> 编辑器里选「执行位置：联邦」能跑跨源 join，**L1（MySQL / PG / SQLite / DuckDB）与 L2（Oracle）同池**，
> 「源清单 ▾」能看到并维护源（重挂 · 换主源）。剩余：扩展的显式安装动作与进度（T3.1 后半）、
> SQL Server 真机（T3.3 待通知）、第二期 L3 桥接。
> 关联：`README.md`（入口与硬约束）、`federation-prototype-design.md`（长什么样）、
> `federation-architecture.md`（为什么这样设计，§8 已知问题为权威）。
> **Oracle / SQL Server 的真机验收**：Oracle **已完成**（`oracle_probe.rs` 台账 + `oracle_federation.rs` 联邦路径）；
> SQL Server 等用户通知（驱动 / 端点）。

---

## 0. 进度记录（最近在前）

| 时间 / 主题 | 做了什么（含取舍与真机结论） | 落点 | 验证 |
| --- | --- | --- | --- |
| 2026-09-18（第三期 —— **L2 挂载路径 T3.2 收圆**） | **Oracle 从“台账已知”变成“真能挂进联邦会话”**：① `AccelKind` 新增 **`Oracle`**（`from_db_type` 认 `oracle` / `ora`）：扩展 `oracle_scanner`、**从 community 仓库装**（`extension_repository` / 新增 `install_sql`，`accel` 与联邦共用一份装机 SQL）、`attach_type` = `oracle_scanner`、**`needs_secret` = true**、**`supports_read_only_attach` = false**；`AccelSource::new`（本地加速档）对 L2 直接拒（新增公开判据 **`local_accel_support`** —— 理由的原文只写一处，门控与组装都说同一句话：“只能做联邦源，凭据要走会话级 Secret”）。② **凭据走会话级 Secret**：`registry::SourceSecret`（手写 `Debug`，口令不随 `{:?}` 出去）+ `oracle_secret_from_url(别名, url)` 纯函数——`oracle://user:pass@host:port/SERVICE` → `CREATE OR REPLACE SECRET rds_<别名> (TYPE ORACLE, HOST/PORT/USER/PASSWORD/SERVICE_NAME)`；缺服务名 / 主机 / 端口非数字都如实报错（服务名不能猜：写 `XE` 会得到 `ORA-01017`），凭据做百分号解码（与 URL 组装侧对称）。`FederatedSource` 加 `secret` 字段（连接串仍原样留着：解析凭据 + 脱敏用），`attach_sql` 有 Secret 就 `ATTACH '<secret 名>' AS 别名 (TYPE oracle_scanner)`——**不带 `READ_ONLY`**。③ **写保护补第三道闸**：`session.rs` 新增 `write_refusal`（**纯函数**，好测）+ `l2_write_refusal`（会话读它）——非读语句且提到 L2 别名（`别名.` / `"别名".` / `` `别名`. ``，前缀是标识符字符就不算，如 `myora.`）或它就是主源 → 拒，文本说清“为什么拦 + 怎么办”；本地临时对象不受影响。④ **挂载只有一处实现**：抽出 `ensure_secret` + `attach_source`（`open` 与 `refresh` 共用；重挂时 Secret 重建，口令变了也跟得上），`open` 装扩展改走 `install_sql`（L2 才带 `FROM community`）。⑤ **工作台侧**：门控加速档加“本地加速支持”那一条（Oracle 在加速档被挡、联邦档计数）；`plan_from_records` 里**组装不出来的源不再阻断整份清单**（与“认不出的驱动”同一口径，点名后继续——L2 的连接串缺件比 L1 常见）；结果区小字按源类型给写法（有 L2 源时：“跨源请写 别名.schema.表（X 写两段：别名.表，不写 schema）”）。⑥ **真机验收**（新文件 `crates/workbench/tests/oracle_federation.rs`，单独一个测试二进制=单独一份临时全局库）：连接记录 → 会话级 Secret → 扩展从 community 装 → **一条 SQL 同时读 MySQL 与 Oracle**（`oracle_query('rds_oracle_src', 'SELECT 1 FROM dual')` 路径，**不在源库建表**）· 挂载状态可用（未限定名走 MySQL 主源）· 重挂 · 写 L2 被引擎拦（点名源）· 本地临时对象照常。**验证**：engine 单测 427 → **435**（Secret 解析 3 例 + L2 只在联邦源 1 例 + 写拒绝 1 例 + 门控/组装 3 例）· workbench 单测 93 → **95** · editor **343** · `ui_contract` 7 · 真机：`oracle_federation` ✅ · `federation_sources`（L1 回归）✅ · `federation_probe` / `duckdb_accel_probe`（3 项）/ `oracle_probe` ✅。⬜ 余：扩展的**显式安装动作与进度**（T3.1 后半，现在仍是执行时装 + 失败原因入状态）、SQL Server 真机（T3.3）。 | `engine/src/duckdb/accel.rs` + `engine/src/duckdb/federation/{registry,session}.rs` + `workbench/src/services/{editor_channels,editor_exec,editor_sources}.rs` + `workbench/tests/oracle_federation.rs`（新） | 单测 435 / 95 / 343（✅）· `ui_contract` 7（✅）· 真机 Oracle × MySQL（✅）· L1 回归（✅） |
| 2026-09-18（第一期收尾后半 —— **源清单浮层**） | **「源清单 ▾」上线（T1.6 收圆）**：① 编辑器新增 `src/sources.rs`——`SourceRow` / `SourceState` / `SourcesSnapshot` + **纯函数** `menu_entries`（汇总行、回退说明、逐行源【失败行不消失，带原话】、动作【重挂全部 / 设为主源（只给可用源）/ 逐源重挂】）与 `button_label`（`源清单：N 源 · 主源 X ▾`），四例单测逐条钉住口径。② 宿主注入 **`SourcesPort`**（只有内存读：`federation::session::snapshot_for` → 界面行；新文件 `workbench/src/services/editor_sources.rs`，含引擎快照 → 界面行的翻译与断言）；渲染路径零 I/O。③ **动作走旁路线程**：`SourceAction`（`RefreshAll` / `RefreshSource` / `SetPrimary`）从「执行队列」的旁路发出（与事务动作同一套），回执 `SourceNote{action, result}` 带**一句可读的做到了什么**（如“已重新挂载 mysql_src（354 张表）”），由轮询泵写进状态栏；`QueryRunner` 相应新增 `refresh_sources`（加速档跟那一条；联邦档给了别名就重挂那一条、否则全挂）与 `set_federated_primary`，旧的 `refresh_accelerated_source` 合并进去（菜单里那项与源清单里的动作同一条路）。④ 工具栏：只在该文档处于**联邦档**时摆「源清单 ▾」（同「重新挂载源库」同一判定）。⑤ **真机验证**（`tests/federation_sources.rs` 扩写）：换主源（引擎快照真值 + 换完照常跨源）· 重挂单源（354 张表）· 全挂（2 个源）· 撤标记后如实拒绝。**验证**：editor 334 → **340**（源清单 4 例 + 窗口级 1 例）· workbench lib 92 → **93** · 全量 editor+workbench **556 项全绿**（含 `ui_contract` 7 项）· 真机 `federation_sources` ✅。⬜ 余：L2 挂载路径（T3.2，Oracle 的会话级 Secret + 无 `READ_ONLY` 的 `ATTACH` + 两段名提示）。 | `editor/src/sources.rs`（新）+ `editor/src/{execution,shared,view/host}.rs` + `workbench/src/services/editor_sources.rs`（新）+ `{editor_exec,view}.rs` + `workbench/tests/federation_sources.rs` | 单测 340 / 93（✅）· 全量 556（✅）· 真机换主源 / 重挂（✅） |
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
| T1.6 | **门控真值 + 源清单 UI**：`editor_channels.rs` 的联邦门控改为真值（≥ 2 个开了开关且可挂的连接）；工具栏「源清单 ▾」浮层（失败行带原因、按源重挂、设为主源） | `crates/workbench/src/services/editor_channels.rs`（✅）+ `crates/editor/src/sources.rs`（新，✅）+ `crates/editor/src/view/host.rs`（✅）+ `crates/workbench/src/services/editor_sources.rs`（新，✅） | ✅ 单测：无源 / 只有自己 / 别人没连上 / 驱动不支持四种原因 · 快照 → 行翻译 · 菜单项（失败行带原话、只给可用源“设为主源”、失败行也能重挂）· 窗口级：只在联邦档出现 · 真机：换主源 / 重挂单源 / 全挂（`tests/federation_sources.rs`） |

> **L2 源的例外**（架构 D14 / §2.1）：门控与挂载对 Oracle 这类 L2 源要额外看两件事——
> 账号有没有权限（建 Secret 与读目标表）、以及“不能带 `READ_ONLY`”带来的写保护缺口。

**第一期验收口径**：编辑器里能把 MySQL 与 SQLite（或 PG）两个源一起挂上，跑一条**跨源 join**，
结果落结果集（徽标 `联邦`）、历史带参与源；其中一个源挂不上时其余照常可用。（✅ 已验：引擎真机探针
`federation_probe` + 工作台真机 `federation_sources`（标记 → 跨源 → 换主源 → 重挂 → 撤标记拒绝）。）

### 第二期 — L3 桥接（拉数 → DuckDB 临时表）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| T2.1 | **条件下推**：`pushdown_predicates` + `sql/filter.rs` 经验；下推不了要明说 | `.../federation/bridge.rs` + `engine/src/sql/` | 单测：能推的推下去（改写后 SQL 可断言）· 不能推的如实返回“全表拉取” |
| T2.2 | **物化**：驱动 Arrow 批 → `analysis.rs` 登记的临时表；分页用 `execute_segment`（可中断）；行数上限 | 同上 | 真机：sqlite 小表 → 临时表 → 与 MySQL 表 join；中断能停 |
| T2.3 | **标注**：结果区标“外部源 X · 已拉 N 行 · 拉取于 HH:MM”（与 L1/L2 的实时读区分） | `crates/editor/src/view/host.rs` | 窗口测试：文案随数据源与截断状态变化 |
| T2.4 | 与 1c 的临时对象共享验证（若 1c 已开工） | —— | —— |

### 第三期 — L2 社区 scanner（**Oracle 真机验收 + 挂载路径均已完成，2026-09-18**）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| T3.1 | **扩展显式管理**：按源装扩展（显式动作 + 进度 + 失败原话）；`extension_directory` 钉应用目录 | `.../federation/session.rs` + `duckdb/extensions.rs` | ✅ 社区扩展源已接（`install_sql`：L2 带 `FROM community`，`accel` 与联邦共用）；⬜ 会话侧的“装机进度 + 显式安装动作”未做（现在仍是执行时装、失败原因入状态门控） |
| T3.2 | **L2 挂载路径（D14）**：会话内建**会话级** Secret（`TYPE ORACLE` + HOST/PORT/USER/PASSWORD/SERVICE_NAME）→ `ATTACH '<secret>' AS <别名> (TYPE oracle_scanner)`（**不带 `READ_ONLY`**）；引擎侧补一道“写 L2 源就被拒”的判定；提示语按源类型给两段 / 三段名 | `.../federation/{session,registry}.rs` + `crates/workbench/src/services/editor_exec.rs` | ✅ 单测：Secret 解析（正常 / 缺服务名 / 缺主机 / 端口非数字 / 百分号凭据 / `Debug` 不泄口令）· L2 `attach_sql` 不带 `READ_ONLY` · 写拒绝三拒两放 · 门控（L2 算联邦源、不算本地加速）· 组装（L2 组装不出来不阻断）；真机：`tests/oracle_federation.rs`（Oracle × MySQL 跨源 · 挂载 · 重挂 · 写拒绝 · 本地临时对象） |
| T3.3 | **真机验收（Oracle ✅ / SQL Server 待用户通知）** | —— | ✅ Oracle（`192.168.3.138:1521` / `XEPDB1`）：装/载 · Secret · 目录两段名 · 表函数 · 跨源 × SQLite（台账 `oracle_probe.rs`，架构 §2.1）· **跨源 × MySQL 走产品执行路径**（`oracle_federation.rs`）；⏳ SQL Server 待通知 |

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
11. 门控：无源 / 有源 / 扩展未装 三种状态下的「执行位置」表现；
12. **L2 源**：连接串缺件（凭据 / 服务名）时的组装提示；一条 SQL 同时读 L1 与 L2；写 L2 被拒且点名源；重挂后 Secret 重建（✅ 已由 `oracle_federation.rs` 覆盖）。

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
