# 洞察模块（M8）· 开发方案（Phase 0–5）

> 状态：**设计定稿（2026-09-15）**，代码尚未开始 · 关联文件：`insight-prototype-design.md`（原型）、`insight-prototype.html`（可交互原型）
> 前置：v1 行为蓝本 `v1/backend/src/core/insight/` + `core/services/{insight_engine,quality_scorer,table_profile_service}.rs`，已迁移至 `crates/insight`；v1 前端蓝本 `v1/frontend/extensions/builtin/workbench/ui/components/panels/`（7 个 Vue 组件 + `insight-store.ts`）
> 复用 `project-dev-plan.md` / `editor-dev-plan.md` 的推进方式：Phase 划分 → 文件落点 → 验收 → 测试场景 → 风险
> **范围**：库 / 表 / 列画像、质量评分、规则引擎与规则管理、Schema 洞察报告、洞察快照历史。**不含** SQL 执行（M5 编辑器）、对象树内省（M4）、Mock 生成（M7）、资源目录（M6）——只经服务与命令协作。

### 已确认决策（2026-09-15）

| # | 决策 | 出处 |
| --- | --- | --- |
| 1 | **规则随项目走**：项目规则落 `{项目}/.RSmeta/insight-rules/`，进 git、可 diff、可手工编辑（正文以文件为唯一真相源） | 本节 §4.1 |
| 2 | **规则作用域三层**：`Builtin`（编译期内嵌）→ `Global`（用户级，跨项目）→ `Project`（项目级），后者覆盖前者 | §4.1 |
| 3 | **规则正文不入库，只入索引**：库表只存 scope / checksum / enabled / 校验状态，正文归文件 | §4.2 |
| 4 | 规则**可禁用**（含内置规则），经索引表抑制记录实现，不改文件 | §4.2 |
| 5 | 规则热加载走**目录监听**（复用 `ThemeRegistry::watch_dir` 同款机制），不设「重新加载」按钮作为唯一入口 | §4.3 |
| 6 | 洞察 UI 归属 `crates/insight`——**2026-09-16 定案为方案 A**（依据：架构硬约束允许 Feature 直接依赖 gpui-kit 并把同一能力的 model / service / view 放同一 crate；`project` 先例已跑通；`panels/` 已 8600+ 行不宜再增） | §3.1 |
| 7 | 多列分析**重新设计**，不照搬 v1（v1 该功能从未跑通） | §5 Phase 3 |
| 8 | 快照持久化链路在 Phase 0 闭合（`InsightStorage` 构造点接线），不留到最后 | §5 Phase 0 |
| 9 | 保留 v1 「项目 → 全局」手动 reload 语义**不采纳**：注册表按项目根缓存，消除调用方义务 | §4.3 |

---

## 0. 进度记录（最近在前）

### 2026-09-18 — 收口（一）：删源库内省路径 + 接结果集清场口

**背景**：按 §10 收口清单的建议顺序做「纯减法 + 一项接线」。清单 #1（`crates/engine/insight-rules/` 重复副本）已由 `e3684d67`（洞察规则收敛）删除，不在本批。

**已完成并验证**（`cargo test -p rds-insight --lib` **225 项不变** + 集成 **13 项** · `rds-workbench --lib` **94 项**（本批 +1：结果集临时表清场）· `insight_entry` **2** + `ui_contract` **7** 全绿；`cargo check --workspace --all-targets` 零告警）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 删源库内省表探查（§10 #2） | `table_profile_service.rs` 与 `InsightService::get_table_profile` **零调用者**（面板走 `insight_engine::get_temp_table_profile`；源库表先取样再探查，D58/D59）→ 整体删除；`TableProfile` 类型保留（临时表内省仍在用） | 删 `crates/insight/src/table_profile_service.rs`；`lib.rs` / `service/mod.rs` 去声明与门面；`insight_engine` 的注释改口径 |
| 接结果集**清场口**（D54 契约的另一半） | 项目切换 / 关闭时清 `TempTableSource::Query`（`tmp_q_*`）：**非阻塞**（`try_drop_in_memory_temp_tables`，照 mock 先例）——拿不到内存库锁就留给下一次切项目 | `workbench/src/components/project_host.rs`（`clear_result_temp_tables` + `refresh_after_open` 调用 + 用例） |
| 澄清（已写回 §10） | **建表侧也零调用者**：`create_duckdb_temp_table` / `ResultService` / `execute_duckdb_analysis` 全链无 UI 调用点 → 「结果集 → DuckDB 分析」整条链路未接；所以**定向回收口**（`drop_temp_table(Query)`）现在无处可接，随「编辑器结果集入口」（§10 #6）一起做 | §10 #3 / #6 |

### 2026-09-18 — Phase 4 收尾：Schema 报告的导出与下钻（D60）

**背景**：Phase 4 一批把门面 / 视图 / 导出函数 / 下钻事件做完了，但两处**宿主动作**悬着——导出按钮「等宿主提供选路径 + 写文件后再画」、下钻「等宿主登记临时表」。本批把这两条断头路补齐（并在 D58 之后把下钻改成源取样，不再建临时表）。

**已完成并验证**（`cargo test -p rds-insight --lib` **225 项**（本批 +3：导出格式标签与扩展名 · 文件名净化 · 面板侧导出事件）· 集成 `column_profile_e2e` **13 项** · `rds-workbench --lib` **73 项**（本批 +1：下钻端到端）· `insight_entry` **2 项** 全绿）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 导出格式与文件名 | `SchemaExportFormat { Json, Markdown }`（菜单标签 / 扩展名）+ `schema_export_file_stem`：只挡**文件名非法字符**、保留中文（ASCII 化会把中文名变成横线）、空名兜底 `schema` | `schema_view.rs` |
| 导出事件 | `InsightEvent::SchemaExportRequested { format, file_stem, content }`：**内容在面板侧编码**（与界面同源的视图模型，D36）——宿主只选路径写文件，不必懂 JSON 分组键与 Markdown 转义；没有报告时不发事件 | `insight_view.rs`（`request_schema_export`）+ `jobs.rs`（未接时记日志） |
| 导出按钮 | 结构 Tab 健康条右上「导出 ▾」（JSON / Markdown 两项）：原型画的两个 `⤓` 图标并成一个菜单 | `insight_view.rs` |
| 下钻落地 | 宿主按 `{conn_id, database, schema, table}` 拼源取样 SQL（与导航树同口径）交 `SampleSource` → 展开右 Dock 面板；**不建临时表**——D58 之后那一步是多余的 | `workbench/src/components/insight_actions.rs`（新）+ `panels/right.rs`（装配 + 持有订阅） |
| 导出落地 | `rfd` 保存对话框（默认名 `schema-<schema 名>.<扩展名>`）→ 写文件 → **状态栏回执**；回执收进 `Shared::say`（写入 notice + 请求宿主重绘，两件事不再各处写一遍） | 同上 + `panels/shared.rs` |
| 测试 | 导出格式标签 / 扩展名 · 文件名净化（中文保留 / 非法字符挡 / 空名兜底）· 面板侧导出（无报告不发、JSON 带真内容与安全名、Markdown 同事件不同编码）· **下钻端到端**（发事件 → 宿主落地：右 Dock 展开 + 面板目标换成源取样，MySQL 反引号） | `schema_view.rs` / `insight_view.rs` / `insight_actions.rs` 测试模块 |

### 2026-09-18 — 文件类数据源 + 三个入口接线（D59）

**背景**（产品口径）：**凡 DuckDB 能分析的资源都能洞察**——数据库导航树、分析存档、草稿箱、编辑器结果集，**包括 Excel 这类需要扩展的文件**（CSV / Parquet / Excel / JSON）。上一批（D58）交的是「源取样通道」，宿主侧入口还欠着（当时记档在案）；本批把通道补成「文件也能走」，并接上三个入口。

**已完成并验证**（`cargo test -p rds-engine --lib` **377 项**（本批 +2：`create_analysis_temp_table_as` 建成且登记 / 失败不留半成品）· `cargo test -p rds-insight --lib` **222 项**（本批 +1：文件来源的读取器映射与路径转义）· 集成 `column_profile_e2e` **13 项** · `rds-analytics-resource` lib **93 项**（本批 +1：`can_view_stats` 三态）+ 窗口 **11 项** · `rds-scratchpad` **36 项** · `rds-workbench` lib **72 项**（本批 +1：取样 SQL 的引号与空段）+ `insight_entry` **2 项** + `ui_contract` **7 项** 全绿）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| **D59** 文件类数据源 | `SampleSource.conn_id` 改 `Option`：`None` = 跑在 **DuckDB 内存库**（文件与已 `ATTACH` 的表）；新增 `on_duckdb` / `duckdb_file`（扩展名 → 读取函数，**映射与 `load_file_source` 共用一处**；路径做单引号转义；认不出的格式直接报「这个格式还不能分析」而不是猜一个读取器） | `insight/src/model.rs` + `engine/src/dbi/engine/duckdb_engine.rs` |
| 取样分两条 | 源库连接 → 引擎 JSON 打型建表（原路，抽为 `sample_from_connection`）；DuckDB 侧 → `create_analysis_temp_table_as`（`CREATE TABLE … AS`，**数据不过 Rust**，类型由 DuckDB 定，失败收半成品） | `insight/src/service/persistence.rs` + `engine/src/duckdb/analysis.rs` |
| 入口①导航树 | 表右键「查看统计」→ 按驱动加引号的限定名 → `open_insight_source_table`；引号与拼装收进 `Shared::insight_sample_sql`（**口径只一处**） | `workbench/src/components/nav_host.rs` + `panels/shared.rs` |
| 入口②分析存档 | 行右键「查看统计」：受管文件走本体文件（`duckdb_file`）；远端引用按 `source_connection_id` + `schema.table` 重新取样；分析表型（本体在 `analytics.duckdb`，要 ATTACH + 重建定义）随后续批次。菜单项是否可用由 `can_view_stats` 定（按 kind + 可读扩展名） | `analytics_resource/src/resource_view.rs` + `workbench/src/components/resource_host.rs` |
| 入口③草稿箱 | 文件右键「查看统计」：判定与落地都问宿主（`ScratchpadHost::can_view_stats` / `view_stats`）——草稿箱不依赖 `engine`，读取器口径不该在那边抄一份（默认实现 `false`，不接洞察的宿主不会摆出点了没反应的入口） | `scratchpad/src/{host,scratchpad_view}.rs` + `workbench/src/components/scratchpad_host.rs` |
| 暂不做 | 编辑器结果集列头「洞察此列」——用户明确说不急（真做时照 `FilterValueHook` 那样注入，**不需要执行期物化**：洞察侧会重跑取样） | — |

### 2026-09-17 — 入口统一：源取样通道（D58）

**背景**：产品口径确定——**凡是能喂给 DuckDB 的数据都能洞察**（数据库导航树 / 分析存档 / 草稿箱 / 编辑器 SELECT 结果集）。它们形态各异，但分析能力只有一套（列画像 / 表探查 / 多列 / 下钻都要临时表），所以先把入口统一成一条通道。

**已完成并验证**（`cargo test -p rds-insight --lib` **221 项**（本批 +3：源目标同形、样本表解析、请求映射）· 集成 **13 项**全绿；本批文件 clippy 零新增告警）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 统一契约 | `SampleSource { conn_id, sql, label }`：入口只给「哪条连接 + 一段只读查询 + 来源标签」 | `model.rs` |
| 统一口径 | `sample_source_to_analysis_table`：外层再包一层 `SELECT * FROM (…) AS rds_sample LIMIT 500` → 落 `tmp_i_` 分析临时表（建表即登记，TTL / 上限 / 回收全现成）。**行数上限只在洞察侧**，不让每个入口各写一次 | `service/persistence.rs`（`SOURCE_SAMPLE_LIMIT`） |
| 目标形态 | `InsightTarget::{SourceColumn, SourceTable}`：Tab / 标题 / 下钻与临时表目标**完全同形**，副标题写「来源 xxx」，差别只在「多一步取样」 | `model.rs` |
| 取数与回填 | `ProfileRequest::{SourceColumn, SourceTable}` → 取样 → 画像 → 接缝把**样本表名回填**面板（`PanelData.source_sample`），保存快照 / 多列 / 下钻接着用**同一份样本**（不重新抽样） | `jobs.rs` + `insight_view.rs`（`set_source_sample` / `sample_table()`） |
| 快照来源 | 保存快照带上来源标签（`SnapshotSaveRequested.source_label`）→ `entity_source` 写「`analytics.orders · amount`」而不是一个过期就没人认识的 `tmp_i_…` | `insight_view.rs` + `jobs.rs` + `service/mod.rs` |
| 测试 | 源目标与临时表目标同形（Tab / 标题 / 副标题 / 空临时表名）；`PanelData::sample_table` 三态（普通目标 / 源目标未取样 / 已取样）；源目标能解析成取样请求 | `model.rs` / `jobs.rs` 测试模块 |

**宿主侧仍欠（一次一行）**：`Shared::open_insight_source_table / open_insight_source_column`（把上表的目标推给面板）还没写，导航树 / 存档 / 草稿箱 / 结果网格的右键菜单也还没接——所以本批交付的是**通道**，四个入口接上即可用（结果集用 `SampleSource::new(conn_id, 原SQL, "结果集 · xxx")`，洞察侧会自己包 LIMIT）。

### 2026-09-17 — 快照收尾三件：Q5 双写回滚 · Q4 保留期定案 · K14 剪链标注

**已完成并验证**（`cargo test -p rds-insight --lib` **218 项**（本批 +2：双写回滚 1 + 剪链标注 1）· 集成 **13 项**全绿；本批文件 clippy 零新增告警）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| **Q5 → D55** 双写失败回滚 | 正文写了、元数据没写时，**把刚写的正文删掉**（要么都成、要么都不留），错误文案写清「已回滚」，回滚自身失败时同时报两件事与出路 | `store/mod.rs`（`rollback_snapshot_body`）+ `service/persistence.rs`（另一处双写点） |
| **Q4 → D56** 保留期定案 | 固定 **30 天**（单一常量，不做配置项）——v1 同值；快照是几 KB 的 JSON，30 天够用；每多一个配置项就多一处能写错的数 | `model.rs` 的 `SNAPSHOT_RETENTION_DAYS`（已有）+ 文档 |
| **K14 → D57** 剪链头部标注 | 清理剪掉链的起段后，最旧一版**不再默默无标记**：界面标「更早的版本已清理」（**数据不改**——置空 `parent` 会丢掉「前面还有历史」这个事实）。判据三合一：最旧一版 + 父版不在列表 + **列表未被分页截断** | `model.rs`（`chain_truncated`）+ `insight_view.rs`（标记三档：当前 / 首版 / 更早的版本已清理） |
| 测试 | 回滚的契约：先把「只有正文」的孤儿写进库 → 调补偿 → 断言原错误未被掩盖、正文与存储用量都回到原样；剪链标注的三态：被剪（标）/ 完整链（不标）/ 满页（不当假阳性） | `store/mod.rs` / `model.rs` 测试模块 |

**诚实记档**：Q5 的端到端失败路径（真的让 `save_meta` 失败）没有做故障注入测试——它需要在测试里造一个「元数据表不可写」的项目库，脆且贵；本批测的是补偿函数本身的契约（把孤儿收回去、不掩盖原错误），接入点是三行 `if let Err(..)`。

### 2026-09-17 — K16 结果集侧收口：查询结果临时表的命名与回收口（D54）

**已完成并验证**（`cargo test -p rds-engine --lib` **373 项**（本批新增 4：命名 1 + 拒跨来源 1 + 定向删除 1 + 建表前缀/登记 1）· `cargo test -p rds-insight --lib` **216 项** + 集成 **13 项** · `cargo test -p rds-workbench --test insight_entry` **2 项**（环境干净后补跑的）全绿；本批文件 clippy 零新增告警）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 命名收口 | `generate_unique_name(source, description)`：`tmp_{缩写}_{描述}_{时间戳}_{8 位随机}`。描述清洗与名字生成从 `analysis.rs` **搬到 `temp_table.rs`**（两处共用一份实现，不再各拼一套）；`sanitize_description` 空描述兜底从 `t` 对齐为 `tmp`（无行为影响） | `duckdb/temp_table.rs` + `duckdb/mod.rs`（导出） |
| 结果集建表改前缀 | `create_temp_table_internal` 从 `rs_<uuid>` 改为 `tmp_q_result_<时间戳>_<随机>`，仍建完即登记——**前缀终于与回收机制对齐** | `services/duckdb_service.rs` |
| 定向删除口 | 新增 `drop_temp_table(conn, source, name)`：前缀守卫（拒跨来源）+ `DROP TABLE IF EXISTS` + 同步摘登记。与 `drop_by_source`（按来源清场）分工：前者丢**一张**，后者清**一批** | `duckdb/temp_table.rs` |
| 洞察侧复用 | `analysis.rs` 的 `generate_table_name` / `quote_ident` 改为转发共享实现；`drop_analysis_temp_table` 保留自己的前缀报错文案后转调 `drop_temp_table` | `duckdb/analysis.rs` |

**K16 至此收口（三块全做完）**，但有一条**诚实的边界**要记下：全仓仍然**没有任何调用者**去建结果集临时表（`create_duckdb_temp_table` / `open_insight_column` 都是零调用）——“结果集 → 临时表 → 洞察”这条路还没接。所以今天既没有活泄漏、也没有可回收对象；接线时按 D54 的契约在丢弃点调 `drop_temp_table` 即可（宿主侧欠账见下一条）。

### 2026-09-17 — Q1 ③ 落地：项目规则信任门（D53）

**已完成并验证**（`cargo test -p rds-insight --lib` **216 项**（本批 +11：存储 5 + 门控 2 + 服务 1 + 视图 3）· 集成 **13 项** · `cargo test -p rds-engine --lib` **360 项**（迁移目录多一个文件，无断言受影响）全绿；本批文件 clippy 零新增告警）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 决定记在哪 | 新表 `insight_rule_trust`（**全局库**，键 = 规范化项目路径，`state ∈ trusted/declined`）——存项目库等于让项目给自己发信任（自我授权） | `crates/engine/migrations/global/025_insight_rule_trust.sql` + `crates/insight/src/service/rule_trust.rs`（新） |
| 门控 | `build_registry` 的项目层：信任不是 `Trusted` 就**不装配**，改为扫盘把「带了什么」（id 列表 + 解析失败条数）记进 `registry.pending_project`（扫描不执行任何 SQL） | `crates/insight/src/lib.rs` + `rule_registry.rs`（`PendingProjectRules`） |
| fail closed | 无记录 / 查库失败 / 库里值不认识 → 未决定（不装配）。读法：进程级缓存 → 未命中**自开一条只读连接**查全局库（**不用 `acquire_sync`**：它在 tokio 运行时内直接报错，而装配路径确实会从 `profile_column_from_table` 这类 async 进来——真失败就会静默少装一层规则） | `lib.rs`（`project_rule_trust` / `apply_project_rule_trust`） |
| 首次弹确认 | 首次在规则管理里拿到「有未信任项目规则」的数据 → **叠一个确认框**（对话框层是栈式的；窗口句柄在 `begin_load` 时存下，回填时 `update_window` 打开）；两个按钮 → `RulesEvent::TrustDecided`。之后再遇到只留横幅 | `rule_view.rs` |
| 常驻横幅 | 列表顶部：多少条 / 叫什么（前 6 条 + 「等 N 条」）/ 目录 / 其中几条解析失败；按钮「信任并加载」/「不加载」（已拒绝时改说「你此前选择了不加载」+「改为信任并加载」） | 同上 |
| 接缝 | `RulesEvent::TrustDecided` → 写全局库 → 推缓存 + 失效注册表 → 重扫索引 → 回填列表；写库失败只挂行内提示（信任状态不变） | `jobs.rs` + `service/mod.rs`（`decide_project_rules_trust`） |
| 测试 | 存储 5（往返 / 按项目隔离 / 路径归一 / 脏值 / 表未建）+ 门控 2（未信任与已拒绝都不装配且 pending 可见 → 信任后进来且 pending 消失；没有规则文件就不打扰）+ 服务 1（无项目报错）+ 视图 3（确认框只弹一次、两个按钮发事件且**不就地改状态**、横幅不被搜索过滤、摘要截断） | 各文件测试模块 |
| 顺带修的测试 | 监听热加载 / 宿主接缝两个用例原以「项目规则自动装配」为前提 → 现在显式信任那个临时项目（并把「为什么要先信任」写在注释里） | `service/watcher.rs` / `jobs.rs` |

**对用户的影响（破坏性）**：项目规则**第一次不再自动生效**——打开「洞察规则」会被问一次；回答记在全局库，之后不再问（横幅可随时改主意）。从没打开过规则管理的项目，其规则不参与分析（fail closed，文档已写）。

**上游踩到的坑（供后来者）**：`GlobalSqlitePool::acquire_sync` 在「已处于 tokio 运行时」的调用点会**直接返回错误**（它自己的契约）——同步读全局库的那段最初用它，单测里全是 `#[tokio::test]` 所以当场就红了。改成自开只读连接后与运行环境无关。

### 2026-09-17 — Q1 ① 落地：规则 SQL 静态门（D52）

**已完成并验证**（`cargo test -p rds-insight --lib` **205 项**（本批 +3）+ 集成 **13 项**全绿；16 条内置规则全过（`test_builtin_count_matches_constant` 的「内置规则必须全部可解析」就是回归网）；本批文件 clippy 零新增告警）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 四步判定 | ① 只能一条语句（仅末尾分号放行）；② 首个关键字必须是 `SELECT` / `WITH`；③ 关键字黑名单（`ATTACH`/`COPY`/`EXPORT`/`IMPORT`/`INSTALL`/`LOAD`/`UNLOAD`/`PRAGMA`/`CALL`/`SET`/`CREATE`/`INSERT`/`UPDATE`/`DELETE`/`DROP`/`ALTER`/`TRUNCATE`/`REPLACE`/`MERGE`/`GRANT`/`REVOKE`/`VACUUM`/`CHECKPOINT`/`SECRET`/`BEGIN`/`COMMIT`/`ROLLBACK`）；④ 表函数黑名单（`read_*` / `parquet_*` / `*_attach` / `*_scan` / `*_query` / `glob` / `sniff_csv` / `query` / `query_table`）——后两类按前缀 / 后缀匹配（这类函数由扩展提供，名字是开放集合，写全必漏） | `rule_registry.rs`（`validate_rule_sql` / `is_forbidden_function`） |
| 防误报 | 校验前先剥**字符串字面量 / 注释 / 双引号标识符**（`strip_literals_and_comments`）：`WHERE name = 'copy'` 与 `"copy"` 都不算违规——字面量是数据、双引号是标识符。这也给了误报出口：**把名字用双引号包起来就能过**（报错文案直说） | 同上 |
| 挂点 | 接在 `validate_rule` 第一条 → 注册表 / 索引器 / 测试共用 `parse_rule_toml` 一个入口，**用户在规则管理里看到的报错原文就是它给的** | 同上 |
| 定位 | 防呆，**不是安全边界**（DuckDB 没有官方解析沙箱，黑名单天然有漏）。报错文案点出规则 id + 撞上的词 + 改法 | 架构 D52 / K1、手册 §4.7 |
| 测试 | 通过组 7 例（含字面量 / 注释 / 双引号 / 末尾分号四个**不该误报**的）、拒绝组 7 例（多语句 / DML / DDL 开头 / `PRAGMA` / `read_csv_auto` / `sqlite_attach` / 未加引号的 `copy`）+ 空 SQL + **挂点回归**（把 `sample_toml` 的模板换成 `read_parquet(...)`，断言解析期报错） | `rule_registry.rs` 测试模块 |

**对用户规则的破坏性变更（有意）**：写了多语句 / DDL / DML / 文件表函数的规则**现在会在解析期变无效**（规则管理里一行红字）。这是 Q1 ① 的全部目的——早失败优于「打开一个项目就执行了它带的 SQL」。下一档（真正的边界）是 **Q1 ③ 项目规则信任门**，需要先拍板产品语义。

### 2026-09-17 — K16 ③+④：内存闸与可观测（D51）

**已完成并验证**（`cargo test -p rds-engine --lib` **357 项**全绿（本批新增 4：`manager` 3 + `temp_table` 1；总数里另有你在改的 `sql_service` / `history_store` 新增的用例）· `cargo test -p rds-insight --lib` **202 项** + 集成 **13 项**全绿；本批文件 clippy 零新增告警——`manager.rs` 那条 `collapsible_if` 是既有代码；workbench 侧因 `crates/editor` 在制品编译不过，**未跑** `insight_entry`）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 内存闸（③） | `configure_connection` 上 `SET memory_limit`（默认 **2GB**）——内存库是进程级单例，DuckDB 默认吃物理内存的 80%，桌面应用把机器吃光不是可接受的失败方式 | `duckdb/manager.rs` |
| 溢写口（③） | `SET temp_directory` 钉 `<RDS_HOME>/tmp`（`paths::temp_dir()`；先 `create_dir_all`，**建不出来就跳过溢写设置、不挡启动**）+ `SET max_temp_directory_size`（默认 **10GB**）；到顶是「溢写到 `tmp/`、慢一点」而不是报错 | 同上 |
| 覆盖与防呆 | `RDS_DUCKDB_MEMORY_LIMIT` / `RDS_DUCKDB_MAX_TEMP_SIZE` 可覆盖；值拼进 SQL 前过 `parse_size_setting` 白名单（数字 + 可选 B/KB/MB/GB/TB）——`SET` 不接受绑定参数，窄白名单同时挡住手滑与注入；非法值回退默认并告警，**配错环境变量不该让程序起不来** | 同上（`size_setting` / `parse_size_setting`） |
| 可观测（④） | `TempTableStats`（按来源分档计数；mock 的两套前缀不重复计）+ `TempTableManager::stats()` + `DuckDBManager::temp_table_stats()`；洞察建表时 ≥ 上限 80% 打 warn（`warn_if_near_capacity`）——登记表只增不减**没有任何外部表现**，这条日志是唯一的提前信号 | `duckdb/temp_table.rs` + `manager.rs` + `analysis.rs` |
| 测试 | `parse_size_setting` 白名单（合法 4 / 非法 6，含 SQL 尾巴）、`size_setting` 覆盖与回退、`configure_connection` **真读回** `current_setting('memory_limit')` / `temp_directory` / `max_temp_directory_size`、`stats` 分档计数 | 各文件测试模块 |

**本批把上一条里的 ③④ 做完了**（上一条按当时状态记录，不改）。**K16 至此只剩结果集侧（`tmp_q_`）未接**——回收策略得与编辑器生命周期一起定。

**踩到的一个真坑**：`duckdb-rs` 的 `execute_batch` **不把换行当语句分隔符**（必须分号）。第一版用 `\n` 拼接，结果**所有**建连接的路径都报 `Parser Error: syntax error at or near "SET"`（12 个测试同时红）。已改为 `settings.join(";\n")` 并补尾分号。

### 2026-09-17 — Q1 核查（规则 SQL 安全边界）：先前的「禁用外部访问」建议作废

**核查结论**（只读代码，无改动）：对 DuckDB 设 `enable_external_access = false` / 「只读连接 / 禁用扩展」这类限制**在现有连接模型下不可行**——`dbi/engine/duckdb_engine.rs` 的 `register_external_database`（`ATTACH`）与 `load_file_source`（`read_csv_auto` / `read_parquet` / `read_excel_auto`）、`duckdb/extensions.rs` 的 `INSTALL` / `LOAD` 都跑在**同一个进程级内存单例**上（就是洞察与结果集临时表所在的连接），且该开关只能在建连接时设、设了回不去 → 会把产品自身的「连接 DuckDB 数据源 / 打开 CSV·Parquet·Excel / 装扩展」一起挡掉。

**Q1 推荐改为**（已写进架构 §11 K1 / §12 Q1 与手册 §4.7，待拍板）：① **解析期静态门**（只放行单条 `SELECT` / `WITH`，禁分号 / `ATTACH` / `COPY` / `read_*` 等——**防呆层，不是安全边界**）；② 诚实声明（规则文件 = 可信本地文件，现状）；③ **项目规则信任门**（推荐新增，真正的边界：项目规则跟着仓库走，克隆不信任的仓库 + 打开项目 = 把它的 SQL 拿到本机执行）。

### 2026-09-17 — 临时表一致化（K16 ①+②：洞察中间表改走 `duckdb::analysis`）

**已完成并验证**（`cargo test -p rds-engine --lib` **345 项**（含新增 `duckdb::analysis` **6 项**）· `cargo test -p rds-insight --lib` **202 项** + 集成 **13 项** · `cargo test -p rds-workbench --test insight_entry` **2 项**全绿；本批文件 clippy 零告警，全仓 `cargo fmt --check` 本就不通过，未跑 fmt）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 分析侧新增 engine 支撑 | `engine::duckdb::analysis` 三个入口：① `with_analysis_temp_table`（**默认姿势**：建 → 用 → 无论成败都收）；② `create_analysis_temp_table` / `drop_analysis_temp_table`（跨函数持有时用；`drop` **只接受 `tmp_i_` 开头**，防误删别人建的表）；③ `cleanup_analysis_temp_tables`（惰性清理）。表名 `tmp_i_<描述>_<紧凑时间戳>_<8 位随机>`——末尾随机不是装饰：管理器的 `generate_name` 只到**秒**，同秒同描述必撞名（多列分析逐列建表就会撞） | `crates/engine/src/duckdb/analysis.rs`（新）+ `duckdb/mod.rs` 挂模块 |
| 前缀 = 回收口 | `ANALYSIS_TABLE_PREFIX = "tmp_i_"` 与 `TempTableSource::Insight` 的前缀**必须一致**：TTL / 上限 / 按来源清理（`list_by_source` / `drop_by_source`）全靠它识别。建表即 `register` | 同上 |
| 补「只腾登记表」的缺口 | `lazy_cleanup_*` 拿不到连接、执行不了 DDL（K16 的第二个事实）→ 真正 DROP 必须由持有连接处补，就是 `cleanup_analysis_temp_tables`。它放在建表**之前**调用：让 TTL / 上限落在「下一次建表」这个廉价时机上 | 同上 + `manager.rs`（新增 `temp_table_manager()` 访问器） |
| 洞察两处样本表改姿势 | `profile_column_from_table` / `batch_evaluate_columns` 原先各自调 `create_duckdb_temp_table`（建 `rs_<uuid>`、不回收）→ 现改为 `with_analysis_temp_table` + `get_column_insight_full_on`（**已持连接**版本，与建表同一把锁，避开 std `Mutex` 自重入死锁） | `crates/insight/src/service/persistence.rs` |
| 打型口径共用 | `infer_type` / `json_to_duckdb_value` 由 `pub` 收到 `pub(crate)`：`analysis` 与结果集那条建表路径**共用同一套 JSON → DuckDB 打型与值转换**，不另立第二份口径 | `services/duckdb_service.rs` |
| 测试 | `analysis.rs` 6 项：前缀与登记 / 同秒不撞名 / 描述清洗 / `drop` 拒收外来名 / 清理只收过期的 / **`body` 报错也把表收掉**。洞察侧行为面沿用原有 202 + 13（不改断言） | `duckdb::analysis` 测试模块 |

**K16 的准确边界（别再把①当全修）**

- **①洞察侧已修**（本批）：洞察自己建的中间产物一律 `tmp_i_` + 用完即删，**不占内存库**。
- **②登记与 DDL 的缺口已补**（本批）：惰性清理名单驱逐后由 `cleanup_analysis_temp_tables` 真正 DROP。
- **③结果集侧（`tmp_q_`）仍未接**：编辑器要求结果集活到用户不用为止，回收策略得与编辑器生命周期一起定（改前缀为 `tmp_q_` + 结果集丢弃 / 项目关闭时 drop；`crates/mock/src/engine.rs` 的 `clear_temp_tables` 就是范本）。这是 K16 唯一剩下的部分。
- **④内存闸与可观测未做**（`SET memory_limit` / `temp_directory` 钉 `.rds/tmp` / 临时表计数上报），等你拍板。
- **诚实记档**：`profile_column_from_table` / `batch_evaluate_columns` 目前**没有宿主侧调用者**（表入口与导航右键欠账），所以这两个函数的行为变化**没有 live 测试能盖住**——集成测试走的是 DuckDB 内存临时表直建路径，engine 侧 6 项单测盖的是 `analysis.rs` 本身。没有写易碎的 live 集成测试来凑覆盖率。

### 2026-09-17 — 死副本与残留规则收口（Q2 / Q3 / K15 处置）

**已完成并验证**（`cargo test -p rds-insight --lib` **202 项** + 集成 **13 项**全绿；本批只改常量与注释，无逻辑改动；clippy 零告警）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| **Q2 = K2** 删重复规则目录 | `crates/engine/insight-rules/` 整目录删除——**没有任何代码引用它**（v1 迁移时留下的死副本，内容还是旧的，带着 `str`）。要找回从 git 历史取 | — |
| **Q3 = K3** 下线 `table-quality-overview` | 它的 SQL 从一张**从不存在的物化表**（`insight_column_stats`）里读逐列统计；静态 SQL 不可能对任意表的每一列算统计（需动态 SQL），而能力已在 Rust 侧（「评估全表」+ 表质量摘要）。留着就是「一条永远跑不通的内置规则」 | `insight-rules/table/` |
| **K15** 下线 `quality-score` | 残留规则：无代码按 id 执行，自述也写着真实评分在 `quality_scorer.rs`；且 SQL 对文本列跑不通（`AVG(VARCHAR)` 是绑定错误）。同一件事不留两份口径 | `insight-rules/quality/`（目录随之清空） |
| 口径同步 | `BUILTIN_RULE_COUNT` 18 → **16**；`jobs` 测试里硬编码的 18 改成引用常量（免得下次再漂）；代码注释与使用手册 §4.8（表 + 注）、README / 原型 / 本方案里的「18 条」全部对齐 | 多处 |
| **顺带的发现（新记 K16）** | **临时表的 TTL/上限在生产里是死代码**：`TempTableManager` 除测试与一个没人调用的 `list_by_source` 外**没有调用者**，而实际建表路径（`create_duckdb_temp_table`）建的表叫 `rs_<uuid>`（不登记、不带 `tmp_q_`/`tmp_i_` 前缀）→ 进程内内存表**永不回收**。而文档写的「TTL 30 分钟」与实际行为不符——这比单纯泄漏更难查。已记档（含三条处置方向），与 Q1 一起等你定 | 架构 §7 行改注 + K16 |

**对用户的影响**：内置规则从 18 条变 16 条（规则管理里少两条）。若曾对这两条做过启停，那份项目层抑制记录会被 `plan_index` 原样保留（不报错、也不会把规则复活）。


### 2026-09-17 — 规则校验补强：Q6 / Q7 落地（K11 / K12 结案）

**已完成并验证**（`cargo test -p rds-insight --lib` **202 项** + 集成 **13 项**全绿；本批文件 `cargo clippy --all-targets` 零告警）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| `value_type` 白名单（Q7 → **D48**） | 解析期校验；报错写清「哪个输出字段用了什么值 + 可用取值清单」。白名单常量与执行器的 `match` 分支一一对应（`f64` / `f64?` / `i64` / `i64?` / `String` / `string` / `String?` / `string?` / `bool` / `bool?` / `usize`） | `rule_registry.rs`（`VALUE_TYPES`） |
| 门控字段必须存在（Q6 → **D49**） | 解析期校验 `[[quality]] field` ∈ `[[output]] json_name`，报错列出该规则已有的输出字段。**值合法地为 `null`（空表算不出空值率）仍算通过**——那是刻意的，要它在 `null` 时也失败就加 `min` | `rule_registry.rs`（`validate_rule`） |
| 校验挂点 | 两条都加在 `parse_rule_toml` 里：注册表 / 索引器 / 测试共用这一个入口，所以**索引里显示的错误原文就是这些文案**（用户能照着改） | `rule_registry.rs` |
| 内置规则改正 | `null-check` 的 SQL 真算出 `null_rate`（`ROUND((COUNT(*) - COUNT(col)) * 1.0 / NULLIF(COUNT(*), 0), 4)`，空表给 NULL → 声明 `f64?`）；全仓 **8 处** `value_type = "str"` 全部改正（`quality-score` 3 × `String?`、`table-column-overview` 4 × `String`、`table-null-overview` 1 × `String`） | `insight-rules/**` |
| 测试 | 解析期 2（白名单外的值 / 门控字段不存在，都断言报错文案含可用取值与规则 id）+ **K11 回归 1**：直接解析**二进制里那一条** `null-check`，在真 DuckDB 临时表上跑——50% 空值必须失败、空表不误报 | `rule_registry.rs` + `rule_executor.rs` |
| 顺带的发现 | 内置 18 条里有 **10 条没有任何代码按 id 执行**（`null-check` / `quality-score` / 四条 `table-*` 等）：它们是规则库条目，不是面板会跑的东西。其中 `quality-score` 另记 **K15**（残留规则 + SQL 对文本列跑不通），与 K3 同类待决 | 架构 §11 |

**对用户规则的破坏性变更（有意）**：`value_type = "str"` 现在会让规则变**无效**（规则管理里一行红字 + 明确文案）。这正是 Q7 的本意（早失败优于静默错）；迁移就是把 `str` 改成 `String` / `String?`。


### 2026-09-17 — Phase 5 三批：存储清理（5.4 收尾）

**已完成并验证**（`cargo test -p rds-insight --lib` **199 项** + 集成 **13 项**全绿；本批文件 `cargo clippy --all-targets` 零告警）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 入口 | 历史 Tab 头行的「清理」：无项目 / 无快照 / 有动作在跑三种情况置灰，且**各自写明原因**（不是点了没反应） | `insight_view.rs`（`begin_cleanup` / `history_busy_ready`） |
| 确认框 | `window.open_dialog` + `DialogButtonProps`（「清理」取 danger 变体）——**在 crate 内就能开**（规则对话框同款），不需要宿主配合；框内写明「删 30 天前的快照」「正文与版本链一并删除，不可撤销」与「当前：存储 … · N 个快照」 | `insight_view.rs` |
| 天数口径 | `model::SNAPSHOT_RETENTION_DAYS = 30`：对话框写的数与实际切档取**同一个常量**（事件不带天数，避免多一个入口就多一个数） | `model.rs` + `jobs.rs` |
| 回执 | `HistoryView.cleanup: Option<CleanupOutcome>`（载荷，下次刷新自然消失）；**两侧条数分开报**：对不上就是半写信号（D16），行内改 danger。没东西可清时说「没有 30 天前的快照」，不报假成功 | `model.rs` + `insight_view.rs` |
| 接缝 | `SnapshotCleanupRequested` → `request_cleanup`（保留天数由接缝持有）；失败只挂行内提示（列表与快照都还在） | `jobs.rs` |
| **修的真问题** | `InsightColumnStore::cleanup_older_than` 写的是 `created_at < (CURRENT_TIMESTAMP - INTERVAL ? DAY)`——**DuckDB 不接受 `INTERVAL` 里的绑定参数**（实测 `Parser Error: syntax error at or near "?"`），所以自迁入以来这条路径**一次也没跑通过**。现改为把天数（`i64`，无注入面）拼入 SQL。是新写的存储层用例第一次真调它才暂露出来 | `store/body.rs` |
| 测试 | 存储层 1（把两侧 `created_at` 回填到 40 天前 → 30 天档两侧各删 2；刚存的不受影响）+ 模型 1（回执三态：空操作 / 正常 / 两侧对不上）+ 视图 1（Root 窗口根：点入口只弹框不发请求 → 确认后发请求且不允许重入 → 回执随载荷回来）+ 接缝与集成各 1（空操作也要有回执、列表不动） | 各文件测试模块 |

**覆盖率说明（不做假定性的「已完全覆盖」）**：真正的删除路径只在存储层验（那里能把时间回填）；服务层与接缝验的是「开库 → 调存储 → 贴回执 → 回列表」以及空操作时的诚实文案。服务层用 `days = 0` 去造删除**不可行**：DuckDB 的时间戳带亚秒而 SQLite 只到秒，0 天档会出现一边删一边不删（那反而是个值得知道的细节，但不是本批要钉的东西）。

**Phase 5 完成**：5.1 保存入口、5.2 历史列表、5.3 版本对比、5.4 用量与清理全部落地。剩下的是**宿主侧欠账**（表入口 / Schema 导出按钮 / 下钻登记临时表），都在并行改动的 `panels/` 里。


### 2026-09-17 — Phase 5 二批：版本对比（5.2 收尾 + 5.3）

**已完成并验证**（`cargo test -p rds-insight --lib` **196 项** + 集成 **12 项**全绿；`cargo test -p rds-workbench --test insight_entry` 2 项全绿（需临时带 `--features opener/reveal`，同上批）；本批文件 `cargo clippy --all-targets` 零告警）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 方向定案 | 对比方向固定为**「选中的那一版 → 最新一版」**（D43）：这个 Tab 问的是「和上次比变了什么」，两个方向都能选只是多一个状态（还要多一套「谁是基准」的文案）。最新那一版**不给点击入口**（`ListItem` 的 disabled 态），状态层也兜一道底——拿最新当基准无从比 | `insight_view.rs` + `service/mod.rs` |
| 视图模型 | `VersionDiffView` / `DiffRowView` / `DeltaView`：行集合与「列」Tab 的**基础统计同源**（同一个 `ColumnProfileView`），所以对比里的数字就是用户已读过的那几个（D44） | `model.rs` |
| 行集合 | 质量分（头号结论，两侧都算得出才给）→ 总行数 / 非空值 / 空值 / 空值率 / 唯一值 / 类型 → 类型专属行（平均 / 长度范围 / 跨度 …）。「空值」与「空值率」在「列」Tab 里合成一行（`12（14.3%）`）便于阅读，而对比要的是**能对齐的两个数**，故拆开；旧版有、新版没有的行（类型变了）如实列出并在新版侧写「—」 | `model.rs` |
| 差值口径 | 一律**按展示精度算**（计数取整、占比 1 位小数、评分取整）：否则会出现「显示 62 → 62 却 +0.4」「显示 8.2% → 8.2% 却 +0.01」这种自相矛盾。解不出数字的行（`3 ~ 8` / `2.5（右偏）` / 类型串）只说「已变」，**不编差值** | `model.rs` |
| 三态着色 | 方向 ≠ 好坏（空值率上升是坏、唯一值上升不一定是好）→ 增 / 减 / 变化无数值 / 不变 用 `info` / `warning` / `primary` / `muted_foreground`，**不用** `success` / `danger` 预设评价；箭头（▲ ▼ ◆ ●）才是方向 | `insight_view.rs` |
| 组件 | 可点版本行用 `ListItem`（hover / 选中 / 禁用三态与其它列表同一套视觉）；对比面板用 `DescriptionList`（`columns(1)` + `bordered(false)` + `label_width` 常量），不手搭表格 | `insight_view.rs` + `ui.rs`（`INSIGHT_DIFF_LABEL_WIDTH`） |
| 载荷口径 | 对比结果就在 `HistoryView.diff` 里（与列表同一个载荷）：面板「有没有对比」只认载荷，选择位在出数时**从载荷反推**——列表刷新后「最新」就变了，留一个指向旧「当前」的选中位比不选中更坏（D43） | `model.rs` + `insight_view.rs` |
| 失败语义 | 对比失败 → `set_compare_notice`：行内提示 + **放掉选中位**（否则留下「亮着却没有面板」的死状态，而选中的意义就是「面板该在」）；列表照旧可见，不抢整页错误态 | `insight_view.rs` + `jobs.rs` |
| 服务护栏 | 拿最新一版当基准 → 「最新一版没有更新的版本可比」；版本不在列表里 → 「这一版已不在历史里（找不到 xxxxxxxx）」。都是**可读的错误**，不是静默给一份空对比 | `service/mod.rs` |
| 引导 | 只有一版时说「再存一版就能对比」；多版且未选中时说「点更早的一版，和当前对比」（行可点这件事必须说出来） | `insight_view.rs` |
| 测试 | 模型 4（方向与千分位 / 完全一致 / 文本行与类型变化 / 空值拆两行）+ 视图 2（选中→事件→出数面板在（`debug_selector` 锚点）→✕ 关掉→刷新后落回；对比失败放掉选中位）+ 接缝 1（扩展上批用例：两次保存之间真插一行，对比看得见 `3 → 4`）+ 集成 1（真项目：两版对比、基准=最新报错、版本不存在报错） | 各文件测试模块 |

**Phase 5 剩余**：TTL 清理入口（`cleanup_old_insight_snapshots` 已就绪，等 **Q4/Q5** 拍板保留天数与双写补偿）。


### 2026-09-17 — Phase 5 一批：快照历史（5.1 + 保存入口）

**已完成并验证**（`cargo test -p rds-insight --lib` **190 项** + 集成 **11 项**全绿；`cargo test -p rds-workbench --test insight_entry` 2 项全绿（当时需临时带 `--features opener/reveal`：`resource_host.rs` 用了 `opener::reveal` 而 `opener` 尚未开该 feature，与本批无关；**已由根 Cargo.toml 补上 `features = ["reveal"]`，现已不需要**）；本批文件 `cargo clippy --all-targets` 零告警）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 保存入口 | 历史 Tab 顶部的「保存」：无项目置灰并写明原因（快照落项目目录），保存中按钮改文案并置灰。**保存入口必须有**：v1 的 `saveCurrentInsight` 没有任何调用方，前端根本点不到 | `insight_view.rs` + `jobs.rs` |
| 视图模型 | `HistoryView` / `HistoryEntryView` / `StorageStatsView`：版本列表 + 每行短版本号（`created_at` 只有秒级精度，D18 撞秒时靠它认人）+ 「当前」/「首版」标记 + 存储用量 | `model.rs` |
| 列表口径 | 顺序**只由存储层的 `ORDER BY` 决定**（视图不再排序：显示顺序与写入顺序只能有一处权威）；分页上限 `HISTORY_PAGE_SIZE` 单一来源（查询与界面提示共用），满一页明示「只列出最近 N 条」（与采样提示 D15 同一立场）；**不另设内层滚动**（面板主体已是滚动区，嵌套滚动滚轮会卡住；Phase 5.2 的对比面板要接在列表下方） | `model.rs` + `insight_view.rs` |
| 用量行 | 存储统计取不到就**整行不显示**（编一个 0 会让人以为历史被清了）；显示串用后端给的（带单位），面板不换算 | `model.rs` + `insight_view.rs` |
| 失败语义 | 保存失败只挂**行内提示**（已有历史必须还在，整页转错误态会让人以为快照丢了）；读历史失败推整页错误态（列表无从部分展示） | `insight_view.rs` + `jobs.rs` |
| 状态分派 | `emit_request_for_tab` 改成「发得出去就进加载态，**发不出去就落空态**」：原先只是沉默返回，而调用方为了「点了有反应」已先摆上骨架——骨架会一直转下去（无项目时切「历史」正是这种情况）。空态文案按 Tab + 项目状态给 | `insight_view.rs` |
| 顺带修的真问题 | `save_column_snapshot` 原先写完快照又**重新开一次项目库**读历史——DuckDB 在同一进程里对同一份文件只允许一个实例，第二次 open 必失败（集成测试实测表现为「快照真落库了，却提示保存失败」）。现改为复用同一个句柄（`read_history`，保存后的读回与单纯读取共用一处口径） | `service/mod.rs` |
| 测试 | 视图模型 3（最新/首版标记与顺序不重排 · 用量行 · 缺类型与满页）+ 视图实体 2（有项目才取数·保存事件·出数落回·失败不清列表；表目标切历史落空态）+ 接缝 1（真实项目：列画像 → 切历史 → 两版保存 → 版本链 → 临时表消失后失败只挂提示）+ 集成 2（真项目目录下保存→读历史→版本链；无项目的错误文案） | 各文件测试模块 |

**Phase 5 剩余**：版本对比面板（`old → new (±Δ)` 三色）；TTL 清理入口（`cleanup_old_insight_snapshots` 已就绪，等 Q4/Q5 拍板保留天数与双写补偿）。

**宿主侧待接**：历史 Tab 的保存 / 读取与其他 Tab 同形——装配处只多两行（`attach` 已把 `SnapshotSaveRequested` / `HistoryRequested` 接进接缝，宿主持有 `Subscription` 即生效）。


### 2026-09-17 — Phase 4 一批：Schema 健康报告（4.1 门面 + 4.2 视图 + 4.3 导出 + 4.4 下钻）

**已完成并验证**（`cargo test -p rds-insight --lib` **184 项** + 集成 9 项全绿；`cargo test -p rds-workbench --test insight_entry` 2 项全绿；本批文件 `cargo clippy --all-targets` 零告警）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 门面 | `InsightService::schema_report_view`：分析器是异步的（走源库内省），门面保持同步口径（同一道阻塞桥），调用方放后台 | `service/mod.rs` |
| 目标 | `InsightTarget::Schema` 补 `database`：分析器的表清单要按 `table_catalog` 过滤，否则同名表会跳库混在一起 | `model.rs` |
| 视图模型 | `schema_view.rs`：四个分区（外键候选 / 类型不一致 / 孤立表 / 冗余列）+ 行语气（中性 / 提示 / 问题）+ 可下钻的表清单；**等级按分数现算**（`Grade`），不再解析领域里的 `health_level` 字符串——顺手把 `schema_analyzer::compute_health` 的分档也换成同一份阀值（原先 50–70 那段自己叫「需改进」） | `schema_view.rs`（新）+ `schema_analyzer.rs` |
| 导出 | `to_json` / `to_markdown`（纯函数）：JSON 分组键用稳定英文（不拿中文当键）、`issue_count` 与界面同源；Markdown 表格转义竖线与换行（否则表会断列），空分区写明「空」的含义 | `schema_view.rs` |
| 结构 Tab | 健康条（分数 + 等级 + 规模 + 需关注项数）+ 四个折叠区（默认全展开，**空区也渲染**并解释「空」是什么）+ 表名下钻热点（每行最多 4 个） | `insight_view.rs` |
| 接缝 | `SchemaReportRequested`（带连接 / 库 / schema）；`TableDrilldownRequested` 只报「看哪张表」——把源表登记成临时表是宿主的活（它才知道连接与临时表约定） | `jobs.rs` |
| 顺带修的真问题 | ① `set_target` 原先一律发 `ProfileRequested`：结构目标因此永远停在加载态。现改为**按目标种类发对应请求**；② 换目标**必须清载荷**——载荷按 Tab 分开存，不清就会渲染出上一个目标的数据（比空白更坏） | `insight_view.rs` |
| 测试 | 视图模型与导出 9（分组 / 等级取数 / 语气 / 问题计数 / 空报告 / JSON 稳定键 / Markdown 可读 / 转义 / 陌生置信度）+ 结构 Tab 实体与窗口 1 | `schema_view.rs` + `insight_view.rs` |

**未接（下一批或宿主侧）**：导出按钮需要宿主提供「选路径 + 写文件」（面板不发一个无人处理的请求）；下钻需要宿主把源表登记成临时表。两者都是宿主侧一两行，但落在正在并行改动的 `panels/` 与编辑器里。


### 2026-09-17 — Phase 3 三批：多列分析的界面与接线（3.2 / 3.3 收尾）

**已完成并验证**（`cargo test -p rds-insight --lib` **174 项** + 集成 **9 项**全绿（连跑 3 次稳定）；`cargo test -p rds-workbench --test insight_entry` 2 项全绿；本批文件 `cargo clippy --all-targets` 零告警）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 数据态改形状 | `InsightPanelState::Data` 不再带载荷：**载荷按 Tab 分开存**（`PanelData { column, table, multi }`）。Tab 条是「同一目标的多个视角」，切 Tab 不该丢掉别的视角已取到的数据（单格子设计下切「表」再切回「多列」会把表单与结果丢掉）（**D35**） | `model.rs` + `insight_view.rs` |
| 渲染以载荷为准 | `render_body` 先看载荷、再看状态：错误态整页接管，没载荷时才分骨架 / 入口提示。于是「切到另一个 Tab 时后台还在取数」不会把已取到的那个 Tab 盖成骨架 | `insight_view.rs` |
| 切 Tab 即取数 | `set_tab` 在**事件路径**补一次取数（载荷缺失才发）：多列 → 表单、列 / 表 → 对应画像。判定只看载荷在不在，不拿 `Loading` 当挡板（表目标先到的是表载荷，拿它挡就会把「切到多列」的第一次请求吃掉） | `insight_view.rs` |
| 多列界面 | 列多选（带**序号**：顺序即参数顺序，相关性是有方向的）+ 规则单选（类型不匹配置灰并写明“需 N 列：…”）+ 执行 + 结果区（键值 / 表格）+ 门控提示 + 失败只挂一行提示（表单与已有结果照旧可见） | `insight_view.rs` |
| 接缝 | 两个新事件：`MultiColumnRequested`（取表单）与 `MultiRunRequested`（执行）；执行失败不推整页错误态 | `jobs.rs` |
| 测试辅助 | 新增 `test_support`（记录型宿主 `EventSink`）：订阅必须被持有这类细节只该错一次 | `test_support.rs`（新） |
| 测试 | 实体 3（切 Tab 取数 / 选择顺序与执行门槛 / 结果与失败不动表单）+ 窗口 1（表单·单值·表格·提示逐帧）+ 接缝 1（真实接线：切 Tab → 表单 → 跑规则 → 结果回填） | 各文件测试模块 |

**发现并修的真问题**：集成测试并行跑时抢**进程级并发配额**（上限 4）而随机报 `洞察分析任务过多`——加串行锁（与 `rule_state_guard` 同一手法），非并发引入。

**Phase 3 收尾**：3.2 / 3.3 / 3.4 均已落地；剩下的是宿主侧入口（`Shared::open_insight_table` 与导航右键「查看统计」）。


### 2026-09-17 — Phase 3 二批：多列分析的数据层（3.2 一半 + 3.3 一半）

**已完成并验证**（`cargo test -p rds-insight --lib` **169 项** + 集成 **9 项**全绿；本批文件 `cargo clippy --all-targets` 零告警）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 列清单 | 与表探查**同源**（同一个 `get_temp_table_profile`）：v1 的 `availableColumns` 恒空是「多列分析从未跑通」的根因，不再另存一份列清单（**D32**） | `service/mod.rs::multi_column_view` |
| 规则清单 | `category = multi` 的规则 → `MultiRuleView`（类型化）：除 `table` 外的参数即**顺序列位**，`applies_to` 即每位的类型要求 | `model.rs` + `insight_engine::list_insight_rules` |
| 可选项判定 | `arity()` / `accepts(kinds)` / `types_hint()`：列数与类型族逐位比对（`Any` 放行；缺位不限制）。类型不符**不在服务层报错**——SQL 自己会拒，界面先把不可用的选法标出来就够（**D33**） | `model.rs` |
| 结果渲染 | `MultiResultView::{Single, Table}`：按**数据形态**分派（`Value::Object` → 键值行；`Value::Array` → 表格，表头取各行键的并集，缺键补「—」）；`json_cell` 统一处理 null / 数值格式 | `model.rs` |
| 参数拼装 | `rule_params(parameters, temp_table, columns)`：`table` 恒为临时表，其余按选择顺序对位；列数不匹配**直接报错**（少传参数会让 SQL 静默变成另一个查询） | `service/mod.rs` |
| 质量门控 | `quality_notes(report)`：只把**未通过**的检查转成提示行（有规则文案用规则的，没有就拼一条可定位的） | `model.rs` |
| 测试 | 视图模型 7（规则解析与列位 / 接受判定 / 单值与列表形态 / 缺键不错位 / 标量兵底 / 门控只留失败 / 结果就地写入）+ 参数拼装 2（对位与不匹配报错）+ 集成 3（真 DuckDB：列清单与候选规则、Pearson 完全相关=1.0、交叉频次表成表） | 各文件测试模块 |

**下一批（Phase 3 三批）**：多列 Tab 的界面与接线——列多选（带序号）+ 规则选择 + 执行按钮 + 结果区（键值 / 表格）+ 门控提示；Tab 切到「多列」时才发取数请求（事件路径，不能在 render 里发）。


### 2026-09-17 — Phase 3 一批：表探查 + 评估全表（3.1 + 3.4 + 2.2）

**已完成并验证**（`cargo test -p rds-insight --lib` **160 项** + 集成 **6 项**全绿；`cargo test -p rds-workbench --test insight_entry` 2 项全绿；本批文件 `cargo clippy --all-targets` 零告警）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 表内省 | `get_temp_table_profile(_on)`：`DESCRIBE` + `COUNT(*)` → 列元数据与行数。走临时表而不是源库（面板的表目标只有 `temp_table`），也不按 catalog 过滤（`ATTACH` 进来的表不得混入）——**D29** | `insight_engine.rs` |
| 视图模型 | `TableProfileView` / `TableColumnView` / `TableQualityView` / `TableEvalProgress`；`PanelData::Column | Table` 取代「数据态只装列画像」 | `model.rs` |
| 表 Tab | 表头（表名 + 行数）+ 评估入口 + 表级质量卡 + 进度行 + 列元数据表（序号 / 列名【PK】/ 类型 / 可空 / 质量）+ 底部**实际**口径；未评估显示 `—`，不产假分数 | `insight_view.rs` |
| 评估全表 | **串行逐列**、每列单独回填（**D30**）：真进度而不是转动图标；单列失败直接报错，不静默跳列。表级摘要与列分数同源（`compute_table_quality`） | `jobs.rs` |
| 列名下钻 | 表 → 列（点列名切「列」Tab，目标持 `temp_table` + 列名 + 类型） | `insight_view.rs` |
| 类型族归位 | 四个判定谓词自 `insight_engine` 迁到 `model`（**唯一来源**）：列画像的分派与表探查的徐标共用，依赖方向变单一（算法层 → 领域词汇） | `model.rs` / `insight_engine.rs` |
| 测试 | 视图模型 6（列 / 无假分 / 进度单调且不丢分 / 摘要与问题列数 / 行数缩写 / 进度边界）+ 内省 2（真 DuckDB：列序与行数、不存在的表报错）+ 接缝 2（表目标→探查回填；评估的进度序列断言有中间态）+ 窗口 1（四类评估状态逐帧）+ 集成 2（真 DuckDB 表探查、评估逐列打分） | 各文件测试模块 |

**测试踩的坑**：观察者（`cx.observe`）的订阅必须由**外层**持有——写在 `cx.update(\|cx\| { let _obs = … })` 里会随闭包一起析构，表现是「进度序列为空」而功能其实正常。

**与原型的两处**（原型已加修正说明）：① 底部写的是**实际**口径（全量统计 + 样本前 5 行），不照搬原型的「500 行采样」（**D31**）；② `PK` 角标只在元数据真带主键时出现，从查询结果建的临时表通常没有。

**待接线**：宿主入口 `Shared::open_insight_table`（与 `open_insight_column` 同形，一行）——本次未改 `panels/shared.rs`（该文件正在并行改动中）；导航右键「查看统计」与结果集入口因此还没接上。

**下一批**：Phase 3.2/3.3（多列分析重新设计：列清单改取真实列元数据 + 规则选择与结果渲染）。


### 2026-09-16 — Phase 2 二批：规则管理对话框（2.3 + 2.4）

**已完成并验证**（`cargo test -p rds-insight --lib` **147 项** + 集成 4 项全绿；`cargo test -p rds-workbench --test insight_entry` 2 项全绿；本批文件 `cargo clippy --all-targets` 零告警）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 对话框 | 新实体 `RulesView` + 模态对话框（`window.open_dialog`）：三层分组（项目 → 全局 → 内置，按**覆盖优先级**而非加载顺序）/ 搜索 / 启停开关 / 校验错误行（错误原文）/ ⧉ 打开规则文件 / 底部统计 | `rule_view.rs`（新文件，~700 行含测试）|
| 视图模型 | `RulesData` / `RuleGroupView` / `RuleRowView` / `RuleRowStatus`（纯数据 + 纯函数，可脱窗口单测）；搜索在**事件路径**重算可见行，render 只读 | 同上 |
| 取数与写库 | `InsightService::{rules_data,toggle_rule,create_rule_file}`：**先同步后读数**；两层索引库（项目库现开 / 全局库进程单例）收在私有 `IndexStores`；门面保持同步口径，阻塞段由调用方放后台 | `service/mod.rs` |
| 接缝 | `jobs::attach_rules(panel, host_cx, 项目根闭包)`；四个事件（重载 / 开关 / 新建 / 打开文件）；订阅回调**只提交任务、不回头改实体**（避免重入） | `jobs.rs` |
| 面板入口 | ⚙ 由「占位禁用」改为**按项目状态启用** → 开窗 + 发一次取数；`InsightView::rules_view()` 供宿主接缝 | `insight_view.rs` |
| 2.4 目录 | `rule_dir`（作用域 → 目录，全集一处拼）/ `create_rule_file`（建目录 + 写模板）/ `new_rule_template`（**能解析但不生效**：`applies_to = []`，文件名与 id 按序号避让） | `service/indexer.rs` |
| 宿主 | 只多一行：`insight::jobs::attach_rules(&insight_panel, cx, 项目根提供者)` | `workbench/src/panels/right.rs` |
| 测试 | 视图模型 9 项（分组顺序 / 抑制记录不被当成项目规则 / 同 id 覆盖 != 禁用 / 错误原文与文件丢失 / 统计 / 搜索 / 过滤保骨架 / 大小写 / 空数据）+ 窗口与实体 3 项（弹窗 + 事件 + 四态逐帧 + 就地翻位）+ 接缝 2 项（真实项目库：加载→开关一路走到规则集；新建目录与模板）+ 目录派发与新建模板 3 项 | 各文件测试模块 |

**排掉的坑**：`MutexGuard` 与进程级规则缓存——开关会写 `apply_disabled_rules`，必须与同类缓存测试串行（`rule_state_guard()`）；首次跑全集时因此而间歇失败。另：单测里不真的拉起系统编辑器（`cfg!(test)` 短路），否则 `cmd.exe` 的输出会混进测试日志。收尾时把作用域 → 目录的派发收敛到 `service::indexer::rule_dir` 一处（D28）——原先视图层与同步器各有一份，`.RSmeta` 的拼写会在两处漂移。

**与原型的两处**（已在 §5 标注）：① 新建入口除「＋ 新建项目规则」外，全局分组在目录缺失时也给「创建目录并新建规则」（K7 的落地形态）；② 启停开关是**受控**的（先就地翻位、后台落库失败时回填真值并把原因挂在状态行），不静默丢掉失败。

**下一批**：2.2 表级「评估全表」+ 进度——已于 Phase 3 一批与表探查视图合并落地（入口在表探查上）。


### 2026-09-16 — Phase 2 第一批：质量评分卡（列级）

**已完成并验证**（`cargo test -p rds-insight --lib` **130 项** + 集成 4 项全绿；`cargo clippy -p rds-insight --all-targets` 对本批文件零告警）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 等级归位 | 新增 `Grade`（五档 + `of(f64)` + `label()`）；列级与表级**共用**一份阀值与文案——表级原先写死 `85/70/50/30`，视图再取一次色就是三处漂移 | `quality_scorer.rs` |
| 评分卡进视图模型 | `ScoreView` / `DimensionView`；`ColumnProfileView::score: Option<ScoreView>`，**全空列不产分**（`total_count > 0` 才给）——全空列若给 0 分，用户会读成「质量差」而非「没数据」 | `model.rs` |
| 评分卡渲染 | 钉在**滚动区之外**（分数是这列的头号结论，不该滚走）；总分行大字 + 四维细条（每维条色按**自己**的等级，短板一眼可见），与分布区共用 `ratio_bar` | `insight_view.rs` |
| 尺寸常量 | `INSIGHT_SCORE_FONT`（1.75rem）；占比条高由分布专用改为两者共用，只留一条常量 | `ui.rs` |
| 测试 | 等级阀值边界（`85/70/50/30` 含端点、`84.99` 等降级）；`level` 字符串与 `Grade::of(总分)` 对账（防「88 分 · 良好」）；权重合计 1.0；空列 `score == None` 且基础统计照给；渲染测试逐帧画过评分卡 | 三文件测试模块 |

**排掉的坑**：`model.rs` 与 `insight_view.rs` 原先都从**私有再导出**路径引入 `Grade` / `QualityScore`（E0603）——四个领域类型是 `pub use` 到 crate 根的，视图侧一律走 `crate::{…}`，新增类型沿同一路径。

**下一批**：2.2 表级「评估全表」+ 进度（复用 `jobs.rs` 形态）；2.3 规则管理对话框（新文件 `insight/src/rule_view.rs`，含改错行与打开规则文件）；2.4 全局规则目录首次写入时创建（K7）。


### 2026-09-16 — Phase 1 第六批：尺寸常量改取外壳（镜像债清零）

**已完成并验证**（`cargo test -p rds-insight --lib` **124 项** + 集成 4 项全绿；零告警）

| 项 | 内容 |
| --- | --- |
| 依赖 | `insight` 新增 `workbench_shell` 依赖（方向 insight → workbench-shell；后者不得依赖任何特性 crate），与 `workbench` 同取一份共用资产 |
| 常量 | `insight/src/ui.rs` 的 4 个**镜像**常量（面板头 / 行高 / 内距 / 空态图标）改为 `pub use workbench_shell::ui::{…}`——镜像两份会在任一侧调整时静默错位，而这几条恰好是面板与外壳必须一致的值；视图侧 `ui::PANEL_HEADER_HEIGHT` 等路径不变 |
| 保留 | M8 专用常量（Tab 条高 / 直方图条高 / 占比条高 / 样本截断 / 行内图标 / 分区标题字号）仍登记在本 crate |

**背景**：原先的结论是「`shared` 不依赖 gpui-kit，跨 crate 尺寸常量没有归宿；出现第三个使用方时再上收」。外壳 crate 落地后归宿已存在（`crates/workbench_shell`，定位「面板状态 + 视图共用资产」），洞察作为第二个视图自持的模块直接接上。


### 2026-09-16 — Phase 1 第五批：取数作业随特性 crate 归位

**已完成并验证**（`cargo test -p rds-insight --lib` **124 项** + 集成 4 项全绿；零告警）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 归位 | `services/insight_jobs.rs`（workbench）→ `insight/src/jobs.rs`：符合面板耦合治理计划 §5 的目标形态「特性 crate 内的 jobs + 面板 drain」。Phase 2 的「评估全表 + 进度」与 Phase 5 的快照保存都要复用同一套后台形态，先摆对位置就不用写两遍 | `crates/insight/src/jobs.rs`（新） |
| 宿主编排面 | 只剩一行：`insight::jobs::attach(&panel, cx, 项目根提供者闭包)`——事件接缝与取数都在特性 crate 内；项目根用**闭包**而非值（项目可能已切换，且解析结果必须在提交任务前脱离 `Shared`） | `crates/workbench/src/panels/right.rs` |
| 契约测试 | `attach` 的宿主契约在本 crate 内验住：订阅被触发 → 提供者被问一次 → 后台取数真的跑起来并回填错误态（不是只验「不 panic」） | `jobs.rs` 测试 |

**测试坑（已写进注释）**：宿主实体必须被持有——只留 `Subscription` 不足以让订阅者存活，实体一旦释放，订阅就被剪掉。

**不变式**：项目根在**提交前**解析成 `PathBuf`（所有权数据）才进后台任务——`Shared` 的 `Rc<RefCell<…>>` 不出线程；弱句柄升级失败即丢结果（面板已关）。

**验证边界**：workbench 侧当前因在途重构（`Shared` 字段搬迁、`NavDropMode` 等符号调整）整体编译不过，因此本步的可验证面收在特性 crate 内（`jobs.rs` 自己验了宿主那一行的契约）。


### 2026-09-16 — Phase 1 第四批：列入口命令 + 重算键位

**已完成并验证**（`cargo check -p rds-app` 零告警；`rds-insight` lib **121 项**；新增 `crates/workbench/tests/insight_entry.rs` **2 项**——走真实接线的端到端）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 1.5 | `Shared::open_insight_column(temp_table, column, data_type)`：入口只发这一条命令（展开右 Dock + 递目标），面板与取数链都在 insight crate | `crates/workbench/src/panels/` |
| 1.6 | `InsightRefresh` 绑 `Ctrl+Shift+R`（key context = `insight`，只在焦点位于面板内生效）；面板 `on_action` 与面板头 ⟳ 走同一条路径 | `crates/app/src/main.rs`、`insight_view.rs` |
| — | app 新增 `insight` 依赖（与 editor / settings 同一口径：app 直接依赖要绑键的 Feature crate） | `crates/app/Cargo.toml` |
| 测试 | `insight_entry.rs`：请求 → 后台执行器 → 回填（数据态 + 友好错误态），首次覆盖装配胶水（不只重测服务层） | `crates/workbench/tests/` |

**未完成**：结果表列头右键「洞察此列」的**界面落点**在结果区（M5/编辑器在途）——宿主侧入口已就位，接线时只需一行；导航树表节点的右键「查看洞察」已能打开面板，表探查取数属 Phase 3。


### 2026-09-16 — Phase 1 第三批：右 Dock 装配 + 后台取数接线

**已完成并验证**（`cargo check -p rds-workbench --lib` 零告警；`rds-insight` lib **121 项**全绿；`cargo build -p rds-app` 通过；**实际启动一次实例**：右侧洞察面板正常渲染、零 stderr）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 装配 | 右 Dock 去掉三行占位：面板实体在**构造期**创建 + 登记 `Shared::insight_panel`（右键入口要在事件路径拿到句柄，懒创建会丢目标），渲染改为转发 `InsightView` | `crates/workbench/src/panels/` |
| 宿主桥 | **不设 trait**：面板与宿主之间只用事件 + 两个公开方法（`set_profile` / `set_error` / `set_project_open`），因为面板没有需要同步回调宿主的能力（⚙ 也是事件）。何时才需要 `MockHost` 那种 trait：出现「面板得同步问宿主」的需求时（如导出目录、只读态） | — |
| 事件接缝 | `InsightEvent::ProfileRequested { target }`（带 payload，宿主不必回读面板）；`set_target` 与 ⟳/重试都发它 | `insight_view.rs` |
| 后台取数 | `insight::jobs`：面板只发请求 → **后台执行器**上跑阻塞的 `profile_column_view` → 弱句柄回填四态（面板已关则丢结果）。项目根在提交时解析成所有权数据（`Shared` 不可跨线程）。宿主侧只剩一行 `jobs::attach(&panel, cx, ‖ 项目根)` 胶水 | `crates/insight/src/jobs.rs`（新，2026-09-16 从 workbench 迁入） |
| 项目开关 | 面板给出「未打开项目：画像可用，规则管理与快照不可用」提示；宿主在**渲染时**同步（同 `apply_*_mode` 口径，值不变不 notify） | `panels/` + `insight_view.rs` |

**一处刻意的降级**：面板头的 ⚙（规则管理）**暂时禁用**并注明「Phase 2 落地」——规则管理对话框属 Phase 2，先给一个「点了没反应」的按钮等于静默失效（同「注册了才宣传」）。`InsightEvent::RulesRequested` 也随之移除（无生产者就不留死变体），Phase 2 接入时一并恢复。

**未完成（下一批）**：1.5 入口接线——结果表列头右键「洞察此列」、导航树表右键「查看统计」；1.6 键位注册。面板现已装配并可打开，但**还没有入口把目标递进来**，所以看到的是空态引导。


### 2026-09-16 — Phase 1 第二批：面板侧编排（1.1）

**已完成并验证**（`cargo test -p rds-insight --lib` **118 项全绿**，上一批 113 → +5；零告警）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 1.1 | `InsightService::profile_column_view(project_root, temp_table, column)`：一次拿齐「取当前项目规则集 → 计算链 → 视图模型」。直方图在 `ColumnInsightFull::histogram` 上而不在 `NumericStats` 里，所以由视图模型构造器一并消费，面板不必分两次取数 | `crates/insight/src/service/mod.rs` |
| 1.1 | `InsightService::describe_error(&CoreError) -> InsightErrorInfo`：错误 → 「文案 + 是否可重试」。**不展示 `CoreError` 的 `Display`**（带 `[code]` 内部错误码）；并发 → 复用引擎那份文案且可重试；结果集失效/过期 → 不给重试（要重新执行查询）；连接抖动 → 可重试；其余原文照给 | 同上 |
| — | `ERR_TOO_MANY_CONCURRENT` 由 `const` 改 `pub`，成为并发文案的**单一来源**（面板原样展示，不再各写一版） | `crates/insight/src/insight_engine.rs` |

**文档随之收敛**：原型 §4「并发受限」与架构 §8 原本写着两份不同文案（「正在分析中，请稍候」vs 引擎那份），已统一为引擎那一份并注明单一来源；架构 §5.3 补上「面板侧编排 + 阻塞语义（宿主放后台线程）」；0.4 记录里的旧文案说明也已改正。

**顺带发现（已修，真缺陷）**：类型族判定原本在 `engine/services/duckdb_service.rs`，用**精确**字符串比较，而 DuckDB 的 `typeof()` 会带参数 / 后缀——`DECIMAL(12,2)` 判不进数值族而落入文本分支（**金额列必中**：没有均值 / 中位 / 直方图，反而多出一行「长度范围」）；`TIMESTAMP_NS` / `TIMESTAMP WITH TIME ZONE` 同理。修法：取基名后比较（时间族按前缀判），并补上 `UBIGINT` 等无符号整型。

同时把四个判定函数（`is_numeric_type` / `is_datetime_type` / `is_binary_type` / `is_array_type`）**移入 `insight_engine.rs`**：唯一使用方是洞察的类型分派，且它们编码的是「哪种类型该用哪些统计量」——与 Phase 0 的 `detect_extremes` 同理，不该由低层持有。engine 侧只留通用的值转换（`duckdb_value_to_json`）与 `is_json_type`。

**新增端到端测试**（本模块首次覆盖「取数 → 映射」全链路）：`crates/insight/tests/column_profile_e2e.rs`（4 项）——真实内存 DuckDB 临时表 → 规则驱动统计 → 视图模型。它建在**独立进程**里（不会与 lib 单测共享 DuckDB 单例与规则缓存），不需要任何全局初始化。价值在两处：① 建表后当场拓出上述 DECIMAL 缺陷；② 用**真实 DuckDB 报错文案**验证 `describe_error` 的字符串匹配确实对得上（不靠猜）。

**验证**：`cargo test -p rds-insight` **120 项 lib + 4 项集成全绿**；`cargo check -p rds-engine --all-targets` 零告警。

**未完成（下一批）**：1.5 入口接线（结果表列头右键「洞察此列」/ 导航树右键「查看统计」）、右 Dock 装配（`workbench/src/panels/` 去掉三行占位）、`InsightHost` 宿主桥（订阅 `InsightEvent` + 提供项目根）、键位注册；以及**无项目态的降级呈现**（原型 §4：⚙ 与历史禁用 + 「打开项目后可保存快照」引导）——它需要宿主告知「项目是否已打开」，故随装配一起落，不先写一个无人调用的开关。上述落点均在 `workbench`，与并行会话正在改的 `panels/` / `view.rs` / `components/` 相交，故本批仍不动。

### 2026-09-16 — Phase 1 第一批：视图层开工（D21 定案 = 方案 A）

**已完成并验证**（`cargo check -p rds-insight --all-targets` 零告警；`cargo test -p rds-insight --lib` **113 项全绿**，基线 91 → +22）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| D21 | 视图归属定案为**方案 A**（视图随 crate）；`insight` 依赖 gpui-kit（测试启用 `test-support`） | `crates/insight/Cargo.toml` |
| 1.2 | 视图模型：`PanelTab`（五 Tab × 2 字）/ `InsightTarget`（四类目标 + 默认 Tab）/ `InsightPanelState`（四态）/ `ColumnProfileView`（四区内容 + 类型分派 + 阈值与文案，**纯函数可单测**） | `crates/insight/src/model.rs` |
| 1.3 | 面板骨架：面板头（⚙ / ⟳）+ 目标头（列名 + 类型徽标 + 空值率）+ Tab 条（`TabBar::underline`）+ 四态（空 / 骨架 / 错误 + 重试 / 数据） | `crates/insight/src/insight_view.rs` |
| 1.4 | 列画像四区 + 类型分派（Numeric / Text / DateTime / Boolean / Unknown）用 `Accordion` 折叠；折叠偏好跨「重算」保留 | 同上 |
| — | 面板与宿主解耦：点 ⚙ / ⟳ 只发 `InsightEvent`，面板不自己取数、不开对话框（D20） | 同上 |
| — | M8 尺寸常量集中登记（其中 4 个与 workbench 外壳必须一致的值是镜像，已注明待上收） | `crates/insight/src/ui.rs` |
| 1.6 | 三个动作定义：`OpenInsight` / `InsightRefresh` / `ReloadInsightRules`（键位待入口批次注册） | `crates/insight/src/commands.rs` |

**顺带发现（已修）**：视图模型的数值分布一开始写成自相矛盾的空占位（数值列的直方图挂在 `ColumnInsightFull::histogram` 而非 `NumericStats` 上，漏接会让数值列**永远看不到分布**），已改为在分派时传入并加两条断言固定。

**未完成（下一批）**：1.1 `InsightService::profile_column` 编排、1.5 入口接线（结果表列头右键 / 导航树右键）、右 Dock 装配（`workbench/src/panels/` 去掉三行占位）、键位注册。三项落点都在 `workbench`，与并行会话正在改的 `panels/` / `view.rs` 相交，故本批不动。

### 2026-09-15 — Phase 0 第四批：0.2 边界归位（**Phase 0 全部完成**）

**已完成并验证**（`cargo check -p rds-insight -p rds-workbench --all-targets` 零告警；`rds-insight` **91 项测试全绿**）

| 原位置 | 新位置 | 说明 |
| --- | --- | --- |
| `engine/persistence/insight_types.rs` | `insight/src/model/types.rs` | 16 个领域类型；`model.rs` 作为模块根声明 `pub mod types;` |
| `engine/persistence/insight_store.rs`（562 行） | `insight/src/store/body.rs` | DuckDB 正文：列快照 + 表/Schema 报告（预留）+ `InsightStorage` 门面 |
| `engine/persistence/insight_meta_store.rs`（317 行） | `insight/src/store/meta.rs` | SQLite 元数据 + 版本链 |
| （原 `insight/src/store.rs`） | `insight/src/store/mod.rs` | 模块根：子模块声明 + 装配点 `ProjectInsightStores` |
| `engine/services/duckdb_service.rs::detect_extremes` | `insight/src/insight_engine.rs::detect_extremes` | 曾是**方向倒置**（低层持有上层的业务启发式与词汇） |
| `workbench/services/persistence_service.rs`（271 行） | `insight/src/service/persistence.rs` | 100% 洞察快照回写；`pub(crate)` → `pub` |
| `workbench/services/result_service.rs` 的洞察半边 | `insight/src/service/mod.rs`（`InsightService`） | 结果集半边留在 workbench |

**关键约束：必须原子搬迁**——`insight_store` / `insight_meta_store` 引用 `insight_types`，且 `detect_extremes` 返回其类型；若先搬类型会造成 `engine → insight` 环。因此**类型 + 两个仓库 + detect_extremes 在同一批内完成**。

**顺带收敛**：`persistence.rs` 里那份重复的 `sha256_hex`（第三份）改为复用 `store::snapshot_checksum`；`workbench` 的 `sha2` 依赖随之成为零使用并移除。engine 侧只剩连接与迁移（`ProjectDatabaseManager` 及其实体），**不再认识「洞察」**。

**未完成**：无。Phase 0 的 §2 / 0.3 / 0.4 / 0.5 / 0.6 / 0.7 / 0.8 / 0.2 均已落地。

### 2026-09-15 — Phase 0 第三批：目录监听热加载（0.6 完成）

**已完成**（`cargo check -p rds-insight --all-targets` 零告警；`rds-insight` **91 项测试全绿**，连续 5 轮无波动）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 0.6 | 规则目录监听：**内容哈希轮询**（非事件监听），默认 2s；变更时重扫三层并应用启停 | `insight/src/service/watcher.rs` |
| 0.6 | **跟随项目切换**：监听器不持固定目录，每轮读 `watched_project_root()` 现算；宿主调 `set_watched_project_root` 告知 | 同上 |
| 0.6 | 索引陈旧标记：监听**不碰数据库**（不把项目库访问拉进后台轮询线程），只置 `index_is_stale()`，由规则管理视图打开时消费 | 同上 |
| 0.6 | 接线：构造期启动监听 + 项目打开/切换时告知当前项目 | `workbench/src/view.rs`（`WorkbenchView::new`）、`workbench/src/components/project_host.rs`（`refresh_after_open`） |

**为何不用 `notify` / `ThemeRegistry::watch_dir`（决策 D22）**

| 方案 | 不用它的理由 |
| --- | --- |
| `notify` | 在本仓只是**传递依赖**（未在 `workspace.dependencies` 声明），直接使用属隐式依赖 |
| `gpui-kit::ThemeRegistry::watch_dir` | 需要 gpui 的 `Context`，而洞察 crate 在视图归属拍板前不依赖 gpui |
| **内容哈希轮询**（选用） | 零新依赖；**去抖天然成立**（不看事件只看内容哈希，编辑器保存/连写多次都只得到一个结论）；几十个小 TOML 的文件读取走页缓存，开销可忽略 |

**本轮发现并修复的竞态**

| # | 问题 | 修法 |
| --- | --- | --- |
| F7 | 基线指纹若在**线程内**首次取样，则 `spawn` 返回后、线程首次取样前的文件变化会被当成「本来就是那样」而**永久漏报** | 基线指纹在**起线程之前**建立并 move 进闭包；测试以 5s 超时暴露过该问题 |

> **验证受阻说明**：`crates/mock` 当前处于编译不过的状态（你正在写的 `mock_view.rs`），而 `workbench` 依赖 `mock`，因此本次 `view.rs` / `project_host.rs` 的接线**只做了人工核对（签名、作用域、字段类型）与编译器无关的验证**，尚未经过类型检查。详见下方「阻塞项」。

### 2026-09-15 — Phase 0 第二批：索引表 + 索引同步器 + 快照链路闭合

**已完成并验证**（`cargo check --workspace --all-targets` 零告警；`rds-insight` **85 项测试全绿**，基线 53 + 新增 32）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 0.5 | 索引表迁移：`insight_rule_index`（含 `enabled` / `checksum` / `load_status` / `load_error`），global 与 project 两库同构 | `engine/migrations/global/024_insight_rule_index.sql`、`project_meta/019_insight_rule_index.sql` |
| 0.5 | 索引扫描器 `scan_scope_dir`：读磁盘 → SHA256 + 试解析 → 索引行（**含解析失败的文件**，用文件名兜底 id 并记录错误原文） | `insight/src/service/indexer.rs` |
| 0.5 | 索引合并 `plan_index`（纯函数）：磁盘为准 + 启停以用户为准 + 内置抑制记录保留 + 文件删失转 `missing` | 同上 |
| 0.5 | 索引表读写 `RuleIndexStore`：`load` / `replace`（单事务整体替换）/ `set_enabled`（唯一用户写入口）/ `disabled_ids` | 同上 |
| 0.5 | 同步编排 `sync_project_rules`：扫描 → 合并 → 写库 → `apply_disabled_rules` → 注册表缓存失效 | 同上 |
| 0.5 | **禁用真正生效**：`apply_disabled_rules` 推入同步可读缓存，`build_registry` 在**覆盖之后**摘除被禁规则（与它来自哪一层无关），并清 `sources` 以免界面显示“存在但用不了”的规则 | `insight/src/lib.rs`、`rule_registry.rs`（`remove_rules`） |
| 0.5 | 规则解析提为自由函数 `parse_rule_toml`（注册表与索引器共用；索引器必须能解析“坏文件”） | `insight/src/rule_registry.rs` |
| 0.7 | 快照链路**装配点** `ProjectInsightStores`：`from_project_db`（复用已打开的项目库，推荐）/ `open`（只有项目根时的便捷入口）/ `save_column_snapshot`（正文 + 元数据**双写** + `parent_version_id` 版本链） | `insight/src/store.rs` |
| 0.7 | 校验和算法单一来源：`engine::persistence::insight_store::snapshot_checksum` 公开，避免第三份 `sha256_hex` | `engine/src/persistence/insight_store.rs` |
| 依赖 | `insight` 重新引入 `rusqlite`（索引存储真的用到了）与 `sha2`（规则文件哈希）——先前 §2-0.5 删掉是因为当时零使用点 | `insight/Cargo.toml` |

**本轮发现的实际缺陷（已修）**

| # | 发现 | 影响 | 处置 |
| --- | --- | --- | --- |
| F5 | `get_latest_meta` / `get_latest_snapshot` / 两处 `get_history` 均为 `ORDER BY created_at DESC`，而 `CURRENT_TIMESTAMP` **只有秒级精度**——同一秒内保存两次时「最新版本」返回任意一条 | 高：版本链会挂错父版本，历史对比比错对象 | ✅ 四处均加 `, rowid DESC` 作确定性兜底 |
| F6 | `get_history` 把 `created_at`（TIMESTAMP）直接读成 `String` → 行级读取报错，而错误被 `.filter_map(|r| r.ok())` **静默吞掉** → 历史列表**恒为空**（phase 5 的“历史版本”会是一个永远空的列表） | 高：功能看似实现、实际恒空 | ✅ SQL 侧 `CAST(created_at AS VARCHAR)`；两处 `filter_map(r.ok())` 改为**向上抛错**（静默少行比报错难查得多） |

**既有问题（非本轮引入，本轮未触碰）**：`crates/engine/src/sql/{split.rs,highlight.rs}` 共 3 个测试失败（未跟踪的新文件，属 editor Phase 0 在制品）；`ui_contract::view_layer_has_no_raw_size_literals` 失败于 `panels/` 裸 `px(`。

**未完成（Phase 0 最后一项）**：0.2 边界归位（4 类文件搬 crate）。

**阻塞项（非本模块）**：`crates/mock` 编译失败（`mock_view.rs`：`IconName` 未引入、`Locale` 缺 `PartialEq`、`Window::open_dialog/close_dialog` 不存在），而 `workbench` 依赖 `mock` → **任何涉及 workbench 的改动（包括 0.2 的一一）都无法做类型检查**。修复后方可恢复全量验证。

### 2026-09-15 — Phase 0 第一批：§2 缺陷修复 + 作用域模型 + 注册表按项目缓存

**已完成并验证**（`cargo check --workspace --all-targets` 零告警；`rds-insight` 71 项测试全绿，基线 53 + 新增 18）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| §2-0.1 | `by_category` 索引**删除**，`list_by_category` 改为从 `rules` 实时派生——消除覆盖时的「重复项」与「一规则现身两个分类」两类不一致 | `insight/src/rule_registry.rs` |
| §2-0.2 | `get_project_rules_dir` 改用 engine 权威常量 `connection_org_store::RS_META_DIR_NAME`，不再硬编码 `.RSmeta`；新增 `RULES_DIR_NAME` 与 `get_global_rules_dir` | 同上 |
| §2-0.3 | `get_or_create_duckdb` 注释改为真实语义（**单例**而非 round-robin 连接池，临时表进程内全局可见） | `insight/src/insight_engine.rs` |
| §2-0.4 | 并发受限文案改用户可读 `洞察分析任务过多，请稍候重试`（常量 `ERR_TOO_MANY_CONCURRENT`）；注释说明**为何刻意快速失败**（同步函数内无法 await，阻塞会卡住 UI 线程） | 同上 |
| §2-0.5 | 删除零使用依赖 `rusqlite` / `once_cell`（后者已被 `std::sync::OnceLock` 取代） | `insight/Cargo.toml` |
| 0.3 | 内部接缝开放：`get_column_stats_internal` / `get_column_sample_internal` / `get_column_histogram_internal` 提 `pub`；新增 `get_column_insight_full_on(registry, conn, ...)`（在调用方已持有的连接上计算，避免 `Mutex` 重复加锁与重复争抢并发额度） | `insight/src/insight_engine.rs` |
| 0.4 | `RuleScope`（`Builtin` / `Global` / `Project`）+ `RuleSource` + `RuleLoadFailure`；注册表记录每条规则的**实际来源**与**解析失败列表**；`load_builtin` / `load_from_dir(dir, scope)` | `insight/src/rule.rs`、`rule_registry.rs` |
| 0.4 | 注册表**按项目根缓存**：`registry_for` / `with_rules` / `reload_insight_rules` / `clear_registry_cache` / `builtin_registry`；移除进程单例 `GLOBAL_REGISTRY` 与 `load_user_rules` | `insight/src/lib.rs` |
| 0.4 | 规则集贯穿分析链路：`get_column_insight_full(_on)` / `get_column_insights` / `get_column_stats_internal` / `execute_insight_rule` / `list_insight_rules` / `list_rules_for_column` 均要求显式传入 `&RuleRegistry`（**基础统计本身由 TOML 规则驱动**，不能取进程级全局） | `insight/src/insight_engine.rs` |
| 0.4 | 门面适配：`ResultService` 相关方法加 `project_root`，统一走 `insight::with_rules`；整表评估改为**取一次规则集**后串行逐列（不再逐列加读锁），单列失败改为记日志而非静默跳过 | `workbench/src/services/{result_service,persistence_service}.rs` |
| 0.4 | 新增 16 号命令等价物：`ResultService::reload_insight_rules`（规则热加载的兜底入口） | `workbench/src/services/result_service.rs` |
| 0.8 | 三个空占位文件接入 `lib.rs` 并在文件头写清 Phase 1 规划内容（不再「文件在此但不参与编译」） | `insight/src/{commands,model,insight_view}.rs` + `lib.rs` |

**本轮发现的实际缺陷（已修 / 待决策）**

| # | 发现 | 影响 | 处置 |
| --- | --- | --- | --- |
| F1 | `insight-rules/table/table-quality-overview.rule.toml` 把 `result_type` 写在 `[meta]` 而非 `[query]`；`RuleMeta` 是 `deny_unknown_fields`，导致**该文件解析失败被静默跳过** → 文档与 `BUILTIN_RULE_COUNT` 都写 18 条，**实际只有 17 条生效**。v1 同病（文件字节一致） | 高：定量核对的口径错了 | ✅ 已修（并把 `builtin = true` 补齐）；新增 `test_builtin_count_matches_constant` 守住常量与实载数一致 |
| F2 | `crates/engine/insight-rules/` 是 `crates/insight/insight-rules/` 的**逐字节重复副本**（首提交带入），**全仓无任何代码引用**（engine 的 `include_dir!` 只指向自己的 `migrations/`） | 中：后来者可能改错副本 | ⬜ **待你确认删除**（未自行动手） |
| F3 | `table-quality-overview` 规则的 SQL 引用表 `insight_column_stats`，该表**在迁移与代码中都不存在**（只有 `insight_column_snapshots`）→ 即使解析修复，执行仍会报「表不存在」 | 中：该规则不可执行 | ⬜ **待决策**：建表 / 改 SQL / 下线该规则 |
| F4 | `ui_contract::view_layer_has_no_raw_size_literals` 失败：`panels/` 含裸 `px(` | — | ⬜ **非本轮引入**：HEAD 版本已有裸 `px(`（该测试在 HEAD 即失败），你未提交的改动又新增数处；本轮未触碰该文件 |

**未开始（Phase 0 剩余）**：0.2 边界归位（4 类文件搬 crate）、0.5 索引表与 `RuleIndexer`、0.6 目录监听热加载、0.7 快照持久化链路闭合。

### 2026-09-15 — 方案定稿（含规则作用域与索引表）

尚无代码改动。范围 = M8 全模块。本轮新增（相较 v1 行为蓝本）：

| 项 | 内容 |
| --- | --- |
| 规则作用域 | 由 v1 的**隐式两层**（内置 + 项目）升为**显式三层**（+ 用户全局），`RuleScope` 枚举化 |
| 规则管理 | 新增索引表 `insight_rule_index`（global / project 各一份）+ `RuleIndexer` 同步器：可枚举、可启停、校验错误可见、checksum 增量热加载 |
| 热加载 | 由「UI 按钮触发」改为「目录监听 + 按钮兜底」 |
| 边界归位 | `insight_types` / `insight_store` / `insight_meta_store` / `detect_extremes` 由 `engine` 迁入 `insight`（见 Phase 0） |
| 不迁移项 | 多列分析的 v1 实现（`availableColumns` 恒空，功能不可达）、`autoOpenVisualization` 死标志、`pendingVisualizationRequest` 半条链路（消费者 `DockviewLayout.vue` 在 v1 已不存在） |

## 1. 现状结论（盘点摘要）

### 1.1 后端（已迁移，质量高）

| 层 | 状态 |
| --- | --- |
| 规则资产（`crates/insight/insight-rules/`，16 条 TOML） | ✅ 与 v1 字节级一致；`include_dir!` 编译期嵌入。**注：其中 1 条因字段位置写错被静默跳过（见 §0 F1，已修）** |
| 规则引擎（`rule_types` / `rule_registry` / `rule_executor`） | ✅ 已迁移；`by_category` 覆盖缺陷**已修**（§0 0.1）；已升级为三层作用域 + 来源与失败记录（§0 0.4） |
| 分析服务（`insight_engine` / `quality_scorer` / `table_profile_service` / `schema_analyzer`） | ✅ 已迁移；53 单测与 v1 **逐个对齐** |
| 快照存储 | ✅ 已迁移；**但全仓无 `::new()` 构造点** → 链路悬空（**已于 0.7 闭合**） |
| 领域类型 | ✅ 已迁移（**归属已于 0.2 纠正**：现位于 `insight/src/model/types.rs`） |
| SQL 迁移（`002_insight_storage` / `008_insight_snapshots`） | ✅ 已随迁移目录迁入（`insight_table_reports` / `insight_schema_reports` 为预留） |
| 服务门面（`workbench/services/{result,persistence}_service.rs`） | ⚠️ 已迁移，**归属错位**（业务服务在壳层，待 0.2 处理）；16 个 v1 命令的对接口径**已全部备齐**（含本轮补上的 `reload_insight_rules`） |
| `get_schema_insight` 等价物 | ❌ 缺：`SchemaAnalyzer::analyze` 是唯一入口且全仓零调用（Phase 4） |
| `reload_insight_rules` 等价物 | ✅ 已补：`ResultService::reload_insight_rules`（兜底入口；常规热加载走 0.6 目录监听） |
| 用户规则加载 | ✅ 已通：`registry_for(project_root)` 首次访问时自动组装三层，不再依赖调用方手动 reload |

### 1.2 前端（V1 蓝本已盘点，v2 为零）

v1 有 7 个 Vue 组件（约 2188 行）+ `insight-store.ts`（607 行，23 个 action）。**v2 目前只有 `RightSidebarPanel::render_insight_placeholder` 的三行占位文字**。

关键：v1 蓝本需**分级对待**——并非全部照搬：

| v1 功能 | 是否真正可用 | v2 处置 |
| --- | --- | --- |
| `ColumnInsightsPanel`（轻量列统计） | ✅ 已挂载，可用 | **照搬语义** |
| `InsightStatsSection`（基础统计 / 分布 / 质量建议 / 样本） | ✅ 经上者间接挂载 | **照搬** |
| `QualityScoreCard`（四维评分） | ⚠️ 组件在，但 `qualityScore` 恒 null（`loadQualityScore` 无调用方） | **重接**（评分算法本身已在后端就绪） |
| `TableProfileView`（表探查） | ✅ 可用 | **照搬** |
| `SchemaInsightPanel`（Schema 健康报告） | ✅ 可用 | **照搬** |
| `InsightHistoryTab`（历史 + diff） | ⚠️ 可加载历史，但**无保存入口**（`saveCurrentInsight` 无调用方） | **重接**（先补保存） |
| `MultiColumnView`（多列分析） | ❌ **从未跑通**：`availableColumns` 从未赋值、`multiColumnRules` 恒空、`canExecute` 恒 false | **重新设计**（决策 #7） |
| `BottomInsightPanel`（底部容器） | ✅ 已挂载 | v2 改为右 Dock 单面板（见原型 §2） |
| `filterByValue` | ❌ 语义错误（把「值」当「列名」传） | **删除**，改「按值筛选结果集」另立（属 M5） |
| `autoOpenVisualization` / `pendingVisualizationRequest` | ❌ 死标志 + 半链路 | **不迁**（图表可视化属 M6/M5，洞察只出 `RenderHint`） |

### 1.3 关键缺口

① 边界归位（4 个文件搬 crate）；② 规则作用域与索引表；③ 用户规则加载与热加载接线；④ 快照存储构造点；⑤ 洞察视图（右 Dock）；⑥ 规则管理视图；⑦ 多列分析重新设计；⑧ Schema 洞察门面。
**可复用**：全部算法与规则资产开箱可用；v1 三个可用面板的布局语义可直接参照。

## 2. Phase 0 前置修复（实证缺陷，与功能解耦）

> **状态：五项均已在 Phase 0 第一批修复**（2026-09-15，见 §0）。下表保留为缺陷记述与修复口径。

这五项是**读源码得到的既有缺陷**，先修可避免后续在错误地基上叠加。

| # | 位置 | 缺陷 | 修法 |
| --- | --- | --- | --- |
| 0.1 | `insight/src/rule_registry.rs:72-77` | `by_category` **只增不减**：同名规则覆盖时（a）同分类 → 列表返回**重复项**；（b）换分类 → 规则**同时出现在两个分类**。`seen_ids` 只在单次扫描内去重，跨「内置 → 用户」无效 | 改 `by_category` 为**从 `rules` 实时派生**（消除整类不一致；18–30 条规模无性能问题）；或 `insert` 前按旧 category `retain` 掉本 id |
| 0.2 | `insight/src/rule_registry.rs:185` | `get_project_rules_dir` 硬编码 `.RSmeta`，未复用权威常量 | 改为 `project::store::RS_META_DIR_NAME`（或 `engine::persistence::connection_org_store::RS_META_DIR_NAME`，取现有单一来源） |
| 0.3 | `insight/src/insight_engine.rs:39-43` | 文档注释描述 **round-robin 连接池**，实现是 `DuckDBManager` **单例** `Arc<Mutex<Connection>>`（`duckdb/manager.rs:80-87`）——注释为 v1 连接池时代遗留、被 1:1 迁移带入 | 改写注释为真实语义（单例 + 全局串行化），删「跨连接临时表不可见」的误导段落 |
| 0.4 | `insight_engine.rs` 各公开函数的 `try_acquire()` | 并发上限 4 且**快速失败**；v1 的错误文案 `"Too many concurrent insight operations, please retry"` 面向开发者，已改为用户可读的 `ERR_TOO_MANY_CONCURRENT`（「洞察分析任务过多，请稍候重试」，已 `pub` 供面板原样展示）；保留 try 不改成排队，面板按「并发受限」设计加载态（见原型 §4） |
| 0.5 | `crates/insight/Cargo.toml` | `rusqlite`、`once_cell` **零使用**（`once_cell` 已被 `std::sync::OnceLock` 取代） | 删除两行依赖 |

## 3. 目标 crate 边界（**Phase 0 / 0.2 已完成**）

```
workbench ──► insight ──► engine ──► shared
   │              │           ▲
   └──────────────┴───────────┘
（workbench 只保留 SQL 结果集服务与监听接线；洞察服务/仓库/类型全归 insight）
```

| 原位置 | 实际迁移目标 | 理由 |
| --- | --- | --- |
| `engine/persistence/insight_types.rs` | `insight/src/model/types.rs` | 16 个领域类型（`ColumnInsightFull` / `QualityScore` / `TableProfile` / `TableQuality` …）是洞察领域词汇，不属数据层 |
| `engine/persistence/insight_store.rs` | `insight/src/store/body.rs` | 洞察快照正文的领域持久化；`ProjectDuckdbConnection` 仍来自 engine |
| `engine/persistence/insight_meta_store.rs` | `insight/src/store/meta.rs` | 同上（元数据 + 版本链） |
| `engine/services/duckdb_service.rs::detect_extremes` | `insight/src/insight_engine.rs` | 唯一调用方是 insight，且返回洞察类型（进度偏差） |
| `workbench/services/persistence_service.rs`（271 行） | `insight/src/service/persistence.rs` | 该文件 100% 是洞察快照回写，借用「工作台持久化」之名名不副实 |
| `workbench/services/result_service.rs` 的洞察转发 | `insight/src/service/mod.rs`（`InsightService`） | 结果集自身的方法（`re_execute_with_filter` / `execute_duckdb_analysis` / `save_cell_update` / `export_result` / 临时表）留在 workbench |

> 与初稿的差异：初稿拟将 `insight_store.rs` 拆为 `column_store` / `table_store` / `schema_store` 三个文件。实际**按原文件整体搬迁**（`body.rs`）——本轮目标是**边界归位**，拆分属内部重构，混在一起会让 diff 难审。

> **过渡期口径**：其余 Feature crate 的 `*_view.rs` 目前多为占位或已删除，视图大多暂收在 `workbench/panels/`。本模块的视图归属见 §3.1——**该决策尚未拍板**。

### 3.1 视图归属：两套相反的在用先例（**2026-09-16 定案：方案 A**）

| 先例 | 做法 | 证据 |
| --- | --- | --- |
| `project`（唯一入 crate 者） | **视图与 model / service 同 crate**：`crates/project/src/ui.rs` + `src/ui/`，crate **依赖 gpui-kit**，宿主依赖经 `ProjectUiHost` 注入（重绘桥 / 编辑区桥 / 排序持久化 / 打开后刷新）；workbench 侧只剩 `components/project_host.rs` 桥接 | `crates/project/README.md` §「视图与 model / service 同 crate」；`project-dev-plan.md` §0「A3」（2026-09-11） |
| `scratchpad`（刚完成反向清理） | **视图留在 workbench**：删除 `src/{commands,model,scratchpad_view}.rs`，crate **不依赖 gpui**，只保留 `models/state/store/trash` | `crates/scratchpad/README.md`："依赖方向：`scratchpad → shared`（视图层在 `workbench`，本 crate 不依赖 gpui）" |

依赖现状：**8 个 Feature crate 中仅 `project` 依赖 gpui-kit**（`scratchpad` / `mock` / `database` / `analytics_resource` / `connection` / `plugin` / `insight` 均不依赖）。`editor/README.md` §7 亦把「是否独立 crate / 视图归属」列为**待你拍板项**——即团队尚未形成统一口径。

| 方案 | 做法 | 优点 | 代价 |
| --- | --- | --- | --- |
| **A. 视图入 insight**（本方案默认） | `insight/src/insight_view.rs` 等自带视图；`insight` 的 `Cargo.toml` 加 `gpui-kit`；`workbench` 只留 `RightSidebarPanel` 装配 | 对齐 `overview.md` §「Feature 可以直接依赖 gpui-kit」与 GPUI-kit 官方指南；`project` 已跑通此路（含宿主桥 `ProjectUiHost` 的成熟范式）；面板状态与域模型同 crate，无需跨 crate 传视图模型 | `insight` 新增 gpui 依赖；需自建宿主桥（参照 `ProjectUiHost`）；与 7 个未迁移 crate 暂时分叉 |
| **B. 视图留 workbench** | 面板内容写在 `workbench/panels/`（如现状）；`insight` 保持纯后端（`insight → engine → shared`） | 与 `scratchpad` 刚确立的口径一致；crate 纯净、编译快 | `panels/` 继续膨胀（现已 8600+ 行）；面板状态与域模型跨 crate；与 `overview.md` 目标架构相背 |

**影响面**（拍板后需同步修改）：`crates/insight/Cargo.toml`（gpui-kit）、`crates/insight/src/lib.rs`（模块声明）、`crates/workbench/src/panels/`（`RightSidebarPanel` 是渲染内容还是仅装配）、`crates/workbench_shell/src/ui.rs`（尺寸常量放哪边）。

> 本文其余部分按 **方案 A** 书写（文件路径与落点均指向 `crates/insight/src/*_view.rs`）；若改选 B，只需把视图文件落点改为 `workbench/panels/`，其余设计（语义 / 布局 / 交互 / 作用域 / 索引表）不受影响。

## 4. 规则作用域与索引（本轮核心设计）

### 4.1 三层作用域

| 层 | 存储位置 | 作用域 | 可写 | 优先级 |
| --- | --- | --- | --- | --- |
| `Builtin` | `crates/insight/insight-rules/`（`include_dir!` 编译期嵌入，16 条） | 所有项目 | ❌ | 最低 |
| `Global` | `{data_dir}/RdataStation/system/insight-rules/` | 所有项目 | ✅ | 中 |
| `Project` | `{项目}/.RSmeta/insight-rules/` | 当前项目 | ✅ | 最高 |

- 加载顺序 `Builtin → Global → Project`，**同名 `meta.id` 后者整体覆盖前者**（沿用 v1 语义，不加前缀、不做字段级合并）。
- `Global` 层为 v2 新增：解决 v1「跨项目复用规则只能手动复制文件」的问题。
- 解析失败**不影响其他规则**：该条标记 `invalid` 并记录原文错误，其余照常加载（v1 已是此语义，见 `rule_registry.rs:80-82`）。

枚举化：

```rust
// crates/insight/src/rule.rs
pub enum RuleScope { Builtin, Global, Project }
```

### 4.2 索引表（正文不入库）

```sql
-- crates/engine/migrations/global/024_insight_rule_index.sql   （Global 层）
-- crates/engine/migrations/project_meta/019_insight_rule_index.sql（Project 层；同构）
CREATE TABLE IF NOT EXISTS insight_rule_index (
    rule_id      TEXT NOT NULL,               -- meta.id
    scope        TEXT NOT NULL CHECK (scope IN ('global','project')),
    category     TEXT NOT NULL,               -- column / multi / table / quality
    name         TEXT NOT NULL,
    version      TEXT NOT NULL,
    source_path  TEXT NOT NULL,               -- 相对项目根 / 相对系统目录，便于项目迁移
    checksum     TEXT NOT NULL,               -- 规则正文 SHA256（增量热加载判据）
    enabled      INTEGER NOT NULL DEFAULT 1,  -- ★ 可禁用；内置规则靠抑制记录
    load_status  TEXT NOT NULL CHECK (load_status IN ('ok','invalid','missing')),
    load_error   TEXT,                        -- TOML 解析错误原文（UI 直接展示）
    loaded_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (scope, rule_id)
);

CREATE INDEX IF NOT EXISTS idx_iri_category ON insight_rule_index(category);
CREATE INDEX IF NOT EXISTS idx_iri_status ON insight_rule_index(load_status);
```

设计要点：

1. **`enabled` 是这张表的最大价值**：内置规则不入库，但允许在**项目库**写一条 `rule_id` = 内置 id、`enabled = 0` 的**抑制记录**——用户终于能在 UI 里关掉一条内置规则，而 v1 只能伪造一个同名覆盖文件（且 `deny_unknown_fields` 要求逐字段照抄）。
2. **`checksum` 驱动增量热加载**：与 `store::body` 的快照 checksum 同一思路；文件未变则不重解析。
3. **`load_error` 让严格 schema 变得可用**：`RuleFile` 全套 `#[serde(deny_unknown_fields)]`（早失败优于静默错，保留），但失败必须**有出口**——入库后 UI 可直接显示「第 3 行多了个 `outputs` 字段」。
4. **物理落两库**：与 `id_prefix`（`G_`/`P_`/`GP_`）的双库约定一致；不采用 `analytics_resources` 那种「单表 + scope 判别列」的做法（那会让项目库里躺着全局数据）。`Builtin` 层不入库（编译期已知，由内存快照参与合并）。
5. **预留**：`insight_table_reports` / `insight_schema_reports` 两张表在 Phase 4 前保持空闲，代码与文档均需标注 `// 预留（Phase 4）`。

同步器（`insight/src/service/indexer.rs`）：

```
扫描三层目录 → 逐文件算 checksum → 与索引对比
  新增 / 变更 / 消失 三类差异
  → 重解析失败 → load_status='invalid' + load_error
  → 写回索引（Global 层写 global 库，Project 层写项目库）
  → 产出内存快照 → 装配 RuleRegistry
```

### 4.3 注册表生命周期（消除「调用方义务」）

v1/v2 现状：`GLOBAL_REGISTRY: OnceLock<RwLock<RuleRegistry>>` 是**进程单例**，而规则目录是**项目级**——「切项目要 reload」靠调用方手动维持（v1 在 `project_commands.rs` 两处手动调用，v2 直接漏了）。

v2 改为：

```rust
// 按项目根缓存，而非进程单例
pub fn registry_for(project_root: Option<&Path>) -> Arc<RwLock<RuleRegistry>>;
```

- 一实例一项目（`project_session.rs`）下并发复杂度极低；项目切换时旧条目自然淘汰。
- 热加载：`watch_dir` 监听 Project 层目录（`app/src/main.rs` 已有 `ThemeRegistry::watch_dir` 同款用法）；Global 层目录应用启动时监听一次。
- UI 保留一个「重新加载规则」动作作为兜底与排障入口（不再是唯一入口）。

## 5. 阶段划分

### Phase 0 — 地基（无 UI，可独立验收）

| # | 任务 | 落点 |
| --- | --- | --- |
| 0.1 | §2 的五项前置修复 | `insight/src/rule_registry.rs`、`insight_engine.rs`、`Cargo.toml` |
| 0.2 | 边界归位：4 类文件搬入 `insight`（§3 表） | `insight/src/{model/types.rs, store/, engine/stats.rs, service/}` |
| 0.3 | 开放内部接缝：`get_column_stats_internal` / `get_column_sample_internal` / `get_column_histogram_internal` 提 `pub`（现有 10 个单测已走这条路径，仅缺公开） | `insight/src/insight_engine.rs` |
| 0.4 | `RuleScope` 三层 + 用户全局目录 + `registry_for(project_root)` 缓存 | `insight/src/{rule.rs, lib.rs, rule_registry.rs}` |
| 0.5 | 索引表迁移（global 024 / project_meta 019）+ `RuleIndexer` 同步器 | `engine/migrations/`、`insight/src/service/indexer.rs` |
| 0.6 | 热加载：Project / Global 目录 `watch_dir` + 兜底 Action | `insight/src/service/watcher.rs`、`app/src/main.rs` |
| 0.7 | 快照持久化链路闭合：项目打开时装配 `InsightStorage` / `InsightMetaStore` | `insight/src/store/mod.rs`、`workbench/src/services/workspace_loader.rs` |
| 0.8 | 残留占位文件处置：`insight/src/{commands,model,rule,insight_view}.rs` 在 `lib.rs` 声明 `mod`（与 `database/src/lib.rs:12` 一致），避免「文件在此但不参与编译」 | `insight/src/lib.rs` |

**验收**：`cargo test -p rds-insight` 全绿且新增作用域 / 索引 / 同步器单测；`cargo check --workspace --all-targets` 零告警。

### Phase 1 — 列画像（最小可用洞察）

| # | 任务 | 落点 |
| --- | --- | --- |
| 1.1 | `InsightService::profile_column(temp_table, column)` 编排（含加载态与并发语义） | `insight/src/service/mod.rs` |
| 1.2 | 视图模型（`InsightPanelState` / `SelectedTarget` / `PanelTab`） | `insight/src/model.rs` |
| 1.3 | 右 Dock 洞察面板骨架：头部（目标名 + 类型徽标 + 动作）/ 四态（空 / 加载 / 错误 / 数据）/ Tab 条 | `insight/src/insight_view.rs` |
| 1.4 | 列画像四区：基础统计 / 数据分布 / 数据质量 / 样本数据（含类型分派：Numeric / Text / DateTime / Boolean / Unknown） | 同上 |
| 1.5 | 入口接线：结果表列头右键「洞察此列」；左侧导航树表右键「查看统计」 | `workbench/src/panels/`（只发命令） |
| 1.6 | Action 与快捷键：`OpenInsight`（已有 Quick Open 项）/ `InsightRefresh` | `insight/src/commands.rs`、`app/src/main.rs` |

**验收**：从结果表右键到列画像出数全链路可走；空态 / 加载 / 错误三态可复现。

### Phase 2 — 质量评分与规则管理

| # | 任务 | 落点 | 状态 |
| --- | --- | --- | --- |
| 2.1 | 质量评分卡：总分 + 等级取色（85 / 70 / 50 / 30 四档）+ 四维进度条（权重 .35 / .25 / .20 / .20） | `insight/src/{quality_scorer,model,insight_view,ui}.rs` | ✅ 第一批 |
| 2.2 | 表级质量聚合：「评估全表」→ `TableQuality` + 逐步进度（避免撞并发上限） | `insight/src/service/mod.rs`、`jobs.rs` | ✅ Phase 3 一批（入口在表探查上） |
| 2.3 | 规则管理视图：三层分组列表 + 启停开关 + 校验错误行 + 打开规则文件 | `insight/src/rule_view.rs`（新文件） | ✅ 第二批 |
| 2.4 | 用户全局规则目录的创建与管理（首次写入时建目录） | `insight/src/service/indexer.rs` | ✅ 第二批 |

**验收**：可禁用一条内置规则并验证其不再出现在适用规则列表；故意写坏一个 TOML 能看见错误原文。

### Phase 3 — 表探查与多列分析

| # | 任务 | 落点 | 状态 |
| --- | --- | --- | --- |
| 3.1 | 表探查视图：列元数据表（序号 / 列名 + PK 角标 / 类型 / 可空 / 质量分）+ 行数 + 评估入口 | `insight/src/insight_view.rs` | ✅ Phase 3 一批 |
| 3.2 | 多列分析**重新设计**：列清单来源改为「当前结果集 / DuckDB 临时表的真实列元数据」（v1 的 `availableColumns` 恒空是该项从未跑通的根因） | `insight/src/service/mod.rs` | ✅ Phase 3 二/三批 |
| 3.3 | 规则选择与结果渲染：单值 KV / `result_type = "list"` 表格 | `insight/src/insight_view.rs` | ✅ Phase 3 三批 |
| 3.4 | 表探查 → 列画像下钻（点列名） | 同上 | ✅ Phase 3 一批 |

### Phase 4 — Schema 洞察报告

| # | 任务 | 落点 |
| --- | --- | --- |
| 4.1 | `get_schema_insight` 门面（补上唯一缺失的命令等价物） | `insight/src/service/mod.rs` | ✅ Phase 4 一批 |
| 4.2 | 报告视图：健康评分条 + 四个折叠区（外键候选 / 类型不一致 / 孤立表 / 冗余列），置信度与严重度分级取色 | `insight/src/schema_view.rs`（新文件） | ✅ Phase 4 一批 |
| 4.3 | 导出 JSON / Markdown | 同上 | ✅ 一批（函数）+ **收尾（D60）**：面板侧编码 + 宿主选路径写文件 + 状态栏回执 |
| 4.4 | 下钻联动：类型不一致的受影响表 → 表探查 | 同上 | ✅ 一批（事件）+ **收尾（D60）**：宿主走源取样（不建临时表） |
| 4.5 | **入口**：导航树「结构洞察」→ `InsightTarget::Schema` | `database/src/nav_view.rs` + `workbench/src/components/nav_host.rs` | ⬜ **未接**（宿主侧无一处构造 `InsightTarget::Schema`，界面打不开「结构」Tab） |

### Phase 5 — 快照历史与版本对比

| # | 任务 | 落点 | 状态 |
| --- | --- | --- | --- |
| 5.1 | 保存快照入口（含 `entity_source`：conn / db / schema / table） | `insight/src/insight_view.rs` | ✅ 一批（`entity_source` 现写 `temp_table=…`：面板手里只有临时表，就如实写） |
| 5.2 | 历史列表（`created_at` + 类型 + 版本链）+ 版本详情 | 同上 | ✅ 二批（另加短版本号与分页提示） |
| 5.3 | 版本对比面板：差异字段与 `old → new (+Δ)` 摘要；颜色分增 / 减 / 不变**三态**（v1 定义了 `.val-same` 却从未使用，此处修正） | 同上 | ✅ 二批（另加「变了但算不出数值」一档，方向用箭头而不用颜色暗示好坏） |
| 5.4 | 存储用量（后端真实统计，**不用 v1 的 `history.length * 2` 前端估算**）+ 清理（默认 30 天，需确认） | 同上 | ✅ 用量（一批）· ✅ 清理（三批：确认框 + 回执 + 成对删；**天数固定 30 天，D56**） |

### Phase 6（候选，不在本期）

类型覆盖补齐（BLOB / 全 NULL 列 P0，JSON 列基础统计 P1，ARRAY 元素分布 P2）、异常检测（IQR + Z-Score）、时序分析、质量评分权重可配置、规则数量扩充（18 → 30+）、报告导出（PDF / HTML）、洞察结果缓存。均见 v1 `design.md` §十二 路线图，**v1 未实现，v2 不承诺**。

## 6. 测试场景清单

| # | 场景 | 层级 |
| --- | --- | --- |
| T1 | 三层作用域加载顺序与同名覆盖（Project 覆盖 Global 覆盖 Builtin） | 单测 |
| T2 | `by_category` 派生正确：同名同分类不重复、换分类不串（回归 §2-0.1） | 单测 |
| T3 | 索引同步器三类差异（新增 / 变更 / 消失）+ checksum 未变不重解析 | 单测 |
| T4 | 非法 TOML：`load_status = invalid` + `load_error` 落库，且**不影响其他规则** | 单测 |
| T5 | 禁用内置规则：抑制记录生效，`rules_for_column_type` 不再返回该 id | 单测 |
| T6 | `registry_for(project_root)` 缓存与项目切换（同根同实例、异根异实例） | 单测 |
| T7 | 列画像五类型分派（Numeric / Text / DateTime / Boolean / 全 NULL → Unknown） | 单测（现有 10 项扩充） |
| T8 | 质量评分四维权重与等级边界（85 / 70 / 50 / 30） | 单测（现有 7 项） |
| T9 | 快照保存 → 历史 → 版本详情 → 对比（版本链 `parent_version_id`） | 集成 |
| T10 | `SchemaAnalyzer` 健康分与四类检测（现有 16 项） | 单测 |
| T11 | 并发超限时的 UI 语义（排队或可读错误，不出现英文重试文案） | 集成 |
| T12 | 契约：洞察视图零裸色 / 零裸 `px(` | `cargo test -p rds-workbench --test ui_contract` |
| T13 | 空态 / 加载 / 错误三态渲染（无数据、非表临时表、连接断开） | 窗口测试（参照 `project/src/ui/tests.rs`） |
| T14 | 多列分析：列清单来自真实临时表、规则选择、结果渲染（单值 + list） | 集成 |

## 7. 风险与对策

| # | 风险 | 影响 | 对策 |
| --- | --- | --- | --- |
| R1 | 边界归位（4 类文件跨 crate 搬迁）触碰 `engine` 复用面 | 编译面广 | Phase 0 单独一轮、只搬不改逻辑；`insight_types` 已在 engine 内被 `duckdb_service` / `persistence/mod.rs` 引用，搬迁时同步改导出 |
| R2 | 索引表引入「文件与库不一致」的新失效模式 | 规则显示与实际加载不符 | `source_path` 缺失 → `missing` 状态显式展示；同步器为**唯一写入者**，UI 不直接改表 |
| R3 | 目录监听在 Windows 上的重复触发 / 抖动 | 频繁重载 | 按 checksum 去抖（变更判定不看事件、只看内容 hash） |
| R4 | 并发上限 4 导致「评估全表」批量失败 | 体验差 | 批量走串行 + 进度；`acquire()` 排队而非 try 失败（§2-0.4） |
| R5 | 大表探查耗时（`LIMIT 500` 采样）导致评分失真 | 误判质量 | 原型与 UI 明示「基于 N 行采样」；不做静默全表扫描 |
| R6 | v1 三个可用面板（TableProfile / SchemaInsight / InsightStats）体量不小 | 工期 | 分 Phase 落地，Phase 1 先出「列画像」主干，其余按序 |
| R7 | 视图归属已定案（方案 A）：与 7 个仍在 workbench 的 Feature 面板分叉 | 一致性 / 可维护性 | 已按 A 落地第一支面板作样板（`crates/insight/src/insight_view.rs`）；宿主桥在装配批次落地（参照 `ProjectUiHost`）。分叉以「面板随 crate」为长期口径逐步收敛 |

## 8. 实现位置映射（设计决策 → 代码文件）

| 设计决策 | 代码文件 |
| --- | --- |
| 三层作用域 `RuleScope` | `crates/insight/src/rule.rs` |
| 注册表按项目根缓存 | `crates/insight/src/lib.rs`（`registry_for`） |
| 规则索引表（global / project） | `crates/engine/migrations/global/024_*.sql`、`crates/engine/migrations/project_meta/019_*.sql` |
| 索引同步器（checksum 增量） | `crates/insight/src/service/indexer.rs` |
| 目录监听热加载 | `crates/insight/src/service/watcher.rs`、`crates/app/src/main.rs` |
| 规则管理视图（启停 / 校验错误） | `crates/insight/src/rule_view.rs` |
| 领域类型归位 | `crates/insight/src/model/types.rs`（自 `engine/persistence/insight_types.rs`） |
| 快照存储归位 | `crates/insight/src/store/{column_store.rs,table_store.rs,schema_store.rs,meta_store.rs}` |
| 异常值检测归位 | `crates/insight/src/engine/stats.rs`（自 `engine/services/duckdb_service.rs::detect_extremes`） |
| 洞察服务门面 | `crates/insight/src/service/mod.rs`（自 `workbench/services/{result,persistence}_service.rs` 的洞察部分） |
| 列画像 / 质量卡 / 表探查 | `crates/insight/src/insight_view.rs` |
| Schema 报告与导出 | `crates/insight/src/schema_view.rs` |
| Action 与快捷键 | `crates/insight/src/commands.rs`、`crates/app/src/main.rs` |
| 右 Dock 装配（仅协议） | `crates/workbench/src/{view.rs,panels/}`（`RightSidebarPanel`） |
| 尺寸常量 | `crates/workbench_shell/src/ui.rs`（新增「洞察（M8）专用尺寸」节） |
| 规则资产 | `crates/insight/insight-rules/`（16 条） |
| 协议契约测试 | `crates/workbench/tests/ui_contract.rs` |

## 9. 验证方式

```sh
# 模块回归（洞察 + 引擎持久化）
cargo test -p rds-insight -p rds-engine --lib -j 2

# 全工作区编译守卫
cargo check --workspace --all-targets -j 2

# 契约（零裸色 / 零裸 px）
cargo test -p rds-workbench --test ui_contract -j 2
```

> `cargo` 命令固定 `-j 2`：并发链接重型 crate 会 OOM（DuckDB 已改动态链接）（见 `project-dev-plan.md` §0 工程配置）。

真机回归矩阵：MySQL / PostgreSQL / SQLite / DuckDB × 列类型（数值 / 文本 / 日期 / 布尔 / 全 NULL）× 明暗主题。

## 10. 未接与预留项（收口清单 · 权威）

> **定位**：本表是「接口状态」的**唯一权威**——哪些东西写好了但**没有入口 / 没有调用者**、哪些是预留、每项的「接 / 删 / 等」建议与量级。架构 §11 的 K 表是**问题视角**（缺陷与取舍），本表是**施工视角**（接口与入口）；两表互相引用，不重复。
> **使用约定**：做完一项 → 在 §0 加一条进度记录 → **从本表删掉该行**（本表只列还没做的）；新增预留接口时同步加行，否则会出现「看起来有、实际没有」的错觉。
> **最近核对**：2026-09-18（逐项用 grep 核实调用者 / 构造点）。

| # | 项 | 现状证据 | 建议 | 量级 / 阻塞 |
| --- | --- | --- | --- | --- |
| 3 | 结果集临时表的**定向回收**（K16 收尾） | `drop_temp_table(TempTableSource::Query)` 零生产调用者；且建表侧（`create_duckdb_temp_table` / `ResultService` / `execute_duckdb_analysis`）**也零调用**——整条「结果集 → DuckDB 分析」链路未接 UI。**清场口已接**（2026-09-18：项目切换清 `tmp_q_*`） | **随 #6（编辑器结果集入口）一起接**：结果集被丢弃 / 替换 / 关文档三处按 D54 契约调 `drop_temp_table` | 小（但依赖 #6） |
| 4 | 表级 / Schema 报告快照（K6） | 两表零写入者；`save_table_quality` / `save_schema_insight` 零调用者 | **接**（需产品点头：表级快照的比对语义与列级不同——行数、列清单都在变） | 中（store 方法已有，缺面板保存入口 + 历史视图 + 对比） |
| 5 | 结构洞察入口（导航右键 → `InsightTarget::Schema`） | 宿主侧零构造（该目标只出现在 insight 内部与测试） | **接**：`NavHost` 加一个方法 + 导航菜单一项（照「查看统计」） | 小 |
| 6 | 编辑器结果集「洞察此列」+ 临时表直连入口 | `open_insight_column` 零调用者；`InsightTarget::Column\|Table` 无人构造 | **留着**（用户明确说不急）；做时照 `FilterValueHook` 注入，**不需要执行期物化**；顺手接 #3 的定向回收 | 中 |
| 7 | 分析表型存档的洞察（`kind = Analysis`） | `can_view_stats` 对该 kind 返回 false | **等**：M6 二期有产生者 + 要 ATTACH `analytics.duckdb` + 用 `definition_sql` 重建 | 中 |
| 8 | `RenderHint`（规则的渲染提示）零消费 | 只在 `lib.rs` re-export | **决定**：做图表契约（映射 `RenderHint` → 渲染方）或删；不做图表就删 | 小（删）/ 中（消费） |
| 9 | 源目标下「多列」Tab 在样本表解析前点击不发请求 | 已知小限制（样本表要等列 / 表目标先取过样） | **决定**：让它自己先取一次样，或维持并在 UI 提示 | 小 |
| 10 | 静态门（D52）是关键字黑名单 | 设计记录（见 `insight-extension-notes.md` §6.4） | **可选加强**：解析级策略检查（解析能力 `engine/src/sql` 已有） | 中 |
| 11 | `insight_view.rs` 体量（约 2500 行代码 + 900 行测试） | 新功能仍在往里加（导出按钮即在此） | **时机触发**：见 §11 规格 | 中 |

> **已移出本表**（完成后从施工单删行，记录见 §0）：#1 删 `crates/engine/insight-rules/` 重复副本（`e3684d67`）；#2 删源库内省路径（`table_profile_service.rs` + 门面，2026-09-18）。

**建议顺序**（性价比）：3（依赖 #6）→ 5（一个小入口开一个完整 Tab）→ 4（要产品点头）→ 其余。

## 11. `insight_view.rs` 按 Tab 位移（规格 · 待触发）

> **触发条件**：下一次要**较大地**动结构 Tab / 历史 Tab，或新增一个 Tab 时**顺手做**；不单独开批（纯位移没有产品收益，单独占一批只是多付一次验证成本）。

### 为什么

`insight_view.rs` 现约 2500 行代码（另有约 900 行测试），内含：状态宿主、头部、Tab 条、五路 Tab 渲染、评分卡与十余个片段函数——新功能仍在往里加，继续下去会成为第二个 5000+ 行视图文件。

### 目标结构（照 editor `view/results/` 那次位移）

```
crates/insight/src/
├── insight_view.rs   # 只留：InsightView 结构体与字段、new、set_target 与各状态回填、
│                     #      emit_request_for_tab / ensure_data_for_tab、Render 转发
└── view/
    ├── mod.rs        # 结构与台账（哪个函数在哪 / 为什么不拆 crate；照 editor results/mod.rs 的写法）
    ├── header.rs     # 头部（目标名 + 类型徐标 + ⚙ 规则管理 + ⟳ 重算）
    ├── column.rs     # 列画像四区 + 评分卡（render_score_card / dimension_row / ratio_bar）
    ├── table.rs      # 表探查 + 评估进度 + 列名下钻热点
    ├── multi.rs      # 多列表单 + 结果渲染 + 字段中文映射
    ├── schema.rs     # 结构报告（健康条 + 四区 + 下钻热点 + 导出菜单）
    └── history.rs    # 历史列表 + 对比面板 + 清理 + 存储用量
```

**命名取舍**：保留 `insight_view.rs` 作为状态宿主（而不是整体搬去 `view/host.rs`）——因为 `InsightView` / `InsightEvent` 是对宿主的 `pub` 面（`workbench` / `jobs` / 测试都在用），搬家会让 `lib.rs` 的 re-export 与引用面全动；本次位移的验收条件就是**引用面零改动**。

### 不做什么

- 不改语义、不改 UI、不加功能、不动 `ui.rs` 常量（纯位移）
- **不拆 crate**：状态（target / state / data / tab）就是一个面板实例的状态，拆出去要跳 crate 传两遍（与 editor 那次判定同口径）
- 测试可以整块移到 `view/tests.rs`，也可以先留在原处——一次只做一件事

### 验收（纯位移的判据）

- `cargo test -p rds-insight --lib` **225 项不变** · `--test column_profile_e2e` **13 项不变**
- `cargo check --workspace --all-targets` 零告警（含 `rds-workbench`）
- `cargo test -p rds-workbench --test ui_contract` **7 项不变**
- `grep -rn "insight_view::" crates/` 的宿主引用**零改动**；`lib.rs` 的 re-export 不变
- `view/mod.rs` 写清结构与台账（不留“这个函数为什么在这里”的疑问）
