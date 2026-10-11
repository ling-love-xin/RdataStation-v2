# 编辑器三模式 · 实现任务书（通用 / SQL / 分析）

> 状态：**待评审**（用户拍板"先出实现任务书"，评审通过后按序开工）
> 依据：`editor-prototype-design.md`（修订 ①~⑯）+ 三模式高保真原型（通用 / SQL / 分析）
> 基线（代码现状，2026-10-11 核实）：
> - Phase 1a（内核 + 文本 + SQL 编辑体验）**已完成**；Phase 1b（SQL 执行闭环）**主体完成**（2026-09-21 USIT 第 1 轮收尾）
> - Phase 1c（分析模式骨架 Cell/Output/Session）**未开始**
> - 模式切换**已实现在工具栏最左**（`editor/src/view/host.rs` `render_toolbar` → `editor-mode-indicator` Button → `request_mode_switch`，与最新原型一致）
> - 结果表格已实现排序/筛选/冻结/右键菜单/复制/分段抓取/**滚动到底自动加载**；**值编辑（写回库）未做**（1b 余项）

---

## 0. 顺序与理由（TL;DR）

推荐执行顺序：**2a 共用表格编辑 → 2c UI 对齐 → 2b 通用模式 → 2d 分析骨架**。

| 期 | 名称 | 一句话 | 依赖 |
| --- | --- | --- | --- |
| 2a | 共用表格：单元格编辑 + 变更集 + 批量提交 | 你最拍板、最闭环的一块；表格是**共用件**（SQL 结果 + 分析内联输出） | 无（1b 表格已就绪） |
| 2c | UI 对齐最新原型 | 状态栏面包屑/方言/微标/重设 + 工具栏分组自定义；改动小、见效快 | 无 |
| 2b | 通用模式：更名 + 环境 + 脚本执行 | 全新能力（当前 Text 无脚本执行） | 状态栏微标部分依赖 2c |
| 2d | 分析模式骨架 1c | Cell/Output/Session + 内联输出 + `.rdsnote` | 内联表格依赖 2a；会话依赖 1b 执行链 |

理由：2a 是「其余全部的地基」（分析内联表格直接复用它的编辑/变更集/提交）；2c 把三模式共享的壳（模式切换、状态栏、工具栏）收口，2b/2d 落地时不会再动壳；2b 新能力与 2d 骨架都吃 2a 的输出形态，放后面。

---

## 1. Phase 2a — 共用结果表格：单元格编辑 + 变更集 + 批量提交

### 目标
结果网格从"只能看"变成"能改、能批量写回"（参考 DBeaver）：双击进编辑、脏单元格标色待提交、批量提交走**单事务**、失败/冲突有可读回执。表格为共用件，分析模式内联输出（2d）直接复用。

### 范围与不做
- **做**：编辑能力判据（纯函数）、单元格编辑交互、变更集模型、批量提交（单事务）、回执与冲突处理、只读四道防线联动、编辑入口 UI（禁用手搓，用组件现成）。
- **不做**：单元格下拉/日期等**列类型化编辑器**（无列类型，维持文本编辑）；行内新增/删除行（DBeaver 的 Insert Row / Delete Row 属二期，需先有行标识）；值预览弹层。

### 任务清单
- [ ] **T2a-1 编辑能力判据**（纯函数，新模块 `editability.rs`）：`editable(通道, 项目态, 文档只读, 连接只读, 行标识, 列) -> Reason`；无主键/复合键/只读 → 编辑入口不亮且给原因（原型 §2.4）。
- [ ] **T2a-2 行标识探测**：结果集来源 SQL 的表名（已有 `export.rs` 的表名猜测）→ 取主键/唯一键（经 `CompletionPort::catalog` 已有目录，或经连接端口补一个 `primary_key(conn, table)`）；探测不到 = 行标识 None → 整表只读。
- [ ] **T2a-3 变更集模型**：`ChangeSet { cells: Vec<CellChange{row, col, original, edited, status}> }`，随结果集切换清空；脏单元格标色（待提交态）；按视图行序映射回数据行序（复用 `view_rows`）。
- [ ] **T2a-4 单元格编辑交互**：双击进编辑（组件 Input / 编辑器内核复用）；Enter 提交、Esc 取消、失焦提交；只读单元格不响应。
- [ ] **T2a-5 批量提交**：`ExecTarget::Commit { changes, table, key }` + `QueryRunner::commit_changes`（默认实现如实不支持）；引擎/宿主生成 `UPDATE … WHERE key=…`（按行标识逐条，**单事务**包装）；影响行数 0 = 冲突（旧值已变），逐条报告。
- [ ] **T2a-6 回执与失败**：回执落状态栏（"已提交 N 条变更（M 条冲突）"）；失败逐条原因；不打断其余提交；**四道防线**（通道/项目/连接只读/无行标识）在提交前拒（沿用 `project_write_check` 同一处闸门）。
- [ ] **T2a-7 批量入口**：结果工具栏「提交变更」按钮（有变更集才亮）+ 变更计数（原型 SQL 模式）；参考 DBeaver 的提交/回滚对。

### 验收标准（测试断言式）
- 单测：编辑能力判据全矩阵（四种只读 × 有/无行标识）；行标识探测（有/无/复合键）；变更集 CRUD 与换结果清空；提交 SQL 生成（含主键 WHERE、列引用转义、单事务包装）；影响行数 0 → 冲突文案。
- 窗口用例：双击进编辑 → Enter 提交 → 变更为「待提交」→ 点提交变更 → 执行器真收到 Commit 目标；Esc 取消不留脏；只读档编辑入口不亮且状态栏说原因。
- 真机：MySQL / PG 各一例：改一格 → 批量提交 → 回读确认落库；并发改同一行（真机或探针）→ 冲突回执。
- 契约：`ui_contract` 全绿（新视图文件登记两份清单）；`check -p rds-editor -p rds-workbench --all-targets` 零告警。

### 涉及文件（现状基线）
`editor/src/{editability.rs(新), store.rs, execution.rs, export.rs(表名猜测复用), ui.rs, commands.rs}` + `editor/src/view/{results/grid.rs, host.rs, tests.rs}` + `workbench/src/services/{editor_exec.rs(commit 实现), editor_connections.rs(主键探测端口)}` + `engine/src/services/*`（写回语句执行）。

### 依赖与风险
- 依赖：1b 表格 delegate 已就绪（`render_td` 自绘可挂编辑、`view_rows` 视图行序）。
- 风险：主键探测缺数据源 → 先接 `CompletionPort::catalog`（已有 schema/表/列），探测不到如实降级只读，不编造行标识。

---

## 2. Phase 2b — 通用模式：更名 + 环境切换 + 外部解释器 + 脚本执行

### 目标
把 `Text` 档做成**通用模式**（无连接档、与 SQL 平行）：改名字面口径；新增**环境切换入口**（Python / R / Rust 外部解释器，可选环境）与**脚本执行**（运行 → 输出落**表格/结构化（复用结果集）**，内联输出归分析模式独有，不动 SQL 语义）。

### 范围与不做
- **做**：`EditorMode::Text → General` 更名与全仓口径同步；环境模型 + 环境目录端口 + 管理环境 modal；环境选择器 UI（缺解释器 → 运行不亮，原型通用模式交互）；`ExecTarget::Script` 执行（spawn 外部解释器、stdin 输入、stdout/stderr 收集、超时/中断、退出码）；状态栏左段（环境 / 文件类型 / LSP）。
- **不做**：**不接 Jupyter 协议**（二期再评 `jupyter-zmq-client` 等）；不做内联输出（分析模式 2d 才有）；不做 LSP 接入（仅状态栏指示）。

### 任务清单
- [ ] **T2b-1 更名与口径**：`model.rs` `EditorMode::Text → General`（含 `label()` = "通用"）；**删除"文本 ⊂ SQL ⊂ 分析 严格递进"注释**，改为"通用=无连接档（文件编辑 + 本地脚本执行），与 SQL 平行"；`mode.rs` / `host.rs` / 状态栏 / 用户可见文案同步；扩展名判定不变（md/log/json/csv → 通用；sql 系列 → SQL；rdsnote → 分析）。
- [ ] **T2b-2 环境模型**（新模块 `env.rs`）：`Environment{ id, name, kind(解释器类型: Python/RScript/Rustc/自定义), command, args, version }` + `EnvPort::list` / `EnvPort::installed(id) -> bool`（宿主注入，默认接系统探测：`python --version` 等）；未注入 = 空环境列表（如实，不假装有）。
- [ ] **T2b-3 环境选择器 UI**：工具栏 env-select（环境名 + 版本 ▾，原型通用模式）→ 菜单列环境 + 「管理环境…」（modal：环境配置表，增删改路径/参数）；选中的环境持久化到文档（与 connection 同类）。
- [ ] **T2b-4 脚本执行**：`ExecTarget::Script{ env, code }` + `QueryRunner::run_script`（默认实现如实不支持）；宿主实现：`spawn(env.command, args)` + stdin 写入代码（或临时文件，Windows 下避免命令行引号问题）+ stdout/stderr 收集 + 超时（可配）+ 中断（kill 进程树）+ 退出码。
- [ ] **T2b-5 输出落结果区**：成功 → 新结果（表格/结构化输出复用结果集表格；纯文本 → Text 输出卡片）；失败 → Error 卡片（退出码 + stderr 摘录）；运行中 → 状态栏「执行中 Ns…」；切环境 → 旧结果标 stale（复用通道切换的 stale 机制，原型通用模式交互）。
- [ ] **T2b-6 状态栏左段**：环境名 · 文件类型 · LSP（通用模式显示，SQL 模式显示连接面包屑——2c 统一壳，本段先做数据源）。

### 验收标准
- 单测：环境模型/持久化往返；`EditorMode::ALL` 顺序与 label（含"通用"）；`ExecTarget::Script` 的 `sql()`/`label()`；脚本执行意图原样到执行器。
- 窗口用例：环境菜单弹出 → 选环境 → 运行按钮真提交 Script 目标；选"缺失环境"→ 运行不亮且状态栏说原因；管理环境 modal 真打开。
- 真机：python 环境端到端（写临时 .py 文件执行，避免 `-c` 引号坑）：`print("hi")` → Text 输出卡片带 "hi"；`sys.exit(1)` → Error 卡片带退出码；超时触发中断。
- 契约：`ui_contract` 全绿；`check --all-targets` 零告警。

### 涉及文件
`editor/src/{env.rs(新), execution.rs, model.rs, mode.rs, shared.rs, lib.rs, view/host.rs, view/dialogs.rs(管理环境), view/widgets/status_bar.rs}` + `workbench/src/services/editor_env.rs(新)` + `app/src/main.rs`（若需命令注册）。

### 依赖与风险
- 风险：解释器探测（`python --version`）在用户机器可能未装 → **未探测到 = 运行不亮 + 原因**，不假装可用；超时中断要 kill 进程树（Windows `taskkill /T`），留探针验证。

---

## 3. Phase 2c — UI 对齐最新原型（壳收口）

### 目标
把三模式共享的外壳对齐到最新原型：状态栏（**通道面包屑 连接▸库▸Schema 可切换** + 方言 + 英文微标 + **行|列编辑切换** + 编码/换行/缩进重设）与 SQL 工具栏**分组自定义**（`···` 三栏面板：图标功能｜可见｜移动 + 竖线分隔符 + 折叠组=首组件+▾ + 重置默认）。

### 现状（已实现，不用重做）
- 模式切换：**已在工具栏最左**（`editor-mode-indicator`，✅ 与最新原型一致）
- 状态栏左右分段结构、连接/通道/TX 段：已有（`status_bar.rs` `StatusInputs`/`labels` 纯函数）
- 工具栏三层（执行级/文档级/通道级）：已有

### 任务清单
- [ ] **T2c-1 通道面包屑**：状态栏最左 `连接 ▸ 库 ▸ Schema`（可切换，同 DBeaver）；schema 列表来自 `CompletionPort::catalog`（已有 `all_schemas`）；切换 → 文档 Schema 绑定（随会话持久化，与 connection 同类）；SQL 模式显示、通用/分析模式不显示。
- [ ] **T2c-2 方言段**：从连接 `db_type` 解析（`editor_connections` 已有）→ 状态栏「方言 PostgreSQL」；未绑定连接不摆（真实值原则）。
- [ ] **T2c-3 英文微标 + 行|列编辑切换**：右段 `Ln · Col · Ch · UTF-8 · LF · Spaces · 大小 · 脏` + **行｜列**切换入口（切换编辑内容模式：列编辑 = 内核 `ColumnarPoint`/`add_cursor_above|below` 原生能力；入口为 PopupMenu/Button，禁手搓）。
- [ ] **T2c-4 编码/换行/缩进重设**（VSCode 式小菜单）：缩进重设 = 内核 `TabSize`（原生，直接可做）；**LF/CRLF 与编码重设内核不支持**（文本模型统一 `\n`）→ 菜单项置灰 + 标注"需外层转换层"（与原型一致，不假装能做）。
- [ ] **T2c-5 工具栏分组自定义**（SQL 模式）：最右 `···` → 三栏面板（图标功能｜可见｜移动）；组=分隔符界定的连续段（执行簇只是例子，**分组通用**）；组可折叠为「首组件 + ▾」、整体移动、插竖线分隔符拆分、重置默认；模式指示器为固定项不参与排序（原型 ⑯ 已定）。
- [ ] **T2c-6 面板持久化**：自定义布局（顺序/可见性/分组）随会话持久化；重置默认清除（恢复"执行 + 全部运行"两按钮布局）。

### 验收标准
- 单测：面包屑分段纯函数（连接/库/schema 缺项不占位）；方言解析（识别/未绑定）；微标格式化（Ln/Col/Ch/大小/脏）。
- 窗口用例：状态栏切 schema → 文档 Schema 绑定真变（真机：PG 切 schema 后执行落新 schema）；行|列切换入口真开列编辑（内核多光标）；重设菜单缩进真改内核 `TabSize`；工具栏面板折叠/移动/插分隔符/重置全链路（复用原型交互矩阵）。
- 契约：`ui_contract` 全绿（状态栏/工具栏改动登记）；`check --all-targets` 零告警。

### 涉及文件
`editor/src/{view/widgets/status_bar.rs, view/host.rs, ui.rs, session.rs(面包屑绑定持久化), lib.rs}` + `workbench/src/services/{editor_connections.rs(方言/库/schema), editor_schema.rs(新, schema 切换端口)}` + `app/src/main.rs`（行|列切换命令注册）。

### 依赖与风险
- 风险：schema 切换要真实影响执行（`USE schema` / 限定名）——**执行时按文档 Schema 绑定生成限定名或设会话默认 schema**，不做成纯 UI 装饰（真机验证）。
- 编码/换行重设明确**不做**（外层转换层另议），菜单置灰说清，避免"看着能点却没用"。

---

## 4. Phase 2d — 分析模式骨架 1c（Cell / Output / Session + 内联输出 + .rdsnote）

### 目标
交付「分析笔记骨架」（dev-plan Phase 1c）：单元序列（**仅 SQL 单元可执行**）+ 内联输出（Table 复用 2a 共用表格 / Text / Error，**内联输出为分析模式独有**）+ 会话（切换 → 全部输出 stale）+ `.rdsnote` 落盘 + 空态引导（原型 §4.4）。

### 现状（已有，不用重做）
- 模式判定/切换矩阵/确认文案/单元粒度：`mode.rs` 已实现且有测试（`sql_to_cells`/`cells_to_sql`/`cells_from_text`/`cells_to_text`、`CELL_SEPARATOR` 文本层）
- 执行链：1b 全齐（分段/筛选/排序/下发/分析 DuckDB/导出/事务/历史/血缘）
- 会话持久化基建：`workbench/src/services/editor_session.rs`（SavedSession：mode/connection/channel）

### 任务清单
- [ ] **T2d-1 单元模型**（新模块 `cell.rs`）：`Cell{ id, kind(Sql|Text), body, output: Option<Output>, state(Idle|Running|Done|Error|Stale) }`；`Output::Table(ResultEntry)` / `Text(String)` / `Error(reason)`；`EditorShared` 挂会话级单元序列。
- [ ] **T2d-2 会话模型**（新模块 `session.rs`）：`Session{ id, default_connection, env }`；会话切换 → `plan_session_switch`（已实现）确认 → 全部输出标 stale；会话列表 + 默认会话。
- [ ] **T2d-3 单元视图**（新 `view/analysis.rs`）：单元卡片（类型徽标：颜色=可执行性、形状=语言）+ ▶ 执行/中断 + ⋮ 菜单（上移/下移/复制/转文本/折叠/删除，全真实动作）+ 耗时/序号/stale 元信息；单元间 hover 插入点（＋代码/＋文本）+ 工具栏「＋插入单元」（原型分析模式）。
- [ ] **T2d-4 单元执行**：`ExecTarget::Cell{ session, cell }` + `QueryRunner::run_cell`（默认实现如实不支持）；宿主实现复用 1b 执行链（**仅 SQL 单元**：`execute_first_segment` 同款；Text 单元不可执行，▶ 不亮）；输出落 `Output`（表格 → 内联 Table，复用 2a；文本/错误 → 卡片）。
- [ ] **T2d-5 `.rdsnote` 落盘**：格式定 **JSON**（dev-plan 1c 备忘；文本层互转函数保留作导出实现）；`SavedSession` 加会话字段（mode 已有）；打开/保存往返。
- [ ] **T2d-6 空态与引导**：空笔记本 → 空态引导卡（§4.4）；Tab 切换/会话切换交互（stale 计数、脏标记，复用原型矩阵）。

### 验收标准
- 单测：单元 CRUD/状态机（Idle→Running→Done/Error/Stale）；会话切换 → 全单元 stale；`.rdsnote` JSON 往返（含单元/输出不跟随）；仅 SQL 单元可执行（Text 单元 ▶ 不亮且状态栏说原因）。
- 窗口用例：插入单元 → 运行 SQL 单元 → 内联 Table 真渲染（复用 2a 表格，可编辑）→ 切会话 → stale 计数 1→2；⋮ 菜单/折叠/上移下移/删除全真实动作；空态引导卡。
- 真机：SQL 单元端到端（PG/MySQL 各一：单元运行 → 内联表格带行数/耗时/血缘）；`.rdsnote` 保存 → 重开还原（含会话绑定）。
- 契约：`ui_contract` 全绿（新视图文件 `view/analysis.rs` 登记）；`check --all-targets` 零告警。

### 涉及文件
`editor/src/{cell.rs(新), session.rs(新), store.rs, execution.rs, shared.rs, lib.rs, view/analysis.rs(新), view/host.rs(模式接线), view/tests.rs}` + `workbench/src/services/{editor_exec.rs(run_cell), editor_session.rs(会话字段)}` + `app/src/main.rs`（如需命令）。

### 依赖与风险
- 依赖：2a 共用表格（内联 Table 编辑/提交直接复用）；1b 执行链。
- 风险：`.rdsnote` JSON 格式一旦落盘难改 → 先定 schema（版本字段），文本层互转保留；单元编辑器 = 内核实例复用（每个单元一个 Input/编辑器），注意 **ElementId 业务键**（单元 id，非下标）与焦点管理。

---

## 5. 跨期口径（每期都守）

1. **命名与注释**：`EditorMode::Text → General`（2b 期改）；全仓删除「严格递进」旧口径；注释与文档简体中文。
2. **零裸 hex / 零裸 px**：取色 `cx.theme().colors.*` + `rds-theme` 语义 token；尺寸 `crate::ui` 常量 + Tailwind 尺度（`rds-ui-spec` 三层约束）；新常量登记 `ui.rs`。
3. **组件选型禁手搓**：Button / Select / Combobox / Menu / TabBar / Accordion / table / List / StatusBar / Input 等 68 模块现成；菜单项抽纯函数；可点元素键盘可达（`track_focus` + `tab_stop`）。
4. **渲染路径不 I/O**：render 纯读；I/O 走事件路径 + 工作线程 + 回执队列（沿用 `ExecQueue`）；不做 `block_on` 阻塞 UI 线程。
5. **测试纪律**：`#[gpui_kit::test]` 禁通配导入；对话框断言「层真渲染 + 直接调提交函数」；真按键路径用 `simulate_event(KeyDown/KeyUp)` 各发一次。
6. **契约**：每期 `ui_contract`（新视图文件登记尺寸/颜色两份清单）+ `check -p rds-editor -p rds-workbench --all-targets` 零告警 + clippy 零新增。
7. **真机台账**：每期涉及执行/写库的改动补探针（`editor_exec_real` / 新探针），标注环境阻塞（如 MySQL/PG 不可达）非代码问题。

---

## 6. 开放决定（评审时确认）

| # | 问题 | 建议 | 影响 |
| --- | --- | --- | --- |
| 1 | 2a 值编辑的写回：编辑器生成 `UPDATE` 还是引擎生成？ | 引擎生成（与改写族同层，方言/引用转义一处管），编辑器只发意图 | 决定 2a 涉及文件 |
| 2 | 2b 通用模式是否纳入 `.rdsnote` 之外的脚本文件类型门控（.py/.r/.rs 高亮） | 高亮走内核已实现（Text 档有语法高亮能力），先不做 LSP | 决定 2b 范围 |
| 3 | 2d `.rdsnote` JSON schema 版本 | `{"version": 1, "cells": [...]}`，`CELL_SEPARATOR` 文本层保留 | 落盘格式一次性定 |
| 4 | 2c 编码/换行重设是否立项（外层转换层） | 本轮不做，菜单置灰+标注；另立事项 | 避免假按钮 |
| 5 | 顺序是否按 2a→2c→2b→2d | 按依赖推荐；用户可改 | 排期 |
