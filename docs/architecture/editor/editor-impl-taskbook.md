# 编辑器三模式 · 实现任务书（通用 / SQL / 分析）

> 状态：**推进中**——Phase 2a（共用表格编辑）**全部完成并测试绿**（T2a-1~7，2026-10-12）；**Phase 2c 已完成 T2c-1~5**（通道面包屑 / 方言段 / 英文微标 / 重设菜单 / 工具栏分组自定义，2026-10-13，rds-editor 505 passed）；剩余 **T2c-6 面板持久化**（含 schema 绑定与缩进跨会话持久化），之后按 2b → 2d 推进。
> 依据：`editor-prototype-design.md`（修订 ①~⑯）+ 三模式高保真原型（通用 / SQL / 分析）
> 基线（代码现状，2026-10-11 核实，2a/2c 完成后更新）：
> - Phase 1a（内核 + 文本 + SQL 编辑体验）**已完成**；Phase 1b（SQL 执行闭环）**主体完成**（2026-09-21 USIT 第 1 轮收尾）
> - Phase 2a（单元格编辑 + 变更集 + 批量提交 + 入口 UI）**全部完成**（T2a-1~7 测试绿：rds-editor 482 passed / rds-workbench lib 134 / 集成测试除环境占用项外全绿）
> - Phase 2c（壳收口）**进行中**：T2c-1~5 已完成（rds-editor 505 / rds-workbench 134 测绿）；T2c-6（面板持久化）未做
> - Phase 1c（分析模式骨架 Cell/Output/Session）**未开始**（2d）
> - 模式切换**已实现在工具栏最左**（`editor/src/view/host.rs` `render_toolbar` → `editor-mode-indicator` Button → `request_mode_switch`，与最新原型一致）
> - 结果表格已实现排序/筛选/冻结/右键菜单/复制/分段抓取/滚动到底自动加载；**值编辑（写回库）已实现**（2a 完成）

---

## 0. 顺序与理由（TL;DR）

推荐执行顺序：**2a 共用表格编辑 → 2c UI 对齐 → 2b 通用模式 → 2d 分析骨架**。

| 期 | 名称 | 一句话 | 依赖 |
| --- | --- | --- | --- |
| 2a | 共用表格：单元格编辑 + 变更集 + 批量提交 | ✅ **全部完成**（T2a-1~7 测绿）；表格是**共用件**（SQL 结果 + 分析内联输出） | 无（1b 表格已就绪） |
| 2c | UI 对齐最新原型 | 状态栏面包屑/方言/微标/重设 + 工具栏分组自定义；改动小、见效快（**T2c-1 完成**） | 无 |
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
- [x] **T2a-1 编辑能力判据**（纯函数，新模块 `editability.rs`）：`editable(通道, 项目态, 文档只读, 连接只读, 行标识, 列) -> Reason`；无主键/复合键/只读 → 编辑入口不亮且给原因（原型 §2.4）。✅ 2026-10-12 前完成（EditGate 六枚举 + `cell_editable`/`result_editable`）
- [x] **T2a-2 行标识探测**：结果集来源 SQL 的表名（已有 `export.rs` 的表名猜测）→ 取主键/唯一键（经 `CompletionPort::catalog` 已有目录，或经连接端口补一个 `primary_key(conn, table)`）；探测不到 = 行标识 None → 整表只读。✅（`completion.rs` trait `primary_key` + `ResultEntry.key_columns`/`row_key()`；`FakeCompletion.primary_key` 只认表名 `orders`）
- [x] **T2a-3 变更集模型**：`ChangeSet { cells: Vec<CellChange{row, col, original, edited, status}> }`，随结果集切换清空；脏单元格标色（待提交态）；按视图行序映射回数据行序（复用 `view_rows`）。✅（`changes.rs` `CellChange`/`ChangeSet` 含 `mark_committed`/`mark_conflict`/`has_conflicts`/`pending_by_row`；`store.rs::DocumentResults.changes` + `change_set`/`mutate_change_set`）
- [x] **T2a-4 单元格编辑交互**：双击进编辑（组件 Input / 编辑器内核复用）；Enter 提交、Esc 取消、失焦提交；只读单元格不响应。✅（`grid.rs` `CellEdit` + `CommitEditHook` + 编辑方法集 + `_edit_sub` 订阅；commit hook 只落 Shared 权威变更集；脏格 `warning opacity(0.16)`）
- [x] **T2a-5 批量提交**：`ExecTarget::Commit { changes, table, key }` + `QueryRunner::commit_changes`（默认实现如实不支持）；引擎/宿主生成 `UPDATE … WHERE key=…`（按行标识逐条，**单事务**包装）；影响行数 0 = 冲突（旧值已变），逐条报告。✅（`execution.rs::CommitReport.per_statement`——执行器只按语句顺序报影响行数；`workbench/editor_exec.rs::EngineQueryRunner::commit_changes`：通道闸 + 独立事务 BEGIN-COMMIT-ROLLBACK + 逐条 affected_rows + 逐条 UPDATE 进历史）
- [x] **T2a-6 回执与失败**：回执落状态栏（"已提交 N 条变更（M 条冲突）"）；失败逐条原因；不打断其余提交；**四道防线**（通道/项目/连接只读/无行标识）在提交前拒（沿用 `project_write_check` 同一处闸门）。✅ 2026-10-12 完成（见下「Phase 2a 完成记录」；实际防线为**五道 + 空变更**复核）
- [x] **T2a-7 批量入口**：结果工具栏「提交变更」按钮（有变更集才亮）+ 变更计数（原型 SQL 模式）；参考 DBeaver 的提交/回滚对。✅ 2026-10-12 完成（`ResultControls.commit`：提交/回滚对只在 `pending_count > 0` 时出现；提交中提交按钮置灰；`discard_pending_changes` 丢弃全部待提交、保留冲突证据；3 个窗口级用例见下「Phase 2a 完成记录」）

### 验收标准（测试断言式）
- 单测：编辑能力判据全矩阵（四种只读 × 有/无行标识）；行标识探测（有/无/复合键）；变更集 CRUD 与换结果清空；提交 SQL 生成（含主键 WHERE、列引用转义、单事务包装）；影响行数 0 → 冲突文案。
- 窗口用例：双击进编辑 → Enter 提交 → 变更为「待提交」→ 点提交变更 → 执行器真收到 Commit 目标；Esc 取消不留脏；只读档编辑入口不亮且状态栏说原因。
- 真机：MySQL / PG 各一例：改一格 → 批量提交 → 回读确认落库；并发改同一行（真机或探针）→ 冲突回执。
- 契约：`ui_contract` 全绿（新视图文件登记两份清单）；`check -p rds-editor -p rds-workbench --all-targets` 零告警。

### 涉及文件（现状基线）
`editor/src/{editability.rs(新), store.rs, execution.rs, export.rs(表名猜测复用), ui.rs, commands.rs}` + `editor/src/view/{results/grid.rs, host.rs, tests.rs}` + `workbench/src/services/{editor_exec.rs(commit 实现), editor_connections.rs(主键探测端口)}` + `engine/src/services/*`（写回语句执行）。

### Phase 2a 完成记录（2026-10-12，T2a-1~7 已测绿）
- **实际落点**（对照"涉及文件"基线）：`editor/src/changes.rs`（变更集）、`completion.rs`（行标识 trait）、`store.rs`（`DocumentResults.changes`）、`execution.rs`（`CommitReport.per_statement`）、`view/host.rs`（提交闸 + 泵回执 + 入口按钮 + 回滚）、`view/results/grid.rs`（`CellEdit` + 脏/冲突底色 + `ResultControls.commit`）、`view/tests.rs`（窗口级用例）；`workbench/src/services/editor_exec.rs`（`commit_changes` 实现）。
- **T2a-6 细节**：提交前复核实际为**五道防线 + 空变更**——① 有提交在途回绝 ② 非源库通道回绝（label 来自 `ExecChannel`）③ `project_read_only` 回绝 ④ 文档连接只读回绝 ⑤ 无行标识回绝 ⑥ 空变更回绝；`commit_updates_rows` 返回 `CommitUpdates{sql, skipped, per_line: Vec<(usize,Vec<usize>)>}`（语句↔数据行/列索引，回执组装原料）；泵分支 `drain_commit_notes`：Ok → per_line×per_statement 逐条组装，影响行数 ≥1 → `mark_committed`，=0 → `mark_conflict` + 冲突文案；Err → `commit_in_flight=None` + 状态栏失败文案。
- **踩坑记录**：`sync_result_view` 末尾按结果成败 `set_message(None)` 会清掉提交文案——必须先 `sync_result_view(cx)` 再 `set_message(Some(..))`（grid 冲突色已投影同步）；借用临时 `Ref` 需先 `let store = shared.results()` 绑定（E0716 四处）；`BTreeMap` 全路径。
- **顺手修（quick_open 测试隔离缺陷）**：现象=workbench 全量并行时 `opening_the_palette_loads_scratchpad_files` 稳定 20s 超时（单跑 0.09s、串行全量绿）。根因= `scratchpad::jobs` 结果队列是**进程级全局单例**（`JOBS: OnceLock<Shared>`），quick_open 每个测试都建 palette、各自的 60ms 泵都会 `drain_file_lists()`——并行时别的泵把文件用例的清单 drain 走丢弃（root 不匹配）→ `files_pending` 卡真、永不重发。修复=`jobs.rs` 暴露跨 crate `pub fn test_lock()`（与 scratchpad 自身测试同一把锁，`#[cfg(test)]` 对依赖不可见故必须 pub），**quick_open 6 个用例全部持锁**（只锁文件用例无效，已实证）。验证=5 连跑全绿（0.5s）。
- **跨会话协作说明**：另一会话给 `settings::model::ToolbarButtonCfg` 新增 `order: i32` 字段（serde default `default_toolbar_order`），`scratchpad_view/chrome.rs` 3 处 + `tests.rs` 2 处构造未跟上导致编译挂——已用 `..Default::default()` 补齐（chrome.rs 混着另一会话的状态栏图标化改动，未随本轮提交；settings 侧由对方会话收尾提交）。
- **T2a-7 细节**：`ResultControls` 加 `commit: Option<AnyElement>`（渲染顺序：筛选 → **提交/回滚** → 分析 → 导出 → 刷新 → 复制）；按钮组只在 `pending_count > 0` 时构造（无变更不占位）；「提交变更 (N)」计数 = 权威变更集待提交条数，提交中（`commit_in_flight` 非空）`Button.disabled(true)`（走 `gpui_kit::base::Disableable`，host 需导入 trait）；「撤销变更」→ `discard_pending_changes` → `ChangeSet::discard_pending()`（只清 Pending，Committed 已落库/Conflict 是证据都保留）→ `sync_result_view` 投影脏格消失；提交回调 `Err` 落状态栏（复用 `submit_pending_changes` 五道防线文案）。
- **T2a-7 测试**：3 个窗口级用例（`tests.rs`）——① `commit_controls_appear_with_pending_changes`：无变更按钮组不出现、改 2 格出现且计数 = `pending_count` ② `commit_controls_submit_then_vanish_on_receipt`：提交被接受后在途、提交中再提交被拦（"已有提交在进行中"）、回执后按钮组消失 ③ `discard_controls_clear_pending_and_keep_conflicts`：撤销清空待提交、冲突格保留、按钮组消失。断言用 `debug_bounds("editor-result-commit-group")`（按钮组容器带 debug_selector）。
- **环境性测试失败（非本轮引入）**：`workbench/tests/zz_fixture_probe.rs` 的 DuckDB fixture `D:\data\123` 被 VS Code（PID 13612）占用 → 真实连接失败（文件独占锁）。`rds-workbench --lib` 132 passed 与基线一致；并行集成测试编译在另一会话构建交错时出现 flaky `can't find crate`（target 元数据竞争，`-j 2` 可规避，与代码无关）。
- **待办（用户侧）**：hygiene-check 报告根目录散落 `build_diag*.log` / `test_*.log` / `wc_*.png|bmp` / `window_capture*`（测试日志 + 用户截图，AI 只归拢不删除，由用户清理）。

### 依赖与风险
- 依赖：1b 表格 delegate 已就绪（`render_td` 自绘可挂编辑、`view_rows` 视图行序）。
- 风险：主键探测缺数据源 → 先接 `CompletionPort::catalog`（已有 schema/表/列），探测不到如实降级只读，不编造行标识。**已化解**：探测经 `completion.rs` 的 `primary_key` trait + `ResultEntry.key_columns`/`row_key()`，探测不到 → 提交前回绝并说明。

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
- [x] **T2c-1 通道面包屑**：状态栏最左 `连接 ▸ 库 ▸ Schema`（可切换，同 DBeaver）；schema 列表来自 `CompletionPort::catalog`（已有 `all_schemas`）；切换 → 文档 Schema 绑定（随会话持久化，与 connection 同类）；SQL 模式显示、通用/分析模式不显示。✅ 2026-10-12 完成（见下「T2c-1 完成记录」）
- [x] **T2c-2 方言段**：从连接 `db_type` 解析（`editor_connections` 已有）→ 状态栏「方言 PostgreSQL」；未绑定连接不摆（真实值原则）。✅ 2026-10-12 完成（`connection.rs::dialect_label` 纯函数——已知驱动映射产品名、未知驱动原样；`StatusInputs/StatusLabels.dialect` 独立段，渲染在连接段（面包屑/文本）之后、正文之前；host 从绑定连接 db_type 解析——未绑定不摆）
- [x] **T2c-3 英文微标 + 行|列编辑切换**：右段 `Ln · Col · Ch · UTF-8 · LF · Spaces · 大小 · 脏` + **行｜列**切换入口（切换编辑内容模式：列编辑 = 内核 `ColumnarPoint`/`add_cursor_above|below` 原生能力；入口为 PopupMenu/Button，禁手搓）。✅ 2026-10-12 完成（见下「T2c-3 完成记录」）
- [x] **T2c-4 编码/换行/缩进重设**（VSCode 式小菜单）：缩进重设 = 内核 `TabSize`（原生，直接可做）；**LF/CRLF 与编码重设内核不支持**（文本模型统一 `\n`）→ 菜单项置灰 + 标注"需外层转换层"（与原型一致，不假装能做）。✅ 2026-10-12 完成（见下「T2c-4 完成记录」）
- [x] **T2c-5 工具栏分组自定义**（SQL 模式）：最右 `···` → 三栏面板（图标功能｜可见｜移动）；组=分隔符界定的连续段（执行簇只是例子，**分组通用**）；组可折叠为「首组件 + ▾」、整体移动、插竖线分隔符拆分、重置默认；模式指示器为固定项不参与排序（原型 ⑯ 已定）。✅ 2026-10-13 完成（见下「T2c-5 完成记录」）
- [ ] **T2c-6 面板持久化**：自定义布局（顺序/可见性/分组）随会话持久化；重置默认清除（恢复"执行 + 全部运行"两按钮布局）。

### 验收标准
- 单测：面包屑分段纯函数（连接/库/schema 缺项不占位）；方言解析（识别/未绑定）；微标格式化（Ln/Col/Ch/大小/脏）。
- 窗口用例：状态栏切 schema → 文档 Schema 绑定真变（真机：PG 切 schema 后执行落新 schema）；行|列切换入口真开列编辑（内核多光标）；重设菜单缩进真改内核 `TabSize`；工具栏面板折叠/移动/插分隔符/重置全链路（复用原型交互矩阵）。
- 契约：`ui_contract` 全绿（状态栏/工具栏改动登记）；`check --all-targets` 零告警。

### T2c-1 完成记录（2026-10-12，面包屑已接线并测绿）
- **落点（文件级）**：`editor/src/connection.rs`（`SchemaInfo{catalog: Option<String>, schema}`——注释：MySQL 无 Catalog 层时 catalog=None、`SchemasPort{fn schemas(conn_id)->Vec<SchemaInfo>}`（要求内存快照，渲染路径可调）、`SchemasHandle=Rc<dyn SchemasPort>`、`Breadcrumb{connection, catalog: Option, schema: Option}`、纯函数 `breadcrumb(conn_id, options, schemas, current_schema)`——**库/schema 段只从「列表+绑定」推导**：未选 schema → 两段都不占（不拿列表第一个假装当前）；绑定在列表找不到 → schema 段仍显示绑定、库段不猜；未绑定 → `UNBOUND_LABEL`；文件尾单测 `breadcrumb_segments_follow_truth`）+ `service.rs`（`Document.schema: Option<String>` + `schema()` 访问器 + `OpenRequest.schema` + `Document::new` 签名加参 + `set_schema`（返回是否真变）/`schema_for`）+ `shared.rs`（`schemas` 端口字段 + `attach_schemas`/`schemas(conn_id)`/`document_schema(id)`——未接端口=空列表如实降级）+ `view/widgets/status_bar.rs`（`StatusControls.breadcrumb: Option<AnyElement>` 最前；**`StatusLabels` 结构重构** `{connection: Option, body, right}`——连接段独立成段，面包屑渲染时用它替代文本连接段；左段渲染 `.children(breadcrumb).when(无breadcrumb, 文本连接段).child(body)`）+ `view/host.rs`（`set_schema(&mut self, Option<String>, cx)` 紧跟 set_channel；render 在 interrupt 前插入 breadcrumb 构造块——`communicating.then(||…)`：`shared.schemas(conn_id)` 空 → 返回 None 退化文本连接段；label = 连接 ▸ 库 ▸ schema（未选显 "schema"）；`DropdownButton::new("editor-breadcrumb")` + 菜单每 schema 一项 → `set_schema(Some)` + 分隔「使用连接默认」→ `set_schema(None)`）+ `workbench/src/services/editor_schema.rs`（**新**：`WorkbenchSchemas` 预载+内存读，与 `editor_completion` 同款——`NavCache::open(conn_id, project_root)` → `all_schemas()` 的 `(catalog, schema, id)` 映射 `SchemaInfo`；`attach(shared, workbench)`；`view.rs` 装配在 editor_completion 后一行）+ `view.rs`（装配）。
- **测试**：rds-editor **482 passed**（478 + `breadcrumb_segments_follow_truth` + 3 窗口用例：`breadcrumb_renders_and_switches_schema_binding`（面包屑渲染 + 切 schema → 文档绑定真变 + 「使用连接默认」解绑）/ `breadcrumb_absent_without_schema_port`（无 schema 列表 → 控件不渲染，退文本连接段）/ `breadcrumb_absent_in_text_mode`）；rds-workbench lib **134 passed**（132 + `load_schemas_maps_catalog_and_schema_segments`（含排序后比较——`list_schemas` 不保证插入序）/ `catalog_segment_stays_empty_without_a_catalog_layer`）。
- **踩坑记录**：`status_bar.rs` render 用 `.when(...)` 缺 `use gpui_kit::prelude::FluentBuilder as _;`（E0599）；`communicating.then(||…)` 闭包返回 `Option<AnyElement>` → `breadcrumb` 类型 `Option<Option<…>>`，构造处 `.flatten()`（E0308）；`.children(controls.breadcrumb)` move 后再 `controls.breadcrumb.is_none()` 借用冲突（E0382）→ 先 `let has_breadcrumb = controls.breadcrumb.is_some();`。
- **未做（本轮）**：面包屑持久化（`Document.schema` 随会话跨重启）——任务书「随会话持久化，与 connection 同类」尚未落到 `session.rs`/`editor_session.rs`（engine EditorContext 加列），排 T2c-6 面板持久化前补；**切 schema 真影响执行**（限定名/会话默认 schema）——验收风险栏「不做纯 UI 装饰」需执行链（`editor_exec`/engine 会话级 `SET search_path`）接上，本轮先落「绑定真变 + 菜单全链路」。

### T2c-2 完成记录（2026-10-12，方言段已接线并测绿）
- **落点**：`connection.rs::dialect_label(db_type)` 纯函数（`mysql_native/mysql`→MySQL、`postgres_native/postgresql/postgres`→PostgreSQL、`sqlite`→SQLite、`duckdb`→DuckDB、**未知驱动原样显示**——猜错方言比不显示更糟）；`status_bar.rs` `StatusInputs.dialect: Option<&str>` + `StatusLabels.dialect: Option<String>`（labels 加「方言 」前缀，输入为裸引擎名）+ 渲染 `.when(dialect.is_some(), …)` 在连接段之后、正文之前；`host.rs` 从绑定连接的 `ConnectionOption.db_type` 解析（`communicating` + 绑定连接才有；未绑定不摆——「跟随当前连接」时方言是谁都说不准）。
- **测试**：rds-editor **484 passed**（+`dialect_labels_known_drivers_and_keeps_unknown`、`dialect_segment_is_separable_and_only_with_a_connection`）。
- **踩坑**：`dialect_text.flatten().as_deref()` E0716（flatten 的临时值撑不过语句）→ 先 `let dialect_owned = dialect_text.flatten();` 再 `.as_deref()`；测试输入给裸引擎名（前缀是 labels 的职责，两边都加会重复「方言 方言 PostgreSQL」）。

### T2c-3 完成记录（2026-10-12，英文微标已接线并测绿；行|列入口已摆，列编辑待内核公开）
- **落点**：`service.rs::document_len(id)`（文档字节数 O(1)，渲染路径可调）+ `shared.rs::document_len`（转发）+ `view/widgets/status_bar.rs`（`StatusInputs` 加 `cursor_offset` / `tab_size` / `bytes`；`StatusControls` 加 `edit_mode: Option<AnyElement>` 放右段最前；`labels` 右段微标化 `Ln {line}, Col {col}, Ch {offset} · UTF-8 · LF · Sp {tab} · {size}` + 只读维度 + 脏 + 已选；`human_size(bytes)` 纯函数 B/K/M/G 四档）+ `view/host.rs`（光标读取加 `state.selected_range().start` 作 Ch；`tab_size` = `TabSize::default().tab_size`（内核未改 → 默认即实际，真值）；`bytes` = `shared.document_len(&self.document)`；行|列切换 `DropdownButton::new("editor-edit-mode")`——「行编辑」checked、「列编辑」disabled + on_click 提示「内核多光标 API 未公开（gpui-base 0.7.1）」）。
- **测试**：rds-editor **486 passed**（+`human_size_formats_b_k_m_g`、`right_segment_uses_english_badges_with_real_values`）；rds-workbench lib **134 passed**。
- **内核核实（多光标）**：gpui-base 0.7.1 `InputBaseState::add_cursor_above/below`（`input/base/state.rs:2150/:2159`）与 `Selections`（`cursor.rs:134`）**已实现但全部 `pub(super)`**——外部 crate 经 `gpui_kit::component::input::EditorState` 不可达；公开面只有 `select_all()`/`cursor_position()`/`selected_range()`。**结论：列编辑能力存在但未公开 → 入口如实置灰 + 标注，不假装能用**；后续若要真列编辑，走 **vendor 补丁暴露 `add_cursor_*`**（项目已有 vendor gpui-component + tab_panel patch 先例）——记入 §6 开放决定。
- **编码/换行/Spaces 数据源**：UTF-8 / LF 是内核文本模型的统一事实（utf8 rope、`\n` 存储）——真值常量，非编造；Spaces = `TabSize::default().tab_size`（=2，内核未设过）——T2c-4 缩进重设后改从 Shared 读。
- **T2c-3 遗留**：列编辑真实多光标（待内核公开）；`editor-edit-mode` 菜单在 headless 下弹层不可靠（走「渲染断言 + 提交函数纪律」），未写窗口用例——入口本身常显（行/列对三模式都是编辑内容语义）。

### T2c-4 完成记录（2026-10-12，重设菜单已接线并测绿；缩进真改内核 TabSize）
- **内核核实（缩进）**：gpui-base 0.7.1 `InputBaseState<M: MultiLineMode>::set_tab_size`（`input/editor/indent.rs:504`，**pub**）挂 `MultiLineMode` impl（`EditorMode: MultiLineMode` 在 `base/kind.rs:67`）→ `EditorState`（=`InputBaseState<EditorMode>` 别名，gpui-base `editor/mod.rs:11`）**可直接调 set_tab_size**——**编译实证**：`self.editor.update(cx, |state, cx| state.set_tab_size(TabSize{…}, cx))` 通过，无需给 rds-editor 加 gpui-base 直接依赖。`TabSize{tab_size, hard_tabs}`——**制表符缩进（hard_tabs=true）内核原生支持**（`indent.rs:13-27`），一并真做。注意：`mode.tab_size()` 读访问器仍是 `pub(super)`——**读不走内核**，走文档级状态（双写）。
- **落点**：`service.rs`（`Document.tab_size: usize`（默认 2）+ `hard_tabs: bool`（默认 false）+ 访问器 + `set_document_tab_size`/`set_document_hard_tabs`（真变判定）+ `document_tab_size`/`document_hard_tabs` 服务查询）+ `shared.rs`（`document_tab_size`/`set_document_tab_size`/`document_hard_tabs`/`set_document_hard_tabs` 转发）+ `view/widgets/status_bar.rs`（`StatusInputs.hard_tabs` + 微标 `Sp {n}` / `Tab` + `StatusControls.reset_menu` 位（edit_mode 后、右段文本前））+ `view/host.rs`（`set_tab_size(size, cx)` 双写文档+内核、`set_hard_tabs(hard, cx)` 双写；重设菜单 `DropdownButton::new("editor-reset-menu")` 三组（分隔 + disabled 组头）：编码（UTF-8 checked + UTF-8(BOM)/GBK disabled）、换行（LF checked + CRLF disabled）、缩进（空格: 2/4/8 按当前态 checked、制表符 checked 按 hard_tabs）——每项闭包先 `let entity = entity.clone()`（链式 move 借用修复 E0382）。
- **测试**：rds-editor **488 passed**（+service `indent_reset_tracks_width_and_hard_tabs_separately`（宽度/制表符独立存储、同值重设未变、ghost 文档回退默认）+ 窗口 `indent_reset_changes_document_and_renders`（直调 set_tab_size(4)/set_hard_tabs(true) → shared 真变 + 内核调用不 panic））；rds-workbench lib **134**、ui_contract **8**。
- **T2c-4 遗留**：编码/换行重设**内核不支持**（文本模型统一 UTF-8 + `\n`）——菜单已置灰标注，真转换需外层转换层（读写文件时按目标编码/换行编解码），排 2b/文件 IO 层；缩进文档状态**随会话持久化未落**（T2c-6 面板持久化一起）。

### T2c-5 完成记录（2026-10-13，工具栏分组自定义已接线并测绿）
- **模型（新 `toolbar.rs`，纯函数 + 12 单测）**：`ToolbarItemId`（执行族五成员 + 格式化 + ⋯更多；`ALL` 目录、`label`/`icon`/`visible_by_default`/`fixed_visible`）——**右对齐簇（通道/连接）与模式指示器不进模型**（原型 ⑯：模式指示器固定最左；右对齐语义稳定，T2c-6 随会话持久化时再放开）；`ToolbarLayout { left: Vec<ToolbarSlot::Item|Separator>, folded }`；组 = 分隔符界定的连续段（分隔符归前一段尾，`groups()` 投影）；纯函数：`default_layout`（执行簇五成员 + 格式化 + ⋯更多，三段间两条分隔符；执行簇默认只显示「执行 + 全部运行」）、`is_visible`/`set_visible`（`fixed_visible` 不可隐藏）、`toggle_collapsed`（按组头）、`move_item`（可跨组）、`move_group`（整段摘出插到相邻组外侧）、`insert_separator`/`remove_separator`/`move_separator`（跨过一项即改变组边界）、`separator_positions`/`has_extra_separator`（>2 即用户新增）、`reset`。
- **接线（host.rs）**：`render_toolbar` 重写——模式指示器固定最左 → 组循环（`render_toolbar_group`：折叠组 = 首组件 + ▾（`DropdownButton`，▾ 收纳组内其余可见成员，只收执行族——格式化要窗口、⋯更多是嵌套菜单，两都放不进折叠菜单）；展开组逐可见项）→ `ml_auto` 右对齐簇（通道 + 连接，不变）→ 最右 `···` 定制按钮（`Popover`——菜单实体缓存会让面板状态陈旧，`Popover::content` 每帧重建）；`render_toolbar_item` 按 id 分派到 `render_exec_run_button`（`editor-exec-run`）/`render_exec_all_button`（`editor-exec-all`）/`render_exec_kind_button`（`editor-exec-current/selection/batch`，`target_for_menu` 显式目标，无选区/语句不足置灰）/`render_format_button`/`render_more_group`；动作语义集中在 `run_toolbar_item_action(id, window, cx)`（执行族 → `execute_menu_kind`/`run_all`/`execute_preferring_selection`；格式化 → `format_document`）。
- **定制面板（`render_toolbar_customize`）**：`Popover::new("editor-toolbar-customize-popover").anchor(Anchor::BottomRight)`；四段——标题、组头行（组名 + 折叠 `Switch` + 组移动↑↓，`small_icon_button` 统一 ghost 图标按钮）、成员行（图标 + 标签｜可见 `Checkbox`（`fixed_visible` 置灰）｜单项移动↑↓）、段尾分隔符行（┃ 竖线分隔符｜↑↓ 移动｜✕ 移除）、底部（＋添加竖线分隔符｜↺ 重置默认）；所有动作走面板方法（`set_toolbar_visible`/`move_toolbar_item`/`move_toolbar_group`/`toggle_toolbar_collapsed`/`insert_toolbar_separator`/`move_toolbar_separator`/`remove_toolbar_separator`/`reset_toolbar`，每个内部 `toolbar.xxx()` + `cx.notify()`）；`small_icon_button`/`item_icon_row` 收 `&App` + 显式 `entity`（Popover content 闭包的 `cx` 是 `Context<PopoverState>`，拿不到面板实体）。
- **结构尺寸**：`ui.rs` 加 `TOOLBAR_CUSTOMIZE_WIDTH`（20rem）/`TOOLBAR_CUSTOMIZE_OFFSET`（4px，Pixels 常量——ui_contract 禁裸 `px`）。
- **测试**：toolbar.rs 12 单测 + tests.rs 6 窗口用例（默认两按钮布局 / 折叠↔展开 / 单项跨组移动 / 整组交换 / 插分隔符 + 重置 / `fixed_visible` 不可隐藏）——全走「渲染断言 + 直调面板方法」纪律；rds-editor **505 passed**（488 + 17 新）、ui_contract **8**。
- **顺手修（模型 bug）**：`move_group` 原实现「删左邻单槽 + 尾插」无法真正换组（分隔符在中间时组序不变）——重写为「整段槽位摘出 + 插到相邻组外侧」（左移插到前组开头前、右移插到后组末尾后），`moving_a_group_swaps_with_its_neighbor` 从红变绿。
- **T2c-5 遗留**：布局状态**随会话持久化未落**（T2c-6 一起：`ToolbarLayout` 序列化进 `SavedSession`）；折叠 ▾ 菜单只收执行族（格式化/⋯更多 在折叠态不可达，用户可展开或移出该组）。

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
| 1 | 2a 值编辑的写回：编辑器生成 `UPDATE` 还是引擎生成？ | **已定（2026-10-12）**：宿主执行器侧生成——`workbench/src/services/editor_exec.rs::EngineQueryRunner::commit_changes` 按行标识逐条生成 `UPDATE … WHERE key=…`，独立事务 BEGIN-COMMIT-ROLLBACK 包装（非 engine crate 层，与改写族同层思路一致；方言/引用转义由执行器一处管，编辑器只发 `CommitUpdates` 意图 + 语句映射） | 已落地（T2a-5/6） |
| 2 | 2b 通用模式是否纳入 `.rdsnote` 之外的脚本文件类型门控（.py/.r/.rs 高亮） | 高亮走内核已实现（Text 档有语法高亮能力），先不做 LSP | 决定 2b 范围 |
| 3 | 2d `.rdsnote` JSON schema 版本 | `{"version": 1, "cells": [...]}`，`CELL_SEPARATOR` 文本层保留 | 落盘格式一次性定 |
| 4 | 2c 编码/换行重设是否立项（外层转换层） | 本轮不做，菜单置灰+标注；另立事项 | 避免假按钮 |
| 5 | 顺序是否按 2a→2c→2b→2d | 按依赖推荐；用户可改。**当前进度（2026-10-13）**：2a（T2a-1~7）与 2c 的 T2c-1~5 已完成（含工具栏分组自定义）→ 下一项 **T2c-6**（面板布局持久化），之后 2b → 2d | 排期 |
| 6 | 分析模式内核（Cell/Output/Session）自研 vs 开源组件 | 用户 2026-10-11 问过"内核设计是否开发完成、有无开源组件支持"，未答复。待 2d 开工前定：gpui-kit 无现成笔记本内核（68 模块清单已核实），倾向自研薄骨架（参考 2d 任务清单） | 决定 2d 范围与工作量 |
