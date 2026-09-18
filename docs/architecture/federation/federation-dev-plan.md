# 联邦查询模块 · 开发方案（第一期 / 第二期 / 第三期）

> 状态：**设计定稿 + 目录落位已完成；第一期（多源挂载）引擎侧已落地（2026-09-18）**——进度见 §0 表（最近在前）。
> 关联：`README.md`（入口与硬约束）、`federation-prototype-design.md`（长什么样）、
> `federation-architecture.md`（为什么这样设计，§8 已知问题为权威）。
> **Oracle / SQL Server 的真机验收等用户通知**（驱动未装）；第三期代码可先写，验收口径写死“待真机”。

---

## 0. 进度记录（最近在前）

| 日期 | 内容 | 状态 |
| --- | --- | --- |
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
| 源登记（“哪些连接用作联邦源”） | ⛔ 无 | 只有连接体系；缺一列标记与源清单快照 |
| 联邦档的执行路径 | ⛔ 无 | `editor_channels.rs` 门控如实写着“尚未注册外部源” |
| L3 桥接 | ⛔ 无 | 驱动与 Arrow 批已有；缺“下推 + 物化 + 上限”的编排 |

---

## 2. 分期与任务表

### 第一期 — 多源挂载（可真机验收：MySQL / PG / SQLite / DuckDB）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| T1.1 | **目录收拢**（已完成） | `engine/src/duckdb/federation/{mod,legacy}.rs` | legacy 测试全绿（✅ 11 项） |
| T1.2 | **源登记**：连接表加“用作联邦源”标记（幂等补列）；`SourceRegistry` 快照（别名 / 状态 / 表数 / 刷新时间）+ 别名重名检测 | `.../federation/registry.rs`（形状 ✅ 已落：`FederatedSource` / `MountState` / `SessionSnapshot` / 别名三件套）+ `engine/src/persistence/workbench_context_store.rs`（🟡 存储未接） | 单测：标记往返 · 快照是内存读（无 I/O） · 重名检测给出冲突别名（✅ 形状已测） |
| T1.3 | **多源会话**：`FederatedSession::open/run/refresh/set_primary`——逐源 `ATTACH … (READ_ONLY)`，**失败不中断**（记原因继续）；主源 `USE`；按源 `DETACH`+`ATTACH`；会话资源边界（`memory_limit` / `temp_directory` / `max_temp_directory_size`，统一过 `configure_connection`） | `.../federation/session.rs`（✅ 已落，复用 `accel` 的 `mount_lock` / `run_sql_on`） | ✅ 真机：MySQL + SQLite 同会话 + 一条 SQL 跨两源 · 坏源带原因保留 · 写源库被拒 · 主源可切（`tests/federation_probe.rs`） |
| T1.4 | **主源与表名解析**：未限定名只在主源；两源同名 → 报错（错误卡片给候选，见原型 §3） | `.../federation/session.rs` + `crates/editor/src/execution.rs`（错误解析） | 真机：同名表场景报错且信息可读；限定名可跨源 join |
| T1.5 | **执行路径**：`QueryRunner` 的 federated 分支在联邦会话上跑（复用 `run`/`run_filtered` 的分流形状）；历史带**参与源清单** | `crates/workbench/src/services/editor_exec.rs` + `engine/src/persistence/history_store.rs` | 真机：编辑器里选联邦档 → 跨源 join 出结果（徽标 `联邦`）· 历史能看到参与源 |
| T1.5b | **扩展状态门控**（**取代原先“建 `engine_extensions` 登记表”的想法**）：扩展“装没装”的**真值在 DuckDB 自己身上**（`duckdb_extensions()` 的 `installed` / `loaded`）——再建一张镜像表只会产生“表说已装、实际没有”的偏差（单一权威）；我们只保留**动作与失败原因**（进程内；跨重启重试本来就合理，环境可能变了）+ 官方 / 社区清单（常量） | `.../federation/session.rs` + `duckdb/{extensions.rs, accel.rs}` | 单测：未装扩展时如实报“扩展 X 未安装”，**不静默联网**；真机：装成功后状态翻转（`duckdb_extensions()` 是真值） |
| T1.6 | **门控真值 + 源清单 UI**：`editor_channels.rs` 的联邦门控改为真值（DuckDB 就绪 + 已注册源 ≥ 1）；工具栏「源清单 ▾」浮层（失败行带原因、按源重挂、设为主源） | `crates/workbench/src/services/editor_channels.rs` + `crates/editor/src/view/host.rs` + 新视图文件（登记进 `ui_contract` 两份清单） | 窗口测试：无源时置灰并给原因 · 有源可选且状态栏显示主源 · 失败源保留在清单里 |

**第一期验收口径**：编辑器里能把 MySQL 与 SQLite（或 PG）两个源一起挂上，跑一条**跨源 join**，
结果落结果集（徽标 `联邦`）、历史带参与源、状态栏显示主源；其中一个源挂不上时其余照常可用。

### 第二期 — L3 桥接（拉数 → DuckDB 临时表）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| T2.1 | **条件下推**：`pushdown_predicates` + `sql/filter.rs` 经验；下推不了要明说 | `.../federation/bridge.rs` + `engine/src/sql/` | 单测：能推的推下去（改写后 SQL 可断言）· 不能推的如实返回“全表拉取” |
| T2.2 | **物化**：驱动 Arrow 批 → `analysis.rs` 登记的临时表；分页用 `execute_segment`（可中断）；行数上限 | 同上 | 真机：sqlite 小表 → 临时表 → 与 MySQL 表 join；中断能停 |
| T2.3 | **标注**：结果区标“外部源 X · 已拉 N 行 · 拉取于 HH:MM”（与 L1/L2 的实时读区分） | `crates/editor/src/view/host.rs` | 窗口测试：文案随数据源与截断状态变化 |
| T2.4 | 与 1c 的临时对象共享验证（若 1c 已开工） | —— | —— |

### 第三期 — L2 社区 scanner（**代码可先写，真机待用户通知**）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| T3.1 | **扩展显式管理**：关掉自动安装/自动加载；按源装扩展（显式动作 + 进度 + 失败原话）；`extension_directory` 钉应用目录 | `.../federation/session.rs` + `duckdb/extensions.rs` | 单测 + 真机：未装扩展时该源报“扩展未安装”而不是静默联网 |
| T3.2 | **社区 scanner 挂载路径**：`mssql`（`ATTACH 'Server=…' AS ms (TYPE mssql)`）与 `oracle_scanner`（**先确认是 ATTACH catalog 还是表函数**，架构 §8 #2） | 同上 | ⏳ 待真机 |
| T3.3 | **真机验收**（SQL Server 2022 / Oracle XE 或用户提供的端点）：连接 · 跨源 join · 下推是否生效 · `READ_ONLY` 是否被支持（架构 §8 #1） | —— | ⏳ 待用户通知 |

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
