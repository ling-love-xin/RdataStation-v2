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

### 2026-09-16 — Phase 1 第五批：取数作业随特性 crate 归位

**已完成并验证**（`cargo test -p rds-insight --lib` **124 项** + 集成 4 项全绿；零告警）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 归位 | `services/insight_jobs.rs`（workbench）→ `insight/src/jobs.rs`：符合面板耦合治理计划 §5 的目标形态「特性 crate 内的 jobs + 面板 drain」。Phase 2 的「评估全表 + 进度」与 Phase 5 的快照保存都要复用同一套后台形态，先摆对位置就不用写两遍 | `crates/insight/src/jobs.rs`（新） |
| 宿主编排面 | 只剩一行：`insight::jobs::attach(&panel, cx, 项目根提供者闭包)`——事件接缝与取数都在特性 crate 内；项目根用**闭包**而非值（项目可能已切换，且解析结果必须在提交任务前脱离 `Shared`） | `crates/workbench/src/panels/right.rs` |
| 契约测试 | `attach` 的宿主契约在本 crate 内验住：订阅被触发 → 提供者被问一次 → 后台取数真的跑起来并回填错误态（不是只验「不 panic」） | `jobs.rs` 测试 |

**测试坑（已写进注释）**：宿主实体必须被持有——只留 `Subscription` 不足以让订阅者存活，实体一旦释放，订阅就被剪掉。

**验证边界**：workbench 侧当前因在途重构（`Shared` 字段搬迁、`NavDropMode` 等符号调整）整体编译不过，因此本步的可验证面收在特性 crate 内（`jobs.rs` 自己验了宿主那一行的契约）。


### 2026-09-16 — Phase 1 第五批：取数链搬进 crate（耦合治理）

**动机**：`layout/panels-coupling-plan.md` §5 定的目标形态是「**特性 crate 内的 jobs** + 面板 drain」——Phase 2 的「评估全表 + 进度」与 Phase 5 的快照保存要用同一套后台形态，先把位置摆对，省得写两遍。

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 搬迁 | 原 `crates/workbench/src/services/insight_jobs.rs` 整体搬入 insight crate（`ProfileRequest` / `handle_event` / `request_profile` 行为一字未改，只把 `insight::` 前缀改成 `crate::`）；workbench 侧删文件、`services/mod.rs` 去掉声明与文档行 | `crates/insight/src/jobs.rs` |
| 宿主胶水 | 新增 `jobs::attach(&panel, cx, 项目根闭包)`：**事件形状不再外泄**，宿主只回答「项目根在哪」；闭包在提交时才解析（项目可能已切换），`Subscription` 由本模块返回 | 同上 |
| 不变式 | 项目根在**提交前**解析成 `PathBuf`（所有权数据）才进后台任务——`Shared` 的 `Rc<RefCell<…>>` 不出线程；弱句柄升级失败即丢结果（面板已关） | 同上 |
| 测试 | `ProfileRequest` 两项纯函数单测随代码搬入（`rds-insight` lib **121 → 123 项**）；workbench `tests/insight_entry.rs` 改 import 后仍走真实接线 | `crates/insight/src/jobs.rs`、`crates/workbench/tests/insight_entry.rs` |

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
| 规则资产（`crates/insight/insight-rules/`，18 条 TOML） | ✅ 与 v1 字节级一致；`include_dir!` 编译期嵌入。**注：其中 1 条因字段位置写错被静默跳过（见 §0 F1，已修）** |
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
**可复用**：全部算法与 18 条规则资产开箱可用；v1 三个可用面板的布局语义可直接参照。

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
| `Builtin` | `crates/insight/insight-rules/`（`include_dir!` 编译期嵌入，18 条） | 所有项目 | ❌ | 最低 |
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

| # | 任务 | 落点 |
| --- | --- | --- |
| 2.1 | 质量评分卡：总分 + 等级取色（85 / 70 / 50 / 30 四档）+ 四维进度条（权重 .35 / .25 / .20 / .20） | `insight/src/insight_view.rs` |
| 2.2 | 表级质量聚合：「评估全表」→ `TableQuality` + 逐步进度（避免撞并发上限） | `insight/src/service/mod.rs` |
| 2.3 | 规则管理视图：三层分组列表 + 启停开关 + 校验错误行 + 打开规则文件 | `insight/src/rule_view.rs`（新文件） |
| 2.4 | 用户全局规则目录的创建与管理（首次写入时建目录） | `insight/src/service/indexer.rs` |

**验收**：可禁用一条内置规则并验证其不再出现在适用规则列表；故意写坏一个 TOML 能看见错误原文。

### Phase 3 — 表探查与多列分析

| # | 任务 | 落点 |
| --- | --- | --- |
| 3.1 | 表探查视图：列元数据表（序号 / 列名 + PK 角标 / 类型 / 可空 / 质量分）+ 行数 + 评估入口 | `insight/src/insight_view.rs` |
| 3.2 | 多列分析**重新设计**：列清单来源改为「当前结果集 / DuckDB 临时表的真实列元数据」（v1 的 `availableColumns` 恒空是该项从未跑通的根因） | `insight/src/service/mod.rs` |
| 3.3 | 规则选择与结果渲染：单值 KV / `result_type = "list"` 表格 | `insight/src/insight_view.rs` |
| 3.4 | 表探查 → 列画像下钻（点列名） | 同上 |

### Phase 4 — Schema 洞察报告

| # | 任务 | 落点 |
| --- | --- | --- |
| 4.1 | `get_schema_insight` 门面（补上唯一缺失的命令等价物） | `insight/src/service/mod.rs` |
| 4.2 | 报告视图：健康评分条 + 四个折叠区（外键候选 / 类型不一致 / 孤立表 / 冗余列），置信度与严重度分级取色 | `insight/src/schema_view.rs`（新文件） |
| 4.3 | 导出 JSON / Markdown | 同上 |
| 4.4 | 下钻联动：类型不一致的受影响表 → 表探查 | 同上 |

### Phase 5 — 快照历史与版本对比

| # | 任务 | 落点 |
| --- | --- | --- |
| 5.1 | 保存快照入口（含 `entity_source`：conn / db / schema / table） | `insight/src/insight_view.rs` |
| 5.2 | 历史列表（`created_at` + 类型 + 版本链）+ 版本详情 | 同上 |
| 5.3 | 版本对比面板：差异字段与 `old → new (+Δ)` 摘要；颜色分增 / 减 / 不变**三态**（v1 定义了 `.val-same` 却从未使用，此处修正） | 同上 |
| 5.4 | 存储用量（后端真实统计，**不用 v1 的 `history.length * 2` 前端估算**）+ 清理（默认 30 天，需确认） | 同上 |

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
| 规则资产 | `crates/insight/insight-rules/`（18 条，不改） |
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

> `cargo` 命令固定 `-j 2`：DuckDB 静态库并发链接会 OOM（见 `project-dev-plan.md` §0 工程配置）。

真机回归矩阵：MySQL / PostgreSQL / SQLite / DuckDB × 列类型（数值 / 文本 / 日期 / 布尔 / 全 NULL）× 明暗主题。
