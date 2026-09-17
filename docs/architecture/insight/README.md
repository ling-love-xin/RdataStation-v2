# 洞察模块（M8）· 模块入口

> **一句话**：把数据变成**结论**——「这份数据长什么样」（库 / 表 / 列画像）与「它能不能用」（四维质量评分），并把「新增一种洞察」从改 Rust 降级为**加一个 TOML 规则文件**（内置 16 条，用户可扩展）。
>
> 本文只提炼**特点 / 边界 / 代码地图 / 硬约束**；细节一律指向本目录内文档，**不复制设计**。
> 状态：**Phase 5 完成 + 规则侧收口 + 临时表一致化（K16 ①~④）**（2026-09-17）——Phase 0–5 全部完成（快照历史 · 版本对比 · 存储清理）；Q6/Q7 落地（解析期校验，K11/K12 结案）；Q2/Q3/K15 处置（删 engine 死副本 + 下线两条跑不通的内置规则，现 **16 条**）；**洞察中间表改走 `engine::duckdb::analysis`**（`tmp_i_` 前缀 · 建表即登记 · 用完即删 · 惰性清理真正 DROP）+ **内存闸与可观测**（D51：`memory_limit` 2GB / 溢写钉 `<RDS_HOME>/tmp` / 登记概览）。测试 **202 项 + 集成 13 项**全绿（engine 侧另增 `duckdb::{analysis, manager, temp_table}` 用例）。下一步：**宿主侧欠账**（表入口 `Shared::open_insight_table` + 导航右键「查看统计」、Schema 导出按钮与下钻）与待拍板项（**Q1 规则 SQL 安全边界**（已核查：DuckDB 侧限制不可行，推荐静态门 + 项目规则信任门）· Q4/Q5 保留期与双写补偿 · **K16 余项：结果集侧 `tmp_q_` 回收时机**）。逐项进度见 `insight-dev-plan.md` §0。
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
| **不自己取数** | 数据入口是 `temp_table`（DuckDB 临时表）或 `conn_id`；洞察不建连接、不执行用户 SQL | 原型 §1.1 |
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
| 表探查 | `crates/insight/src/table_profile_service.rs`（源库内省）、`insight_engine.rs`（临时表内省 `get_temp_table_profile`）（现状：✅ Phase 3 一批） |
| 结构洞察（外键推断 / 类型不一致 / 孤立表 / 冗余列 / 健康分） | `crates/insight/src/schema_analyzer.rs`（分析器）+ `schema_view.rs`（视图模型与导出）（现状：✅ Phase 4 一批；导出与下钻的宿主侧待接） |
| 领域类型（16 个 `pub struct/enum`） | `crates/insight/src/model/types.rs`（现状：✅ 已归位） |
| 快照存储（列 / 表 / Schema 三类 + 元数据） | `crates/insight/src/store/{mod.rs, body.rs, meta.rs}`（现状：✅ 已归位；**同一进程对同一项目库不得重叠 open**，见架构 D41；按天数清理已可用） |
| 服务门面（画像 / 评分 / 规则 / 快照编排） | `crates/insight/src/service/{mod.rs, persistence.rs}`（现状：✅ 已归位）；结果集半边留在 `crates/workbench/src/services/result_service.rs` |
| 洞察面板（五 Tab） | `crates/insight/src/insight_view.rs`（现状：✅ 五 Tab 全部落地——列画像 + 质量卡 · 表探查 + 评估全表 · 多列分析 · Schema 报告 · 快照历史与版本对比；不显示假数据） |
| 规则管理对话框 | `crates/insight/src/rule_view.rs`（现状：✅ Phase 2 二批） |
| Schema 报告与导出 | `crates/insight/src/schema_view.rs`（现状：✅ Phase 4 一批；导出函数已就绪，导出的宿主按钮待接） |
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

1. **不自己取数**：数据入口只有 `temp_table` 与 `conn_id` 两种；不建连接、不执行用户 SQL。
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

## 5. 测试与验证

```sh
# 模块回归（洞察 + 引擎持久化）
cargo test -p rds-insight -p rds-engine --lib -j 2

# 契约（零裸色 / 零裸 px）
cargo test -p rds-workbench --test ui_contract -j 2

# 全工作区编译守卫
cargo check --workspace --all-targets -j 2
```

- 真机回归矩阵：MySQL / PostgreSQL / SQLite / DuckDB × 列类型（数值 / 文本 / 日期 / 布尔 / 全 NULL）× 明暗主题。
- 逐阶段验收场景见 `insight-dev-plan.md` §6（T1–T14）。
- **基线**：`cargo test -p rds-insight` 当前 **202 项**全绿（迁移基线 53：`rule_executor` 13 / `schema_analyzer` 16 / `insight_engine` 10 / `quality_scorer` 7 / `rule_registry` 7；Phase 0 新增 38；Phase 1 两批新增 29；Phase 2 两批新增 23；Phase 3 三批新增 31；Phase 4 一批新增 10；Phase 5 三批新增 15；规则校验补强新增 3（白名单 1 + 门控字段 1 + K11 回归 1）），另有**集成测试 13 项**（`cargo test -p rds-insight --test column_profile_e2e`：真实 DuckDB 临时表 → 规则统计 / 表探查 / 评估全表 / 多列规则 / **快照历史 · 版本对比 · 清理（真项目目录）** → 视图模型），**新增功能不得减少**。临时表一致化（D50）的 engine 支撑另有 6 项：`cargo test -p rds-engine --lib duckdb::analysis`。

## 6. 文档地图

| 文档 | 什么时候读它 |
| --- | --- |
| `insight-prototype-design.md` | **长什么样 / 怎么交互**：核心语义与规则作用域 / 右 Dock 面板布局 / 四种目标视图分派 / 状态与空态矩阵 / 规则管理对话框 / 主题映射与尺寸常量 / GPUI 落点 / **§10 与 V1 的逐项对照** |
| `insight-architecture.md` | **为什么这样设计 / 怎么运转**：概念模型与八条不变式 / 分层与 crate 归属 / 状态所有权（单一写入者）/ 六条数据流 / **D1–D51 决策表** / 并发与资源 / 降级矩阵 / 测试策略 / 实现位置映射 / **§11 已知问题 K1–K16（权威：含无 SQL 沙箱 · 清理剪断版本链 · 临时表回收（K16：洞察侧已修（D50/D51），结果集侧未接））** / §12 待确认 Q1–Q7（Q3 / Q6 / Q7 已定） |
| `insight-dev-plan.md` | **做什么、做到哪**：已确认决策 9 项 / **§0 进度记录** / 现状盘点 / **§2 五项实证缺陷** / 目标 crate 边界 / **§4 规则作用域与索引表设计** / Phase 0–5 任务 / 测试场景 T1–T14 / 风险 R1–R7 / 实现位置映射 / 验证命令 |
| `insight-user-guide.md` | **怎么用**：入口 / 界面导览与怎么看数字 / 典型流程 / **§4 规则编写指南（对外契约：三层作用域 · 字段全表 · `value_type` 表 · 质量门控语义 · 可照抄示例 · 安全边界）** / **§4.8 内置规则 16 条一览** / FAQ 排查 / USIT 验收清单 |
| `insight-prototype.html` | 可交互示意稿（明暗双主题；列画像 / 质量卡 / 表探查 / 规则管理含禁用与校验失败态） |
| `../overview.md` | M8 定位与 crate 依赖方向（§九大模块与 crate 对应） |
| `../connection/connection-dialog-architecture.md` | 作用域与快照（`G_`/`P_`/`GP_`）的既有范式参照 |
| `../ui/ui-design-spec.md` | 三层约束（主题 token / 尺寸常量 / 组件规格） |

> 五件套已齐（模块入口 / 原型设计 / 交互稿 / 架构与设计理念 / 开发方案 / 使用手册）。

## 7. 下一步

| 类别 | 项 |
| --- | --- |
| 已拍板（不阻塞） | 规则随项目走 · 三层作用域 · 正文不入库只入索引（开发方案 已确认决策 1–3） |
| ✅ Phase 0 已落地 | 五项缺陷修复 · 内部接缝开放 · `RuleScope` + `registry_for` · 索引表与同步器 · **启停生效** · 快照链路闭合 · 占位文件接入 `lib.rs` · **目录监听热加载** · 四类文件边界归位（逐项见开发方案 §0） |
| ✅ Phase 1 已落地 | 右 Dock 面板装配 · 列画像四区 · 入口命令 `open_insight_column`（`Ctrl+Shift+R`）· 后台取数 `insight::jobs::attach`（六批，逐项见开发方案 §0） |
| ✅ Phase 2 已落地 | 列级质量评分卡 · 规则管理对话框（三层分组 / 启停 / 校验错误行 / 新建规则）· K7 全局规则目录 · 表级评估全表 + 进度（逐项见开发方案 §0） |
| Phase 3（已落地） | 表探查视图 + 列名下钻 · 多列分析（真实列清单 + 规则执行 + 结果渲染）；⬜ 宿主侧入口（`Shared::open_insight_table` 与导航右键「查看统计」） |
| Phase 4（进行中） | ✅ 门面 · 报告视图 · 导出函数 · 下钻事件；⬜ 导出按钮与下钻的宿主侧接线（选路径 / 登记临时表） |
| Phase 5（已完成） | ✅ 快照历史（保存入口 · 版本列表 · 存储用量）· ✅ 版本对比（方向固定为「选中 → 最新」）· ✅ 存储清理（确认框 · 成对删 · 回执）；⬜ 保留天数取值待拍板（Q4/Q5） |
| 待确认 | **规则 SQL 安全边界（架构 §12 Q1）**——已核查：DuckDB 侧「禁用外部访问 / 只读连接」不可行（会砸掉产品自身的 ATTACH / read_csv / INSTALL，同一内存单例），推荐「静态门 + 项目规则信任门」待拍板 · 存储清理保留天数与双写补偿（Q4/Q5）· **K16 余项：结果集侧 `tmp_q_` 回收时机**（内存闸与可观测已落地，见 D51） |
