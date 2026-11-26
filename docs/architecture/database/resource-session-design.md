# 分析资源与会话模型 · 全面设计（目标形态）

> 状态：设计稿，2026-11-27。落地分支 `feat/analysis-session-owner`（worktree
> `D:\RdataStation\RDS\rds-v2-session-owner`）。
> 分工：口径与落地记录见 [`../resource-session-model.md`](../resource-session-model.md)（【已定】/【已落地】四档标记）；
> 实例/会话机制的事实与证据见 [`analysis-instance-sessions.md`](analysis-instance-sessions.md)（已合入 `main`）。
> **本文只写目标形态、契约与收口计划**——不重新论证已落地事实，已落地的只标锚点。

---

## 0. 判据：用户能看见、能用、能验收的八条行为

设计的对错只按这张表判。每条都能在界面上点出来，也都能写成断言。

| # | 行为 | 反面（说明设计错了） |
|---|---|---|
| B1 | 打开项目：源段有 `analytics` / `rds_global` 两行底座，表段列出底座里的业务表（含 `G_*`） | 表段混进 `tmp_i_` / `tmp_m_` / 插件表 |
| B2 | 关掉一个 SQL 编辑器：它自己的结果集从面板消失，别人的一张不动 | 关 A 把 B 的结果集一起删了；或关 A 后 `rds_tmp` 里还留着它的表 |
| B3 | 编辑器 A 的结果集，面板与编辑器 B 能按裸名直接用 | 只有 A 自己能读（说明临时表落成了会话 `TEMP`） |
| B4 | 同一个文档从「本地加速」切到「联邦」：结果集还在，主源变了，底座可见性变了 | 切通道后结果集报「表不存在」 |
| B5 | 点底座行上的「卸载」：底座标已卸载，它里面的表从表段消失；点「挂回」恢复 | 卸载把 `ATTACH` 真摘了，别的文档当场报 catalog 不存在 |
| B6 | mock 落库 / 追加成功后，`tmp_m_` 不再残留；导出 / 存草稿后仍保留 | 落库后还要靠切项目才清得掉；或存完草稿就不能再落库 |
| B7 | 一条慢查询被超时中断，只影响它自己那条会话，其它会话照常执行 | 一句长查询把整个实例的分页 / 面板刷新全卡死 |
| B8 | 面板上任何一行的身份可由 `key` 唯一定位（同名不同来源不合并） | `orders` 同时是项目表 / 全局表 / 结果集，面板只出现一行 |

---

## 1. 五层资源（唯一的两句话）

**会话 = 一份工作的执行上下文；临时表 = 实例资源；可见性 = 会话级参数。**

| 层 | 实现物 | 谁写 | 谁读 | 寿命 | 回收触发 |
|---|---|---|---|---|---|
| **实例** | `AnalysisInstance`（进程唯一 `open_in_memory`，`instance.rs:1-…`） | 装配层一次打开 | 所有模块 | 进程级 | 退出进程；`RDS_HOME` 切换时底座重挂 |
| **会话** | `AnalysisSession`（`Connection::try_clone`，`instance.rs:535 ensure_session`） | `ensure_session(owner, label)`，owner 1:1 | 内核、编辑器、面板、洞察 | 跟 owner 走（文档 / 面板 / 进程 / 洞察） | 关文档 `close_session`；切项目 `close_all_sessions`（`rds_tmp` 不动） |
| **临时表** | `memory.rds_tmp` 普通表 + `TempTableManager` 登记（`TempEntry{created,owner}`） | 结果集 / 载入 / 洞察 / mock 建表即登记 | 面板表段、编辑器、洞察 | 看谁生的（见 §2） | 关文档 `drop_temps_of(owner)`；mock 落库即删；洞察前缀惰性清理；用户手动清 |
| **底座** | `analytics` / `rds_global`（实例级 `ATTACH`，`federation::session::bases()`） | 宿主 `analysis_bases::sync` / `ensure_base` | 面板源段 + 表段来源 | 项目打开期间 | 切项目重挂；**卸载 = 遮蔽**（`hide_base`，不 `DETACH`） |
| **源别名** | `MountRegistry`（`instance.rs:89-…`，别名 → conn_id） | 挂载方 `reserve_alias`（`instance.rs:626`） | 面板、内核、别名冲突判定 | 实例级 | `unmount_conn`（`instance.rs:730`）：`DETACH` + 放别名 + 关该源会话 |
| 旁路 | 资源归档自己的 `duckdb::Connection::open`（`analytics_resource/global/sql.rs:47`） | 资产侧 | 资产侧 | 自持缓存 | 卸载时关（语义未定，见 §8） |

两句话的推论（设计必须守住）：

1. **关会话不删临时表**：`rds_tmp` 里的表是实例资源，`TEMP` 才是会话私有——所以结果集绝不能落 `TEMP`（B3/B4 就靠这条）。
2. **卸载底座不是摘地基**：`ATTACH` 是实例级事实，摘掉会让别的文档当场报错（B5），所以卸载只做**呈现层遮蔽**，并在行上如实写边界（显式 `analytics.表` 仍可达）。

---

## 2. 生命周期矩阵（触发 × 结果）

| 触发 | 实例 | 会话 | 临时表 | 底座 | 源别名 |
|---|---|---|---|---|---|
| 打开项目 | 首次 `open`，挂两个底座 | 只建 `process` 一条 | 无 | `ATTACH analytics` / `rds_global`（只读后者） | — |
| 打开 SQL 编辑器 | — | `ensure_session(doc_id)` | — | 该会话继承 `search_path` | 依法挂载计划申请 |
| 关 SQL 编辑器 | — | `close_session(id)` 并销 owner 映射（`instance.rs:588`） | `drop_temps_of(doc_id)`（并集：实例册 ∪ 管理器册） | 不动 | **不 DETACH**（`ATTACH` 是实例级） |
| 切通道（加速 ⇄ 联邦） | — | 同一 owner 复用同一条会话 | 不动（`use_catalog` 只改 `search_path`） | 加速档：不进 `search_path`；联邦：默认进 | 计划里的源申请别名；`retain_mounted` 清本会话记录 |
| 切主源 | — | `use_catalog(alias)` → `<alias>.main,memory.rds_tmp` | 不动 | 主源可见性依通道 | — |
| 挂源（两态按钮） | — | 挂进**当前**会话 | — | — | `reserve_alias` → `ATTACH` |
| 卸源（两态按钮） | — | 逐会话 `forget_mounted`；以它为住源的 `reset_search_path` | — | — | `DETACH` + `release_alias` |
| 卸底座（两态按钮） | `hidden_bases.push(alias)` | 不动 | 不动 | **不 `DETACH`** | — |
| 挂回底座 | `hidden_bases.retain` | 不动 | 不动 | 本来就在 | — |
| mock 生成 | — | 进程面会话（`process`） | 建 `tmp_m_*`，owner `mock` | — | — |
| mock 落库 / 追加成功 | — | — | **删这张表 + 销登记**（`drop_temp`，失败保留） | 目标文件 `ATTACH`→写→`DETACH` | 临时 `ATTACH_ALIAS` 用完即摘 |
| mock 导出 / 存草稿 | — | — | 保留（可继续换出口） | — | — |
| 手动清理临时表 | — | — | `drop_temp` / `drop_all_temps`（删新 `rds_tmp` 与旧 `memory.main` 两处） | — | — |
| 切项目 | 底座 `refresh_bases` | `close_all_sessions` | **不动**（仍可手动清） | 摘旧挂新 | 全部释放 |
| 关进程 | 全部销毁 | 全部销毁 | 内存库消失（表不落盘） | — | — |

---

## 3. owner 词表（唯一权威）

owner 就是会话身份，**不许为空**（`ensure_session` 首行 `instance.rs:540` 拒空，空会让不同面板串成一条）。

| owner 取值 | 谁用 | 寿命 | 进源段吗 |
|---|---|---|---|
| `process` | 导出、结果集建表、洞察取样（`process_session`，`instance.rs:666`） | 进程级 | 否（`is_analysis_session` 排除，`instance.rs:580`） |
| `data_objects` | 数据对象面板自己的会话（`data_objects_load.rs:455`） | 面板级 | 是（挂着源就进） |
| `{doc_id}` | SQL 编辑器：加速档与联邦档（刀10/11a） | 文档关即销 | 是 |
| `insight` | 洞察模块（owner 固定） | 洞察面板级 | 否（不挂源） |
| `accel:{conn_id}` | 载入 / 归档「在这个源上再跑一条 SQL」（`accel_owner`，`instance.rs:671`；`session_for_conn`，`instance.rs:680`） | 连接级 | **否**（`is_analysis_session` 按前缀排除） |

**现存不一致（设计依据，不是猜测）**：

| # | 现象 | 证据 | 后果 |
|---|---|---|---|
| O1 | 内核加速档用**裸 `conn_id`** 当 owner | `kernel.rs:174-175` 传 `&prepared_accel.source.conn_id` | 该 owner 不以 `accel:` 开头 → `is_analysis_session` 判它是分析会话 → 它挂着 `rds_src` 就在源段多出一行「来源不明」的会话源 |
| O2 | `AnalysisChannel::Accelerated` 的文档注释仍写「会话按源 id 缓存」 | `analysis/channel.rs:29-31` | 与刀10 口径相反，后来人按注释改会改回去 |
| O3 | `C_data_objects` 曾被当成 owner 写进文档 | `mod.rs:1615` 是**连接 id**，`data_objects_load.rs:455` 才是 owner | 误读会开出第二条面板会话 |

**收口方向**（D17，见 §8）：所有会话 owner 只能从词表取值，`kernel::open` 的加速档改成由调用方传 owner（洞察 / 引擎内部路径传 `accel:{conn_id}` 或 `process`，不再传裸 id），并把 `accel:` 判定下沉成 `AnalysisInstance::is_reserved_owner(owner)` 一类单点函数。

---

## 4. 通道矩阵

| 档 | 源集合 | 主源 | 底座在 `search_path` | 底座在面板 | 会话 owner | 中断口 |
|---|---|---|---|---|---|---|
| 源库直连 | — | — | — | 不适用 | 无会话（`SessionHandle::None`） | 驱动自己 |
| 本地加速 | **1 个**（`rds_src`） | `rds_src` | **否**（B5 的另一半：加速看不见底座） | 底座行仍在，表段仍列底座（口径：底座是项目级地基，不随通道藏） | 编辑器 `{doc}`；载入路径 `accel:{conn}` | `AnalysisSession::interrupt` |
| 联邦（单源） | 1 个 | 计划主源（可无） | **是**（默认挂，可遮蔽） | 全部可联邦 | `{doc}`（`FederatedPlan.session_owner`） | 同上 |
| 联邦（多源） | N ≥ 1 | 计划主源 | 同上 | 同上 | 同上 | 同上 |
| 进程面 | 0 | `memory` | 是（恒含 `memory.rds_tmp`） | 不适用 | `process` | 同上（无 watchdog 之外的控制） |

**设计结论**：通道之间只差三个参数——`sources`（0/1/N）、`primary`、`bases_visible`；代码里不该再有第四种"专用连接"。剩下的例外只有 `ProcessDuckDb`（不挂源的内部底座），它是**入口**不是通道（导出 / 取样 / 结果集都跑它）。

---

## 5. 接口契约（引擎侧）

规则：**实例方法负责状态，会话方法负责执行**；谁申请谁释放；跨层不许自拼 SQL 名。

| API | 位置 | 前置 | 幂等 | 失败语义 |
|---|---|---|---|---|
| `AnalysisInstance::instance()` | `instance.rs` | 装配层已开 | 是 | `Err` 文本说明未初始化 |
| `ensure_session(owner, label)` | `instance.rs:535` | owner 非空 | 是（同 owner 复用活会话） | 空 owner / 状态锁污染 |
| `close_session(id)` | `instance.rs:588` | — | 已关返回 `false` | 不 `DETACH`（`ATTACH` 是实例级） |
| `close_all_sessions()` | `instance.rs:610` | — | 是 | `rds_tmp` 不动 |
| `reserve_alias(wanted, conn_id, kind)` | `instance.rs:626` | wanted 非保留字 | 是（同 conn 回原名） | 保留别名 / 空名 → `Err` |
| `unmount_conn(conn_id)` | `instance.rs:730` | 源已挂 | 是 | 逐会话 `forget_mounted` + `reset_search_path` |
| `hide_base(alias)` / `show_base(alias)` | `instance.rs:384/401` | 只认 `bases()` 里的别名 | 是 | 源别名一律拒 |
| `register_temp(name, created_by, note)` | `instance.rs` | 名非空 | 是（同名覆盖登记） | — |
| `drop_temp(name)` | `instance.rs:896` | — | 是 | 先删表再销两处登记；删不到也销登记 |
| `drop_temps_of(owner)` | `instance.rs`（刀14） | — | 是 | 单张失败不阻断，收完回第一条错 |
| `AnalysisSession::mount_sources(&[src])` | `instance.rs` | 别名归属已核 | 是（已挂算 Ready） | 别名被占 → `Failed` 点名占位者 |
| `retain_mounted(&[alias])` | `instance.rs` | — | 是 | 只清记录，不 `DETACH` |
| `use_catalog(alias)` | `instance.rs` | 别名在 `mounted_sources` | 是 | `search_path = <alias>.main,memory.rds_tmp` |
| `reset_search_path()` | `instance.rs` | — | 是 | 退回 `memory.rds_tmp`（主源没了也不报错） |
| `forget_mounted(alias)` | `instance.rs` | — | 是 | 只清记录 |
| `accel::ensure_on_instance(source, owner)` | `accel.rs:460` | 扩展就绪 | 是 | 挂不上直接回原因（不等查询报表不存在） |
| `MockEngine::write_temp_table_to_database(...)` | `mock/src/engine/mod.rs:533` | **调用方不得持内存库锁** | 否（写一次） | 写入失败优先；`Create` 失败回滚空表；成功即删临时表 |

调用链（读的方向 = 谁调谁）：

```
编辑器执行 ──► QueryRunner(document) ──► kernel::open(SessionPlan)
                                   ├─ Source        → SessionHandle::None
                                   ├─ ProcessDuckDb → instance.process_session()
                                   ├─ Accelerated   → accel::ensure_on_instance(src, owner=?)
                                   └─ Federated     → ensure_session(owner) → mount_sources → retain_mounted → use_catalog
面板挂载 ──► data_objects_load ──► accel::ensure_on_instance(src, "data_objects")
面板卸载 ──► SourceAction::ToggleMount ──► instance.unmount_conn(conn)
面板底座 ──► SourceAction::ToggleBase  ──► instance.hide_base / show_base
关文档   ──► editor on_removed ──► instance.drop_temps_of(doc) ──► close_session(session.id)
mock 落库 ──► mock_generator::persist_table_at ──► MockEngine::write_temp_table_to_database ──► instance.drop_temp
```

---

## 6. 面板信息架构

### 6.1 分段（三段，不是四段）

`Group::ALL = [Source, Table, File]`（`mod.rs:190`，段顺序唯一权威）。**临时表没有独立段**：结果集 / 载入 / 洞察样本都在**表段**，靠 `pill` + `life` 区分（`mod.rs:920-960`）。

这么设计的理由只有一条：口径要求"卸了底座，表段默认就空"。若给临时表独立段，"空"就不成立；若把临时表并进表段当普通表，卸底座后表段又不空。所以临时表必须**在表段里但可判身份**。

| 段 | 一级行 | 二级（展开） | 空态 |
|---|---|---|---|
| 源 | 已登记连接（含文件库）、会话挂上的源、两个底座 | 挂载源的库表 / 未挂载给「挂载」 | 「源 0」常驻（段头带计数） |
| 表 | 项目分析库业务表（`res_` 持久表 + 项目自建）、`G_*`、资产·表型、结果集（`result:`）、载入（`load:`） | 表结构（列 / 索引按既有面板） | 「表 0」 |
| 文件 | `f_*` 视图（加源加载的文件）、资产·文件型 | 预览 | 「文件 0」 |

**现状事实（核对过，不是推断）**：`Collected.insight_samples`（`mod.rs:478`）采集了 `tmp_i_` 清单，但 `objects()` 里**没有**渲染它的循环——洞察样本在面板上不出现（与"洞察没有下游"一致）。该字段目前只被测试 `sample()` 使用，属"采集但不渲染"；D19 因此只剩"登记是否该进实例册"这一层，不涉及界面。

### 6.2 行的五个正交维度

`Object { key, name, note, channel, life, state, group, pill, version, sql_ref, source_action, expand_via }`（`mod.rs:203`）：

| 维度 | 取值 | 规则 |
|---|---|---|
| `group` | 源 / 表 / 文件 | 只看 DuckDB 容器，不看业务身份 |
| `pill` | 资产 / 项目自建 / 全局 / 结果集 / 载入 / 未登记 / 底座 / 已挂载… | 业务身份，`&'static str`（不临时拼） |
| `life` | 会话级 / 项目级 / 进程级 | 决定"关什么就没了"的提示 |
| `state` | Ready / Failed(原因) / Unloaded / Missing | `Failed` 让 `can_analyze` 变假、chevron 换空位 |
| `source_action` | 无 / `ToggleMount` / `ToggleBase` | **只源行有，叶子零按钮**（铁律第 42 条） |

`key` 前缀即身份族：`base:` / `conn:` / `asset:` / `project:` / `persistent:` / `global:` / `result:` / `load:` / `fileview:`。B8 靠它（`mod.rs:2261-2270` 有唯一性用例）。

### 6.3 底座行（B5 的全部界面事实）

| 状态 | 副标题 | `state` | 动作位 | 表段 |
|---|---|---|---|---|
| 挂着 | 项目分析库 · `<path>` / 全局分析库 · 只读挂载 | Ready | `ToggleBase{hidden:false}` | 列它的表 |
| 已卸载 | 「已卸载 · 表段不再列它（显式限定名仍可达）」 | `Failed("已卸载 · 会话视角遮蔽（ATTACH 未动）")` | `ToggleBase{hidden:true}` | 清空该底座来源清单（`mod.rs:773-796`） |

`state` 用 `Failed` 而不是新枚举，是为了让"不可分析"这条既有判定自然成立；文本里把边界写出来，避免用户以为真摘了。

### 6.4 刷新模型（现状 + 设计判定）

- 脏信号：`Shared::data_objects_epoch`（`workspace.rs:53-59`）。宿主后台线程比廉价指纹（结果集数、连接数、资产数…），变了只 bump 信号；**重取由面板自己做**（面板可能没开）。
- 取数：`DataObjectsTab::refresh` 同步调 `data_objects::collect(&shared)`（`data_objects_tab.rs:373-376`），render 只在 `seen_epoch != epoch` 时排刷新（`:2870`）。
- `collect` 里有 I/O：`analysis_tables_of()` 以只读模式**另开一条连接**读项目 `analytics.duckdb`（`source_picker/load.rs:196`）；`global_tables()` 读全局库；`list_result_sets()` 要拿进程 DuckDB 的锁。

**设计判定**：这套是"单线同步 + 脏标记"，在 200 行以内没问题；但 `collect` 的 I/O 落在 UI 线程，一旦项目库被写者锁住（联邦写 / 落库），面板刷新会卡界面。**留作 D21**：把 `collect` 拆成"纯内存事实"（实例状态、`Shared` 快照）+ "文件事实"（两张库的表清单，后台取、缓存 1 个 epoch），面板先画内存事实，库清单到了再替换。

---

## 7. 不变量（可测，编号供 commit / review 引用）

| # | 不变量 | 当前守护 |
|---|---|---|
| I1 | 临时表不随会话消失（关文档删的是 owner 那批，不是会话 `TEMP`） | `drop_temps_of` + `on_removed` 顺序（先删表再关会话） |
| I2 | `rds_tmp` 表跨会话按裸名可见 | `search_path` 必含 `memory.rds_tmp`（`use_catalog` / `reset_search_path` / `ensure_shared_schema`） |
| I3 | 一个 owner 只有一条活会话；关会话必销 owner 映射 | `ensure_session` / `close_session`（`instance.rs:548/596`） |
| I4 | 的别名唯一：同别名被两个源看中，后到者 `Failed` 并点名占位者 | `alias_owner` + `mount_sources` 前置核对 |
| I5 | 卸载底座不产生"catalog 不存在"错误 | `hide_base` 只写列表，不 `DETACH` |
| I6 | 加速档看不见底座；显式限定名仍可达 | `use_catalog` 的 `search_path` 不含底座 |
| I7 | 表段只列底座业务表 + 资产 + 临时表三类，不列 `tmp_i_` / 插件表 | `objects()` 的来源清单 |
| I8 | 叶子零按钮；源行只有**一个**动作位 | `source_action: Option<_>` |
| I9 | 每行 `key` 全局唯一 | `mod.rs` 用例 |
| I10 | mock 落库成功后该临时表已不存在；失败仍在 | 刀16 |
| I11 | 慢查询只中断本会话 | `run_with_timeout` + 每会话 `InterruptHandle` |
| I12 | 面板 owner 恒为 `data_objects`，不追聚焦文档 | `data_objects_load.rs:455` |
| I13 | 别处不许为 `rds_tmp` 表拼 `memory.main` 或裸名以外的引用 | `qualified()`（`temp_schema.rs`） |

---

## 8. 收口计划（每刀独立提交、编译回归；顺序按风险排）

| 刀 | 目标 | 改动面 | 验收 | 风险 |
|---|---|---|---|---|
| **D17** ✅ | 统一 owner 词表（§3 O1/O2）：`kernel::open` 加速档不再传裸 `conn_id`；`accel:` 判定收敛成单点函数；`AnalysisChannel::Accelerated` 注释按刀10 重写 | `kernel.rs` 加速档、`channel.rs` 注释、`instance.rs` 前缀常量 | 洞察里跑一次加速后，面板源段**不新增**行；`is_analysis_session` 用例补一条 | **已落地 `72486305`**：owner 定为源级 `accel:{conn_id}`（洞察 / 引擎内部没有文档身份，与 `session_for_conn` 同键）；判定收敛为 `ACCEL_OWNER_PREFIX` + `is_accel_owner()`（残键 `accel:` 不算） |
| **D18** | 本地加速"一次一库"落到引擎侧：`rds_src` 被别的 conn 占着时**报错并引导改联邦**，不再静默 `_2` | `accel.rs::ensure_on_instance`、`instance.rs::reserve_alias` 调用点 | 两个连接同时开本地加速 → 第二个给出可读拒绝 | 与 `analysis-instance-sessions.md` §7.3 的"应撤回编号"一致；但若用户要"多加速"就得反过来拍（**要拍**） |
| **D19** | 洞察临时表不进表段 / 不进共享登记：`tmp_i_` 只留在管理器册，不 `register_temp` 进实例册（或登记但 `objects()` 明确过滤） | `temp_table/analysis.rs`、`mod.rs` 来源清单 | 表段无 `tmp_i_`；洞察重算仍能读到自己的样本 | 洞察"没有下游"，但 `tmp_i_` 目前靠实例共享才能被结果集二次分析引用——先确认不需要这条 |
| **D20** | 资源归档与实例双开同一 `analytics.duckdb` 的落定 | `analytics_resource/global/sql.rs:47` vs `ensure_base` | **必须真机验**（用户已说不做真机验证 → 本条挂起） | 双写 / 锁冲突 |
| **D21** | `collect` 的库 I/O 后台化 + 1 epoch 缓存（§6.4） | `data_objects/mod.rs::collect`、`data_objects_tab.rs::refresh` | 打开项目后首帧先出内存事实；库清单半拍内替换 | 会引入"清单短暂不完整"，要配空态文案 |
| **D22** | 会话源段去重契约：`session_sources` 与 `accel_sources` 同别名只出一行（含主源标注） | `objects()` 源段合成 | 同一源被两条会话挂上时源段仍只有一行 | 去重键要选 `alias` 而非 `conn_id` |

**已拍板、不需再论证**：D17 的方向（词表统一）与 D19 的方向（洞察不进表段）来自 §3/§6 的既有口径；D18 与 D20 需要用户拍；D21/D22 属工程改进，可以先做 D22 再做 D21。

---

## 9. 已知边界（写清，不装作没有）

1. **底座"卸载"挡不住显式限定名**：`analytics.表` 照样能查（DuckDB 没有"按会话禁用 catalog"的开关）。行上文案已经写明（§6.3）。
2. **`rds_tmp` 不落盘**：进程退出即消失，临时表"寿命长"的上限是进程。
3. **一个实例只能有一个 `memory` catalog**：所有 owner 共享 `rds_tmp`，所以"私有"只能靠不进面板 + owner 登记，不能靠隔离。
4. **面板刷新是同步的**：`collect` 在 UI 线程读两个库（§6.4），大项目下这是下一处瓶颈。
5. **`TEMP` 与 `rds_tmp` 是两种寿命**：`TEMP` 随会话消失（I3 的反面），`rds_tmp` 不随；混用会让 B3/B4 失效。

---

## 10. 与既有文档的关系

- 口径（已定 / 已落地）：[`../resource-session-model.md`](../resource-session-model.md)
- 机制与证据（已合入 main）：[`analysis-instance-sessions.md`](analysis-instance-sessions.md)
- 面板不变量（铁律 A–H 组）：[`../layout/data-objects-invariants.md`](../layout/data-objects-invariants.md)
- 登记 vs 挂载：[`../layout/analysis-sources-spec.md`](../layout/analysis-sources-spec.md)
- 执行上下文（Source 档网络驱动，部分口径已作废）：[`../connection/execution-context-model.md`](../connection/execution-context-model.md)

---

## 11. 目视验收清单（对着界面逐条点）

### 11.0 本轮运行环境（worktree 完全隔离，2026-11-27）

- `RDS_HOME` = `<worktree>\.rds\home`（主仓 `RdataStation-v2\.rds` 一点不碰）。
- 示例项目 = `<worktree>\.rds\home\data\samples\示例项目`，由**产品代码**生成：
  - `data/sample.sqlite`（SQLite，customers 6 / orders 12）
  - `data/sample.duckdb`（DuckDB，**2.0.1 写**，实测打开并查到 `orders` 12 行）
  - `.RSmeta/analytics.duckdb`（项目分析库，2.0.1 迁移生成，实测可打开）
  - `welcome.sql`
- 导航树自带两条示例连接：`G_conn_sample_sqlite` / `G_conn_sample_duckdb`，
  `use_duckdb_fed = false`（可直连、**未**本地加速——加速由源段那颗两态按钮决定）。
- 真机 `MySQL / PG`（192.168.3.138）在全局连接里；driver 已改成内核实现
  `mysql_native` / `postgres_native`（原先写的是 sidecar 插件 id `mysql`/`postgres`，
  插件未装时在导航树里表现为「驱动缺失」）。
- **Oracle / MSSQL 未写入**：内核没有这两个驱动实现（`crates/driver/src/native/` 只有
  `duckdb / mysql_native / postgres_native / sqlite`），`oracle`/`mssql` 只出现在类型声明表；
  按用户决定（2026-11-27）等驱动到位再加，避免"看着有、点不开"。
- 启动：`$env:RDS_HOME="$PWD\.rds\home"; .\target\debug\rds-app.exe`

运行：worktree 内 `cargo run -p rds-app`（先构建好，见构建产物
`target\debug\rds-app.exe`）。

| 步 | 操作 | 期望 |
|---|---|---|
| 1 | 打开项目 → 数据对象面板 | 源段「源 N」里有 `analytics`（项目分析库 · 路径）与 `rds_global`（全局分析库 · 只读挂载）两行，pill「底座」；表段列出项目业务表 + `G_*`，**没有** `tmp_i_` / `tmp_m_` |
| 2 | 点 `analytics` 行的动作位 | 副标题变「已卸载 · 表段不再列它（显式限定名仍可达）」；表段里项目表与资产表消失，`G_*` 与结果集仍在；再点一次恢复 |
| 3 | 开两个 SQL 编辑器，各跑一条 `SELECT 1 AS a` 并把结果存为结果集 | 面板表段出现两行 pill「结果集」；关掉其中一个编辑器 → 只消失它自己那行 |
| 4 | 在另一个编辑器里 `SELECT * FROM "tmp_q_…"`（面板上那行的名） | 能读到（临时表实例共享，不随会话） |
| 5 | 打开一个 MySQL/PG 连接 → 面板源段该连接行点「挂载」 | 行变「已挂载 · 项目级」，展开能列源库表；再点一次「卸载」→ 回「未挂载」，编辑器里以它为住源的查询下一句报 catalog 不存在时行上有提示 |
| 6 | 编辑器切「本地加速」→ 跑 `SELECT * FROM <表>` | 能用裸名查到源表；查 `analytics.<表>` 仍可达（说明加速只是把底座移出 `search_path`，不是摘掉） |
| 7 | mock 面板生成 1000 行 → 「落库」到项目分析库 | 成功；再生成一次同名并「追加」→ 冲突预检按唯一列给出可读拒绝；落库成功后 `MockEngine::temp_tables()` 里不再有刚落那张（面板不列 `tmp_m_` 是既有口径，这条靠日志/断言看） |
| 8 | 一条 `range(1e7)×range(1e7)` 在另一个编辑器跑，同时在前一个编辑器跑 `SELECT 1` | 前者按超时被中断（提示「语句超过 N ms 被中断（本会话已恢复…）」），后者照常返回 |

---

## 12. 变更记录

| 日期 | 变更 |
|---|---|
| 2026-11-27 | 初稿：把 16 刀之后的形态收敛成「判据 B1–B8 / 五层资源 / 生命周期矩阵 / owner 词表 / 通道矩阵 / 接口契约 / 面板信息架构 / 不变量 I1–I13 / 收口计划 D17–D22 / 边界」；核对中新发现三处不一致（O1 内核加速档用裸 `conn_id` 当 owner → 会误进源段；O2 `AnalysisChannel::Accelerated` 注释与刀10 相反；`insight_samples` 采集但不渲染） |
| 2026-11-27 | D17 落地（`72486305`）：加速档 owner 定源级 `accel:{conn_id}`，`accel:` 前缀判定收敛单点；§11 补「11.0 本轮运行环境」（worktree 隔离 home、示例项目双源由 2.0.1 生成、真机 MySQL/PG 改 native 驱动、Oracle/MSSQL 等驱动） |
