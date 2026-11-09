# 工作台面板模块划分与跨模块耦合

状态：已落地（2026-09-16）。拆分前 `crates/workbench/src/panels.rs` = 10190 行；拆分为 6 个模块后
对外路径 `crate::panels::X` 保持不变（`mod.rs` 重导），组件层、宿主与 7 个集成测试零改动。

## 1. 为什么拆、为什么这样切

判定依据是**职责与生命周期的归属**，不是行数：

| 块 | 归属 | 行数 |
| --- | --- | --- |
| `panels/mod.rs` | 面板协议与装配（`SidebarPanel` 字段 + `new` + `BasePanel` / `ComponentPanel` / `Render` 分发 + 资源/插件占位） | 253 |
| `panels/shared.rs` | `Shared`（面板/组件/宿主三方共用状态） + `ProjectActionRequest` | 298 |
| ~~`panels/nav.rs`~~ → `crates/database/src/nav_view.rs`（模块根）+ `nav_view/` 7 子模块 | M4 数据源导航：类型与纯函数、键盘方法、树渲染、分组/标签/拖拽、连接增删改（**已下沉** 2026-09-16；**已拆分子模块** 2026-09-20） | 642（根）/ 7918（总） |
| `panels/scratchpad_panel.rs` | M5 草稿箱：模板/排序/压平、编辑与剪贴板/回收站、行渲染、内容搜索 | 3504 |
| `panels/editor.rs` | 中央编辑区：SQL 草稿区 + 结果区 + 连接对话框宿主（~~属性面板宿主~~ ✅ 已抽为 `property_panel.rs`，2026-10-09） | 722 |
| `panels/property_panel.rs` | 属性面板 `PropertyHostPanel`（中央 Dock 独立 tab，与编辑器同级别）：属性/数据两页签 + 洞察 + General 双/三栏 + 子项类别列表；状态在 `property` crate | 1060 |
| `panels/right.rs` | `RightSidebarPanel`（洞察 / Mock / 历史） | 194 |

拆分**之后新增**的面板模块：`panels/resources.rs`（M6 资产库装配：构造期建实体 + 宿主端口 + 轮询回填；**视图与状态在 `analytics_resource` crate 内**，本模块只转发渲染）。

切分接缝来自**原有代码的自然边界**：`impl SidebarPanel` 内导航方法与草稿箱方法本来就是两段连续代码，
`Render for SidebarPanel` 的 4 分支分发点即面板模式边界。

**没有拆 crate。** 按 `settings-crate-design.md` §1 判定四条：导航/草稿箱的状态仍挂在 `Shared`（不独立于窗口与
项目生命周期）、使用方只有 workbench 一个（不满足"使用方 ≥2"）。真正需要独立 crate 的是**视图下沉**
（见 §5 P2），不是现在。

## 2. 边界规则（新增代码必须遵守）

1. **面板协议实现在 `mod.rs`**：`impl BasePanel / ComponentPanel / Render for SidebarPanel` 集中一处，
   子模块只提供 `render_*` 方法与业务方法。新增面板模式 → 在 `mod.rs` 加分发分支 + 新模块文件。
2. **可见性**：Rust 的隐私作用域是"定义模块 + 其后代"，因此子模块**可读父模块私有字段**（`SidebarPanel`
   的 30+ 字段无需改 `pub(crate)`）；反向（父模块读子模块）必须 `pub(super)`。目前只有 3 处需要：
   `DatabaseNavView` 的 4 个 facet 字段、`PropertyState`、`ScratchpadSearchView` 的 3 个字段，以及
   `render_database_nav` / `render_scratchpad` / `ensure_scratchpad_pump` 三个方法。
3. **跨模块引用显式列举**：`use super::nav::{NavDragPayload, PropertyState, nav_draft_append};`，
   **禁止 `use super::*` / `use gpui_kit::*` 出现在测试模块**（会把 gpui 的 `test` 宏带入作用域 →
   `recursion limit reached while expanding #[test]`）。
4. **`scratchpad_panel` 的 `_panel` 后缀是硬约束**：若模块名为 `scratchpad`，会在 `panels` 内遮蔽
   `scratchpad` crate（`use scratchpad::…` 与 `Shared::scratchpad_watch` 的字段类型都指向该 crate）。
5. **新增面板特性落点清单**：新模块文件 → `mod.rs` 声明 + `pub use` 重导 → `ui_contract.rs` 两份契约清单
   登记（契约 2c 会强制，漏登记直接测试失败）→ 本文档与相关架构文档的映射表补行。

## 3. 跨模块耦合的真实通道 = `Shared`

拆分只解决了"文件级"影响，**模块级相互影响仍全部经由 `Shared`**。审计（2026-09-16）：

| 字段 | 使用模块 | 语义 |
| --- | --- | --- |
| `notice` | editor / nav / scratchpad | 三方共写同一条提示文案 |
| `selected` / `connections` | nav / editor | 选中连接与列表 |
| `driver_catalog` | nav / editor | 驱动 id → 类型/显示名 |
| `open_edit` / `new_connection_request` | nav → editor | ✅ 已端口化（`EditorBridge::edit_connection` / `new_connection`，S2a）；字段已删 |
| ~~`editor_set`~~ | — | ✅ **已删（B11/B12，2026-09-16）**：SQL 注入改走 `Shared::request_query(QueryRequest)`（私有字段 + 方法对），消费点在宿主 render；`EditorBridge::insert_sql` 一并删除 |
| `scratchpad_search` | scratchpad 写 → editor 读 | 内容搜索结果落中央区 |
| `scratchpad_file_info` | scratchpad 写 → 宿主 overlay 读 | 文件属性弹窗（右键 → 属性；信息少走弹窗不走面板，2026-10-10） |
| `open_file_request` | scratchpad 写 → shared → 宿主 | 在编辑器中打开文件（`QueryRequest` 同形，已收为私有字段 + 方法对） |
| `project_ui` | editor / scratchpad | 项目管理 UI 状态 |
| `layout_locked` / `prev_left_sidebar_width` / `prev_right_sidebar_width` | 设置页 → 宿主 | 布局锁定 + 边栏宽度同步（`settings` 写、宿主 render 读；M-layout，2026-10-28） |

共 15 个字段跨模块。它们**没有类型级保护**：任何模块都能写任何字段，改一处语义不会有编译错误，
只能靠约定与 review。

> 另一类不在此列的字段是**宿主 ↔ 特性 crate 视图的实体弱句柄**（`mock_panel` / `mock_details` / `insight_panel` / `resources_panel` / `property_panel` / `plugin_manager`）：
> 它们只在 workbench 内部读写——特性 crate 拿到的是注入的宿主端口，根本不认识 `Shared`——用途是
> "宿主转发渲染 / 取面板选中项"（例：M6 的存档详情读左栏选中项；状态栏读 Mock 面板的运行中任务——“面板被切走时任务还在跑”）。
> `open_property` / `open_plugin_manager` / `open_mock_detail` 同列：中央 tab「已建则聚焦、未建则新建」的创建/聚焦通道。
> 注意 `plugin_manager` 是**强持有**（关 tab 后再次点击复用同一实体，搜索/筛选状态不丢），
> `property_panel` 是弱句柄（关闭即释放，下次重建）。
> 新增同类句柄只需更新 `ui_contract.rs` 的白名单，不必改本表。
>
> 同类还有 M6 的五个对话框流转字段（`version_dialog` / `repair_dialog` / `trash_dialog` / `tag_dialog` / `conflict_dialog`）：它们不跨模块——轮询写、
> 侧栏 render 消费与清会话，crate 拿到的只是对话框状态的克隆——但它们是
> “取数在后台、开窗在 render”的**两段式**例子：轮询里没有 `Window`，所以只置 `pending`。
>
> M7 的 `pending_temp_cleanup` 也在此列：切项目时拿不到内存库连接锁（出口任务不可取消且整段持锁）
> 就置位，等任务收尾那一拍由 `mock_host::take_job_done` 重试——两边都在 workbench 内部，不跨模块。
> M6 的 `reference_stale` 同属宿主内部资源槽位：按 `resource_id` 记 reference 快照失效集合，
> 懒校验写入、接受成功或切项目清除，不进索引状态统计，也不跨模块。

## 4. 与 gpui-kit 约束的差距（审计）

已合规（有测试固化）：

- 零裸色值 / 零裸 `px(...)`：`ui_contract.rs` 契约 2a / 2b 全绿；
- 状态与同步：可交互状态放 `Shared`，`render` 从 Shared 读取（`apply_left_mode` / `apply_right_mode`
  为宿主权威同步点）；回调只更新状态 + `cx.notify()`；
- 组件选型：对话框 / Tab / Switch / Input / Button / 菜单均已用 `gpui_kit::component`。

未合规（按严重度）：

| 级别 | 问题 | 实测 | 位置 |
| --- | --- | --- | --- |
| P1 | **事件路径同步 I/O 阻塞 UI 线程** | 18 处 `block_on`（nav.rs 4 + scratchpad_panel.rs 14，含各自 `Runtime::new`） | `commit_copy_connection` / `share_connection_to_project` / `unshare_connection_from_project` / `delete_connection`；草稿箱 `commit_scratchpad_edit` / `delete_scratchpad_selection` / `undo_scratchpad_delete` / `restore_scratchpad_trash` / `remove_scratchpad_reference` / `apply_scratchpad_relink` / `open_scratchpad_location` |
| P1 | ~~**render 内同步读盘（真）**~~ ✅ 已修（2026-09-16） | `editor.rs` 连接详情卡：`load_navigator_tree` 改为 `ensure_analysis_tree` 后台加载 + 回填，render 只读缓存（原先选中联邦连接那一帧阻塞） | `panels/editor.rs` |
| P2 | render 内写状态 + 入队后台任务 | `nav_view/rows.rs` 的 `render_connection_row` / `render_nav_node` → `ensure_nav_loaded`（改 `database_nav` + `enqueue_load` + 起轮询，I/O 在工作线程）；`property_panel.rs` 的 `render_property_panel` → `ensure_props_pump`（I/O 在工作线程）；`render_scratchpad` → `request_scratchpad_load` | nav_view/rows.rs、property_panel.rs、scratchpad_panel.rs |
| P3 | 自绘控件（技术债） | ~~树展开字符~~ ✅ 已换图标（2026-09-20）· ~~文本按钮~~ 多数已换 `Button`；**仍欠**：行尾 `+`/`✎` 与面板头「新建分组」仍是自绘 `div` + `on_click`（已补齐 20/24px 命中区，但仍无焦点态） | nav_view/chrome.rs、rows.rs、editors.rs、scratchpad_panel.rs、editor.rs |

> 订正：`gpui-kit-dev` skill 原「已知反例」将 `ensure_nav_*` / `render_property_panel` / `render_history_placeholder` / `load_scratchpad` 列为"render 内同步读盘"——实测：`ensure_nav_loaded` 只入队（I/O 在工作线程）、`right.rs::render_history_placeholder` 是纯占位、`load_scratchpad` 函数已不存在。**真正的 render 内同步读盘是 `panels/editor.rs` 连接详情卡的 `load_navigator_tree`（上表 P1）**，此前未被记录。
> 另：`settings::SettingsService::*` 访问器走 `cx.global::<Settings>()`，render 内调用不构成 I/O 违规。

## 5. 架构调整建议（按优先级）

### P0 — 把 `Shared` 按职责分组，特性状态归还特性

现状：`Shared` 同时承载宿主级状态（活动面板 / 三模式 / Quick Open / 选中 / 连接列表 / 项目会话 /
宿主重绘桥 / 各方弱句柄）与**特性内状态**（`driver_catalog` /
`scratchpad_search` / `property_target` / `open_edit` / `editor_set` / `new_connection_request` /
`focus_nav_search` / `open_file_request`）。

建议：`Shared` 只留宿主级状态 + 三类**桥**（`NavBridge` / `EditorBridge` / `ScratchpadBridge`），
特性内状态收回各自模块（`DatabaseNavView` 已在做，其余溢出字段同理）。
**字段级去向与迁移顺序见 `panels-coupling-plan.md`**（S1、S2a 已落地：
`nav_cache_epoch` / `result_epoch` 失效戳 + `EditorBridge` 两个对话框请求）。

### P0 — 请求标记 → 端口/命令，而不是共享可变字段

现状：跨模块协作靠"写字段 + 对方 render 时 `take()`"（`editor_set` / `open_edit` /
`new_connection_request` / `property_target` / `open_file_request` / `scratchpad_pump_request`）。
写方与读方没有类型契约，谁都能写，语义靠注释。

建议：改为**端口对象**（持有 `Rc<dyn Fn(...)>` 的强类型接口）或**事件**，与本仓库既有方向一致：

- 正例 1：`crates/editor/src/shared.rs` 的 `EditorShared` + `services/{editor_exec, editor_files, editor_connections, editor_session}.rs` 的端口接线；
- 正例 2：`SidebarEvent` 已把"导航 → 宿主"的 6 种协作做成枚举事件；
- 依据：`rds-architecture` skill「Feature 间不得直接依赖对方的 view；协作走 command / event / shared service」。

收益：跨模块写入点从"任意字段"收敛为"少数方法"，改语义时编译期可查，且未来视图下沉到特性 crate 时
（`nav.rs` → `crates/database`）只需搬端口，不必搬 `Shared`。

### P1 — 事件路径同步 I/O → 后台任务 + 轮询回填

草稿箱已经给出正例：`services/scratchpad_jobs.rs`（工作线程 + `enqueue_*` / `drain_*` + 面板 render
期间回填，见 `scratchpad-architecture.md` K1/K1b）。把 nav.rs 的 4 处连接操作与 scratchpad_panel.rs 的
14 处元数据级操作迁到同一形态，是**纯机械改造**，且能直接消除点击卡顿。

### P2 — 视图下沉到特性 crate（与本次拆分同源）

`mock` / `insight` 已是正例（crate 自带 view + `WeakEntity` 句柄，workbench 只装配）；`crates/database/src/database_view.rs`
与 `commands.rs` 至今是 3 行空占位，说明这是既定路线。现在模块边界 = 未来的 crate 边界，
`crates/database/src/nav_view.rs`（原 `panels/nav.rs`，**已下沉**）、`panels/scratchpad_panel.rs` → `crates/scratchpad` 的搬迁成本已降到最低。

**前置条件**：先做 P0（否则搬 `nav.rs` 会连带搬走半个 `Shared`）。

### 明确不改的

- **不拆 workbench crate**：它是装配宿主（Dock / 标题栏 / 活动栏 / Quick Open），拆开只会把装配逻辑摊到多处。
- **不改 Dock 三模式与 `Shared` 作为面板状态载体**：`gpui-kit-dev` skill 认可"可交互状态放 `Shared`"，
  本次拆分也未破坏该口径；P0 只是把 `Shared` 收窄到宿主级。
- **不做全量 `Entity` 化重构**：把面板状态改成 `Entity<T>` 是另一个量级的改动，收益（更强的生命周期
  与通知语义）不足以支撑当前成本；P0 的桥 + 端口已经消除主要耦合。

## 6. 实现位置映射表

| 设计决策 | 实现位置 |
| --- | --- |
| 模块划分与边界规则 | `crates/workbench/src/panels/{mod,shared,nav,scratchpad_panel,editor,right}.rs` |
| 对外路径稳定性（重导） | `crates/workbench/src/panels/mod.rs`（`pub use`） |
| 契约 2c（新模块必须登记扫描） | `crates/workbench/tests/ui_contract.rs` |
| 跨模块耦合通道（待收敛） | `crates/workbench/src/panels/shared.rs` 的 12 个字段（§3 表） |
| 同步 I/O 待迁点（P1） | `nav.rs` 4 处 / `scratchpad_panel.rs` 14 处 `block_on` |
| 后台任务正例（迁移目标形态） | `crates/workbench/src/services/scratchpad_jobs.rs`、`nav_jobs.rs` |
| M6 资产库面板装配（crate 内视图 + 宿主端口 + 后台取数） | `panels/resources.rs`、`components/resource_host.rs`、`services/resource_jobs.rs` |
| 工作台 tab 业务数据概览卡片（后台取数 + 纯读渲染） | `panels/overview.rs`（渲染）、`services/overview.rs`（取数）、`panels/editor.rs`（宿主字段与任务） |
| 插件管理页（中央常驻 tab，目录模块 + 复用左栏数据面） | `panels/plugin_manager/{mod,render,cards,detail,widgets,tests}.rs`；入口见 `panels/left_tabs.rs` 与 `view.rs` |

## 7. 工作台 tab 的「业务数据概览」卡片（2026-11-08）

### 7.1 口径

用户口径：「统计指的是**项目中的**连接、洞察、mock 等各个模块的**业务数据**」，呈现形态为**卡片**；
`docs/architecture/ui/ui-todo.md` 是用户升级后自测项，不在本次范围。

因此四张卡的主数值一律取**真实业务数据**，模块能力数只放副行（不拿「我们支持多少」冒充「用户用了多少」）：

| 卡 | 主数值 | 来源 | 库 / 位置 |
| --- | --- | --- | --- |
| 连接 | 连接总数 | `Shared::connections`（`ConnectionItem` 清单） | 内存 |
| 项目元数据 | 元数据表行数 | `tables` 表 `COUNT(*)` | SQLite `{项目}/.RSmeta/project.db`（**只读打开**，与主写连接并发读安全） |
| 洞察画像 | 已存报表条数 | `insight_table_reports` + `insight_schema_reports` 的 `COUNT(*)` | DuckDB `{项目}/.RSmeta/analytics.duckdb`（建表语句在 `crates/insight/src/store/body.rs:439/:503`） |
| 分析库 | 分析库表数 | `mock_generator::existing_tables_at()`（`information_schema.tables` 的 `main` schema） | 同上 DuckDB；副行带 `mock::generator_catalog::all_specs().len()`（生成器种类，代码目录） |

列数、已连接数、项目名进副行/次级信息。

### 7.2 线程模型（为什么不在 render 里查库）

`render` 是纯读路径（gpui-kit 硬约束：不做 I/O、不写 `Shared`）。取数全部在
`services::overview::load()` 里同步完成，由 `EditorPanel::spawn_overview` 放进
`cx.background_executor()` 的阻塞任务，结果经 `weak.update` 回填到面板字段 `overview`，
再 `cx.notify()` 出帧。连接清单与连接状态在**入队时**从 `Shared` 克隆一份过去
（背景线程不能碰 `Rc<RefCell<..>>`）。

触发点只有两个：`render` 入口的 `ensure_overview`（首帧 / 项目根变化时取一次，`overview_for`
比对项目根，不会每帧重取）、以及卡片右上「刷新」按钮（`reload_overview`，忽略当前快照重取）。
任务句柄存 `overview_task: Option<Task<()>>`（覆盖即取消上一枚）。

### 7.3 失败降级（不阻塞、不 panic）

- 项目未初始化 / 缺 `project.db`：项目卡显示「—」，一条原因进 `problems`；
- 分析库文件不存在（本项目还没跑过查询/洞察/mock）：洞察卡与分析库卡显示「—」+ 原因；
- 洞察画像表还没建（本项目没跑过该类洞察）：按 0 计，不算失败（`query_row` 失败即跳过该表）；
- 卡片顶部只展示前两条 `problems`（`warning` 色），不占满页面。

**风险与取舍**（必须知道的边界）：分析库取数是 `duckdb::Connection::open`（读写模式，与既有
`mock_generator::open_analysis_db` 同口径）。Windows 上项目分析库存在单句柄争用问题
（`crates/store/src/persistence/project_db.rs:379-385`、`:452-455`），若此刻主写连接正持有该库，
这里会**失败并降级为「—」**，不会阻塞 UI。要彻底消除该争用，需要走「由持有连接的模块自报计数」
或「主连接上的只读查询通道」，那是跨 crate 的改造，本次未做。

### 7.4 验证

- `cargo check -p rds-workbench --all-targets -j 1` → exit 0
- `cargo test -p rds-workbench --lib services::overview -- --test-threads=1` → **2 passed**
  （`counts_connections_without_project`：未开项目只用内存数据且 `problems` 为空；
  `reads_project_metadata_from_sqlite`：造最小 `.RSmeta/project.db`，断言表 2 / 列 3、
  分析库缺失走降级并给出可读原因）
- `cargo build -p rds-app` → exit 0；应用重启后 `stderr` 无 `panic`/`ERROR`
- **未覆盖**：卡片实际版式（列宽/换行/字号）属布局层，headless 无可断言接缝，靠人工验收；
  「点刷新确实重新取数」只经代码路径确认（同一函数被按钮与首帧共用），无点击级用例。

### 7.5 顺带修掉的编译阻塞（不属于本功能）

`crates/result_panel` 在途改动把 `RESULT_HEADER_HEIGHT` 删了（`result_panel/src/ui.rs:147`
注释称「全仓零引用」），但 `crates/editor/src/support/ui.rs:34` 的 re-export 仍引用它，
导致 `rds-editor` 编译失败（E0432）并连带阻断 `rds-workbench` / `rds-app` 构建。
本次从 re-export 列表移除该项（全仓仅此一处引用，语义与那边的删除一致），未改 `result_panel` 任何文件。

