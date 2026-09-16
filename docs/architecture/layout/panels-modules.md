# 工作台面板模块划分与跨模块耦合

状态：已落地（2026-09-16）。拆分前 `crates/workbench/src/panels.rs` = 10190 行；拆分为 6 个模块后
对外路径 `crate::panels::X` 保持不变（`mod.rs` 重导），组件层、宿主与 7 个集成测试零改动。

## 1. 为什么拆、为什么这样切

判定依据是**职责与生命周期的归属**，不是行数：

| 块 | 归属 | 行数 |
| --- | --- | --- |
| `panels/mod.rs` | 面板协议与装配（`SidebarPanel` 字段 + `new` + `BasePanel` / `ComponentPanel` / `Render` 分发 + 资源/插件占位） | 253 |
| `panels/shared.rs` | `Shared`（面板/组件/宿主三方共用状态） + `ProjectActionRequest` | 298 |
| `panels/nav.rs` | M4 数据源导航：类型与纯函数、键盘方法、树渲染、分组/标签/拖拽、连接增删改 | 4851 |
| `panels/scratchpad_panel.rs` | M5 草稿箱：模板/排序/压平、编辑与剪贴板/回收站、行渲染、内容搜索 | 3504 |
| `panels/editor.rs` | 中央编辑区：SQL 草稿区 + 结果区 + 属性面板宿主 + 连接对话框宿主 | 1252 |
| `panels/right.rs` | `RightSidebarPanel`（洞察 / Mock / 历史） | 194 |

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
| `property_target` | nav 写 → editor 读 | 导航双击对象 → 属性面板请求 |
| `selected` / `connections` | nav / editor | 选中连接与列表 |
| `driver_catalog` | nav / editor | 驱动 id → 类型/显示名 |
| `open_edit` / `new_connection_request` | nav 写 → editor 消费 | 打开连接对话框请求 |
| `editor_set` / `new_connection_request` | nav 写 → mod/editor 消费 | SQL 注入请求 |
| `scratchpad_search` | scratchpad 写 → editor 读 | 内容搜索结果落中央区 |
| `open_file_request` | scratchpad 写 → shared → 宿主 | 在编辑器中打开文件 |
| `project_ui` | editor / scratchpad | 项目管理 UI 状态 |

共 12 个字段跨模块。它们**没有类型级保护**：任何模块都能写任何字段，改一处语义不会有编译错误，
只能靠约定与 review。

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
| P1 | **render 内同步读盘（真）** | `editor.rs` 连接详情卡：`render` 内按需调 `load_navigator_tree(&global_analysis_db_path())` 读分析库文件（缓存键 = 当前连接） | `panels/editor.rs`「Round 25：数据库导航区」块 |
| P2 | render 内写状态 + 入队后台任务 | `render_connection_row` / `render_nav_node` → `ensure_nav_loaded`（改 `database_nav` + `enqueue_load` + 起轮询，I/O 在工作线程）；`render_property_panel` → `enqueue_properties`；`render_scratchpad` → `request_scratchpad_load` | nav.rs、editor.rs、scratchpad_panel.rs |
| P3 | 自绘控件（技术债） | `tool_btn` / 树展开字符 / 文本按钮 | nav.rs、scratchpad_panel.rs、editor.rs |

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
**字段级去向与迁移顺序见 `panels-coupling-plan.md`**。

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
`panels/nav.rs` → `crates/database`、`panels/scratchpad_panel.rs` → `crates/scratchpad` 的搬迁成本已降到最低。

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
