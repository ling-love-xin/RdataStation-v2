# 分析资源与会话模型（口径草案 · 待审）

> **状态**：口径草案 + 落地记录（2026-11-27）。文中每条都标了归属，看清标记再判断跑没跑偏：
> - 【已定】＝用户已明确拍板的口径，不要再翻。
> - 【已落地】＝按本文落进代码，并给了编译/实测依据（哪一刀见括注）。
> - 【建议】＝本文推出的一步，**尚未得到确认**，可能就是我理解偏的地方。
> - 【待定】＝有两个以上合理选项，需要人拍。
>
> **基线**：2026-11-26 首次落笔（`main` = `b7571511`）；2026-11-27 落地在隔离分支
> `feat/analysis-session-owner`（worktree `D:\RdataStation\RDS\rds-v2-session-owner`）。所有代码锚点为当次实测，改前重读文件。
>
> **覆盖范围**：分析引擎侧（`engine/src/duckdb`、`engine/src/analysis`）+ 数据对象面板 + 编辑器通道。**不覆盖** Source 档（网络驱动）的执行上下文，那份见 [`connection/execution-context-model.md`](connection/execution-context-model.md)。

---

## 0. 一句话结论

**会话 = 一份工作的执行上下文；临时表 = 实例资源；可见性 = 会话级参数。**

三者分开之后：会话随通道变、随文档走；临时表不随会话走，寿命看谁生的；底座挂不挂只是可见性，不是另一种会话。

现状的偏移不在语义，而在**实现有三套并行**：分析实例会话、（历史遗留的）连接级加速会话、资源归档的独立连接。归并的终点是一套：一个实例、一种会话类型、三种临时表寿命。

---

## 1. 名词收紧

| 词 | 定义 | 不是什么 |
|---|---|---|
| **分析实例** | 进程内唯一的 DuckDB 内存实例（`AnalysisInstance`），catalog 共享、TEMP 各会话独立（`instance.rs` 测试 `scratch_only_here`） | 不是"每个编辑器一个内存库" |
| **会话** | 实例上的一条连接（`try_clone`），带 owner、label、挂载集合、`USE` 主源、`search_path`、中断 | 不是"一次查询"，不是"一个 tab" |
| **临时表** | 实例 `memory` catalog 里的 `rds_tmp` schema 普通表（跨会话可见） | 不是会话 TEMP（TEMP 会话私有，见 §4） |
| **可见性** | 某会话能不能用裸名看到底座（`analytics` / `rds_global`） | 不是"底座存不存在" |
| **资源归档** | 独立持久连接，读资源库（文件 / 全局分析库 / 项目分析库），后期可卸载 | 不是分析实例的别名 |

---

## 2. 四类模块

| # | 模块 | 职责 | 载体 | 现状 | 差距 |
|---|---|---|---|---|---|
| 1 | **资源归档** | 持久连接看资源库（文件 / 全局 / 项目分析库），后期支持卸载释放 | 自持 `duckdb::Connection::open` 并缓存（`analytics_resource/global/sql.rs:47`） | 已是独立持久连接 | ① 与实例 `ATTACH … READ_ONLY` 同一文件**双开**，需真机验；② 卸载语义未定义 |
| 2 | **洞察** | 只做表 EDA 探查，**没有下游**，对象只有临时结果集 | 无独立会话；`insight` 全是 `_on(conn)` 形态，连接由调用方传 | `tmp_i_` 进 `rds_tmp` + 前缀清理 | 与"没有下游"不符：应私有化（见 §4） |
| 3 | **mock** | faker 生成数据，DuckDB 只负责落地到草稿箱/系统管理器 | 进程面 `DuckDBManager` 的 in-memory 临时表（`mock/src/engine/mod.rs:533`，`tmp_m_`） | 与编辑器**同一实例** | 【已落地·刀16】落地成功即删这张临时表（失败保留供重试）；面板不列 `tmp_m_` |
| 4 | **数据对象面板 + SQL 编辑器** | 核心工作台 | 编辑器：`ensure_session(doc_owner)`；面板：**无会话**，借假 owner `C_data_objects`（`mod.rs:1550`） | 见 §3–§5 | 面板 owner、加速档 owner、单源联邦、底座可卸 |

---

## 3. 通道三档归一

【已定】本地加速 = 单源联邦（无底座）。机制上一查即知：`accel::ensure_on_instance`（`accel.rs:456-474`）做的就是

```
session_for_conn(conn_id) → mount_sources([1 个源]) → use_catalog(alias)
```

用的就是联邦那份 `FederatedSource`，没有第二套挂载机制，也没有复制/物化。

所以三档的差异**只有参数**：

| 档 | 源集合 | 主源 | 底座可见性 | 会话 owner |
|---|---|---|---|---|
| 源库（驱动直跑） | — | — | — | 无会话（`SessionHandle::None`） |
| 本地加速 | 1 个 | `rds_src` | **不可见** | `{doc}`【已落地·刀11a】 |
| 联邦 | N 个（单源也合法） | 计划主源 | **默认挂、可人为卸** | `{doc}`【已落地·刀11a/11b】 |

底座"不可见"的实现口径（刀11b）：`use_catalog` 切主源时把 `search_path` 设成
`'<主源>.main,memory.rds_tmp'`——主源与临时表按裸名可用，`analytics` / `rds_global`
不在默认路径里；显式 `analytics.x` 仍可达。

【已落地·刀11a】单源联邦放开：`AnalysisChannel::from_toggle` 的 `>= 2` 改 `>= 1`，
`editor_channels.rs` 的 `ready.len() >= 2` 改 `!ready.is_empty()`，文案改"至少需要一个"。

【已落地·刀22】「哪些连接算联邦源」定成**有值即算**：`use_duckdb_fed` 三态里
`Some(true)`（已开本地加速）与 `Some(false)`（可直连、没开加速）都算源，`None`（驱动不支持直连）不算。
此前三处只认 `Some(true)`，而数据面板按 `fed_enabled = use_duckdb_fed.is_some()` 说「能挂」，两套口径打架：

- 门控 `editor_channels.rs::federated_availability`：跳过条件 `!= Some(true)` → `is_none()`；
- 记录组装 `editor_host.rs::WorkbenchFederatedSource::records`：`== Some(true)` → `is_some()`；
- 运行态补源 `editor_exec/federation.rs::federated_plan`：同上。

原注释「`Some(false)` 等用户手动挂载/登记再进场」是**空承诺**——`records()` 从不读 `analysis_sources`，
`Some(false)` 的连接没有任何入口能进联邦。配套两件：`project::service::ensure_sample_connections` 补
`ensure_conn_registered`（示例播种直写全局连接，绕过了唯一登记点，示例项目 `analysis_sources` 一直是 0 行）；
开关文案统一成 UI 真名「DuckDB 直连（本地加速 / 用作联邦源）」。

【已落地·刀10/11a】会话 owner 一律 `{doc}`：
- 加速档 `accel::ensure_on_instance(source, owner)` 用 `ensure_session(owner, "本地加速")`
  （原 `session_for_conn(&source.conn_id)`）；
- 联邦档 `FederatedPlan` 拆出 `session_owner`（文档）与 `owner`（本文档绑定连接，只用于判主源与写历史 `conn_id`）；
- `QueryRunner` 五条路径（`run` / `fetch_next` / `run_filtered` / `run_sorted_down` / `refresh_sources`）与 `set_federated_primary` 全部加 `document`，队列从 `ExecJob.document` 透传。

【建议·未做】归并清单（剩下的）：

- 删 `kernel::open` 的 `Accelerated` 分支（`kernel.rs:153`），统一走 `Federated{owner}` + 参数 `{sources: [1 个], primary: rds_src, visibility: BasesHidden}`；
- `accel::ensure_on_instance` 降级为**能力函数**（只保证扩展就绪 + 源可挂），不再开会话；
- `accel_owner` / `session_for_conn` 退役（现在只剩内核加速档还按源 `conn_id` 开会话：
  那一路调用方——洞察 / 引擎内部——手里没有文档身份，见 `kernel.rs` 注释）；
- 保留 `rds_src` 作为单源加速的**默认别名**（用户 SQL 里 `rds_src.x` 继续可用），但由统一别名分配产出。

归并后"加速"与"联邦"在代码里只差一个 `visibility` 参数。

---

## 4. 临时表的三种寿命

【已定】"从各个地方进入的临时表都是实例共享，下游一般只有数据面板"。这决定**编辑器的结果集不能是会话 TEMP**，必须是 `rds_tmp` 普通表（否则切通道换会话就丢）。

由此三类分开：

| 类别 | 存放 | 可见性 | 寿命 | 现状 |
|---|---|---|---|---|
| 编辑器结果集（`tmp_q_` / `tmp_l_`） | `rds_tmp` 普通表 | 实例共享，面板可见 | 【已落地·刀14】绑**文档**（owner 登记，关文档删这一批） | `TempTableManager` 值改成 `TempEntry { created, owner }`，`execution_service → duckdb_service → manager → 实例登记` 全程透传文档 id；关文档 `on_removed` 调 `drop_temps_of(owner)` |
| 洞察（`tmp_i_`） | `rds_tmp` 普通表 | 私有，不进面板 | 随清理兜底 | 曾建议"会话 TEMP"，但【已定】临时表实例共享优先，故仍落 `rds_tmp`；"私有"靠不进面板 + `lazy_cleanup_insight_tables`；owner 固定 `insight` |
| mock（`tmp_m_`） | 一次性连接 | 无 | 【已落地·刀16】落地成功即收 | 走进程面 `DuckDBManager`，与编辑器同实例；owner 固定 `mock`；`write_temp_table_to_database` 成功返回前删掉这张临时表并销登记（失败保留，重试还要它）。**"落地"只指写进项目分析库**：导出文件 / 存草稿箱后仍留着，用户还能接着换另一个出口（2026-11-27 拍板） |

【已落地·刀11b】落点钉 catalog：

- `qualified()` 改生成 `memory.rds_tmp."名"`，`ensure_shared_schema` 改
  `CREATE SCHEMA IF NOT EXISTS memory.rds_tmp; SET search_path = 'memory.main,memory.rds_tmp'`；
  实例控制连接与每条会话都用它（不再各拼一份）。
- **实测（2.0.1）**：只写 schema 名的路径会跟着 `USE` 漂——`USE src` 之后
  `SET search_path = 'main,rds_tmp'` 直接报 `No catalog + schema named "rds_tmp" found`
  （相对名被解析成 `src.rds_tmp`）；而 `SET search_path = 'memory.rds_tmp,src.main'`
  可用，两侧表都能按裸名访问。所以第 3 条"待实测"已闭合：**接受 catalog 限定写法**，
  裸名 `tmp_q_x` 在切主源之后照样查得到。
- 探针是一次性的（`crates/engine/examples/sp_probe.rs`，已删，不入库）。

【已落地·刀14】owner 登记：`TempTableManager` 的 registry 值从 `Instant` 改成
`TempEntry { created, owner }`，`register(name, owner)` 带归属；`TempTableSource::default_owner()`
给模块自建表定默认归属（`Query/Load → process`、`Insight → insight`、`Mock → mock`、`Plugin → plugin`）。
文档身份沿三条链透传到底：

- 结果集：`QueryRunner::{run/run_filtered/run_sorted_down/analyze}` → `ResultService` →
  `execution_service` → `duckdb_service` → `manager::register_temp_table(name, owner)`
  → `AnalysisInstance::register_temp(name, owner, …)`；owner = 文档 id。
- 手动物化（"存为结果集" / 排序物化）：`sort.rs` 直接传 `self.document.as_str()`。
- 洞察 / mock / 导出：各传自己的固定 owner（`insight` / `mock` / `process`）。

收口只有一处：`AnalysisInstance::drop_temps_of(owner)` —— 名单取**实例登记册 ∪ 临时表管理器**
（两处并集，控制连接重开过时只查一处会漏），逐张复用 `drop_temp`（同一落点 + 销登记），
失败不阻断其余表。调用点在编辑器 `on_removed`（`crates/editor/src/view/host/panel_traits.rs`）：
关文档 → `drop_temps_of(document)` → 再关这条会话（会话的别名 / ATTACH 随连接消失；
`rds_tmp` 里的表**不因关会话消失**，这是"临时表 = 实例资源"的直接体现）。

单测：`dropping_temps_of_one_owner_spares_the_others`（A 的表收掉、B 的表与登记完好）。

---

## 5. 数据对象面板

### 5.1 段划分（【已定】维持现状）

源 / 表 / 文件三段（刀7 起常驻，空段也画「源 0」），临时表在**结果集段**（`result_sets` / `loaded_tables` / `insight_samples`）。

这个结构正好满足两条看似冲突的口径：

- "卸了底座，表段默认就空" —— 表段只看底座；
- "面板能看到临时表" —— 临时表在结果集段。

【已定】**不要把临时表并进表段**，否则卸底座后表段就不空了。

### 5.2 源段

【已定】「挂载 / 卸载」是**一颗两态按钮**，只在源段，给**每个能连 DuckDB 的连接**（刀9 后判据 = `attach_capable && fed_enabled`）。

【已定】没有"联邦源"概念：点联邦通道后整个面板都可联邦；面板源分"有能力挂载 / 无能力挂载"两档。

【已定·原表述更正】面板**有自己的一条会话**（口径 11 第②条"每个 SQL 编辑器 + 数据面板 = 一条会话"），
owner 固定 `data_objects`（`data_objects_load.rs:455`，本地加速挂载用它开会话）。
`C_data_objects`（`mod.rs:1561`）是面板查全局库用的**连接 id**，不是 owner——原先把两者写成
"借假 owner"是笔误。编辑器与面板之间的临时表共享靠**实例资源**（`memory.rds_tmp`），不靠共用会话，
所以面板不必去追当前聚焦文档的 id。

### 5.3 表段

【已定】表段 = **当前挂着的底座里的业务表**（项目分析库业务表 + `G_*`）。`G_*` 出现是因为联邦默认挂全局底座；卸了即空。

【已定】去掉洞察（`tmp_i_`）、插件等表。

【已落地】"业务表"判据取**白名单**（`res_*` 前缀，`split_persistent_tables` 已实现）：
新前缀不会因为"没被排除"而偷偷漏进表段。`analysis_tables_of`（`source_picker/load.rs:196`）
只供**选择器**用（列 `main` 全部表，不做业务表判定）。

### 5.4 底座行

现状 `base:analytics` / `base:rds_global`，pill「底座」。

【已定】底座默认挂上、**可人为卸载**。
【已落地·刀11b】"可卸"与"本地加速看不见底座"用**同一个机制**：会话级可见性
（`use_catalog` → `<主源>.main,memory.rds_tmp`，底座不进 `search_path`；显式 `analytics.x` 仍可达）。
会话换通道时 `retain_mounted` 只清本会话的挂载记录（不 `DETACH`）——`ATTACH` 是实例级事实，
真 `DETACH` 会波及别的文档正在跑的联邦。
【已落地·刀13】卸载的**悬空收尾**：`AnalysisInstance::unmount_conn` 卸掉一个源之后，
逐个会话摘掉该别名（`forget_mounted`）；凡是以它为「主源」的会话调 `reset_search_path`
（检测到 catalog 不在 → 清主源记录、写 `primary_note`、退回 `memory.rds_tmp`）。
没有这一步，卸载之后那些会话的未限定名查询会一直报 catalog 不存在。
【已落地·刀15】"卸载底座"定为**会话视角遮蔽**（不 `DETACH`）：

- 引擎：`AnalysisInstance::{hide_base, show_base, hidden_bases, base_visible}`，记录在
  `InstanceState.hidden_bases`；只认 `federation::session::bases()` 里的别名，源别名一律拒。
- 面板：`Collected.hidden_bases` 直读实例；底座行改为「已卸载 · 表段不再列它」并给
  `SourceAction::ToggleBase { alias, hidden }` 一颗两态按钮（卸载 ⇄ 挂回）；
  项目底座遮蔽时 `analysis_tables` / 资产行来源清单清空，全局底座遮蔽时 `global_tables` 清空
  ——"卸了就空"落成同一份判断，不在四个循环里各加 if。
- 边界如实写进行文案：**显式限定名 `analytics.表` 仍可达**（会话级遮蔽挡不住裸限定名，
  DuckDB 侧也没有这个开关）。单测 `hiding_a_base_empties_its_table_rows`。
【已落地】§5.3 的表段判据定为**白名单**（`res_*` 前缀，`split_persistent_tables` 已实现），
不做黑名单：新前缀不会因为"没被排除"而偷偷漏进表段。

---

## 6. 现状差距清单

| # | 差距 | 锚点 | 归属 |
|---|---|---|---|
| 1 | 加速档会话按连接，跨文档共享 `USE` / 中断 | `accel.rs:460`、`instance.rs::accel_owner` | 【已落地·刀10/11a】按文档 `ensure_session` |
| 2 | 加速档看得见底座 | `accel.rs::ensure_on_instance` | 【已落地·刀11b】`search_path` 不含底座 |
| 3 | 单源联邦被 ≥2 卡住 | `analysis/channel.rs::from_toggle`、`editor_channels.rs` | 【已落地·刀11a】`>= 1` |
| 4 | 临时表无 owner，关文档不删 | `temp_table.rs`（只有 `drop_by_source`） | 【已落地·刀14】`TempEntry{created,owner}` + 全链透传文档 id + `drop_temps_of` |
| 5 | 临时表落点不钉 catalog | `temp_schema.rs` | 【已落地·刀11b】`memory.rds_tmp`（实测可用） |
| 6 | 卸载源之后主源会话悬空 | `instance.rs::unmount_conn` | 【已落地·刀13】`forget_mounted` + `reset_search_path` |
| 7 | 洞察 `tmp_i_` 进共享 schema，与"没有下游"不符 | `duckdb/temp_table/analysis.rs` | 【建议】落点已钉 memory；面板已不列（`objects()` 注释 903-908） |
| 8 | mock 与编辑器同实例，面板还列 `tmp_m_` | `mock/src/engine/mod.rs:533` | 【已落地·刀16】面板不列（2026-11-18 口径）；落地成功即删临时表 + 销登记（失败保留供重试） |
| 9 | 资源归档与实例双开同一 `analytics.duckdb` | `analytics_resource/global/sql.rs:47` vs `attach_bases` | 【待定】需真机验 |
| 10 | 表段不按业务表过滤、恒无资产行 | `source_picker/load.rs:196`、`data_objects/mod.rs:764` | 【已落地】表段名单 = `analysis_tables_of()`（项目分析库 `main` 全表，即"底座里有什么就列什么"），按 `res_` 白名单切「持久表 / 项目自建」两段；资产·表型本体已被 `asset_tables` 从表段剔除、只从资产行进（pill「资产」、只读、带版本） |
| 11 | 内核加速档拿裸 `conn_id` 当会话 owner → 洞察跑一次加速，面板源段多一行「来源不明」 | `analysis/kernel.rs` 加速档、`instance.rs::is_analysis_session` | 【已落地·刀17】owner 定源级 `accel:{conn_id}`；`ACCEL_OWNER_PREFIX` + `is_accel_owner()` 单点判定 |
| 12 | 联邦门控/执行只认 `Some(true)`，面板按 `is_some()` 说「能挂」；示例项目 `analysis_sources` 0 行 | `editor_channels.rs`、`editor_host.rs`、`federation.rs`、`project/src/service.rs` | 【已落地·刀22】三处判据统一 `is_some()`；示例播种补 `ensure_conn_registered`（幂等，实测 0 → 2 行） |

---

## 7. 与既有文档的关系

- [`connection/execution-context-model.md`](connection/execution-context-model.md)（2026-10-23）覆盖 **Source 档网络驱动**的执行上下文，本文不重复。其中两句已被本轮推翻，**读那份时按本节为准**：
  - §0.4「Accelerated 维持按 `conn_id` 复用本地会话」→ 作废，见 §3；
  - §5「不做清单：Accelerated / Federated 不拆 per-editor 独立实例」→ 作废，见 §3。
  - 该文所列 `SESSIONS` 缓存、源指纹复用键均已删除（刀2b `5c400152`），文中相应段落只作历史对照。
- [`layout/data-objects-invariants.md`](layout/data-objects-invariants.md)：面板不变量，本文 §5 是其上层模型。
- [`layout/analysis-sources-spec.md`](layout/analysis-sources-spec.md)：注册 vs 挂载的区分，本文沿用。
- [`database/analysis-instance-sessions.md`](database/analysis-instance-sessions.md)：实例与会话的机制细节。
- [`federation/`](federation/)、[`editor/`](editor/)：通道与执行器落点。

---

## 8. 待拍板项

| # | 问题 | 建议 | 状态 |
|---|---|---|---|
| 1 | 加速档会话 owner 改文档级？ | 改。不改则 `USE` / 中断跨文档串 | 已定·刀10/11a |
| 2 | 洞察是否改用会话 TEMP？ | ~~改~~：与"临时表实例共享"冲突，**维持落 `rds_tmp`**，只把它挡在面板外 | 已修口径 |
| 3 | 编辑器结果集是否加 owner 登记、关文档删？ | 加。否则"关 A 就不能用"不成立 | 已定·刀14（全链透传文档 id） |
| 4 | 临时表落点钉 `memory.rds_tmp`？ | 钉。实测 `search_path` 接受 catalog 限定写法 | 已定·刀11b |
| 5 | 面板 owner 取当前编辑器文档 / tab / 独立？ | 面板 = 自己的那条会话（口径 11 第②条），owner 固定 `data_objects`；跨模块共享靠实例临时表 | 已定·原表述更正 |
| 6 | 底座卸载：会话级遮蔽 vs 真 `DETACH`？ | 会话级遮蔽（显式限定名挡不住，需说清） | 已定·刀15（`hide_base` / `ToggleBase`） |
| 7 | 表段"业务表"判据：白名单 vs 黑名单？ | 白名单（`res_*` + 资产表），避免新前缀偷偷漏进来 | 已定·刀15（`res_*` 白名单维持） |
| 8 | mock 是否退出面板可见范围？ | 退出，只留落地结果（落地成功即删临时表，见刀16） | 已定·刀16 |
| 9 | 资源归档是否改为"卸载时才开、读完即关"？ | 先解决双开问题，再定缓存策略 | **仍待拍**（需真机验） |

---

## 9. 变更记录

| 日期 | 变更 |
|---|---|
| 2026-11-26 | 初稿：从"联邦内核收口 + 数据对象面板"讨论中抽出会话/临时表/可见性三层；标出对 `execution-context-model.md` 两处作废口径 |
| 2026-11-27 | 落地记录回填：刀10（加速 owner 按文档）、刀11a（联邦 owner 拆 `session_owner`、单源联邦放开）、刀11b（`search_path` 全限定 + `retain_mounted` + `reset_search_path`）；补 2.0.1 实测结论（`memory.rds_tmp` 可作 `search_path` 项）；洞察口径由"会话 TEMP"修正回 `rds_tmp`（实例共享优先） |
| 2026-11-27 | 刀13：`unmount_conn` 卸源后收尾悬空会话（`forget_mounted` + `reset_search_path`）；更正"面板借假 owner `C_data_objects`"的笔误——面板 owner 是固定 `data_objects`，`C_data_objects` 只是查全局库的连接 id |
| 2026-11-27 | 刀14：临时表 owner 登记落地——`TempTableManager` 值改 `TempEntry{created,owner}`，`register/ list_by_owner / owner_of`；`TempTableSource::default_owner()` 定模块默认归属；`execution_service → duckdb_service → manager → instance.register_temp` 全链透传文档 id（`QueryRunner` 五条执行路径 + `ResultService` + `sort.rs` 手动物化）；新增 `AnalysisInstance::drop_temps_of(owner)`（实例册 ∪ 管理器册并集），关文档在 `on_removed` 收批并关会话 |
| 2026-11-27 | 刀15：底座"卸载"落地为会话视角遮蔽——引擎 `hide_base/show_base/hidden_bases/base_visible`（`InstanceState.hidden_bases`，不 `DETACH`）；面板底座行给 `SourceAction::ToggleBase{alias,hidden}` 一颗两态按钮，遮蔽时清空项目/全局表来源清单（表段即空）；表段白名单（`res_*`）确认为已定；单测 `hiding_a_base_empties_its_table_rows` |
| 2026-11-27 | 刀16：mock "落地完即关"——`write_temp_table_to_database` 在写入与解挂都成功后收掉这张临时表（有实例走 `AnalysisInstance::drop_temp`，无实例退回进程面内存库自删 + 销登记），失败路径保留供重试；`MockGenInfo` 自带预览快照，落地后结果区显示不受影响 |
| 2026-11-27 | 校准 §6-10：表段名单一直是 `analysis_tables_of()`（底座全表），不是"不按业务表过滤"；资产本体已从表段剔除、只从资产行进——原【待定】改【已落地】 |
| 2026-11-27 | 拍板刀16 边界：**"落地完即关"只覆盖写进项目分析库**（落库 / 追加）；导出文件、存草稿箱不删临时表，让用户还能继续换出口。无临时表残留只靠这一条 + 切项目 `clear_temp_tables` 兜底 |
| 2026-11-27 | 刀17（D17）：加速档 owner 词表收口——内核 `kernel::open` 的 `Accelerated` 分支不再用裸 `conn_id` 当 owner，改源级 `accel:{conn_id}`（与 `session_for_conn` 同键）；`ACCEL_OWNER_PREFIX` / `is_accel_owner()` 成唯一判定点（残键 `accel:` 不算），`AnalysisChannel::Accelerated` 的"会话按源 id 缓存"注释按刀10 重写 |
| 2026-11-27 | 刀18：示例项目自带双数据源——`project::service::ensure_sample_data` 用**当前内核的 duckdb crate** 生成 `data/sample.sqlite` + `data/sample.duckdb`（2.0.1 写，`orders` 12 行实测可读），`ensure_sample_connections` 登记两条全局连接（`use_duckdb_fed=false`），`create_sample` 每次进入幂等跑一遍；不再依赖仓库里 1.5 时代的 `examples/instance-project` |
| 2026-11-27 | 刀22（D22）：联邦源判据由「只认 `Some(true)`」改为「`use_duckdb_fed` 有值即算」（门控/记录/运行态三处对齐，与面板 `fed_enabled = is_some()` 一致）；开关文案统一为「DuckDB 直连（本地加速 / 用作联邦源）」；示例播种补 `ensure_conn_registered`（实测 `analysis_sources` 0 → 2 行，别名 `sqlite` / `duckdb`） |
