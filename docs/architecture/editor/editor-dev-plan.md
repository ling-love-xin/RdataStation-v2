# SQL 编辑器模块 · 开发方案（Phase 0 / 1a / 1b / 1c）

> 状态：**方案待确认（2026-09-15）**——**尚未开始实现**，本轮只产出文档（设计 / 架构 / 原型 / 交互稿）。
> 关联：`editor-prototype-design.md`（长什么样）、`editor-architecture.md`（为什么这样设计，§7 现状与 §12 已知问题为权威）、`editor-prototype.html`（交互稿）。
> 开工前必须先关闭 `editor-architecture.md` §13 的待确认项；其中 #1（Dock 标签能力）与 #3（格式化选型）是 Phase 0 的第一批动作。

---

## 0. 进度记录（最近在前）

| 日期 | 内容 | 状态 |
| --- | --- | --- |
| 2026-09-16（1b：B11 + B12 —— 编辑器成为唯一的 SQL 编辑器） | **旧 SQL 框退场，导航与 M1 桥改走编辑器契约**：① **请求入口**（B11）——`Shared` 新增 `QueryRequest{conn_id, sql, run}` + 私有字段与方法对（`request_query` / `take_query_request`，与 S3b 的 `open_file_request` 同口径：菜单 / 拖拽 / 后台回填都拿不到 `Window`）· 宿主新增 `WorkbenchView::open_query_document`（复用条件很窄：未命名 + 内容空 + **同一绑定**，否则新建；带 SQL 时用 `Shared::append_sql` **追加不覆盖**；`run = true` → `EditorHostPanel::run_all`，**M4 遗留的“查看数据不自动执行”在此关闭**）· 导航四处改调（在 SQL 编辑器中打开 / 查看数据 / 生成 SQL / 拖拽落点），`SidebarEvent::{EditorSqlRequest, OpenSqlEditor}` 两个变体随之下岗；`EditorBridge::insert_sql` 删除。② **删旧 SQL 区块**（B12）——`panels/editor.rs` 里的 Textarea + 内联执行闭包（直连全局分析库文件）+ 结果表 + 历史列表 + `use_duckdb_fed` 门控（约 250 行）与其全部状态（`sql_textarea`/`query_result`/`sql_history`/`last_executed`/`sql_for`/`pending_sql`/`_sql_sub`）及 `apply_nav_drag`/`insert_sql`/`clear_sql` 全删；`mock` 标题从 SQL 区里摘出来独立成块。③ **M1 未保存草稿拦截换实现**（panels-coupling-plan §2 的 S2 遗留项）——`components/project_host.rs` 的 `ProjectEditorBridge` 不再读 `Shared` 的三个镜像字段，改读 `EditorShared`（**草稿 = 未命名且非空的编辑器文档**；`clear` = 宿主关掉未命名文档，有路径的文件不碰）；`EditorShared` 因此提前到 `WorkbenchView::new` 构造（`build_host` 要用它）。**结果**：`Shared` 字段 **27 → 23**（`editor_dirty` / `editor_sql` / `editor_clear` / `result_epoch` 四个退场），契约 4 白名单同步。**验证**：editor 单测 **158 → 159**（宿主 `run_all` 与快捷键同路 + 空文档不打扰执行器）· `ui_contract` 7 项 · `dialog_host_layer` 4 项 · `cargo check --workspace --all-targets` 零告警 · clippy 无新增告警（editor crate 零警告）。⬜ 余：旧路径的**项目只读模式拒绝执行**要在新执行入口补回（目前只有连接侧只读维度，见架构 §13 #9） | ✅ 已完成 |
| 2026-09-16（1b：B1 切片一 — 连接绑定） | **“这条 SQL 发到哪”变成文档属性**（架构 §12 #26 的前半段关闭）：① editor —— `connection.rs`（新的**端口**：`ConnectionOption` 快照 + `ConnectionsPort::{options, ensure_connected}`，`chip_for` / `status_text` 是纯函数：`●P·orders` / `○ 未绑定连接` / 认不出的 id 显示 `?` + 原 id，**不假装它还在**）· `Document.connection`（与 `mode` / `read_only` 同类）+ `OpenRequest::with_connection` + `EditorService::{set_connection, connection_for}` · **执行通道带连接**：`QueryRunner::run(connection, sql)`、`ExecRequest` / `ExecOutcome` 都带 `connection`，`EditorShared::submit` 自己把文档的绑定取出来交给通道（调用方不需要知道连接从哪来）· 工具栏右侧**连接选择器**（菜单当前项打勾；选一个先**自动建连**，建连失败**就不绑定**并留原因；首项「跟随当前连接」= 1a 口径，如实摆着）· 状态栏最左新增连接段（只在会通信的模式出现）。② workbench —— `services/editor_connections.rs`（实现端口：列表读 `Shared::connections` **内存快照**（渲染路径不能 I/O）、建连走 `nav_runtime::{is_connected, connect_entry}` 与导航同一条；建连成功后把快照里的该条改为已连接，免得状态栏刚说“已绑定”却显示空心点）· `editor_exec.rs` 把连接交给 `SqlService::execute(conn_id, …)` · 「新建查询」带**当前选中的连接**（原型 §1.2 入口语义）。**验证**：editor 单测 **148 → 158**（连接文案 5 项纯函数 + 绑定进状态栏 / 建连失败不绑定 / **绑定真的传到执行端口**（真按键 `ctrl-enter`） / 选择器按模式显隐）+ workspace check 零告警 + editor clippy 零警告。⬜ 余：绑定随会话持久化（需 engine 侧加列）· 只读锁 ⓘ（连接侧策略来源）· 执行位置三通道（B13）· 导航右键带连接（B11） | ✅ 切片一完成 |
| 2026-09-16（1b：B16 关闭口径 + 新建入口 + 工具栏执行级） | **B16 主体完成**：① **关闭口径统一**——Dock 的 **每标签 ✕ 根本不存在**（组件库只有标签栏 `⋯` 菜单里的 `Dock.Close`），而它是否摆出来由 `TabGroup::is_closable` → **当前标签**的 `closable(cx)` 决定；面板把判据改成 `!is_dirty()` 后，脏文档那里**直接没有关闭项**（既不会静默丢，也不会“点了没反应”），关闭脏文档的唯一入口是 `Ctrl+W` → 三态确认；保存后脏点消失，关闭项自然回来。② **新建入口**：Quick Open（`Ctrl+P`）命令组新增 **「新建查询 / 新建笔记 / 新建文件」** → `WorkbenchView::new_editor_document(mode)`（三档走同一条路：开文档 → 建面板；标题由服务层未命名编号保证不重名）——在此之前“关掉最后一份文档”就没有任何回到编辑器的入口。③ **工具栏执行级（执行 ▾）**：用组件库 `DropdownButton` 做**分体按钮**（主按钮 = 执行“选区优先 → 当前语句”，与 `Ctrl+Enter` 同一条路；菜单 = 当前语句 / 选区 / 全部）· 新增纯函数 `statement_target`（忽略选区）与 `ExecMenuKind` / `target_for_menu`（显式目标，不做“选区优先”推断），菜单项**只放今天真能跑的三项**（批量执行 / 新结果标签属 1b，执行计划属 B10）· 执行 § 只在 SQL 模式出现（文本模式按能力表不通信，分析模式的执行是笔记级动作，归 1c）。**验证**：editor 单测 **144 → 148**（菜单目标三项独立 + 空文档三项全 Empty · 脏文档无可关闭判据 · 工具栏按模式分层）+ `cargo check --workspace --all-targets` 零告警 | ✅ 已完成 |
| 2026-09-16（1a：A9 对话框收尾） | **A9 全部收尾**：① `view/dialogs.rs`——三个对话框（**关闭三态** / **保存失败二次确认** / **模式切换确认**），文案表在 `mode::ConfirmKind`（标题说“要发生什么”、正文说“代价”、按钮说“按下去做什么”，**不出现“确定”这种看不出后果的词**，已有穷举单测）；单元粒度不做内嵌单选（对话框重建由 Root 决定，`Rc<Cell<_>>` 改了不会重绘）→ **两个动作按钮就是选择本身**（“整篇一个单元”/“按语句拆分”）。② **关闭三态落地入口在编辑器侧**：`request_close_document`（干净直接关 / 脏先问）+ `resolve_close_choice`（保存 → 先写盘再关，未命名先进另存为；不保存 → `close_document_now`；取消 → 什么都不做）+ 写盘失败二次确认（重试 / 另存为 / 取消，**绝不把脏文档当干净关掉**）。③ **系统文件对话框**：`rfd` 只在 workbench（`services/editor_files.rs`），打开是宿主动作，另存为做成**端口注入**（`EditorShared::attach_save_path_picker`）——编辑器不依赖 `rfd`，而且“没接端口”是**可读的失败**（状态栏说“未接入系统文件对话框”），不是静默。④ **模式指示器进了工具栏**（原型 §2.2 最左控件 `[SQL] ▾` → 菜单三档，当前项打勾）：需确认的切换**未确认前模式一点不动**（禁止静默切换）；`u1` 工具栏其余部分（执行族/格式化/历史/更多/执行位置/连接）仍未实现，**因此不放那些按钮**。⑤ `Ctrl+O` / `Ctrl+Shift+S` 注册（两键内核未占用；`Ctrl+O` 只被组件库绑在 `Command` context）。**验证**：editor 单测 **131 → 144**（对话框层真渲染 / 未确认前不过状态 / 确认后按所选粒度拆单元 / 不保存不改磁盘 / 未命名保存后写盘才关 / 取消另存为不关 / 未接端口留原因 / `Ctrl+Shift+S` 标题跟随）· ui_contract 5 项（扫描范围扩到 `dialogs.rs`）· 全工作区 `check --all-targets` 零告警（**顺手修了上一提交漏的两个 `NavReorder*` 导入，app 才能编过**）· ⚠️ **headless 下对话框按钮的“真点击”没做**：`debug_bounds` 的坐标与鼠标命中测试对不上（单跑能中、全套跑必不中），改成“断言层与按钮真渲染 + 直接驱动落地入口”（§12 #29） | ✅ 已完成 |
| 2026-09-15（1a：编辑器接入 workbench） | **编辑器已进中央 Dock**（A9 接线）：`crates/workbench/{Cargo.toml, src/view.rs}`——新增 `editor.workspace` 依赖、`WorkbenchView` 两个字段（`editor_service` / `editor_hosts`）、构造期初始开一份未命名 SQL 文档、`init_workspace` 用**链式** `DockLayout::tabs().panel_view(旧).panel_view(新)` 把编辑器面板接进中央 tab 组（**与旧面板并存，不替换**：旧面板退役属 B12）、新增 `open_in_editor(path, window, cx)`（走 `persist::open_file` + `DockArea::add_panel`，同文档复用不重建）。**仅改必要三处**，旧行为不变。验证：`cargo check --workspace --all-targets` 零告警（editor → workbench → app）+ editor 63 项 / workbench 52 项全绿。← **到此 app 启动后中央区就有一个可写、可高亮、带真实状态栏的编辑器标签** | ✅ 已完成 |
| 2026-09-15（1a：A13 + A15 收尾） | **A13 文件档位完成**：`limits.rs`（**判定纯函数** `tier_for_size`：<50MB 常规 / 50–200MB 大文件 / ≥200MB 超大；`tier_for_path` 是 I/O 只走打开事件路径；7 项单测含**真 200MB 稀疏文件**）· 文档 `Document.tier()`（档位是文档属性，与 mode/read_only 同类）+ `OpenRequest::with_tier`（≥200MB 同时置编辑器只读）· `persist::open_file` 按实际大小定档，**超大文件不整份读进内存**（原型 §4：“不建编辑器会话”）· 面板：**提示卡**（`warning` 色描边，说清限制而不是让用户撞上）。⬜ “关折叠”只有判据、**缺内核开关**（已记入代码与 A13 行）· **A15 契约完成**：`ui_contract` 扫描范围扩到 `crates/editor/src`（host / status_bar / result_grid / highlight 四个视图文件），**尺寸与颜色两项契约都过**；顺手把 `host.rs` 里最后一处 `.px(rems(…))` 改成 `px_2()`（局部间距走 Tailwind 尺度）并删掉对应常量。**editor 单测 121 → 131** · `ui_contract` 5 项全绿 | ✅ 已完成 |
| 2026-09-15（1a：A12 会话持久化） | **A12 完成**：**engine**——`engine_contexts` 表补 **`mode` 列**（`CREATE TABLE` 改新库 + `ensure_column` 幂等 `ALTER` 补老库）+ `load_latest_editor_context()`（启动只恢复最近一份）+ `EditorContext` 加 `mode`（唯一使用方，无破坏）· **修一个既有隐患**：`GlobalSqlitePool::acquire_sync` 原用 `Handle::current()`，在无 runtime 的线程（GPUI 主线程 / 普通 `#[test]`）**直接 panic**；现改为“在 tokio 上下文里返回可读错误 + 否则自建短命 runtime 驱动”· **editor**——`session.rs`（`SavedSession` + `SessionStore` 端口 + `MemorySessionStore`，5 项单测；**会话标识 = 路径键**，未命名文档不持久化；模式键 `text/sql/analysis` 与展示文案解耦，`EditorMode::as_key/from_key`）· 面板 `session_snapshot` / `restore_session` / `save_session_now`（**`Ctrl+S` 成功后与关文档时**落库）· **workbench**——`services/editor_session.rs`（真 store：包 `WorkbenchContextStore`）+ `WorkbenchView::restore_last_session`（`defer_in` 里读库；用**会话内容**开文档而非重读磁盘；恢复后收掉启动时那份空标签，走正常关闭路径）。**验收**：`crates/workbench/tests/editor_session_real.rs` 3 项在**真 SQLite** 上跑通（关文档→保存→重启→光标 12 与**分析模式**都回来 / 未命名不写库 / 会话 id 是路径键）· editor 115 → **121** · engine 300 → **301** | ✅ 已完成 |
| 2026-09-15（1a：A11 查找 / 替换） | **A11 完成——且结论是“零自建”**：先按“内核无 handler”的旧判断自建了一套（`find.rs` 匹配器 + `find_bar.rs` 查找栏 + 5 个 action + 4 条键位），实测后发现**判断是错的**：内核在 `Input` context 里绑了 `ctrl-f`/`ctrl-h` **并注册了 listener**（`state.rs:4195` 的 `on_action_search`/`on_action_replace`），界面由组件库 `SearchPanel` 渲染成浮层；按键被内核消费后外层根本收不到（已写入架构 §12 #24）。**自建部分已全部撤除**（含 `find.rs` / `find_bar.rs` / 相关 action、键位、测试），改为两条**验证测试**：（1）`the_kernel_find_panel_takes_over_on_ctrl_f`，（2）`ctrl_h_opens_the_kernel_replace_panel`（判据：按键后焦点被内核交给查找框，这是“面板真开了”的可观察信号）· editor 单测 113 → **115**（净增 2：撤除自建后又删掉 8 项查找自测）。**教训**：判断“内核有没有这个能力”要读 `on_action` 注册表，不能只 grep 会话字段 | ✅ 已完成（零告警） |
| 2026-09-15（1a：A14 最小执行 + 结果网格） | **A14 完成**：`execution.rs`（**执行目标解析**纯函数：选区 > 光标所在语句 > 全部，语句定位走 `engine::sql::split`；`QueryRunner` 执行端口（宿主注入）；`ExecChannel` 工作线程 + mpsc + 主线程轮询——同 `nav_jobs` 模式，**不引 tokio**）· `store.rs`（`ResultStore` 结果单权威——视图只是投影）· `view/widgets/result_grid.rs`（组件库 `DataTable`/`TableState` + `TableDelegate`，**首次使用该组件**，空态直说原因）· 面板：`Ctrl+Enter`/`Ctrl+Shift+Enter` 两个动作 + 轮询泵 + 结果区（有结果或执行中才占位）+ 状态栏「执行中…」· 文本模式**拒绝执行并说明原因**（读能力表 `execute`）· workbench：`services/editor_exec.rs`（`EngineQueryRunner`：`SqlService` + 活动连接，取数一律走 `batches`/`to_rows()`）· app 注册 `ctrl-enter` / `ctrl-shift-enter`。**editor 单测 100 → 113**（含 4 项真按键的窗口测试：光标所在语句 / 执行全部 / 失败留原因 / 文本模式拒绝）· **真机四库实测通过**（`crates/workbench/tests/editor_exec_real.rs`）：`✅ mysql 43ms · ✅ postgres 173ms · ✅ sqlite 1ms · ✅ duckdb 6ms`（各 1 行 × 1 列，列名 `n`）· ⬜ 未做：结果区高度可拖拽 / 多结果标签 / 执行历史 UI / 中断与超时（归 1b）· 连接绑定仍是“当前活动连接” | ✅ 已完成（零告警） |
| 2026-09-15（1a：A10 动作与快捷键） | **A10 部分完成（可独立完成的部分全做完）**：新增 `crates/editor/src/edit.rs`（**行注释开关纯函数**：选区→行块、空行跳过、缩进保持、选区结束在行首不吞下一行、CRLF/UTF-8 边界；13 项单测）· `crates/editor/src/commands.rs`（`SaveDocument` / `ToggleComment` / `CloseDocument`，namespace `editor`）· 面板 `key_context("editor")` + **`track_focus`**（没它快捷键落不到 `on_action`——**实测踩到**）+ 状态栏新增“动作失败原因”段（未命名需另存为 / 只读拒绝 / 有未保存改动，**不允许“按了没反应”**）· **关闭由宿主执行**：`close_document_in_dock(area, panel, window, cx)`（面板在自己的 `update` 里让 Dock 移除自己会**重入 panic**，见架构 §12 #23）· `focus_self`（宿主打开已打开文档时切到该标签，并同步服务层当前文档）· workbench：`close_active_editor` + `Ctrl+W` 的 `on_action`。app 层注册 **3 条键位**：`ctrl-s` / `ctrl-/` / `ctrl-w`（context 均为 `editor`）。**editor 单测 78 → 85**（含 5 项真按键盘的窗口测试：`ctrl-s` 真写盘、未命名真报原因、`ctrl-/` 真改文并可逆、只读真拒、宿主关闭真关文档+脏文档真被拦）· editor crate **clippy 零警告**（顺手清掉 4 条旧告警）· ⬜ 未做（**因此未注册、未宣传**）：`Ctrl+Enter`（随 A14 执行）· `Ctrl+F`/`Ctrl+Shift+F`（随 A11/B10）· `Ctrl+O` 与另存为（需系统文件对话框，属对话框批次） | ✅ 已完成（零告警） |
| 2026-09-15（1a：A9 持久化层） | **A9 持久化层完成**：`persist.rs` —— 唯一的 I/O 入口（只从事件路径调用，服务层与渲染路径保持零 I/O）；`open_file` 对已打开路径只激活（**不重读，避免冲掉未保存编辑**）；`save_document` **先写盘后清脏**（写失败不留假状态）；`save_as` 换路径保身份；`external_modified` 用 mtime 检测外部修改（含文件消失）；面板新增 `save()`。**editor 单测 55 → 63**（新增 8 项持久化测试，含真临时目录 I/O）· **测试逮到一个真 bug**：`if let Some(x) = shared.service().find…` 的只读借用活到分支体内 → 分支里 `update()` 撞 `RefCell already borrowed`（已修）· ⬜ 剩余：对话框与 workbench Dock 注册 | ✅ 已完成（零告警） |
| 2026-09-15（1a：A6 + A7） | **A6 只读两维度**：编辑内核 `.readonly()` 跟文档 `ReadOnly.editor`；**状态栏把两个维度分开写**（「只读」/「连接只读」，互不蕴含）。**A7 状态栏**：`view/widgets/status_bar.rs`（组件库 `StatusBar`），文案计算 `labels()` 为纯函数；左＝模式 + **语句数（`engine::sql::split_statements` 词法切分）** + 未保存，右＝两个只读维度 + Ln/Col + 已选字数；**语句数缓存**（仅内容变化时算一次，渲染期不扫描）；无数据源的字段（方言/编码/换行/缩进）**不显示占位**。**editor 单测 49 → 55** | ✅ 已完成（零告警） |
| 2026-09-15（1a：A5 模式切换） | **A5 逻辑完成**：`mode.rs` 新增切换矩阵（`SwitchPlan` / `ConfirmKind` / `SwitchContent` / `CellGranularity`）——**五个方向逐项对照原型 §1.3**（文本→SQL 免确认；SQL→文本/分析、分析→SQL、文本↔分析均需确认且给内容变换与提示语），另含**换会话**确认；内容变换为纯函数：`sql_to_cells`（粒度二选一，按**词法级**切分）、`cells_to_sql`（`;\n\n` + 尾分号）、`cells_from_text`/`cells_to_text`（文本层标记 `-- %%`，1c 落盘格式待定）。视图侧 `sync_mode` 按模式刷新着色与只读。**editor 单测 37 → 49**（新增 11 项矩阵测试 + 1 项窗口测试）· ⬜ 对话框待 A9 一并接 | ✅ 已完成（零告警） |
| 2026-09-15（1a：A4 高亮） | **A4 完成**：`view/highlight.rs` 实现 `DocumentRangeSemanticTokensProvider`——把 `engine::sql::highlight` 的词法区间编码成 LSP 语义 token（delta 编码、跨行按行切分、列按字符计），**颜色交给主题按 token 名解析**（keyword/type/function/string/number/comment/operator/punctuation/variable；**Identifier 不上色**）；文本模式不上色，分析模式留 1c 逐单元处理；大文件（>1MB）降级不上色。依赖新增：`engine` / `lsp-types`（0.97 随 gpui-base）/ `anyhow`（后两者已登记进根 Cargo.toml 唯一入口）。**editor 单测 28 → 37** | ✅ 已完成（工作区 check 零告警） |
| 2026-09-15（1a：A2 + A3） | **A2/A3 面板与内核视图**：新增 `crates/editor/src/{shared.rs, ui.rs}` + `view/{mod.rs, host.rs}`——`EditorHostPanel` 一个面板 = 一个标签 = 一份文档（Dock 提供标签条，面板只给 `tab_name`/`title`/脏点/`closable`）；内核用组件库 `input::{Editor, EditorState}`（零手搓）并接线 `InputEvent::Change` → `EditorService::set_content`（唯一权威）；`shared` 提供跨面板可克隆句柄（视图只读、变更走事件路径）；`ui.rs` 登记本 crate 结构尺寸（不反向依赖 workbench）；依赖只加 `gpui-kit`。**editor 单测 24 → 28**（新增 4 项 headless 窗口测试：标题跟随 / 脏点判据 / 3 文档并存 / 渲染不 panic）· **A4 方案按 API 核实修正**：组件无 SQL grammar，高亮走 `DocumentRangeSemanticTokensProvider` + 主题 token 名（见任务表） | ✅ 已完成（`cargo check --workspace --all-targets` 零告警） |
| 2026-09-15（1a 开动：A1 完成） | **A1 `EditorService`**：`crates/editor/src/service.rs`——文档集合唯一权威（打开/关闭/激活/重命名/内容/保存/模式/只读），`DocumentId` 一经生成永不变（另存为只换路径标题），去重规则“同路径重复打开=激活”，脏判据 `content != baseline`；**13 项单测**全绿（含 Windows 大小写归一的平台分支、关闭当前落到邻居、只读拒写）；`editor` 单测 11 → **24 项** | ✅ 已完成 |
| 2026-09-15（MySQL 事务修复 + 并发亲和结论） | **① MySQL `BEGIN` 修复**：`native/mysql.rs` 新增 `needs_text_protocol` / `execute_via_text_protocol`——事务控制与会话语句（`BEGIN`/`START TRANSACTION`/`COMMIT`/`ROLLBACK`/`SAVEPOINT`/`SET`/`USE`）改走 **文本协议**（`sqlx::raw_sql`），不再报 `1295`；探针实测：MySQL 事务内计数 = 1、`ROLLBACK` 生效 ✅ · **② P0.2c 并发亲和探针**（四库）：**SQLite / DuckDB 并发下仍同句柄；MySQL / PG 会另开物理连接**（另一侧报 `1146` / `relation does not exist`）→ **1b 的事务与会话必须 per-session 独占连接**（架构 §12 #2） · **③ DuckDB 扩展约定**写入依赖治理（外部预编译优先，扩展走指定目录 `INSTALL`，不为扩展重编内核） | ✅ 已完成（探针 12 项全绿） |
| 2026-09-15（P0.9 基线 + Phase 0 收官） | **P0.9 完成**：本模块**依赖增量 = 0**（未引 tree-sitter，sqlglot-rust 早已在依赖里）；`cargo build -p rds-app -j 2` 增量编译 **2m37s**，debug 二进制 **≈137 MiB**（数据入 §6）· **Phase 0 地基已全部完成**（P0.1–P0.10：仅余 P0.6 的“驱动层真实 `affected_rows`”按计划转 1b）· 验证汇总：引擎 297 项 / editor 11 项 / shared 22 项 / 台账探针 10 项 / 事务探针 8 项 全绿；`cargo check --workspace --all-targets` 零告警 | ✅ 已完成 |
| 2026-09-15（P0.2 实跑 + 三个结果保真度缺陷） | **P0.2 有结论**（真机四库）：PG / SQLite / DuckDB — **会话亲和成立**且 `ROLLBACK` 真实生效；驱动级事务（`execute_in_transaction`）四库中三库通过；**MySQL 的显式 `BEGIN` 被 prepared 协议拒绍**（1295，需改走驱动事务 API）→ 回写架构 §12 #2 / §7.3 #3 · **实跑又抓出三个真缺陷并修复**：① 各驱动只填 `batches`，而历史行数读的是恒空的 `total_rows` **字段**（已改用 `total_rows()`）；② `arrow_value_at` 漏了 Int32/UInt64/Float32 等位宽，兜底是 `format!("{:?}", array)`——**把整列 Debug 打印进每个单元格**（已修 + 3 项回归）；③ MySQL 列类型探测 `bool` 优先 → `COUNT(*)` 显示成 `true`（已按声明类型定排行）→ 架构 §12 #21 / #22 | ✅ 已完成（实跑验证：引擎 297 项 · shared 22 项 · 探针 10 项 · 事务探针 8 项全绿） |
| 2026-09-15（探针实跑：台账结论落地） | **P0.10 已实跑（10 项全绿）**，结论已回写原型 §7.4「行为级事实」：**类型标注 / 血缘 / 作用域 / 下推 / 差异** 均**实测可用**；差异粒度到 `SelectItem`/`Expr`/`OrderByItem`，**SELECT 列表 / WHERE / ORDER BY / LIMIT 改动都能检出**（原先担心的“漏条件改动”**已被实测推翻**）· **两处保留**：`qualify_columns` 只部分限定（`id` 仍裸列）、`unnest_subqueries` 改写为 `INNER JOIN + DISTINCT`（NULL 语义不等价）· **两处纠正**：① 格式化**不会丢注释**（行内/尾随注释→解析失败→原样返回，架构 §12 #3 改述）② `transpile` 对脚本是**静默截断**（`"SELECT 1; SELECT 2;"` → `Ok("SELECT 1")`，生产路径同样）→ 升级为 🔴（§12 #19）· 三条不变量已提升为断言（同句只 `Keep` / WHERE 改动有 `Update` / 换型 = `Remove+Insert`） | ✅ 已完成 |
| 2026-09-15（台账候选探针 + 探针编译错误修正） | **P0.10 探针就绪**：`crates/engine/tests/sqlglot_capabilities.rs`（9 组真实 SQL：作用域 / 血缘 / 类型标注 / 差异 / 下推 / 限定与展开 / 本地计划 / 转译单条限制 / 格式化注释保真；**报告式** + 仅弱断言）· **修正一个上轮埋下的编译错误**：`crates/engine/tests/transaction_affinity.rs` 写的是 `use engine::…`——集成测试是**独立 crate**，本包库目标名是 `rds_engine`（`engine` 只是**其它** crate 的依赖别名）；证据：`crates/{connection,mock}/tests/*` 分别用 `rds_connection::` / `rds_mock::` | ✅ 已完成（编译验证待本机执行，见 §6） |
| 2026-09-15（提交 + 台账二次核验） | **模块已提交**（`ac0d75f`，24 文件 / 5625 行：文档 5 件 + `crates/editor` 骨架 + engine SQL 原语 + 历史字段 + 事务探针；暂存时避开同工作区其它会话的在途改动，混文件 `Cargo.lock` / `sql/engine.rs` / `docs/architecture/README.md` 按 hunk 级分离）· **sqlglot 台账二次核验（读实现）**：修正 4 处（关键字是**逐词枚举变体** / `Token::position` 是**字符**下标 / `transpile` **只吃单条** / `builder` 证据行号），补 `Schema`+`MappingSchema`、`qualify_columns`、`optimize`、`plan`（仅本地计划）、`executor`（不采用）等条目（原型 §7.4） · **修正一个真缺陷**：高亮区间改为「字符偏移 → 字节偏移 + 取原文区间」（原实现按 `value` 回查，转义字符串落空、引号与注释标记取不到）→ 架构 §12 #20 | ✅ 已完成（编译验证待本机执行，见 §6） |
| 2026-09-15 | 文档阶段：模块入口 + 原型设计 + 架构（含现状盘点与四个假底座）+ 本方案 + 交互稿（`editor-prototype.html`，1554 行，24 条交互断言） | ✅ 已完成 |
| 2026-09-15（修订） | 按评审意见补充：**执行通道与执行族正交**（源库/加速/联邦）· 结果区从三模式过滤收敛为「筛选（本地/下发）+ DuckDB 分析」· **结果血缘与通道徽标** · 结果集只读边界与网格归属（架构 §3.6）· 任务表新增 B13/B14/B15/C9 与测试场景 30–36 | ✅ 已完成 |
| 2026-09-15（Phase 0 第一批） | **P0.4 语句切分**：`engine::sql::split`（词法级状态机 + 26 项表驱动测试）+ `SqlEngine::split_statements` + `sql_parser_service::split_sql` 委托重写 · **P0.5 历史字段**：`SqlHistoryEntry` + `save_sql_history(_into)`（耗时/成功/失败原因/行数真实，失败也留痕）+ 4 项单测 · **P0.7 骨架**：新建 `crates/editor`（`model` 能力表 + `mode` 判定规则表 + README）+ workspace 登记 · `QueryResult::from_batches` 不再把 `total_rows` 当作 `affected_rows` | ✅ 已完成（编译验证待本机执行，见 §6） |
| 2026-09-15（Phase 0 待办） | P0.1 Dock 标签能力验证 · P0.2 事务会话亲和验证 · P0.3 格式化选型 · P0.6 驱动层真实 `affected_rows` · P0.8 SQL 高亮注册 · P0.9 平台基线 | ⬜ 未开始 |
| 2026-09-15（P0.3 完成） | **格式化不再依赖新库**：sqlglot-rust 已自带 `generate_pretty`；`SqlEngine::format` 改为“多语句逐条生成 + `;\n\n` 拼接”，解析失败原样返回；旧测试从“非空”升级为 7 项回归 | ✅ 已完成 |
| 2026-09-15（P0.8 完成 · P0.2 探针就绪） | **P0.8 换路线并完成**：不再引 tree-sitter，改用 sqlglot tokenizer → `engine/src/sql/highlight.rs`（字节区间 + 类别，**13 项单测**）· **P0.2 探针就绪**：`crates/engine/tests/transaction_affinity.rs`（四类库、环境变量注入、临时表作亲和判据、不改用户数据） | ✅ P0.8 完成；P0.2 待真机运行 |
| 2026-09-15（评审决策 + P0.1 静态核实） | **分段抓取已确认纳入 1b**（新增 B5b + 测试场景 37–39）· 自动刷新/钉住 1b 顺带 · **P0.1 结论（静态）**：Dock **没有“关闭前否决”钩子**（`DockArea` 收到 `TabGroupEvent::ClosePanel` 后直接 `remove_panel_id`；`Panel::closable(cx)` 是唯一闸门；`with_renderer` / `remove_panel` 均为 `pub`）→ 关闭语义默认走“草稿兜底”（架构 §13 #15） | ✅ 已完成 |
| — | Phase 0 地基与技术验证 | ⬜ 未开始 |
| — | Phase 1a 内核 + 文本/SQL 编辑体验 + 最小执行 | ⬜ 未开始 |
| — | Phase 1b SQL 执行闭环（DBeaver 一档） | ⬜ 未开始 |
| — | Phase 1c 分析模式骨架（Cell/Output/Session，仅 SQL 单元） | ⬜ 未开始 |
| — | Phase 2 Python/Rust 内核 · 变量桥 · 富输出 · `.ipynb` | ⬜ 未开始 |

---

## 1. 现状结论（盘点摘要）

| 层 | 状态 | 证据 |
| --- | --- | --- |
| 编辑器视图 | ⛔ 只有一个 `Textarea`（无高亮/行号/补全），且仅在 `use_duckdb_fed` 连接下出现 | `crates/workbench/src/panels/` L6094 / L6809 / L6779 |
| 执行链路 | ⛔ 同步直连全局分析库文件，与连接无关、无取消/超时/事务/批量；且内联在 render 闭包内（违反 render 零 I/O） | 同上 L6821-6825；`services/query_runner.rs` |
| 结果区 | ⛔ 定宽 `div` 表格（无虚拟化/排序/过滤/分页） | 同上 L6921-6950 |
| 历史 / 导出 | 🟡 20 条 JSON 回填 / 仅 CSV | `services/query_history.rs`、`services/query_export.rs` |
| 多文档 / 快捷键 / Action | ⛔ 无 | `view.rs` L302、`commands.rs`、`app/src/main.rs` L80-110 |
| engine SQL 能力（解析/执行/历史/模板/上下文） | ✅ 已迁移但**多数零接线** | 架构 §7.2 |
| 元数据内省（补全数据源） | ✅ 已迁移（异步）但零消费 | `crates/database/src/metadata_service.rs` |
| 四个假底座（格式化/切句/事务/历史字段） | 🔴 必须先修 | 架构 §7.3 |

**关键缺口**：① 编辑器本体（内核 + 三模式）；② 执行管线（抽取 + 后台化）；③ 语句切分与格式化重做；④ 事务与历史真实化；⑤ 分析模式的模型（Cell/Output/Session）。

---

## 2. 阶段划分

```
Phase 0（地基，无 UI）
   ├─ 技术验证：Dock 标签能力 / 事务会话亲和 / 格式化选型
   ├─ 后端修复：语句切分 · 历史字段 · affected_rows
   ├─ 结构：crates/editor 骨架 + 依赖接线
   └─ 验证：SQL 语法高亮注册（tree-sitter-sql）
        │
        ├──► Phase 1a（内核 + 文本模式 + SQL 编辑体验 + 最小执行）→ 交付「能用的编辑器」
        │        │
        │        └──► Phase 1b（执行闭环：选区/当前语句/批量/取消/事务/结果区/历史/导出）→ 交付「合格的 SQL 客户端」
        │                 │
        │                 └──► Phase 1c（Cell/Output/Session + 内联输出 + .rdsnote）→ 交付「分析笔记骨架」
        │                          │
        │                          └──► Phase 2（Python/Rust 内核 · 变量桥 · 富输出 · 互操作）
```

**为什么要分 1a/1b/1c 而不是一次做完**：1a 结束即可自证价值（编辑器可用）；1b 与 1a 的验证手段完全不同（前者靠窗口/单测，后者靠真实端点回归）；1c 的复杂度在"会话与输出"，与"编辑体验/执行语义"耦合会互相拖慢。

---

### Phase 0 — 地基与技术验证（不含 UI）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| P0.1 | **Dock 标签能力验证**：`Panel::{title, title_suffix, closable}` 能否承载脏点与“关闭前确认” | ✅ **静态结论已出（2026-09-15）**：脏点走 `title_suffix` 可行；**关闭无否决钩子**（`DockArea` :1257 收到 `TabGroupEvent::ClosePanel` 即 `remove_panel_id`；`closable(cx)` 只是静态许可）。可选路径：① 草稿兜底（默认）② 自绘标签条 + `with_renderer` + 确认后 `remove_panel` | 结论已写回架构 §12 #18 / §13 #15；运行期探针（脏点渲染 + 两个关闭路径）随 1a 的 A2 一并验证 |
| P0.2 | **事务会话亲和验证**：连接池下 `BEGIN` 与后续语句是否同一物理连接（临时表作判据，四类库均可跑） | ✅ **已实跑出结论（2026-09-15）**：`crates/engine/tests/transaction_affinity.rs`（含驱动级事务路径用例）—— PG / SQLite / DuckDB **亲和成立 + `ROLLBACK` 真实生效**；**MySQL 显式 `BEGIN` 被 prepared 协议拒绍（1295）**，驱动级事务可用 | 结论已回写架构 §12 #2 / §7.3 #3；1b 实现事务状态机时：**MySQL 的 begin/commit/rollback 必须改走驱动事务 API** |
| P0.3 | **格式化实现选型** | ✅ **选定（2026-09-15）**：sqlglot-rust 自带 `generate_pretty` + `parse_statements_with_comments`（**无需新依赖**） | `engine/src/sql/formatter.rs` 重写；回归从“非空”升级为 7 项（非 Debug 打印 / 可再解析 / 多语句不丢 / 失败原样返回 / 空输入 / 前导注释 / 各方言参数）；残留：行内注释丢失（**已源码核实** `gen_statement` 只 emit 前导注释） |
| P0.4 | **语句切分实现**：词法级扫描器（`'…'` `"…"` `` `…` `` `--` `/* */` `$$…$$`），返回 `Vec<SqlStatement{start,end,line}>` | **`crates/engine/src/sql/split.rs`**（✅ 已完成）+ `SqlEngine::split_statements` + `sql_parser_service::split_sql` 委托 | 表驱动单测 26 项（含各方言字面量/注释/嵌套/占位符/多字节/未闭合/行号）；`psql` 过程体 `$$` 块不被切开 |
| P0.5 | **历史字段贯通**：`save_sql_history` 增参（`elapsed_ms / success / error / rows_affected / rows_returned / db_type`）+ 失败路径也写 | ✅ `engine/src/persistence/history_store.rs`（`SqlHistoryEntry` + `save_sql_history` / `save_sql_history_into`）、`engine/src/services/sql_service.rs`（成功/失败双路径写入 + `db_type_of` 助手） | 单测：成功/失败各写一条且字段真实（耗时 > 0、成功标志正确、失败含原因）；存储可注入（`*_into`）不碰用户真实历史 |
| P0.6 | **影响行数**：`QueryResult::from_batches` 不再把 `total_rows` 当作 `affected_rows`（✅）；历史在写语句上记 `rows_affected`（已接线） | `crates/shared/src/models.rs`、`engine/src/services/sql_service.rs` | 遗留（转 1b）：**驱动层返回真实 affected_rows**（见架构 §12 #17） |
| P0.7 | **crate 骨架与依赖接线**：`crates/editor`（lib + `model` 能力表 + `mode` 判定表 + README）、workspace 依赖唯一入口登记 | ✅ `Cargo.toml`（members + 依赖别名）、`crates/editor/{Cargo.toml,README.md,src/*}` | `cargo check --workspace --all-targets -j 2` 零告警；`cargo test -p rds-editor --lib` 全绿（workbench → editor 的依赖线等到 1a 首次使用再连，避免死依赖） |
| P0.8 | **SQL 高亮注册验证** | ✅ **方案已变且已落地（2026-09-15）**：改用 **sqlglot tokenizer**（`sqlglot_rust::tokens`，带注释与行列位）→ `engine/src/sql/highlight.rs` 产出「字节区间 + 类别」，**不需要 tree-sitter、不需要联网取包** | 单测 **13 项**（关键字/类型/函数/字符串含引号/**转义字符串**/数字/占位符/注释/标点运算符/升序不重叠/**中文 SQL 字节区间**/未闭合降级/区间助手）；视图层只负责“类别 → 主题色”。行为级事实已记原型 §7.4（空白被丢弃、关键字逐词变体、`position` 是字符下标、`quote_char` 只给带引号标识符） |
| P0.9 | **平台与性能基线**：记录编译时间增量（新增依赖后 `cargo build -p rds-app`）与二进制体积变化 | ✅ **已记录（2026-09-15）** | 数据已写入本文件 §6：**依赖增量 = 0**（未引 tree-sitter）；增量编译 2m37s；debug 二进制 ≈137 MiB |
| P0.10 | **台账候选验证用例**（原型 §7.4 标 ⚪ 的项：作用域 / 血缘 / 类型标注 / 差异 / 下推 / 限定与展开 / 本地计划 / 转译单条限制 / 格式化注释保真） | ✅ **探针已就绪并已实跑**：`crates/engine/tests/sqlglot_capabilities.rs`（离线、不连库；报告式 + 三条已确认的不变量断言）；**结论已回写原型 §7.4**（可用 / 保留 / 纠正逐条列明） | 重跑：`cargo test -p rds-engine --test sqlglot_capabilities -j 2 -- --nocapture --test-threads=1`（10 项）；可用项提升为 `engine::sql::*` 公开 API 时随 1b/1c 一并补断言式单测 |

> Phase 0 结束时：后端"假底座"全部转真 + 结构就位 + 三个技术未知消除。

---

### Phase 1a — 内核 + 文本模式 + SQL 编辑体验（+ 最小执行）

**目标**：打开 `.sql` / `.txt` 就能得到"键盘优先、状态可信"的编辑体验；`Ctrl+Enter` 能把整篇 SQL 跑出结果（结果区先做基础版）。**此阶段结束即"能用的编辑器"**。

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| A1 | `EditorService`：打开/关闭/激活/重命名文档；`DocumentId` 稳定生成；文档集合状态 | ✅ **已完成**：`crates/editor/src/service.rs`（纯逻辑、零 I/O、不碰 GPUI） | 服务层单测 **13 项**全绿（含：同路径重复打开=激活且不新开标签 · 大小写/分隔符归一的平台差异 · 重命名保身份不保路径 · 关闭当前落到邻居 · 脏判据只比内容（撤销回原值即干净）· 只读拒写） |
| A2 | **多文档标签条**（依 P0.1 结论）：标签、脏点、关闭、`+` 新建、溢出处理 | ✅ **面板侧已完成**：`crates/editor/src/view/host.rs`（Dock 提供标签条，面板只给 `tab_name` / `title` / `title_suffix` 脏点 / `closable`）· ⬜ 待接：workbench 中央 Dock 的 tab 组注册（需先有“打开文件”入口，A9） | 窗口测试：3 文档并存（3 个面板、标题各自正确、只有被改的那个脏）· 标题跟随后同· 保存后脏点消失· 面板名/可关闭稳定· 渲染不 panic |
| A3 | 内核视图接入：`Editor` + `EditorState` 每文档一份 + 行号 + 只读态 | ✅ **已完成**：`view/host.rs` 用 `gpui_kit::component::input::{Editor, EditorState}`（零手搓）；`EditorState` → `EditorService` 回写由 `subscribe_in(InputEvent::Change)` 接线；只读态来自文档 `ReadOnly.editor` | 窗口测试：面板渲染一帧不 panic；只读/可编辑由 `readonly(read_only)` 控制（A6 再补三种组合的断言） |
| A4 | **SQL 高亮**：方言选择 + 主题角色映射（缺角色复用最接近标准 token） | ✅ **已完成**：`crates/editor/src/view/highlight.rs`——`SqlSemanticTokensProvider` 实现 gpui-base 的 `DocumentRangeSemanticTokensProvider`（**不走 tree-sitter**，组件库也无 SQL grammar）；`TokenClass → 主题词汇`映射表见文件头（`Parameter → variable`，**`Identifier` 不上色**）；颜色由活跃 `HighlightTheme` 按名字解析（**零裸色值**，主题切换自动重着色） | 单测 **9 项**：图例覆盖 / Identifier 不上色 / delta 编码 / 中文按**字符**计列 / 跨行区间按行切分（LSP token 必须单行）/ 仅产出可视区间 / 大文件降级 / 未闭合词法不影响编辑；真机明暗核对随 1b |
| A5 | **模式判定与切换**：规则表（§1.2）+ 切换矩阵（§1.3，含确认对话框与内容变换）；能力表驱动 chrome | ✅ **全部完成**：`mode.rs` —— 判定（记忆 > 扩展名 > 文本）+ **切换矩阵**（`plan_switch` 纯函数：确认类型 / 内容变换 / 提示语；禁止静默切换）+ 内容变换（`sql_to_cells` 走 `engine::sql::split`、`cells_to_sql`、`cells_from_text` / `cells_to_text`，文本层分隔标记 `-- %%`）+ **四种确认的文案表**（`ConfirmKind::{title, body, confirm_label, picks_granularity}`）；视图：`EditorHostPanel::sync_mode` 按模式刷新（着色 / 只读）· **工具栏模式指示器**（`[SQL] ▾` → 菜单）· `request_mode_switch` → 需确认则弹 `dialogs::open_switch_confirm`，`confirm_mode_switch` 落地（先写回服务层再重载内核，内容只有一条真值路径） | 单测 **14 项**（矩阵逐项 + 粒度二选一 + 字符串内分号不被误拆 + 单元文本往返 + 空输入 + 四种确认文案不重复/不含“确定” + 粒度选择器只在“转单元”出现）+ 窗口测试 **4 项**：需确认的切换未确认前不过状态、确认后按所选粒度拆单元、文本→SQL 免确认、确认回调能独立驱动 |
| A6 | **只读两维度**：编辑器只读（文档属性）与连接只读（策略）分别表达 | ✅ **已完成**：`model::ReadOnly` 两字段 → 编辑内核 `.readonly()` + **状态栏分别显示**「只读」/「连接只读」（互不蕴含）；文档属性来自 `EditorService`，同一窗口四种组合并存不互相影响 | 单测：四种组合逐项断言（仅编辑器只读不等于连接只读，反之亦然）+ 窗口测试：切模式/渲染不 panic |
| A7 | **编辑器状态栏**（真实值）：Ln/Col、选区字数、语句数（来自 `split.rs`）、方言、编码/换行/缩进、模式、脏 | ✅ **已完成**：`view/widgets/status_bar.rs`（用组件库 `StatusBar`，不手搓）——文案计算是**纯函数** `labels()`（可穷举断言）；左：模式短标签 + **语句数** + 未保存；右：两个只读维度 + Ln/Col + 已选字数 | 单测 6 项：语句数随内容变（1/3/9）+ 脏可见 + 文本模式不显示语句数 + 四种只读组合 + 光标/选区真实值。**方言 / 编码 / 换行 / 缩进暂无数据源（1b / A12）→ 不显示占位**（零 UI 造数据） |
| A8 | **脏状态**：输入事件 → 与 baseline 比较 → 置脏/清脏；标签脏点与状态栏同步 | ✅ **已完成**：`service.rs` 的 baseline 比较 + `view/host.rs` 的 `InputEvent::Change` 回写；标签脏点由 `title_suffix` 渲染 | 单测：编辑→脏、撤销回原值→干净、保存→干净 |
| A9 | 保存 / 另存为 / 外部修改检测 / 关闭三态确认（保存失败二次确认） | ✅ **已完成**：`persist.rs`（读 / 写 / mtime 外部修改检测 / `open_file` / `save_document` / `save_as`）· `EditorHostPanel::{save, save_as, document_path, document_mode, clear_message}` · `WorkbenchView` 的 `editor_service` + `editor_hosts` + `open_in_editor` · **关闭三态**：`request_close_document` / `resolve_close_choice`（保存 / 不保存 / 取消，写盘失败再问一次）· **另存为**：路径选择端口（`EditorShared::attach_save_path_picker`；workbench 用 `rfd` 实现）· **对话框**：`view/dialogs.rs`（关三态 / 保存失败 / 模式切换）· **系统文件对话框**：`services/editor_files.rs`（打开是宿主动作、另存为是端口）+ 键位 `ctrl-o` / `ctrl-shift-s` | 单测 8 项（持久化）+ **窗口测试 6 项**（关三态真弹框 / 不保存不写盘 / 未命名保存后写盘才关 / 取消另存为不关 / 未接端口留原因 / 另存为标题跟随）+ `cargo check --workspace --all-targets` 零告警 |

#### A9 收尾：workbench 接线配方（2026-09-15 **API 已逐条核实**，照此执行即可）

1. **`crates/workbench/Cargo.toml`**：加 `editor.workspace = true`（依赖键已在根 `Cargo.toml` 登记为 `rds-editor`）。
2. **`crates/workbench/src/view.rs`**：
   - 导入：`editor::model::{DocumentId, EditorMode}` · `editor::service::OpenRequest` · `editor::shared::EditorShared` · `editor::view::host::EditorHostPanel`；
   - `struct WorkbenchView` 加两字段：`editor_service: EditorShared`（跨面板共享的文档集合）与 `editor_hosts: Vec<Entity<EditorHostPanel>>`（已开面板，按 `DocumentId` 复用）；
   - `WorkbenchView::new`：`editor_service: EditorShared::new()`，并初始打开一份未命名 SQL 文档（`open(OpenRequest::untitled("", EditorMode::Sql))`）；
   - `init_workspace`：**先把 `EditorShared` clone 到局部变量**（否则 `cx.new(|cx| …)` 闭包里借 `self` 会冲突），再建 `EditorHostPanel`；中央 tab 组用**链式** `DockLayout::tabs().panel_view(legacy_handle, cx).panel_view(host_handle, cx)`（`panel_view(mut self, Arc<dyn PanelView>, &App) -> Self`，`gpui-base/src/dock/layout/builder.rs:112`，已核实可链式）。
   - 新增 `pub fn open_in_editor(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>)`：走 `editor::persist::open_file(&self.editor_service, &path, editor::mode::resolve_mode(&path, None))`；已打开的就按 `DocumentId` 在 `editor_hosts` 里找面板复用（不新建），新开的 `cx.new(|cx| EditorHostPanel::new(service, id, window, cx))` 后用 **`area.add_panel(panel, DockPlacement::Center, None, window, cx)`**（`gpui-base/src/dock/dock_area.rs:402`，已核实）加入中央 tab 组。
3. **命令与快捷键（A10）**：`Ctrl+S` → `EditorHostPanel::save`；`Ctrl+/` → 行注释开关；`Ctrl+W` → 关闭当前文档（**宿主执行**：`WorkbenchView::close_active_editor` → `close_document_in_dock`；面板自己发起会重入，见架构 §12 #23）。按项目规矩在 `crates/app/src/main.rs` 绑定 keymap（**注册了才宣传**）：已注册 `ctrl-s` / `ctrl-/` / `ctrl-w`（context `editor`）。`Ctrl+O` 待系统文件对话框落地后再注册。
4. **对话框（A9，已完成）**：`crates/editor/src/view/dialogs.rs`（关三态 / 保存失败二次确认 / 模式切换确认）与它们的落地入口 `editor::view::host::{request_close_document, resolve_close_choice, request_save_as}`；挂 `Root::render_dialog_layer`（workbench 根视图已有）。对话框要能弹出来，窗口的根视图**必须是 `Root`**（`Root::update` 找不到会 panic）——写窗口测试时要把夹具包在 `Root::new(...)` 里并渲染对话框层（见 `view/tests.rs` 的 `dialog_harness`）。
| A10 | Actions 与快捷键：`Ctrl+S` / `Ctrl+Enter`（最小执行=执行全部）/ `Ctrl+/` / `Ctrl+F` / `Ctrl+Shift+F` | ✅ **可独立完成的部分已完成**：`commands.rs`（Action 声明）· `edit.rs`（行注释纯函数，14 项单测）· 面板 `key_context("editor")` + `track_focus` + `on_action` · 状态栏“失败原因”段 · `close_document_in_dock`（宿主执行关闭）· app 层注册 `ctrl-s` / `ctrl-/` / `ctrl-w` / `ctrl-enter` / `ctrl-shift-enter`。**5 项真按键盘的窗口测试**（写盘 / 报原因 / 注释可逆 / 只读拒绝 / 宿主关闭）。`Ctrl+F` 归 A11（内核能力，见该行）；`Ctrl+Shift+F` 待 B10 格式化 | 真机：已注册键全部生效（**禁止“只宣传未注册”**）；单测层面已用 `simulate_keystrokes` 逐键验证 |
| A11 | 查找 / 替换（优先用组件能力，缺则自建） | ✅ **已完成：用组件能力，零自建**。`Ctrl+F` → 内核 `input::Search`、`Ctrl+H` → `input::Replace`（两者都已绑在 `Input` context 且有 listener），界面由组件库 `SearchPanel` 渲染为浮层；本 crate **不注册键位、不自建查找栏**。曾按错误判断自建一套（见变更日志），已全部撤除 | 窗口测试 2 项：`Ctrl+F` / `Ctrl+H` 后焦点被内核交给查找框（“面板真开了”的可观察信号）+ 渲染不 panic。**真机**：在编辑器里按 `Ctrl+F` 能看到查找面板、`Ctrl+H` 出现替换行 |
| A12 | **工作区上下文持久化**：光标/选区/模式/连接绑定落 `workbench_context_store`（扩展表字段） | ✅ **已完成**：engine 侧 `editor_contexts` 补 `mode` 列（新库建表带、老库幂等 `ALTER`）+ `load_latest_editor_context`；editor 侧 `session.rs`（端口 + `SavedSession`）+ 面板 `session_snapshot` / `restore_session` / `save_session_now`；workbench 侧 `services/editor_session.rs`（真 store）+ `restore_last_session`（`defer_in` 读库）。**保存时机（1a）**：`Ctrl+S` 成功后与关文档时（定时节流保存属 1b）· **恢复范围（1a）**：只恢复最近更新的那一份 · **连接绑定**仍缺（待 1b，与 §12 #26 同一件事） | 集成测试 3 项（真 SQLite）：关闭→重开恢复光标与模式 · 未命名文档不写库 · 会话 id 是路径键 |
| A13 | 大文件档位：>50MB 关补全/折叠、>200MB 只读提示卡（分块加载推到 1b 后） | ✅ **已完成（“关折叠”除判据外待内核开关）**：`limits.rs`（纯函数 `tier_for_size` + I/O 层 `tier_for_path`；7 项单测含**真 200MB 稀疏文件**）· `Document.tier()` + `OpenRequest::with_tier`（≥200MB 置编辑器只读）· `persist::open_file` 按实际大小定档且**超大文件不读进内存** · 面板提示卡（`warning` 描边）。⬜ “关折叠”需要内核开关（`gpui-base` 0.6.1 未暴露）——判据 `disables_folding()` 已立，接线待内核 | 单测：边界（49.9 / 50 / 199.9 / 200 MB）· 窗口测试 3 项（超大：只读 + 内容长度 0 + 提示卡；大：可编辑 + 提示；常规：无提示卡） |
| A14 | 最小执行 + 基础结果网格（`DataTable`）：执行全部 → 结果集 → 表格 + 行数/耗时 | ✅ **已完成**：`execution.rs`（目标解析 / `QueryRunner` 端口 / `ExecChannel` 工作线程 + 轮询）· `store.rs`（`ResultStore` 结果单权威）· `view/widgets/result_grid.rs`（组件库 `DataTable` + `TableDelegate`）· 面板两个执行动作 + 结果区 + 状态栏「执行中…」· workbench `services/editor_exec.rs` 注入引擎执行器。⬜ 余：结果区分栏可拖拽 · 多结果标签 · 中断/超时（1b） | 集成测试：执行 → 结果入库 → 网格（真按键 + 假执行器，4 项）；**真机四库各一次 SELECT 已实测通过**（`editor_exec_real.rs`：mysql/postgres/sqlite/duckdb） |
| A15 | 契约与回归：`ui_contract` 扫描范围扩到 `crates/editor/src` | ✅ **已完成**：`crates/workbench/tests/ui_contract.rs` 的尺寸/颜色两份扫描表都加入 editor 的四个视图文件（`view/host.rs` · `widgets/status_bar.rs` · `widgets/result_grid.rs` · `view/highlight.rs`）；顺手清掉最后一处 `.px(rems(…))`（改为 `px_2()`）与对应常量 | `cargo test -p rds-workbench --test ui_contract -j 2` **5 项全绿** |

**1a 验收口径**：能新建/打开/编辑/保存 `.sql` 与 `.txt`；SQL 有高亮与真实状态栏；模式可判定可显式切换；`Ctrl+Enter` 能跑通一次查询并看到表格结果。

---

### Phase 1b — SQL 执行闭环（DBeaver 一档）

**目标**：把"执行"这件事做合格——目标可选、能中断、有事务、有历史、结果可操作、错误能定位；并对外开放自动执行接口（关闭 M4 遗留项）。

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| B1 | 连接绑定与切换：选择器（P/G/GP 短码 + 运行态）、自动建连（同 M4 口径）、失败可读原因 | ✅ **切片一已完成（2026-09-16）**：`connection.rs` 端口 + `Document.connection` + 执行通道带连接（`QueryRunner::run(conn, sql)`）+ 工具栏「连接 ▾」（菜单打勾 / 选后自动建连 / 失败不绑定并留原因 / 「跟随当前连接」项）+ 状态栏连接段 + workbench `services/editor_connections.rs`（列表取内存快照、建连走 `nav_runtime`）+ 「新建查询」带当前选中连接。⬜ 余：**绑定随会话持久化**（engine `editor_contexts` 加列；重启后绑定不丢）· 只读锁 ⓘ（要连接侧写策略来源，与 A6 的连接只读维度合流）· 执行位置三通道（B13）· 导航右键「在 SQL 编辑器中打开」带连接（B11） | 窗口测试：绑定→状态栏可见 · 建连失败不绑定且原因可读 · **绑定的连接真的到了执行器**（真按键路径） · 选择器只在 SQL 模式出现 |
| B2 | **执行族**：执行当前语句（用 `split.rs` 定位）/ 选区（有选区优先）/ 全部 / 批量（逐条独立） | `execution.rs` | 单测：目标解析（光标位置 × 选区 × 空文档）；集成：批量 3 语句 → 3 结果集，含 1 失败不中断 |
| B3 | 取消与超时：中断按钮 + `Ctrl+Break` 等价入口；超时按连接的 `timeout_ms` | `execution.rs` + `engine::SqlService::cancel_query` | 真机：长查询中断返回；超时返回可读错误 |
| B4 | **事务**（依 P0.2 结论）：真实状态机 + 状态栏 TX 区（开启时长 / 提交 / 回滚 / 自动提交开关） | `engine::SqlService` + `view/widgets/status_bar.rs` | 集成：begin → insert → rollback → 数据未变；跨语句事务状态一致 |
| B5 | 结果区完整化：结果集标签条（上限 5 + 淘汰 + **通道徽标 + 血缘摘要**）、工具栏（行数/耗时/连接/**筛选 + 下发开关**/**分析**/导出/刷新/复制）、截断提示、`affected_rows` 展示 | `view/widgets/result_grid.rs` + `store.rs` | 窗口/真机：滚动流畅；DML 显示影响行数；标签显示通道与血缘 |
| B5b | **分段抓取**（已确认）：数据源按“已抓取窗口 + 取下一段”实现（`known_row_count() -> Option<u64>` / `window()` / `fetch_next(limit)` / `rows(offset,limit)`），固定 **1000 行/段**起步，未知总数显示 `N+`；导出区分“仅已抓取”与“抓全量” | `store.rs`（窗口状态）+ `view/widgets/grid/`（“取下一段”入口与 `N+` 展示）+ `execution.rs`（后台抓取） | 集成：>10,000 行表 → 首段 1000 行 + 状态行 `1000+`；取下一段追加不重复；导出两种语义行数一致（测试场景 37–39） |
| B6 | 错误回填：错误位置解析（多家驱动格式）→ 诊断 + 定位 + 聚焦；无位置则只提示 | `execution.rs` + `view/` | 单测：≥8 种真实错误文本；真机：故意写错列名可定位 |
| B7 | **导出**：CSV / JSON / INSERT / Parquet / XLSX（后者经 DuckDB 临时表） | `execution.rs` / `persist.rs` | 集成：5 种格式落盘可回读（CSV/JSON 断言内容） |
| B8 | **历史面板**：右 Dock `History` 实装（列表 / 搜索 / 重放 / 删除 / 清空），字段真实 | `view/history.rs` + `engine::history_store` | 集成：执行 3 次 → 历史 3 条且耗时>0；失败也留痕 |
| B9 | **补全**：上下文判定 + 排序 + 元数据缓存（TTL 30s）+ 降级（关键字/函数 + 说明） | `completion.rs` + `database::MetadataService` | 单测：上下文判定与排序；真机：`FROM` 后出表、`t.` 后出列 |
| B10 | 格式化 / 转译 / 执行计划接线（用 P0.3 的格式化实现；EXPLAIN 按方言生成） | `commands.rs` + `engine::sql::SqlEngine` | 真机：格式化结果可往返解析；EXPLAIN 在 4 类库均返回。**硬约束（已实测，§12 #19）**：转译**必须**先按 `sql/split.rs` 切分再逐条转译——`transpile` 对脚本会**静默丢弃**第二条及以后的语句；并先写“脚本不得丢语句”的回归；执行计划的**权威来源是源库 EXPLAIN**（本地 `plan` 仅作降级，DDL 不支持） |
| B11 | 对外接口：`open_sql(conn_id, sql)` / `execute_all(...)`（供 M4「在 SQL 编辑器中打开 / 查看数据」自动执行） | ✅ **已完成（2026-09-16）**：`Shared::request_query(QueryRequest{conn_id, sql, run})`（私有字段 + 方法对，与 `open_file_request` 同口径：生产端拿不到 `Window`）· 宿主 `WorkbenchView::open_query_document`（**复用条件很窄**：未命名 + 内容空 + 同一绑定；否则新建；带 SQL 时**追加**不覆盖；`run = true` 则调 `EditorHostPanel::run_all`）· 导航四处改调（在 SQL 编辑器中打开 / 查看数据 / 生成 SQL / 拖拽；前两者的 `SidebarEvent` 变体已删） | 窗口测试：`run_all` 与 `Ctrl+Shift+Enter` 同路且空文档不打扰执行器 · `ui_contract` 7 项 · `dialog_host_layer` 4 项全绿 |
| B12 | 移除遗留：删除 `EditorPanel` 的 SQL 区块与内联执行闭包；连接详情卡/导航树按架构 §3.3 迁出 | ✅ **SQL 区块已删（2026-09-16）**：旧 `panels/editor.rs` 的 Textarea + 内联执行闭包 + 结果表 + 历史列表 + `use_duckdb_fed` 门控（约 250 行）全删，连同其状态与 `apply_nav_drag`/`insert_sql`/`clear_sql`；**SQL 编辑器只剩 `crates/editor`**；M1 未保存草稿拦截改看“未命名编辑器文档”（`EditorShared`，不再经 `Shared` 镜像字段）。⬜ 余：连接详情卡 / 导航树本体按 §3.3 迁出（属 P2 视图下沉）· **旧路径的“项目只读模式拒绝执行”需要在新执行入口补回**（见下方余项） | `cargo check --workspace --all-targets` 零告警；`Shared` 字段 27 → 23（`editor_dirty`/`editor_sql`/`editor_clear`/`result_epoch` 四个退出历史） |
| B13 | **执行通道**：工具栏「执行位置」指示器（源库 / 本地加速 / 联邦）+ `channel_status(conn_id) -> Result<(), Reason>` 可用性判定 + 不可用原因 + 通道随文档持久化 + 切通道失效提示 | `crates/editor/src/channel.rs`（新）+ `view/sql_mode.rs` + `connection::secret` / `engine::duckdb::federation` | 集成：三通道各跑一次；未开加速的连接下拉置灰并给原因；写语句在加速通道被拒；切通道后旧结果标灰 |
| B14 | **筛选下发源库**：筛选框 + `▢ 下发源库` 开关 → `re_execute_with_filter` → **新结果集**（lineage = 原结果 + 条件 + 通道） | `execution.rs` + `engine::services::execution_service` | 集成：本地筛选不重查；下发后新结果集且原结果保留；含 `LIMIT` 的原查询给出提示（架构 §12 #15） |
| B15 | **DuckDB 分析入口**：结果集「分析 ▾」→ `execute_duckdb_analysis` → 新结果集（lineage = 临时表 + 分析 SQL）；支持“桥接当前可见行” | `execution.rs` + `engine::services::execution_service` | 集成：对结果集做 COUNT / GROUP BY 各一次，产出新结果集；桥接过滤可用 |
| B16 | **接住 Dock 标签关闭入口**（A9 尾巴）：让所有关闭路径都不静默丢改动；顺带把工具栏按原型 §2.2 分层 | ✅ **主体已完成（2026-09-16）**：`closable(cx) = !is_dirty()`——组件库的 TabGroup 没有每标签 ✕，`⋯` 菜单的 `Dock.Close` 按**当前标签**的该判据显隐，因此脏文档那里没有关闭项（静默丢与“点了没反应”两种坏结果都避开了）；关闭脏文档的唯一入口是 `Ctrl+W` → `request_close_document`（三态）· **新建入口**：Quick Open 三条命令 → `new_editor_document(mode)` · **工具栏**：模式指示器 + SQL 模式的 **执行 ▾**（`DropdownButton` 分体按钮；菜单项 = 当前语句 / 选区 / 全部，纯函数 `statement_target` / `target_for_menu`）。⬜ 余：格式化 / 历史 / ⋯更多 / 执行位置 / 连接随 B10 / B8 / B13 / B1 落地（**未实现就不放**）· 自绘标签条（能在 ✕ 上弹确认）仍属可选后续 | 窗口测试：脏文档 `closable == false` 且脏点仍在、保存后恢复可关 · 工具栏按模式分层（文本 / 分析不放执行）· 单测：菜单三项目标各自独立 |

**1b 验收口径**：达到"合格 SQL 编辑器"——执行族齐备、可中断、有事务与历史、结果区可过滤可导出、错误可定位、补全可用。

---

### Phase 1c — 分析模式骨架（Cell / Output / Session）

**目标**：`.rdsnote` 笔记可选连接、可编辑单元、可执行 SQL 单元并看到内联输出；Python/Rust 只留扩展位。

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| C1 | 模型：`NoteBook` / `Cell` / `Output` / `run_seq` / stale 判定（纯函数） | `crates/editor/src/notebook.rs` | 单测：插入/删除/移动后 id 稳定；stale 规则（改源/会话重启/依赖变化） |
| C2 | `Session` trait + `SqlSession`：连接 + DuckDB 分析命名空间 + `CREATE TEMP TABLE` 作为变量 | `session.rs` | 集成:临时表创建后，后续单元可 `SELECT * FROM t` |
| C3 | 笔记视图：笔记头（会话选择器 / 全部运行 / 重启会话 / 清空输出）+ 单元卡片（类型/执行/⋮/折叠/stale）+ 插入点 | `view/notebook_view.rs` | 窗口测试：3 单元渲染、执行态切换、插入与排序 |
| C4 | 内联输出：`Table`（预览 + 展开）/ `Text` / `Error`（含定位） | `view/widgets/output_block.rs` | 窗口/真机：三种输出可见；"展开"切到结果面板（同一结果集） |
| C5 | 键盘：`Shift+Enter` / `Ctrl+Enter` / `Alt+Enter` / `Ctrl+Shift+Enter` / `Esc` 单元焦点模型 | `commands.rs` + 视图 | 真机：全键位可用（与 `editor-notebook` context 匹配） |
| C6 | `.rdsnote` 读写：单元 + 元数据 +（可选）输出引用；与项目版本链对接留接口 | `persist.rs` | 集成：写→读往返一致；输出引用可失效并提示 |
| C7 | 模式转换：SQL → 分析（整篇单单元，含确认）/ 分析 → SQL（拼接脚本） | `mode.rs` | 窗口测试：两条转换路径 + 确认分支 |
| C8 | Markdown 单元：渲染态 / 编辑态切换 | `view/notebook_view.rs` | 真机：双击进编辑、失焦渲染 |
| C9 | **分析沉淀**：结果区的 DuckDB 分析可一键沉淀为分析单元（单元源 = 分析 SQL，输出 = 结果集，临时表 = 会话变量） | `notebook.rs` + `view/notebook_view.rs` + `execution.rs` | 窗口/真机：从结果区发起分析 → 生成单元 → 下游单元可引用该临时表 |

**1c 验收口径**：能创建 `.rdsnote`、选连接、写多个 SQL/Markdown 单元、`Shift+Enter` 依次执行并有内联输出、重启会话后全部标 stale、保存后重开内容与结构一致。

---

### Phase 2 — 后续（本次不排期）

| 主题 | 内容 | 前置 |
| --- | --- | --- |
| Python 内核 | sidecar（复用 `plugin/src/sidecar`）+ `ipykernel` + Jupyter 消息协议；单元 `PY` | 1c 的 `Session` 接口固化 |
| Rust 内核 | WASM 沙箱（wasmtime/extism，无外部工具链）或单元级 `cargo script`；需进度/取消/缓存 | 同上 |
| 变量桥 | 会话间以 **Arrow** 交换（SQL 临时表 ↔ DataFrame ↔ Rust） | Python/Rust 内核就位 |
| 富输出 | `Chart`（`component::chart`）/ `Insight`（M8）/ 输出导出 | 1c 输出注册表 |
| 互操作 | 导入/导出 `.ipynb`（nbformat 4.5） | 1c 模型稳定 |
| 其他 | 参数绑定（prepared statement）、拖拽插入限定名、`minicatalogs`、变量浏览器、使用手册 | — |

---

## 3. 测试场景清单

**Phase 0（后端）**
1. 切分：单语句 / 尾分号 / 多语句 / 空语句 / 字符串内分号 / 行注释 / 块注释 / `$$` 块 / 反引号标识符 / 未闭合引号（不 panic，报错）
2. 历史：成功记录耗时>0、失败也写并含原因、行数/影响行数正确、`db_type` 非 `unknown`
3. 执行结果结构：SELECT 无 `affected_rows`、DML 有

**Phase 1a（编辑体验）**
4. 模式判定：`.sql`→SQL、`.rdsnote`→分析、`.md`/`.txt`/未知→文本、入口语义覆盖、显式记忆优先
5. 切换：文本↔SQL 内容不变；SQL→文本 显示"结果标灰"提示；切换确认可取消
6. 脏状态：输入即脏、撤销回原值干净、保存干净、标签脏点与状态栏同步
7. 关闭：三态确认（保存/丢弃/取消）+ 保存失败二次确认
8. 外部修改：磁盘被改后保存 → 三态（重载/覆盖/取消）
9. 只读：编辑器只读不可输入；连接只读可输可执行但写语句被拦
10. 持久化：关闭重开恢复光标/选区/模式/连接绑定
11. 大文件：>50MB 关补全；>200MB 只读卡
12. 快捷键：`Ctrl+S` `Ctrl+Enter` `Ctrl+/` `Ctrl+F` `Ctrl+Shift+F` 全部生效

**Phase 1b（执行闭环）**
13. 目标解析：无选区执行当前语句（光标在中间语句）/ 有选区执行选区 / 全部 / 批量
14. 批量：3 条语句、第 2 条失败 → 3 个结果集且失败标红，其余成功
15. 取消 / 超时：长查询中断；超时给可读错误
16. 事务：begin→insert→rollback 数据不变；commit 生效；状态栏状态与时长正确
17. 结果区：10 万行滚动流畅；快速过滤生效；上限 5 淘汰最旧；截断提示
18. 导出：CSV/JSON/INSERT/Parquet/XLSX 落盘可回读
19. 历史：执行 3 次（含 1 次失败）→ 面板 3 条且字段真实
20. 补全：`FROM` 后表、`t.` 后列、无连接降级为关键字+函数
21. 错误定位：故意写错列名 → 定位到行；无位置信息时不误报第 1 行

**Phase 1c（分析模式）**
22. 单元生命周期：插入/删除/移动后 id 稳定、输出不错位
23. 会话变量：单元 1 建临时表 → 单元 2 可查；重启会话后全 stale
24. stale：改单元 1 源 → 自身与下游标 stale；重跑后清除
25. 持久化：`.rdsnote` 写读往返一致；输出引用失效时给出提示
26. 转换：SQL→分析（整篇单单元）内容不丢；分析→SQL 拼接脚本

**跨阶段**
27. 主题：明暗切换下编辑区/结果区/单元卡/输出区对比度（`theme-preview.html` 与交互稿为基准）
28. 契约：`ui_contract` 扫描 `crates/editor/src` 零裸色值、零裸 `px(`
29. 无项目：工作台覆盖选择器时编辑器不额外报错

**通道与结果血缘（2026-09-15 修订新增）**
30. 通道门控：未开本地加速的连接 → 执行位置中加速项置灰且行尾有原因；联邦无源时给“注册新源…”入口
31. 通道切换：源库 → 加速后旧结果集标灰 + 顶部提示；分析模式下全部单元标 `stale`
32. 通道写限制：加速通道下 `INSERT` 被拒并提示切回源库（不得落本地副本）
33. 筛选本地 vs 下发：本地筛选不重查（无新结果集）；下发产生新结果集且原结果保留
34. 重查包裹语义：原查询含 `ORDER BY` 时结果仍有序；含 `LIMIT` 时给出“将去掉 LIMIT”提示
35. DuckDB 分析：对结果集 COUNT / GROUP BY → 新结果集（血缘 = 临时表 + 分析 SQL），原结果保留
36. 分析沉淀（1c）：结果区分析 → 生成单元 → 下游单元引用该临时表；重启会话后 `stale`

**分段抓取（2026-09-15 新增）**
37. 首段与未知总数：>10,000 行的表 → 初始抓 1000 行、状态行显示 `1000+`；“取下一段”追加且不重复、不跳行
38. 导出语义：区分“仅导出已抓取”与“抓全量后导出”——提示文案与实际落盘行数一致
39. 抓取中取消：取下一段过程中取消 → 已抓取部分仍可用于筛选/排序/导出（不丢已取数据）

---

## 4. 风险与对策

| 风险 | 影响 | 对策 |
| --- | --- | --- |
| Dock 标签无法满足关闭拦截（P0.1） | 1a 需自绘标签条（+约 300 行，且与 Dock 视觉重复） | Phase 0 先验证；不成立则采用"自绘标签条 + Dock 仅作容器"并记录到架构 §12 |
| 事务会话亲和不可得（P0.2） | 事务功能只能"看起来能用" | 验证先行；不可得则引入 per-session 独占连接并评估对池化的影响 |
| 格式化选型失败（P0.3） | 1b 的"格式化"缺口 | 退路：只做轻量缩进与关键字大写（不做完整重排），并在 UI 明示能力边界 |
| SQL grammar 与主题角色不匹配（P0.8） | 高亮角色缺失或颜色异常 | 缺角色复用最接近标准 token（零裸色）；必要时补产品语义 token |
| editor 独立 crate 引入循环依赖 | 编译结构被破坏 | 依赖方向固定为 `workbench → editor → engine/database/shared`；editor 不得依赖 workbench（架构 §3.1） |
| 与 M4/M5 的接口漂移（`editor_set` / 草稿箱打开文件） | 收编时改动面扩大 | 契约先行（架构 §3.4），1b 的 B11/B12 一次性切换并在文档中更新映射 |
| 1c 的 `Session` 抽象过早固化 | 第二期 Python 内核接入时返工 | 抽象只保留"执行/中断/状态/命名空间"四件事；Python 相关字段（ZMQ/内核握手）**不进第一版接口** |
| 输出引用（DuckDB 临时表）生命周期 | `.rdsnote` 重开后输出失效 | 输出块显式区分"可展开的真实数据"与"已失效（重跑即可）"；默认不持久化全量输出 |
| 现有 `EditorPanel` 迁出牽连 M3/M4 宿主职责 | 迁移期功能回归 | 按架构 §3.3 逐项迁出，先迁编辑器部分；每步跑 `-p rds-workbench` 全测 |
| **通道可用性判定无来源**（架构 §12 #13） | UI 给不出准确的“加速不可用原因” | 1b 前定义 `channel_status`；暂无来源时按“未注册 Secret”降级提示，并在文案里说明是推断而非事实 |
| **重查包裹语义**（含 `ORDER BY` / `LIMIT`） | 重查结果与原查询不一致，用户困惑 | 定规则 + 显式提示（测试场景 34）；必要时提供“重查后保留 LIMIT”开关 |
| **ATTACH 快照新鲜度** | 用户拿旧快照做决策 | 结果集徽标 + 状态栏“快照” + “重新 ATTACH”入口（原型 §5.7） |
| 通道/血缘增加结果集数量 | 上限 5 与淘汰策略被更快触发 | 淘汰时提示来源；后续评估“按通道分组淘汰” |
| 编译时间与体积增长（新增 grammar 等依赖） | 开发体验 | P0.9 记录基线；新依赖一律进 workspace 唯一入口表并评估必要性 |

---

## 5. 实现位置映射（设计决策 → 代码）

| 设计决策 | 代码文件 |
| --- | --- |
| 文档/模式/只读/能力表 | `crates/editor/src/{model.rs, mode.rs}` |
| 唯一执行入口 + 后台任务 | `crates/editor/src/execution.rs`（模式参考 `crates/workbench/src/services/nav_jobs.rs`） |
| 语句切分（词法级） | `crates/editor/src/split.rs`（替换 `engine/src/services/sql_parser_service.rs::split_sql`） |
| 结果单权威 + 上限淘汰 | `crates/editor/src/store.rs` |
| 补全 | `crates/editor/src/completion.rs` + `crates/database/src/metadata_service.rs` |
| 会话与单元 | `crates/editor/src/{session.rs, notebook.rs}` |
| 持久化（工作区上下文 / `.rdsnote`） | `crates/editor/src/persist.rs` + `engine/src/persistence/workbench_context_store.rs` |
| 三模式视图 | `crates/editor/src/view/{host,text_mode,sql_mode,notebook_view}.rs` |
| Action 与快捷键 | `crates/editor/src/commands.rs` + `crates/app/src/main.rs` |
| 宿装配（中央区） | `crates/workbench/src/view.rs::init_workspace` |
| 历史字段 / 事务 / affected_rows | `engine/src/{persistence/history_store.rs, services/sql_service.rs}` |
| 格式化实现 | `engine/src/sql/formatter.rs` |
| SQL 高亮注册与主题角色 | `crates/editor/src/view/` + `assets/themes/{rds-theme.json, product-tokens.json}` |
| 尺寸常量 | `crates/editor/src/ui.rs` |
| 执行通道与门控 | `crates/editor/src/channel.rs` + `connection/src/secret.rs` + `engine/src/duckdb/federation.rs` |
| 筛选下发 / DuckDB 分析 | `crates/editor/src/execution.rs` + `engine/src/services/execution_service.rs` |
| 结果血缘与通道徽标 | `crates/editor/src/store.rs`（结果集元数据） |
| 网格渲染层（将来可提炼到 shared） | `crates/editor/src/view/widgets/grid/`（`GridDataSource` / `GridEditSink`，架构 §3.6） |
| 契约测试范围 | `crates/workbench/tests/ui_contract.rs` |
| 交互稿 | `docs/architecture/editor/editor-prototype.html` |

---

## 6. 验证方式

```sh
# 模块回归（editor 服务层 + 引擎 SQL 层）
cargo test -p rds-editor -p rds-engine --lib -j 2

# 契约（零裸色 / 零裸 px，扫描 editor + workbench）
cargo test -p rds-workbench --test ui_contract -j 2

# 全工作区编译守卫
cargo check --workspace --all-targets -j 2

# P0.2 / P0.2b 真机探针（事务会话亲和 + 驱动级事务；凭据只从环境变量读；共 8 例）
# PowerShell：
#   $env:RDS_TEST_MYSQL_URL="mysql://root:root@192.168.3.138:3306/mysql"
#   $env:RDS_TEST_PG_URL="postgres://postgres:postgresql@192.168.3.138:5432/postgres"
#   $env:RDS_TEST_SQLITE_PATH="<SQLite 文件的**副本**>"   # 原文件若是 Fossil 仓库（T.fossil）会被 Fossil 锁；探针也别直连用户仓库
#   $env:RDS_TEST_DUCKDB_PATH="D:\data\123"               # 这里是**文件**不是目录
cargo test -p rds-engine --test transaction_affinity -j 2 -- --nocapture --test-threads=1

# P0.10 能力探针（离线；报告式 + 三条不变量断言；共 10 例）
cargo test -p rds-engine --test sqlglot_capabilities -j 2 -- --nocapture --test-threads=1
```

**P0.2 / P0.2b / P0.2c 实跑结论（2026-09-15，四类库真实端点）**

| 库 | 顺序亲和（`BEGIN` + 临时表 + `ROLLBACK`） | 并发亲和（两条并发语句） | 驱动级事务（`execute_in_transaction`） |
| --- | --- | --- | --- |
| PostgreSQL | ✅ 成立 + 回滚生效 | ❌ 池另开物理连接（另一侧 `relation does not exist`） | ✅ 可用 |
| SQLite | ✅ 成立 + 回滚生效 | ✅ 两侧均可见 | ✅ 可用 |
| DuckDB | ✅ 成立 + 回滚生效 | ✅ 两侧均可见 | ✅ 可用 |
| MySQL | ✅ 成立 + 回滚生效（`BEGIN` 已改走文本协议） | ❌ 池另开物理连接（另一侧 `1146 Table doesn't exist`） | ✅ 可用 |

> 结论：**顺序执行可以靠池碰运气，并发不行**——MySQL/PG 的池在忙时会另开物理连接，
> 因此 1b 的事务 / 会话**必须 per-session 独占连接**（不能依赖池的顺序巧合）。
> SQLite / DuckDB 是单句柄语义，不受影响。MySQL 的显式 `BEGIN` 已改走**文本协议**
> （`sqlx::raw_sql`，`native/mysql.rs::needs_text_protocol`），不再报 `1295`。

- 每阶段结束：上列命令全绿 + §3 对应场景真机走通（`cargo run -p rds-app -j 2`）。
- 真机回归矩阵（1b 起每轮至少一遍）：MySQL / PostgreSQL / SQLite / DuckDB × 执行族（当前语句 / 选区 / 全部 / 批量）× 只读 / 可写 × 明暗主题。
- **P0.9 基线（2026-09-15）**：本模块**未新增任何依赖**（sqlglot-rust 已在依赖里，**不引 tree-sitter**）→ 依赖增量 = 0（D12 / P0.3 / P0.8 的选型目标已达成）。参考值：`cargo build -p rds-app -j 2` 增量编译（insight + workbench + app）**2m37s**；`target/debug/rds-app.exe` = **143,705,088 字节（≈137 MiB，debug + DuckDB 静态链接）**。
- 阶段完成后回填 §0 进度表，并同步 `editor-architecture.md` §12 与 `editor-prototype-design.md`（若交互有调整）。

---

## 7. 待确认（开工前的阻塞清单）

见 `editor-architecture.md` §13（10 项）。其中**必须先回答**的 5 项：

| # | 问题 | 建议 |
| --- | --- | --- |
| 1 | 多文档标签：Dock 自带 / 自绘 | Dock 自带（待 P0.1 验证） |
| 2 | SQL → 分析 转换粒度 | 整篇单单元 + 二次"按语句拆分" |
| 3 | 批量执行语义 | 逐条独立、失败不中断 |
| 4 | 分析模式首期语言 | SQL + Markdown（Python/Rust 第二期） |
| 5 | 是否独立 `crates/editor` | 是（§3.2 判定四条成立） |
| 6 | 加速 / 联邦通道遇写语句 | 直接拒绝 / 允许但仅落本地副本 | 直接拒绝 + 提示切回源库 |
| 7 | 执行通道默认值 | 总是源库 / 记忆上次 | 记忆上次（随文档绑定） |
| 8 | DuckDB 分析结果落点 | 新结果集 / 覆盖 / 直接作为分析单元 | 1b 新结果集（血缘）；1c 可沉淀为分析单元 |
| 9 | 标签关闭语义 | ① 草稿兜底（关闭即落草稿 + 最近关闭恢复）/ ② 自定义标签条后弹三态确认 | **①**（关闭入口单一、无绕过）；② 作为体验升级后置（API 已确认可行） |
