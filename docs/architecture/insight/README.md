# 洞察模块（M8）· 模块入口

> **一句话**：把数据变成**结论**——「这份数据长什么样」（库 / 表 / 列画像）与「它能不能用」（四维质量评分），并把「新增一种洞察」从改 Rust 降级为**加一个 TOML 规则文件**（内置 16 条，用户可扩展）。
>
> 本文只提炼**特点 / 边界 / 代码地图 / 硬约束**；细节一律指向本目录内文档，**不复制设计**。
> 状态：**Phase 5 完成 + 规则侧收口 + 临时表一致化（K16 收口）+ 规则安全边界（Q1 全落地）+ 快照收尾（Q4/Q5/K14 定案）+ 入口统一（D58 源取样通道）+ 四个入口全部接线（D59/D60）+ 结构洞察改走驱动元数据（D62，SQLite 可用）**（2026-09-18）——Phase 0–5 全部完成；内置规则 **16 条**；**临时表** D50 · D51 · D54；**规则边界** D52 + D53；**快照** D55 · D56 · D57；**入口** D58；**结构洞察取数** D62；**边界口径**见架构 §1.1（导航树所见 + 草稿箱/分析存档两块文件）。测试 **226 项 + 集成 14 项**全绿，另有**真机四库**两个用例（`insight_schema_real` / `insight_source_real`）。下一步：**编辑器结果集入口**（顺带 K16 的定向回收），见 `insight-dev-plan.md` §10。
>
> **边界**：本模块拥有**画像 / 评分 / 规则 / 报告 / 快照历史**。SQL 执行与结果集属 M5 编辑器；对象树与元数据内省属 M4；连接与运行态属 M3；Mock 属 M7；资源目录属 M6。洞察**不自己取数**——数据来自 M5 建立的 DuckDB 临时表或 M3 的连接，只经服务/命令与它们协作。

## 1. 模块特点

### 产品行为

| 特点 | 含义 | 出处 |
| --- | --- | --- |
| **一次分析 = 一个目标 + 一份结论** | 不存在「全局洞察」；面板头部永显当前目标，杜绝「这数是哪来的」 | 原型 §1.1 |
| **分析粒度随目标分派** | 列 / 表 / 多列 / Schema 共用**一个面板**，按目标类型分派渲染，**不做四套面板** | 原型 §3 |
| **结论可追溯到算法** | 每个数字来自统计量 / 评分维度 / 规则之一；UI **不发明结论** | 原型 §1.1 |
| **采样必须明示** | 表探查与批量评估基于 `LIMIT 500` 采样，UI 常驻「基于 N 行采样」 | 原型 §3.2、§4 |
| **面板收敛为一处** | v1 的「右栏轻量统计 + 底栏四 Tab」两处合并为**单个右 Dock 面板**（17.5rem） | 原型 §1.3 |
| **凡 DuckDB 能分析的资源都能洞察** | 入口四个（导航树 / 分析存档 / 草稿箱 / 编辑器结果集），**含 CSV / Parquet / Excel / JSON 这类文件**（靠 DuckDB 扩展直接读）；数据怎么进 DuckDB 由来源定，分析能力只有一套（已接前三个） | 架构 D58/D59 |

### 分析与规则

| 特点 | 含义 | 出处 |
| --- | --- | --- |
| **规则是唯一对外扩展面** | 「新增一种洞察」= 加一个 `.toml`（SQL 模板 + 输出映射 + 质量门控 + 渲染提示），不改 Rust | 原型 §5 |
| **规则三层作用域** | `Builtin`（内嵌 16 条）→ `Global`（跨项目）→ `Project`（`.RSmeta/insight-rules/`，进 git），同名**整体覆盖** | 原型 §1.2、开发方案 §4.1 |
| **内置规则可禁用** | 靠索引表抑制记录实现，**不改文件**（v1 只能伪造同名覆盖文件，且 `deny_unknown_fields` 要求逐字段照抄） | 开发方案 §4.2 |
| **规则正文不入库，只入索引** | 文件是唯一真相源（可 diff / git / 手改）；库只存 scope / checksum / enabled / 校验状态 | 开发方案 §4.2 |
| **校验失败不连坐** | 单条 TOML 非法只标红该条并记录原文，其余规则照常可用 | 原型 §1.2、§5 |
| **质量评分四维加权** | 完整性 .35 / 唯一性 .25 / 类型一致 .20 / 分布 .20；等级阈值 85 / 70 / 50 / 30 | 原型 §2、§6.2 |

### 架构与约束

| 特点 | 含义 | 出处 |
| --- | --- | --- |
| **视图归属 = 方案 A**（2026-09-16 定案） | 视图与本 crate 的 model / service 同 crate，依赖 `gpui-kit`（架构约束允许并鼓励）；`workbench` 侧只装配与订阅事件；面板自己不做 I/O | 开发方案 §3.1、架构 D21/K8 |
| **依赖只向下** | `workbench → insight → engine → shared`；洞察不依赖任何业务 Feature crate | 开发方案 §3 |
| **不自己取数** | 数据入口只有两种：**已有临时表**（结果集 / 分析表，直接分析）与**源取样** `SampleSource`（洞察侧自己包 `LIMIT 500` 落 `tmp_i_`，D58/D59）；不建连接、不执行用户 SQL、**不反拼装**数据（D42） | 原型 §1.1、架构 D58/D60 |
| **快照双写** | 正文进项目 DuckDB（`insight_column_snapshots`），元数据 + 版本链进项目 SQLite（`insight_snapshots`） | 开发方案 §1.1 |
| **零裸值** | 颜色取主题 token（缺失角色补 `product-tokens.json`），尺寸进 `ui.rs`（新增「洞察（M8）专用尺寸」节） | 原型 §6 / §7 |
| **组件不手搓** | 折叠区 / 表格 / Tab 条 / 对话框 / 开关 / 菜单一律用 gpui-kit 组件 | 原型 §8 |

### 工程与文档

| 特点 | 含义 | 出处 |
| --- | --- | --- |
| **算法层冻结移植** | 规则资产自 v1 迁入（现 16 条：2026-09-17 下线两条跑不通的残留规则，见架构 K3/K15）（其中 1 条曾因字段位置写错被静默跳过，已修，见开发方案 §0 F1）；迁移基线 53 个单测与 v1 逐个对齐，Phase 2 后共 **130 项** | 开发方案 §1.1 / §0 |
| **先修地基再叠功能** | Phase 0 处理 5 项实证缺陷（`by_category` 覆盖 / 常量复用 / 误导注释 / 并发语义 / 未用依赖）+ 4 类文件边界归位 | 开发方案 §2、§3 |
| **v1 蓝本分级对待** | 照搬（可用）/ 重接（组件在数据空）/ 重设计（从未跑通）/ 不迁（死链路），**逐项有账** | 原型 §10 |
| **文档先于实现** | 本轮出模块入口 + 原型设计 + 交互稿 + 开发方案；实现按 Phase 0–5 推进 | `insight-dev-plan.md` §5 |

## 2. 边界

- **做**：库 / 表 / 列画像、四维质量评分（列 + 表聚合）、规则引擎与规则管理（三层作用域 / 启停 / 校验可见 / 热加载）、多列分析、Schema 洞察报告与导出、洞察快照历史与版本对比。
- **不做**：SQL 执行与结果集（M5）、连接与只读策略（M3）、对象树与内省（M4）、Mock 数据生成（M7）、分析资源目录（M6）、图表可视化（洞察只出 `RenderHint`，渲染归 M5/M6）、跨库对比 / AI 建议 / 报告调度（v1 路线图 P2/P3，v2 不承诺）。

## 3. 代码地图（括号内为现状）

| 想改 | 去哪 |
| --- | --- |
| 规则类型 / 作用域 | `crates/insight/src/rule_types.rs`、`rule.rs`（现状：✅ 三层作用域已落地） |
| 规则加载 / 覆盖语义 | `crates/insight/src/rule_registry.rs`（现状：✅ 已迁移，`by_category` 有覆盖缺陷，见开发方案 §2） |
| 规则执行（SQL 模板 → 结果） | `crates/insight/src/rule_executor.rs`（现状：✅ 已迁移） |
| 规则索引同步 / 目录监听 | `crates/insight/src/service/indexer.rs`、`service/watcher.rs`（现状：✅ 已实现；**解析失败的原文同时含语义校验**：`value_type` 白名单 + 门控字段存在，D48/D49） |
| 列画像计算（统计 / 样本 / 直方图） | `crates/insight/src/insight_engine.rs`（现状：✅ 已迁移；内部接缝 `*_internal` 待提 `pub`） |
| 质量评分（四维 + 等级） | `crates/insight/src/quality_scorer.rs`（现状：✅ 已迁移） |
| 规则管理对话框（三层分组 / 启停 / 错误原文 / 新建） | `crates/insight/src/rule_view.rs`（现状：✅ Phase 2 / 2.3，入口为面板头 ⚙） |
| 异常值检测 | `crates/insight/src/insight_engine.rs`（`detect_extremes`；现状：✅ 已归位，自 `engine/services/duckdb_service.rs`） |
| 表探查 | `crates/insight/src/insight_engine.rs`（`get_temp_table_profile`：DuckDB 临时表内省 `DESCRIBE`，面板走这条）（现状：✅ Phase 3 一批；源库内省的 `table_profile_service.rs` 已于 2026-09-18 **删除**——零调用且未在真机跑通，源库表先取样再探查，D58/D59） |
| 结构洞察（外键推断 / 类型不一致 / 孤立表 / 冗余列 / 健康分） | `crates/insight/src/schema_analyzer.rs`（分析器；元数据走**驱动接口**，无方言 SQL，D62）+ `schema_view.rs`（视图模型与导出）（现状：✅ Phase 4 一批；导出与下钻已接（D60）；✅ **入口已接**（2026-09-18）：导航树「结构洞察」→ `SchemaRef`；✅ **取数改走驱动元数据**（D62）：SQLite 也因此可用；真机四库验证） |
| 领域类型（16 个 `pub struct/enum`） | `crates/insight/src/model/types.rs`（现状：✅ 已归位） |
| 快照存储（列 / 表 / Schema 三类 + 元数据） | `crates/insight/src/store/{mod.rs, body.rs, meta.rs}`（现状：✅ 已归位；**同一进程对同一项目库不得重叠 open**，见架构 D41；按天数清理已可用） |
| 服务门面（画像 / 评分 / 规则 / 快照编排） | `crates/insight/src/service/{mod.rs, persistence.rs}`（现状：✅ 已归位）；结果集半边留在 `crates/workbench/src/services/result_service.rs` |
| 源取样通道（入口统一契约） | `crates/insight/src/model.rs`（`SampleSource::{new, on_duckdb, duckdb_file}` / `InsightTarget::{SourceColumn, SourceTable}`）、`service/persistence.rs`（`sample_source_to_analysis_table` 按 `conn_id` 分流：源库连接走引擎打型，`None` 走内存库 `CREATE TABLE … AS`） |
| 文件类数据源的读取器口径 | `crates/engine/src/dbi/engine/duckdb_engine.rs`（`file_reader_function`：CSV / Parquet / Excel / JSON；`load_file_source` 与洞察**共用**它）、`crates/engine/src/duckdb/analysis.rs`（`create_analysis_temp_table_as`） |
| 入口接线（三个右键「查看统计」） | 导航树 `crates/database/src/nav_view.rs`；分析存档 `crates/analytics_resource/src/resource_view.rs`（`can_view_stats` + `ResourcesHost::request_view_stats`）；草稿箱 `crates/scratchpad/src/{host,scratchpad_view}.rs`（`can_view_stats` / `view_stats` 走端口）；宿主实现 `workbench/src/components/{nav_host,resource_host,scratchpad_host}.rs` 与 `panels/shared.rs`（`insight_sample_sql` + `open_insight_source_*`） |
| 洞察面板（五 Tab） | `crates/insight/src/insight_view.rs`（现状：✅ 五 Tab 全部落地——列画像 + 质量卡 · 表探查 + 评估全表 · 多列分析 · Schema 报告 · 快照历史与版本对比；不显示假数据） |
| 规则管理对话框 | `crates/insight/src/rule_view.rs`（现状：✅ Phase 2 二批） |
| Schema 报告与导出 | `crates/insight/src/schema_view.rs`（现状：✅ Phase 4 一批 + **导出落地**（D60）：面板「导出 ▾」→ JSON / Markdown → 宿主选路径写文件） |
| 视图模型 | `crates/insight/src/model.rs`（现状：✅ 已落地 `PanelTab` / `InsightTarget` / `InsightPanelState` / `PanelData` / `ColumnProfileView` / `TableProfileView` / `MultiColumnView` / `HistoryView`；阈值与文案是纯函数） |
| M8 尺寸常量 | `crates/insight/src/ui.rs`（现状：✅ M8 专用值在本文件；面板头 / 行高 / 内距等与外壳必须一致的值**重导出** `workbench_shell::ui`，不镜像） |
| Action 与快捷键 | `crates/insight/src/commands.rs`（现状：✅ 动作已定义，键位待入口批次）+ `crates/app/src/main.rs` |
| 右 Dock 装配（仅协议） | `crates/workbench/src/panels/`（`RightSidebarPanel`）、`view.rs`（`RightPanel::Insight`） |
| 规则监听启动 / 项目切换跟随 | `crates/workbench/src/view.rs`（`WorkbenchView::new`）、`components/project_host.rs`（`refresh_after_open`） |
| 规则资产（16 条 TOML） | `crates/insight/insight-rules/` |
| 规则索引表迁移 | `crates/engine/migrations/global/024_*.sql`、`project_meta/019_*.sql`（现状：✅ 已落地） |
| 快照表迁移 | `crates/engine/migrations/project_analysis/002_insight_storage.sql`、`project_meta/008_insight_snapshots.sql`（✅ 已迁入） |
| 尺寸常量 | `crates/workbench_shell/src/ui.rs` |
| 契约测试范围 | `crates/workbench/tests/ui_contract.rs` |

数据链路：`insight（画像 / 评分 / 规则编排）→ engine::DuckDbService（临时表）或 engine::SqlService（源库采样）→ 目标库`；快照：`insight → InsightColumnStore（项目 DuckDB）+ InsightMetaStore（项目 SQLite）`。**无 HTTP / IPC 层。**

## 4. 改这个模块前必须遵守

1. **不自己取数**：入口只给 `SampleSource`（或指已有临时表），「取样 → 落 `tmp_i_`」统一由 `service/persistence.rs` 做；不建连接、不执行用户 SQL、**不反拼装**数据（D42）。
2. **规则只有三个写入者**：文件（正文）、索引同步器（索引）、用户启停动作（`enabled`）。**UI 不直接改索引表、代码不硬编码规则**。
3. **规则正文永不入库**：库表只承载索引与状态；正文以文件为唯一真相源。
4. **同名整体覆盖**：不做字段级合并，不引入前缀命名空间（沿用 v1 语义）。
5. **单条规则失败不连坐**：解析失败只标记该条，不得中断整批加载。
6. **采样必须明示**：任何基于采样出的结论，UI 必须标出采样行数。
7. **render 零 I/O**：计算一律后台任务 + 结果回填（沿用 M4 `nav_jobs` 模式）。
8. **稳定标识**：`rule_id` / `snapshot_id` / `version_id` 永不变，不做下标键。
9. **零裸值**：颜色走主题 token（缺角色先补产品语义 token），尺寸进 `ui.rs`（裸 `px(` 会被契约测试拦下）。
10. **组件不手搓**：折叠 / 表格 / Tab / 对话框 / 开关 / 菜单用 gpui-kit 组件。
11. 注释与文档用简体中文，说明意图与取舍（不复述代码）。
12. `cargo` 命令固定 `-j 2`（并发链接重型 crate 会 OOM（DuckDB 已改动态链接））。
13. **分析中间产物一律走 `engine::duckdb::analysis`**：不要自己 `CREATE TABLE` 建样本表——名字不在 `tmp_i_` 前缀下，TTL / 上限 / 按来源清理就都看不见它（K16），并且默认用 `with_analysis_temp_table` 用完即收。
14. **项目层规则的装配必须过信任门**（D53）：改 `build_registry` 的项目层时，未信任就不装配、只记 `pending_project`；信任记录只能进全局库，**不得**写项目路径下的任何文件。

## 5. 测试与验证

```sh
# 模块回归（洞察 + 引擎持久化）
cargo test -p rds-insight -p rds-engine --lib -j 2

# 契约（零裸色 / 零裸 px）
cargo test -p rds-workbench --test ui_contract -j 2

# 全工作区编译守卫
cargo check --workspace --all-targets -j 2
```

**真机（四库 + 扩展源）**：用例都按环境变量自跳过（未设不算失败）；**sh / bash 下一律加单引号**
（`D:\…` 的反斜杠会被吃掉 → 驱动在工作目录建空库 → 假通过）。变量名与 `editor_exec_real.rs` 同一套：
`RDS_TEST_{MYSQL_URL, PG_URL, SQLITE_PATH, DUCKDB_PATH}`，另加 `RDS_TEST_ORACLE_URL`。

```sh
# 结构洞察：information_schema 方言 + 报告（真库建 rds_probe_schema_*，跑完 DROP）
cargo test -p rds-workbench --test insight_schema_real -j 2 -- --nocapture --test-threads=1

# 源取样 → 列画像 / 表探查（真库建 rds_probe_source_*，跑完 DROP）
# 同一文件里还有扩展源那条腿（Oracle via community 扩展 oracle_scanner，未设则跳过）
cargo test -p rds-workbench --test insight_source_real -j 2 -- --nocapture --test-threads=1
```

- 真机回归矩阵：MySQL / PostgreSQL / SQLite / DuckDB（+ 扩展源 Oracle）× 列类型（数值 / 文本 / 日期 / 布尔 / 全 NULL）× 明暗主题。
- 逐阶段验收场景见 `insight-dev-plan.md` §6（T1–T14）。
- **基线**：`cargo test -p rds-insight` 当前 **226 项**全绿（迁移基线 53：`rule_executor` 13 / `schema_analyzer` 16 / `insight_engine` 10 / `quality_scorer` 7 / `rule_registry` 7；Phase 0 新增 38；Phase 1 两批新增 29；Phase 2 两批新增 23；Phase 3 三批新增 31；Phase 4 一批新增 10；Phase 5 三批新增 15；规则校验补强新增 3；规则 SQL 静态门新增 3；项目规则信任门新增 11；快照收尾新增 2；源取样入口新增 3；文件类数据源新增 1；Schema 导出与下钻新增 3；**取数回归 1；**结构洞察改走驱动元数据（方言 6 项 → 新 3 项）****），另有**集成测试 14 项**（`cargo test -p rds-insight --test column_profile_e2e`：真实 DuckDB 临时表 → 规则统计 / 表探查 / 评估全表 / 多列规则 / **快照历史 · 版本对比 · 清理（真项目目录）** / **与内置 `SUMMARIZE` 交叉校验** → 视图模型），**新增功能不得减少**。临时表一致化（D50/D51/D54）与文件类数据源（D59）的 engine 支撑在 `duckdb::analysis` / `duckdb::manager` / `duckdb::temp_table` / `duckdb_service` 四处（`cargo test -p rds-engine --lib -- duckdb::analysis duckdb::manager duckdb::temp_table duckdb_service`），其中 `duckdb::analysis` 现 **8 项**（含 CTAS 2 项）。引擎单测总量当前 **428 项**（`cargo test -p rds-engine --lib`）。

## 6. 文档地图

| 文档 | 什么时候读它 |
| --- | --- |
| `insight-prototype-design.md` | **长什么样 / 怎么交互**：核心语义与规则作用域 / 右 Dock 面板布局 / 四种目标视图分派 / 状态与空态矩阵 / 规则管理对话框 / 主题映射与尺寸常量 / GPUI 落点 / **§10 与 V1 的逐项对照** |
| `insight-architecture.md` | **为什么这样设计 / 怎么运转**：概念模型与**九条不变式** / 分层与 crate 归属 / 状态所有权（单一写入者）/ 六条数据流 / **D1–D62 决策表** / 并发与资源 / 降级矩阵 / 测试策略 / 实现位置映射 / **§11 已知问题 K1–K19（K1 / K14 / K16 / K17 已处置；K18 定案保留；K19 待查）** / §12 待确认 Q1–Q7（**Q1 / Q3 / Q4 / Q5 / Q6 / Q7 已定案**） |
| `insight-dev-plan.md` | **做什么、做到哪**：已确认决策 9 项 / **§0 进度记录** / 现状盘点 / **§2 五项实证缺陷** / 目标 crate 边界 / **§4 规则作用域与索引表设计** / Phase 0–5 任务 / 测试场景 T1–T14 / 风险 R1–R7 / 实现位置映射 / 验证命令 / **§10 未接与预留项（收口清单 · 权威）** / **§11 `insight_view.rs` 按 Tab 位移规格（待触发）** |
| `insight-extension-notes.md` | **实现手段与第三方扩展的调研记录**（讨论稿）：现状约束（外部编译库 / 单例 / 已有扩展机制）/ 边界（三层实现 · 扩展可换实现不可换语义 · 准入四件套）/ 候选扩展逐项评估（`dq` / `stats_duck` / `datasketches` / `stochastic` + 顺带发现）/ 可参考的扩展设计（GE / Deequ / dbt / Soda / gatekeeper …）/ **§6 可采取之处（不引扩展也能拿的 10 条）** / 探针口径 |
| `insight-user-guide.md` | **怎么用**：入口 / 界面导览与怎么看数字 / 典型流程 / **§4 规则编写指南（对外契约：三层作用域 · 字段全表 · `value_type` 表 · 质量门控语义 · 可照抄示例 · 安全边界）** / **§4.8 内置规则 16 条一览** / FAQ 排查 / USIT 验收清单 |
| `insight-prototype.html` | 可交互示意稿（明暗双主题；列画像 / 质量卡 / 表探查 / 规则管理含禁用与校验失败态） |
| `../overview.md` | M8 定位与 crate 依赖方向（§九大模块与 crate 对应） |
| `../connection/connection-dialog-architecture.md` | 作用域与快照（`G_`/`P_`/`GP_`）的既有范式参照 |
| `../ui/ui-design-spec.md` | 三层约束（主题 token / 尺寸常量 / 组件规格） |

> 五件套已齐（模块入口 / 原型设计 / 交互稿 / 架构与设计理念 / 开发方案 / 使用手册）；另有 **`insight-extension-notes.md`**（实现手段与第三方扩展的调研记录，契约部分是架构 D61）。

## 7. 下一步

| 类别 | 项 |
| --- | --- |
| 已拍板（不阻塞） | 规则随项目走 · 三层作用域 · 正文不入库只入索引（开发方案 已确认决策 1–3） |
| ✅ Phase 0 已落地 | 五项缺陷修复 · 内部接缝开放 · `RuleScope` + `registry_for` · 索引表与同步器 · **启停生效** · 快照链路闭合 · 占位文件接入 `lib.rs` · **目录监听热加载** · 四类文件边界归位（逐项见开发方案 §0） |
| ✅ Phase 1 已落地 | 右 Dock 面板装配 · 列画像四区 · 入口命令 `open_insight_column`（`Ctrl+Shift+R`）· 后台取数 `insight::jobs::attach`（六批，逐项见开发方案 §0） |
| ✅ Phase 2 已落地 | 列级质量评分卡 · 规则管理对话框（三层分组 / 启停 / 校验错误行 / 新建规则）· K7 全局规则目录 · 表级评估全表 + 进度（逐项见开发方案 §0） |
| Phase 3（已完成） | 表探查视图 + 列名下钻 · 多列分析（真实列清单 + 规则执行 + 结果渲染）· ✅ 宿主侧入口（导航右键「查看统计」，D59 补齐） |
| Phase 4（已完成） | ✅ 门面 · 报告视图 · 导出函数 · 下钻事件 · ✅ **导出与下钻的宿主接线**（导出：面板算内容 + 宿主选路径写文件 + 状态栏回执；下钻：源取样通道，不建临时表。D60） |
| Phase 5（已完成） | ✅ 快照历史（保存入口 · 版本列表 · 存储用量）· ✅ 版本对比（方向固定为「选中 → 最新」）· ✅ 存储清理（确认框 · 成对删 · 回执）· ✅ 保留天数定案固定 30 天（D56） |
| 入口（D58/D59） | ✅ 源取样通道 · ✅ 导航树 / 分析存档 / 草稿箱三个右键「查看统计」· ✅ **文件类数据源**（CSV / Parquet / Excel / JSON，含 excel 扩展）；⬜ 编辑器结果集列头「洞察此列」（用户明确不着急）· ⬜ 分析表型存档（本体是 `analytics.duckdb` 库文件，要 ATTACH + 重建定义） |
| 待确认 | 规则安全边界若**再严一档**：`insight_rule_trust` 加规则集内容指纹（现绑定项目路径，见 D53 取舍）· 快照双写若要做故障注入测试（现只测补偿函数契约，见 D55） |
| **收口清单（施工）** | **未接 / 预留项的逐项状态、接或删建议与量级 → 开发方案 §10**（唯一权威；含 K2 重复目录待删 · K6 表级快照待接 · K16 结果集定向回收待接 · 编辑器结果集入口待接；~~结构洞察入口~~ 与 ~~`RenderHint`~~ 已于 2026-09-18 结案） |
| **实现手段** | **三层边界与扩展准入 = D61**：规则 → 内置 SQL → 扩展；扩展可换实现不可换语义、产物不许成为长期格式；调研记录与**可采取之处（10 条）** → `insight-extension-notes.md` |
