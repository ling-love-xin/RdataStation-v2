# 项目管理模块 · 开发方案（P0 + Phase A/B/C）

> 状态：**已实现（Phase A/B 主体 + Phase C1/C2）**（2026-09-11，`cargo check --workspace --all-targets` 零告警；engine 221 / project 23（含 9 项窗口测试）/ workbench 14 测试全绿） · 关联文件：`project-prototype-design.md`（原型）、`project-prototype.html`（可交互原型）、`project-view-architecture.md`（视图架构与测试）
> 前置：v1 行为蓝本 `v1/backend/src/commands/project_commands.rs`；v2 后端已迁移（`crates/project`：`store.rs` / `models.rs`；P0 会话 `workbench/src/services/project_session.rs`）
> 复用 `connection-dev-plan.md` / `scratchpad-dev-plan.md` 的推进方式：Phase 划分 → 文件落点 → 验收 → 测试场景 → 风险
> **范围**：项目**增删改查与生命周期**。**提升/引用（promote/snapshot）不在本模块**（另立设计，见原型 §12）。

### 已确认决策（2026-09-11）

| # | 决策 |
| --- | --- |
| 1 | 选择器**覆盖中央区**，保留五段外壳（活动栏/状态栏可见） |
| 2 | 切换/关闭项目时**拦截未保存草稿**（保存 / 不保存 / 取消） |
| 3 | **不支持多窗口打开同一个项目**（`.RSmeta/project.lock`），被锁项目给**只读打开 / 仍要打开**逃生口 |
| 4 | **提升/引用不属于项目管理**，本期不做（占位保留或移除） |
| 5 | 删除磁盘数据需**输入项目名**二次确认 |
| 6 | 存储结构树提供**「打开 `.RSmeta`」入口**（排障用） |
| 7 | 「从示例项目开始」（C3）**首版必需** |
| 8 | 固定（pin）与排序方式**持久化**（`project_info.is_pinned` + `settings` 的 `projects.sort_mode`） |
| 9 | 软删提供**「已移除项目」找回入口** |
| 10 | 新建 / 打开 / 重定位的「位置」= **系统目录选择器**（`prompt_for_paths`，仅目录 / 单选）+ 目标路径预览（原型 §6「位置/名称」） |
| 11 | **远程项目仅模型层预留**（`ProjectPath::Remote`，DuckLake），本期不做——对话框内明示范围 |

## 0. 进度记录（最近在前）

### 2026-09-11 — 位置字段：系统目录选择器 + 目标预览 + 远程范围说明

对齐原型 §6（「位置 = 目录选择 + 预览 `位置/名称`」）：

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 目录选择 | 新建 / 打开目录 / 重新定位三个对话框的路径行改为「输入框 + 浏览…」：`App::prompt_for_paths`（`files:false, directories:true, multiple:false`）→ `Window::spawn` 等待 oneshot → `AsyncWindowContext::update` 取 `&mut Window` 回填 `InputState::set_value`（取消保持原值） | `crates/project/src/ui.rs`（`pick_directory` / `directory_row`） |
| 目标预览 | 新建对话框实时显示 `目标：位置/名称`（原型要求），并加一行范围说明「当前版本仅支持本地目录；DuckLake 远程项目待后续版本」 | 同上（`target_preview`） |
| 窗口测试 | +2 项：选中目录回填（并断言选择器选项为「仅目录 / 单选」）、取消保持原值 | `crates/project/src/ui/tests.rs` |

> 远程项目现状：`models.rs` 有 `ProjectPath::Remote { url, project_id }`（DuckLake 预留）且 `store.rs` 注释提及，但 `CreateProjectInput` / `ProjectStore::create` / `inspect_target` 均只有本地路径；v1 蓝本里有远程命令分支，v2 明确划在 M1 范围外（§8「不做」）。

### 2026-09-11 — A3：项目视图迁入 `project` crate（视图与 model/service 同 crate）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| A3 | 项目视图整体迁入 `crates/project/src/ui.rs`：`ProjectUiHost` 承载全部宿主依赖（状态句柄 / `ProjectUiNotifier` / `ProjectEditorBridge` / 排序偏好回调 / 打开后刷新）；`OpenProject` 取代 workbench 的 `ProjectSession`；示例项目落点改为 crate 内推导（`engine::migration::get_system_dir`）；清掉只写不读的 `PickerState::sort_initialized` | `crates/project/src/{ui.rs,lib.rs}`、`Cargo.toml`（+ `gpui-kit`） |
| A3 | workbench 侧新增宿主桥（`ViewNotifier` / `EditorBridge` / `build_host`），`view.rs` / 标题栏 Popover 均通过 host 调用项目视图；`Shared.project` 类型改 `project::ui::OpenProject`；`WorkbenchView::new(cx)` 在构造期装配 host（无 render 内 I/O） | `crates/workbench/src/components/project_host.rs`、`panels.rs`、`view.rs`、`services/project_session.rs`、`app/src/main.rs` |
| 窗口测试 | 11 项（见 `project-view-architecture.md` §5）：选择器 / 设置 / 菜单渲染、新建对话框空名校验、浏览目录回填与取消、删除确认名称匹配、未保存拦截（关闭 / 打开）、排序持久化回调、锁占用逃生口、卡片三分支构造、删除确认对话框 | `crates/project/src/ui/tests.rs` |
| 目录选择 | `pick_directory` / `directory_row` / `target_preview`：系统目录选择器 + 目标路径预览 + 远程范围说明 | `crates/project/src/ui.rs` |
| 验证 | `cargo check --workspace --all-targets` 零告警；`cargo test --workspace -j 2` 34 个目标全绿（**451 通过 / 0 失败**）；`cargo build -p rds-app -j 2` 通过 | — |

### 2026-09-11 — B5 收尾 + B8 + B1（语义对话框 / 卡片菜单 / 尺寸相对化）

| 项 | 内容 | 落点 |
| --- | --- | --- |
| B5 | 6 个自绘模态层改为语义组件：新建 / 打开目录 / 删除确认（输入项目名）/ 重新定位用 `Dialog`，锁占用 / 未保存拦截用 `AlertDialog`；删除 `render_overlays` / `DialogKind` / `dialog_shell` 与 `view.rs` 调用点。校验错误改存 `ProjectUiState::dialog_error`（builder 每帧重读），提交失败保持打开，成功由回调 `window.close_dialog` 显式关闭（Enter 与按钮同路径；不用 `Dialog::on_ok` 返回值，以免被拦截动作另开对话框时 pop 错栈顶） | `project_ui.rs`、`view.rs` |
| B5 | 未保存拦截拆为「准备」（另存草稿 + 清空编辑区）与「推进」（执行被拦截的打开 / 关闭）两段 | 同上 |
| B8 | 项目卡片改「一个可见主操作（打开）+ 更多菜单」：固定/取消固定、恢复、在资源管理器中显示、移出列表、重新定位…、删除数据… 收进 `DropdownMenu`，破坏性命令用分隔线隔离；固定状态改用 `Star` 图标常显 | 同上 |
| B1 | 工作台视图层尺寸改相对 scale：`px(...)` → gpui rem helper（`w_12()` / `h_px()` / `rems(n)` 等）；只能收 `Pixels` 的 API（Dock 起步宽度、`Dialog::w`、`Form::label_width`）改 `cx.theme().font_size * N`，随界面缩放 | `view.rs`、`panels.rs`、`connection_dialog.rs`、`project_ui.rs`、`settings_view.rs` |
| 验证 | `cargo check --workspace --all-targets` 零告警；`cargo test --workspace -j 2` 34 个目标全绿（442 通过 / 0 失败）；`cargo build -p rds-app -j 2` 通过 | — |

### 2026-09-11 — Phase A/B 主体 + Phase C1/C2 实现

**已完成**

| 项 | 内容 | 落点 |
| --- | --- | --- |
| B1 | 全局库迁移 `019_add_project_ui_state.sql`：`is_pinned`/`pinned_at`/`removed_at` + 索引；查询改造（过滤已移除、固定置顶）+ `set_project_pinned`/`soft_delete_project`/`restore_project`/`list_removed_projects`；`save_project_info` 改 upsert 以保留固定/软删列 | `crates/engine/migrations/global/019_*.sql`、`global_db.rs` |
| C1 | 项目锁：OS 文件锁（进程退出自动释放）+ `project.lock.owner` 展示占用者；`probe`/`release`；单测 2 项 | `crates/project/src/lock.rs` |
| A1/B* | `ProjectService`：最近/全部/已移除列表、创建、打开、只读打开、关闭、重命名、固定、归档、软删、恢复、硬删、移出、校验、目标探测；单测 2 项 | `crates/workbench/src/services/project_service.rs` |
| A2–A4 | 选择器（最近/全部/已移除 Tab、搜索过滤、排序、固定、失效态、缺失驱动、锁徽标、空态/错误态）、标题栏项目槽可点、项目菜单 | `crates/workbench/src/components/project_ui.rs`、`view.rs` |
| A3/B3 | 新建项目对话框、打开目录对话框、删除确认（输入项目名）、只读/仍要逃生口、未保存拦截；示例项目入口 | 同上 |
| B2/B3/B4 | 项目设置（概览 / 重命名 / 存储 `.RSmeta` 大小 + 打开 `.RSmeta` / 依赖自检 / 刷新） | 同上 |
| A7 | Action：`SwitchProject`（Ctrl+Shift+P）/ `CloseProject`（Ctrl+Shift+W） | `commands.rs`、`view.rs`、`app/src/main.rs` |
| A6 | 编辑区脏状态（与最近一次执行不同）→ 切换/关闭拦截 | `panels.rs`（`EditorPanel`） |

**二次迭代补全（2026-09-11）**

| 项 | 内容 | 落点 |
| --- | --- | --- |
| 排序持久化 | 新增 `settings.projects.sort_mode`（`Projects` 节 + `SettingsService::project_sort_mode/set_project_sort_mode`）；选择器首帧从设置读取，切换即持久化 | `crates/settings/src/{model,lib}.rs`、`project_ui.rs` |
| 未保存「保存」分支 | 拦截对话框新增「保存并继续」——把编辑区 SQL 另存为项目草稿 `未命名.sql`；`editor_clear_requested` 信号驱动 `EditorPanel` 清空 | `project_ui.rs`、`panels.rs`（`Shared` / `EditorPanel`） |
| 只读强制禁写 | 只读打开时禁用：SQL 执行、草稿新建/重命名/删除、重命名、归档、创建版本 | `panels.rs`、`project_ui.rs` |
| 重新定位（U5） | 失效卡片「重新定位」：校验新目录 `.RSmeta` → 改写名册 path 与 `project.json` | `project_service::relocate`、`project_ui` |
| 版本链列表（B8） | 新迁移 `project_meta/018_project_versions.sql` + 项目设置「版本」分节（列表 + 创建快照） | `crates/engine/migrations/project_meta/018_*.sql`、`project_service::{list_versions,create_version}`、`project_ui` |
| 集成测试目录 | 新增 `crates/project/tests/project_store.rs`（3 项，经公开 API） | `crates/project/tests/` |

**剩余有意取舍**：「删除磁盘数据」只删 `.RSmeta`（保留用户文件），非整目录删除。

### 2026-09-11 — edition 2024 升级 + 项目菜单组件化

| 项 | 内容 |
| --- | --- |
| edition 2024 | 根 `Cargo.toml` `edition = "2024"`；修复 2024 破坏性变更：`connector.rs` 隐式借用模式下的 `ref`/解引用模式（2 处）、`mock` crate 保留字 `gen` 标识符重命名、`view.rs` `.then(|| ...)` 闭包对 `cx` 的双重独占借用（改为显式 `if`）、`render_*_activity_bar` 返回 `Div` 以避开 RPIT 生命周期捕获 |
| 工程配置 | `.cargo/config.toml`：`[env] RUST_MIN_STACK`（workspace 全量 codegen 需更大 rustc 线程栈）、`test-all` 别名固定 `-j 2`（并行链接 DuckDB 静态库会耗尽内存，导致 rustc `STATUS_STACK_BUFFER_OVERRUN` / `link.exe` LNK1102） |
| B5（部分） | 项目菜单改用 `Popover` + ghost `Button` 触发（`render_menu_content` 只提供内容）；删除自绘绝对定位弹层 |
| 验证 | `cargo test --workspace -j 2`：34 个测试目标全绿、**442 项通过 / 0 失败**；`cargo build -p rds-app -j 2` 通过 |

### 2026-09-11 — 符合性对齐（GPUI-kit coding-guides）

对照官方《编码指南》/《设计指南》自查后启动整改：

| 项 | 处置 |
| --- | --- |
| 架构归属：service 未与 model 同 crate | ✅ `project_service.rs` 从 workbench 迁入 `crates/project/src/service.rs`；`.RSmeta` 常量统一到 `store::RS_META_DIR_NAME` |
| 仓库硬约束未允许 `feature → gpui-kit` | ✅ 更新 `.agents/skills/rds-architecture` 与 `overview.md`（按指南：feature 可依赖 UI 基础设施） |
| render 内副作用 | ✅ 选择器列表加载 + 排序偏好初始化 → `WorkbenchView::new`（构造期）；EditorPanel 脏状态 → `InputEvent::Change` 订阅；清空编辑区 → 宿主命令回调（事件上下文） |
| 本地化 label 作 ElementId | ✅ `picker-tab-{key}` / `menu-{key}`（domain 键，不用中文 label） |
| 公开字段 struct 未 `#[non_exhaustive]` | ✅ `ProjectSummary` / `CreateProjectInput`（+ `new`/`with_*` builder）/ `OpenedProject`（+ `into_parts`）/ `ProjectVersionRow` / `PickerState` / `ProjectUiState` / `ProjectInputs` |
| 视图 / 对话框仍在 workbench | ✅ 已迁（批次 A3）：项目视图整体落入 `crates/project/src/ui.rs`，宿主依赖由 `ProjectUiHost`（状态句柄 / `ProjectUiNotifier` / `ProjectEditorBridge` / 排序偏好回调 / 打开后刷新）注入；`settings` 直接依赖已断开 |
| 自绘 menu / dialog（无 focus trap / Escape / 方向键） | ✅ 项目菜单改 `Popover` + `Button` 触发；新建 / 打开目录 / 删除确认 / 重定位改 `Dialog`，锁占用 / 未保存拦截改 `AlertDialog`（焦点陷阱、Escape、遮罩关闭、footer 布局由组件负责） |
| 直接 `px(...)` | ✅ 工作台视图层（`view.rs` / `panels.rs` / `project_ui.rs` / `connection_dialog.rs` / `settings_view.rs`）改 rem helper 或 `cx.theme().font_size * N`；仅 1px hairline 保留 `h_px()`（指南允许的 physical boundary 例外） |
| （工程）edition 2024 | ✅ 全仓 `edition = "2024"`（`.cargo/config.toml` 加 `RUST_MIN_STACK`；`test-all` 别名固定 `-j 2`） |
| 卡片 hover-only 操作（design-guides） | ✅ 一个可见主操作（打开）+ 次要命令进 `DropdownMenu`（破坏性命令分隔）；固定状态用图标常显 |

### 2026-09-11 — 方案定稿（含 schema 迁移）

尚无代码改动。范围 = 项目 CRUD / 生命周期。相较初稿新增：**全局库迁移 `019_add_project_ui_state.sql`**（固定/软删字段）与名册查询改造。建议按 Phase A 起步。

## 1. 现状结论（盘点摘要）

| 层 | 状态 |
| --- | --- |
| 域模型（`crates/project/src/models.rs`：`Project` / `ProjectInfo` / `ProjectPath` / `ProjectStatus` / `ProjectConfig` / `Version` / `Versioned<T>`） | ✅ 已迁移 |
| 存储（`crates/project/src/store.rs`：`ProjectStore::create/load/…`、`ProjectManager`、`check_project_missing_drivers`） | ✅ 已迁移（`mod tests` 现有 6 项） |
| 全局库项目 CRUD（`global_db.rs`：`get_recent_projects` / `get_all_projects` / `save_project_info(_smart)` / `open_project(_by_path)` / `get_project_by_path` / `update_project_info` / `update_project_last_opened` / `delete_project` / `delete_project_info`） | ✅ 已迁移，但 `project_info` **无固定/软删字段**（需迁移 019） |
| P0 当前项目会话（`project_session.rs` + `Shared::project`） | ✅ 只读解析，无创建/切换动作 |
| 项目视图（`project_view` / `promote_dialog` / `snapshot_dialog` / `commands` / `model`） | ⚠️ 空占位 |
| 标题栏项目槽 | ⚠️ 仅显示名称，无交互 |
| 项目锁 / 未保存拦截 / 选择器 / 设置视图 | ❌ 未实现 |
| workbench 依赖 `project` | ❌ 未接线 |

**关键缺口**：① 生命周期服务编排；② 选择器（Read/搜索/排序/固定/已移除）；③ 新建 + 示例项目（Create）；④ 项目设置（Update/Delete/自检）；⑤ 项目锁 + 未保存拦截（健壮性）；⑥ 名册持久化（迁移 019）。
**可复用**：底层存储与全局库 CRUD 基本齐备；本期以「服务编排 + 视图 + 接线 + 一条名册迁移」为主。

## 2. CRUD → 任务映射

| CRUD | 原型条目 | 任务 |
| --- | --- | --- |
| **Create** | C1 新建空项目 / C2 打开目录 / C3 示例项目（必需） | A2、A3、B3 |
| **Read** | R1 会话解析 / R2 最近 / R3 全部 / R4 搜索 / R5 排序 / R6 打开 / R7 元信息 / R8 固定 / R9 已移除找回 | A1、A2、A4、A5、B2、B5、B6 |
| **Update** | U1 重命名 / U2 描述 / U3 默认连接 / U4 归档 / U5 重定位 / U6 固定 | A4、B1、B2、B4、B6、C2 |
| **Delete** | D1 软删（置 `removed_at`）/ D2 硬删（磁盘，输入名确认）/ D3 前置校验 | A4、B1、B4、B7 |

## 3. 阶段划分

### P0 — 接线与共享状态（前置）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| P0.1 | 依赖接线：`crates/workbench/Cargo.toml` 增 `project.workspace = true`（根别名已存在） | `Cargo.toml` | `workbench → project → engine/shared` 无环 |
| P0.2 | `Shared` 增：`project_service`、`project_epoch: Rc<Cell<u64>>`（切换后自增，触发面板重载）、`editor_dirty: Rc<Cell<bool>>`（未保存拦截信号） | `crates/workbench/src/panels.rs` | 编译通过；切换后各面板观察到 epoch 变化 |
| P0.3 | `project_session::resolve()` 收敛为调用 `ProjectService`（单一全局库入口，去重复 `Runtime::new`） | `workbench/src/services/project_session.rs` | 行为等价，零告警 |

### Phase A — 生命周期核心闭环（Create / Read / 打开 / 切换 / 关闭 + 未保存拦截）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| A1 | `ProjectService`：`list_recent`（含路径有效性 + 缺失驱动）、`list_all`、`open_by_path`、`open_by_id`、`create`、`close` | `crates/workbench/src/services/project_service.rs`（新） | 集成测试覆盖创建/open/close/list |
| A2 | 选择器视图：视图 Tab（最近/全部/已移除占位）、卡片（状态徽标/路径/元信息/缺失驱动/失效态）、↑↓ 选择、行内操作、空态；**覆盖中央区** | `crates/project/src/project_picker_view.rs`（新，`Entity`） | 无项目态渲染选择器；空态/失效态正确 |
| A3 | 新建项目对话框（名称/位置/描述/默认连接/初始化选项）+ 三类冲突分支 + 进度态 | `crates/project/src/create_project_dialog.rs`（新） | 校验与文案符合原型 §6 |
| A4 | 标题栏项目槽 + 项目菜单（切换/设置/重命名/显示位置/归档/关闭），缺失驱动警告行 | `crates/workbench/src/view.rs` | 各菜单项触发对应动作 |
| A5 | 切换/关闭联动：写会话、`project_epoch += 1`、刷新标题栏/草稿箱/连接/导航；关闭回选择器 | `panels.rs` / `view.rs` / `workspace_loader.rs` | 切换后无跨项目串数据 |
| A6 | 未保存拦截：切换/关闭前读 `editor_dirty` → 「保存 / 不保存 / 取消」；取消中止 | `workbench/src/view.rs` + `crates/project/src/`（确认框） | 脏缓冲区下取消切换生效 |
| A7 | 命令与快捷键：`NewProject` / `OpenProjectPicker` / `SwitchProject` / `CloseProject` / `RenameProject` | `crates/project/src/commands.rs` + `workbench/src/commands.rs` | Quick Open 可检索 |
| A8 | 收尾：`cargo check --workspace` 零告警；无新增 `unwrap/expect`；架构红线复核 | 全仓 | 全绿 |

### Phase B — 项目 CRUD 补全与设置（名册迁移 / Update / Delete / 找回）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| B1 | **全局库迁移 `019_add_project_ui_state.sql`**：`project_info` 增 `is_pinned INTEGER DEFAULT 0` / `pinned_at` / `removed_at` + 索引；改造 `PROJECT_SELECT_COLUMNS` 与各查询，`get_recent_projects` / `get_all_projects` 过滤 `removed_at IS NULL`；新增 `set_project_pinned` / `list_removed_projects` / `restore_project` | `crates/engine/migrations/global/019_add_project_ui_state.sql`（新）+ `global_db.rs` | 迁移幂等；旧数据默认未固定/未移除；查询结果正确 |
| B2 | 项目设置视图（概览/存储/默认项/版本/依赖自检/危险区）；概览重命名（U1，仅显示名）、描述（U2）→ `update_project_info` + 同步 `project.json`/`project.db` → 标题栏即时更新 | `crates/project/src/project_settings_view.rs`（新）+ `ProjectStore` | 三处一致；标题栏同步 |
| B3 | 存储分节：根路径（复制/显示位置）+ `.RSmeta` 只读结构树与大小 + **「打开 `.RSmeta`」**；**示例项目（C3）**内置资产 + 创建入口 | 设置视图 + 选择器 + 资源 | 大小与磁盘一致；示例可创建并打开 |
| B4 | 默认项（U3 `save_config`）、归档/取消归档（U4）、危险区：软删（D1 置 `removed_at`）、硬删（D2 输入项目名 → 磁盘删除，先释放锁；D3 前置校验） | 设置视图 + `ProjectService` | 软/硬删可区分；硬删需输入名；先关当前项目 |
| B5 | 「已移除」找回（R9）：视图 + 恢复（清 `removed_at`）；失效路径「重新定位」（U5） | 选择器 + `ProjectService` | 找回后回到名册；重定位后可打开 |
| B6 | 查询增强：搜索过滤（R4）、排序（R5，**持久化**到 `settings` 的 `projects.sort_mode`）、状态筛选、固定/取消固定（R8, U6）并置顶 | 选择器 + `crates/settings` | 重启后固定与排序保持 |
| B7 | 依赖自检（`check_project_missing_drivers` 展示 + 复制指引 / 忽略）；版本分节只读列表 | 设置视图 | 可见、可复现 |

### Phase C — 健壮性与边界

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| C1 | **项目锁**：打开时在 `.RSmeta/project.lock` 写入 pid/时间；关闭/退出释放；陈旧锁按 pid 存活回收 | `crates/project/src/store.rs` + `ProjectService` | 双实例打开同项目被拒；崩溃后陈旧锁可回收 |
| C2 | **锁逃生口**：被占用时弹「只读打开 / 仍要打开 / 取消」；只读模式（状态栏标注、禁写操作）；「仍要打开」仅对陈旧锁生效 | 选择器 + `ProjectService` + 状态栏 | 三支路行为符合原型 §8 |
| C3 | 收尾：`cargo check --workspace` 零告警；测试全绿；文档回填 | 全仓 | 全绿 |

> **不做**：提升/引用（promote/snapshot）、移动/另存项目目录、DuckLake 远程项目。

## 4. 测试场景清单

1. **创建（空目录 / 示例项目）**：新建对话框与示例入口 → `.RSmeta/{project.db,analytics.duckdb,project.json,project.lock}` → 名册出现 → 标题栏更新
2. **创建冲突**：非空非项目 → 阻止；已是项目 → 提示打开；非法字符 → 内联错误、按钮禁用
3. **打开/导入目录**：含 `.RSmeta` 直接打开；空目录询问创建；结构不全拦截；`.rdata-station` 旧结构尝试迁移
4. **查询**：最近/全部/已移除切换、搜索过滤（名称/路径）、排序（最近/名称/创建）、状态筛选、无匹配态
5. **固定与排序持久化**：固定若干项 → 重启 → 固定项仍置顶、排序方式保持
6. **软删 / 找回 / 硬删**：软删后隐藏且「已移除」可见 → 恢复回到名册；硬删输入项目名后可执行，展示将删路径；删除当前项目先走拦截+关闭
7. **更新**：重命名只改显示名（磁盘目录不变，名册/.RSmeta/标题栏一致）；描述；默认连接；归档/取消归档；路径失效重定位
8. **读取边界**：路径失效置灰仅可移出 / 重定位；`missing_drivers` 在卡片与设置一致
9. **切换项目**：A → B，草稿箱/连接/导航换源、编辑区重置、无 A 残留
10. **未保存拦截**：脏缓冲区下切换/关闭 → 保存/不保存/取消三支路正确
11. **项目锁**：双实例打开同项目被拒 → 只读打开可进入且标注；陈旧锁可回收；「仍要打开」接管成功
12. **迁移**：旧库升级到 019 后，既有项目默认未固定/未移除，名册显示不变（幂等）
13. **主题**：明暗切换核对选择器卡片/徽标/对话框/菜单/危险区（`theme-preview.html` 为基准）

**自动化覆盖（2026-09-11）**：上述场景中无需真实项目库即可验证的部分，已落为
`crates/project/src/ui/tests.rs` 的 11 项 GPUI headless 窗口测试——场景 1/2（新建对话框开得起来、
空名校验、浏览目录回填 / 取消保持、目标预览构造）、4（选择器 / 设置 / 菜单渲染、排序回调）、
6（删除确认名称匹配）、10（未保存拦截：关闭与打开两向）、11（锁占用逃生口对话框）、
13 的渲染面（卡片三种分支构造）。其余场景（真实建库、双实例、迁移）仍走
`project_store.rs` 集成测试与 §7 手动清单。

## 5. 风险与对策

| 风险 | 对策 |
| --- | --- |
| **新增名册迁移（019）与既有 M1 测试** | 迁移只加列、不改列；`ALTER TABLE ADD COLUMN` 有默认值，旧行安全；迁移幂等（按版本号） |
| 软删改为 `removed_at` 后，旧 `delete_project_info` 语义变化 | 保留 `delete_project_info`（硬删登记）另用途；软删走新方法；两者测试区分 |
| 固定/排序偏好落点分散（列 vs 设置） | 固定 = per-project 列；排序方式 = 全局 `settings`，职责分明 |
| 改动底层 M1 存储波及既有 6 项测试 | 不改表结构（只加列）；新增测试独立文件 |
| 切换项目未保存草稿丢失 | A6 统一拦截；`editor_dirty` 单一信号源 |
| `project_session` 与 `ProjectService` 路径分裂 | P0.3 合并单一入口；常量统一 `RS_META_DIR_NAME` |
| 事件式刷新遗漏面板 → 跨项目串数据 | `project_epoch` 单一信号源，各面板订阅重载（场景 9 覆盖） |
| 项目锁失效（崩溃残留）阻塞打开 | 锁文件记 pid/时间；存活检测 + 陈旧回收 + 逃生口（场景 11） |
| 软删/硬删误操作 | 入口分离 + 硬删输入项目名 + 展示绝对路径；软删可找回（场景 6） |
| 全局库未初始化时选择器裸报错 | 结构化状态 + 「未就绪 + 重试」 |
| 视图置于 `project` 却依赖 workbench | 视图为 `Entity`，由 `workbench`/`app` 组合；`project` 只依赖 `engine/shared` |

## 6. 实现位置映射（设计决策 → 代码文件）

| 设计决策 | 代码文件 |
| --- | --- |
| 项目服务编排（列表 / 创建 / 打开 / 关闭 / 更新 / 删除 / 找回 / 版本） | `crates/project/src/service.rs`（feature crate 内，符合 GPUI-kit 指南「model/service/view 同 crate」） |
| 会话解析（env → 最近 → 空态） | `crates/workbench/src/services/project_session.rs`（返回 `project::ui::OpenProject`） |
| 项目选择器 / 菜单 / 对话框 / 设置（UI） | `crates/project/src/ui.rs`（视图与 model / service 同 crate；选择器与设置由宿主渲染为受控 overlay，菜单为 `Popover` + 卡片 `DropdownMenu`，对话框为 `Dialog` / `AlertDialog`） |
| 项目视图 ↔ 宿主桥（重绘 / 编辑区 / 排序偏好 / 打开后刷新） | `crates/workbench/src/components/project_host.rs` |
| 项目锁（OS 文件锁 + 占用者信息） | `crates/project/src/lock.rs`（`.RSmeta/project.lock` / `project.lock.owner`） |
| 名册迁移（固定/软删字段） | `crates/engine/migrations/global/019_add_project_ui_state.sql`（新） |
| 全局库项目 CRUD / 固定 / 已移除 | `crates/engine/src/persistence/global_db.rs` |
| 排序方式偏好 | `crates/settings`（`projects.sort_mode`） |
| 命令 / Action | `crates/project/src/commands.rs` + `crates/workbench/src/commands.rs` |
| 标题栏项目槽 + 项目菜单 | `crates/workbench/src/view.rs`（`render_title_bar`；菜单内容由 `project::ui::render_menu_content` 提供） |
| 会话共享与刷新信号 | `crates/workbench/src/panels.rs`（`Shared`；`project` 字段类型为 `project::ui::OpenProject`） |
| 存储 / 模型（不改表，只加列） | `crates/project/src/store.rs` / `models.rs` |
| 示例项目 | 运行时生成到 `{data_dir}/RdataStation/samples/示例项目`（含 `welcome.sql`） |
| 依赖接线 | 根 `Cargo.toml`、`crates/workbench/Cargo.toml` |
| 主题 token（如需补） | `assets/themes/rds-theme.json` |

> `crates/project/src/promote_dialog.rs` / `snapshot_dialog.rs`：**不在本期范围**，保持占位或移除（评审时定）。

## 7. 验证方式

- 每阶段：`cargo check -p rds-project -p rds-workbench -p rds-app --all-targets` 零告警 + 对应测试
  （`crates/project/tests/` 集成测试 + `crates/project/src/ui/tests.rs` 窗口测试）
- 迁移：`cargo test -p rds-engine`（迁移套件）+ 手工核对旧库升级
- UI：`cargo run -p rds-app` 手动走通 §4 清单（先用 `RDS_PROJECT_PATH` 验证有项目态，再清空验证选择器）
- 主题：明暗切换核对 token（`docs/architecture/theme/theme-preview.html` 为基准）
- 阶段完成后回填本文件「状态」与原型文档同步
