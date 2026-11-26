# 分析资源与会话模型（口径草案 · 待审）

> **状态**：口径草案，**不含任何实现**，等审。文中每条都标了归属，看清标记再判断跑没跑偏：
> - 【已定】＝用户已明确拍板的口径，不要再翻。
> - 【建议】＝本文推出的一步，**尚未得到确认**，可能就是我理解偏的地方。
> - 【待定】＝有两个以上合理选项，需要人拍。
>
> **基线**：2026-11-26，`main` = `b7571511`。所有代码锚点为当次实测，改前重读文件。
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
| 3 | **mock** | faker 生成数据，DuckDB 只负责落地到草稿箱/系统管理器 | 进程面 `DuckDBManager` 的 in-memory 临时表（`mock/src/engine/mod.rs:612`，`tmp_m_`） | 与编辑器**同一实例**，且面板 `source_kinds` 列了 `Mock`（`data_objects/mod.rs:1025`） | 落地完即关；面板不该看到 `tmp_m_` |
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
| 本地加速 | 1 个 | `rds_src` | **不可见** | 【建议】`{doc}` |
| 联邦 | N 个 | 计划主源 | **默认挂、可人为卸** | `{doc}` |

【已定】现状三处偏移：

1. **加速档会话是连接级**（`accel:{conn}`，`instance::accel_owner`）。两个编辑器用同一连接加速 → 同一条会话 → `USE` / `search_path` / 中断互相污染。这是真 bug，不是省资源。
2. **加速档也是底座可见**：`ensure_on_instance` 从不摘底座，底座又是实例级 `ATTACH`（`attach_bases`），所以加速档当然看得见。是漏做。
3. **单源联邦被卡住**：计划侧 `plan_from_records` 明确"只有一个源也照样联邦"，但 `editor_channels.rs:204` 要求 `== Some(true)` 的连接 **≥2** 才可用。

【建议】归并清单：

- 删 `kernel::open` 的 `Accelerated` 分支（`kernel.rs:153`），统一走 `Federated{owner}` + 参数 `{sources: [1 个], primary: rds_src, visibility: BasesHidden}`；
- `accel::ensure_on_instance` 降级为**能力函数**（只保证扩展就绪 + 源可挂），不再开会话；
- 会话 owner 一律 `{doc}`，`accel_owner` / `session_for_conn` 退役；
- 保留 `rds_src` 作为单源加速的**默认别名**（用户 SQL 里 `rds_src.x` 继续可用），但由统一别名分配产出。

归并后"加速"与"联邦"在代码里只差一个 `visibility` 参数。

---

## 4. 临时表的三种寿命

【已定】"从各个地方进入的临时表都是实例共享，下游一般只有数据面板"。这决定**编辑器的结果集不能是会话 TEMP**，必须是 `rds_tmp` 普通表（否则切通道换会话就丢）。

由此三类分开：

| 类别 | 存放 | 可见性 | 寿命 | 现状 |
|---|---|---|---|---|
| 编辑器结果集（`tmp_q_` / `tmp_l_`） | `rds_tmp` 普通表 | 实例共享，面板可见 | 【建议】绑**文档**（owner 登记，关文档删这一批） | 只按前缀管，**无 owner**，关编辑器后仍可访问（与"关 A 就不能用"相反） |
| 洞察（`tmp_i_`） | 【建议】**会话 TEMP** | 私有，不进面板 | 随会话销毁 | 进 `rds_tmp` + `lazy_cleanup_insight_tables` 兜底 |
| mock（`tmp_m_`） | 一次性连接 | 无 | 落地完即关 | 走进程面 `DuckDBManager`，与编辑器同实例 |

【建议】两个机制性修正：

1. **落点钉 catalog**：`qualified()` 只生成 `rds_tmp."名"`（`temp_schema.rs:29-31`，**不带 catalog**），`CREATE SCHEMA IF NOT EXISTS rds_tmp` 也是裸名 → 落点跟着当时的默认 catalog 漂（`USE 主源` 之后就落到源 catalog）。这是"卸载底座后临时表没位置"的机制性原因。建议钉成 `memory.rds_tmp.<名>`。
   - 待实测：`search_path` 是否接受 `memory.rds_tmp` 这种 catalog 限定写法；不接受则所有引用走全限定名，代价是本机手写 SQL 里裸名 `tmp_q_x` 查不到。
2. **加 owner 登记**：`TempTableManager` 现在只有 `drop_by_source(prefix)`，没有 owner 维度。补一条"文档 → 表名"登记，`close_document` 时按 owner 删。这样才满足"B 能用 A 的结果集、关 A 后不能用"。

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

【待定】面板没有自己的会话，现在借 `C_data_objects` 假 owner（`mod.rs:1550`）。它的 owner 该取谁：
- (a) 当前聚焦编辑器的文档 id；
- (b) 当前 tab 的 owner；
- (c) 维持独立 owner（则 A/B 的临时表不共享，与 §4 冲突，不推荐）。

### 5.3 表段

【已定】表段 = **当前挂着的底座里的业务表**（项目分析库业务表 + `G_*`）。`G_*` 出现是因为联邦默认挂全局底座；卸了即空。

【已定】去掉洞察（`tmp_i_`）、插件等表。

【待定】"业务表"判据：白名单（`res_*` + 资产表）还是黑名单（排除 `tmp_` / 插件前缀）。现状 `analysis_tables_of`（`source_picker/load.rs:196`）列 `main` schema **全部表不过滤**；`collect()` 另塞 `tmp_q_`/`tmp_l_`/`tmp_i_`/`G_*`；`assets: Vec::new()` → 生产表段**恒无资产行**（而 `objects()` 支持资产行）。

### 5.4 底座行

现状 `base:analytics` / `base:rds_global`，pill「底座」，`source_action: None`（`mod.rs:565-587`，不可卸）。

【已定】底座默认挂上、**可人为卸载**。
【建议】"可卸"与"本地加速看不见底座"用**同一个机制**：会话级可见性（`USE memory` + 底座不进 `search_path`；显式 `analytics.x` 仍可达）。
【待定】卸载语义：会话级遮蔽（只影响当前会话）还是真 `DETACH`（所有会话都看不见，含正在跑的联邦）。单实例前提下真 `DETACH` 会波及他人，建议前者。

---

## 6. 现状差距清单

| # | 差距 | 锚点 | 归属 |
|---|---|---|---|
| 1 | 加速档会话按连接，跨文档共享 `USE` / 中断 | `accel.rs:456-474`、`instance.rs::accel_owner` | 【已定】要改 |
| 2 | 加速档看得见底座 | `ensure_on_instance` 无摘底座动作 | 【已定】要改 |
| 3 | 单源联邦被 ≥2 卡住 | `editor_channels.rs:204` | 【已定】要改 |
| 4 | 临时表无 owner，关文档不删 | `temp_table.rs:275+`（只有 `drop_by_source`） | 【建议】要改 |
| 5 | 临时表落点不钉 catalog | `temp_schema.rs:29-31` | 【建议】要改，需实测 |
| 6 | 面板借假 owner `C_data_objects`，凭空造会话 | `data_objects/mod.rs:1550` | 【待定】 |
| 7 | 洞察 `tmp_i_` 进共享 schema，与"没有下游"不符 | `duckdb/temp_table/analysis.rs:37` | 【建议】 |
| 8 | mock 与编辑器同实例，面板还列 `tmp_m_` | `mock/src/engine/mod.rs:612`、`mod.rs:1025` | 【建议】 |
| 9 | 资源归档与实例双开同一 `analytics.duckdb` | `analytics_resource/global/sql.rs:47` vs `attach_bases` | 【待定】需真机验 |
| 10 | 表段不按业务表过滤、恒无资产行 | `source_picker/load.rs:196`、`mod.rs:1647` | 【待定】 |

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

| # | 问题 | 建议 |
|---|---|---|
| 1 | 加速档会话 owner 改文档级？ | 改。不改则 `USE` / 中断跨文档串 |
| 2 | 洞察是否改用会话 TEMP？ | 改。符合"没有下游"；若将来要在面板复用洞察结果，则退回 `rds_tmp` + owner |
| 3 | 编辑器结果集是否加 owner 登记、关文档删？ | 加。否则"关 A 就不能用"不成立 |
| 4 | 临时表落点钉 `memory.rds_tmp`？ | 钉。需先实测 `search_path` 的 catalog 限定写法 |
| 5 | 面板 owner 取当前编辑器文档 / tab / 独立？ | 取当前编辑器文档（tab 会切视角，破坏"一份工作"语义） |
| 6 | 底座卸载：会话级遮蔽 vs 真 `DETACH`？ | 会话级遮蔽 |
| 7 | 表段"业务表"判据：白名单 vs 黑名单？ | 白名单（`res_*` + 资产表），避免新前缀偷偷漏进来 |
| 8 | mock 是否退出面板可见范围？ | 退出，只留落地结果 |
| 9 | 资源归档是否改为"卸载时才开、读完即关"？ | 先解决双开问题，再定缓存策略 |

---

## 9. 变更记录

| 日期 | 变更 |
|---|---|
| 2026-11-26 | 初稿：从"联邦内核收口 + 数据对象面板"讨论中抽出会话/临时表/可见性三层；标出对 `execution-context-model.md` 两处作废口径 |
