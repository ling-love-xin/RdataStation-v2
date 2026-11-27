# 项目视图架构 · 宿主桥与窗口测试（M1 / A3）

> 状态：**已实现**（2026-09-11） · 关联：`project-prototype-design.md`（交互语义）、`project-dev-plan.md`（任务与进度）
> 适用读者：改 `crates/project/src/ui*` 或 `crates/workbench/src/components/project_host.rs` 的人

## 1. 目标与约束

GPUI-kit 编码指南要求「同一业务能力的 model / service / view / dialog 放同一 feature crate」。项目视图原先在 `crates/workbench/src/components/project_ui.rs`，与 `crates/project` 的 model / service 分离，且反向依赖 `workbench::Shared`、`workbench::WorkbenchView` 与 `settings::SettingsService`。

A3 把视图整体迁入 `crates/project/src/ui.rs`，并把「宿主能力」抽成显式契约：

| 约束 | 落地方式 |
| --- | --- |
| `project` 不依赖 `workbench` / `settings` | 全部外部依赖经 `ProjectUiHost` 注入，crate 只多一个 `gpui-kit` 依赖 |
| 宿主仍是唯一的渲染与 I/O 权威 | 视图只返回 `Div`；I/O 只发生在事件回调（对话框点击 / Action / 构造期） |
| 状态只有一份 | state / session 以 `Rc` 共享，宿主与视图看到的是同一份 |

## 2. 契约：`ProjectUiHost`

```rust
// 六个「闭包桥」字段各自的类型别名（紧邻 ProjectUiHost 定义，`pub` 可见以便宿主写签名）
pub type SortSaver = Rc<dyn Fn(ProjectSort, &mut App)>;
pub type HostAction = Rc<dyn Fn(&mut App)>;
pub type ConnectionProvider = Rc<dyn Fn(&App) -> Vec<ConnectionOption>>;
pub type BuildTagProvider = Rc<dyn Fn() -> String>;
pub type CreateLocationGetter = Rc<dyn Fn(&App) -> Option<String>>;
pub type CreateLocationSaver = Rc<dyn Fn(&str, &mut App)>;

// 项目中心的两个投递口（列表由宿主渲染在全局设置里，动作要回到主窗口执行）
pub enum HomeAction { Switch(PathBuf), CreateProject, OpenFolder, Sample, Close }
pub type HomeDispatch = Rc<dyn Fn(&ProjectUiHost, HomeAction, &mut App)>;  // 入参带 host，宿主侧不必自引用
pub type HubOpener = Rc<dyn Fn(&mut Window, &mut App)>;

pub struct ProjectUiHost {
    pub state: Rc<RefCell<ProjectUiState>>,          // 选择器 / 菜单 / 设置 / 对话框错误 / 锁 / 只读 / 版本删除确认
    pub session: Rc<RefCell<Option<OpenProject>>>,   // 当前项目（根 + 名），与宿主 Shared.project 同源
    pub notifier: Rc<dyn ProjectUiNotifier>,         // 重绘宿主视图
    pub editor: Rc<dyn ProjectEditorBridge>,         // 未保存草稿判定 / 内容 / 清空 / 清脏
    pub save_sort: SortSaver,                        // 排序偏好持久化
    pub on_opened: HostAction,                       // 打开项目后的宿主刷新
    pub on_closed: HostAction,                       // 关闭项目后的宿主清理（#21）
    pub connections: ConnectionProvider,             // 默认连接候选（U3）
    pub build_tag: Option<BuildTagProvider>,         // 当前构建标识；None = 不记 rds.build_tag
    pub last_create_location: CreateLocationGetter,  // 上次新建父目录
    pub save_create_location: CreateLocationSaver,   // 记住本次新建父目录
    pub home: Rc<RefCell<Option<HomeDispatch>>>,     // 设置「项目」页的动作投递口（None = 就地执行）
    pub on_open_hub: Rc<RefCell<Option<HubOpener>>>, // 「项目中心…」→ 打开设置窗口「项目」页
}
```

闭包字段抽成类型别名（而不是写在字段上）的原因：`ProjectUiHost` 的字段类型原本各是一条完整的 `Rc<dyn Fn…>`，clippy 的 `type_complexity` 会在每个字段报一次；抽别名后字段只出现名字，`with_*` 签名也跟着统一。构造方式不变——`Rc::new(闭包)` 交给字段时仍按字段类型做 unsized coercion，宿主侧不用改。

构造用 `ProjectUiHost::new(state, session, notifier)` + `with_editor` / `with_sort_saver` / `with_on_opened` / `with_on_closed` / `with_connections` / `with_build_tag` / `with_create_location(get, save)`（`#[non_exhaustive]`，字段不对外展开）。未注入时全部退化为 no-op（`build_tag = None`、`last_create_location` 恒 `None`、`home` / `on_open_hub` 为 `None`）。

`home` / `on_open_hub` 之所以是 `Rc<RefCell<Option<_>>>` 而非 `Option<Rc<…>>`：它们在 `init_workspace` 里注入，此时 `ProjectUiHost` 已经 clone 给了多个持有者（选择器面板、设置面板、workbench 字段），只能改槽内值、不能替换字段；`with_home` / `with_hub_opener` 取 `self`（非 `mut self`）就是为了让 builder 写法与其它 `with_*` 一致，又不破坏已 clone 出去的副本。

**动作投递的语义**：`act_*`（`ui/actions.rs`）是选择器里全部入口的统一出口——有投递口就 `home(host, action, cx)` 交给宿主、没有就就地跑原逻辑（单测与无宿主场景行为不变）。宿主把 `Switch` 接到 `request_switch`、`CreateProject`/`OpenFolder` 接到 `request_create_project`/`request_open_folder`、`Sample` 接 `create_sample`、`Close` 接 `request_close`——都要 `&mut Window`，而 `App` 拿不到窗口，所以投递闭包在注入时捕获 `window.window_handle()`，在回调里用 `AnyWindowHandle::update` 取回主窗口再跑。这样设置窗口里的点击产生的一切副作用（拦截对话框、`do_close`、`open_path`）都落在**主窗口**上，设置窗口本身不关。

三个桥 trait 只描述行为，不描述宿主类型：

| trait | 方法 | workbench 实现 |
| --- | --- | --- |
| `ProjectUiNotifier` | `notify(&mut App)` | `WeakEntity<WorkbenchView>` → `entity.update(cx, notify)` |
| `ProjectEditorBridge` | `is_dirty` / `sql` / `clear(window, cx)` / `mark_clean` | 读写 `Shared::{editor_dirty, editor_sql, editor_clear}` |
| （排序偏好）| `Fn(ProjectSort, &mut App)` | `SettingsService::set_project_sort_mode` |

`()` 实现了 `ProjectEditorBridge`（全 no-op），因此测试与无编辑区宿主可以省略 `with_editor`。

### 为什么 notifier 是 `WeakEntity`

host 存在 `WorkbenchView` 的字段里（`project_host: Option<ProjectUiHost>`），而 notifier 指向该实体。若用强 `Entity` 就形成 `WorkbenchView → host → Entity<WorkbenchView>` 的引用环，视图永不析构。弱引用 + `WeakEntity::update` 失败即忽略，正好符合「窗口已关，不必重绘」。

### 状态所有权

| 数据 | 宿主侧字段 | 视图侧入口 |
| --- | --- | --- |
| 当前项目 | `Shared.project` | `host.current()` / `host.root()` / `host.set_current(...)` |
| UI 状态 | `Shared.project_ui` | `host.state`（`Rc` 同源） |
| 编辑区 | `Shared.editor_*` | `host.editor` |
| 排序偏好 | `settings.json` | `(host.save_sort)(...)`；构造期首帧由宿主直读 |

```mermaid
graph LR
    subgraph workbench
        W[WorkbenchView] --> H[ProjectUiHost]
        W --> S[Shared]
        H -.Rc 同源.-> S
    end
    subgraph project
        H --> U[ui: 选择器/菜单/设置/对话框]
        U --> SV[service / store / lock]
    end
```

### 模块布局（2026-11-23 拆分；2026-11-24 覆盖层退役后回到五个）

`ui.rs` 只留**公共面**（选择器/排序/筛选类型与纯函数、`ProjectUiHost`、`ProjectUiState`、`ProjectInputs`、`PendingAction` 与 `HomeAction`），实现按职责拆成五个子模块（原第六个 `ui/hub.rs` 随项目中心迁入设置页删除）：

| 文件 | 内容 |
| --- | --- |
| `ui/host.rs` | `write_blocked`（按**目标项目**判定的只读守卫）/ `read_only_blocked`（目标=当前项目）/ `notify_success` / `notify_error` / `current_summary` |
| `ui/actions.rs` | 打开 / **切换（`request_switch` / `switch_now`）** / 关闭 / 创建 / 重定位 / 删除 / 移出 / 归档 / 项目信息 / 默认连接 + 选择器加载与筛选；**动作投递** `dispatch_home` + `act_create_project` / `act_open_folder` / `act_sample` / `act_switch` / `act_close` / `act_open_hub`（有宿主投递口就交出去、否则就地执行） |
| `ui/dialogs.rs` | 语义对话框与行内校验提示（`location_hint` / `folder_hint` / `target_preview` / `memo_hint` / footer） |
| `ui/picker.rs` | 选择器渲染、卡片（含「当前」标记）、卡片菜单（只读置灰由 `item_write_locked` 按卡片判定）、标题栏项目菜单（入口全部走 `act_*`） |
| `ui/settings.rs` | `SettingsSnapshot` / `load_settings_snapshot` / `render_current_settings`（全局设置「项目」页的第二块；有会话才返回元素，含版本台账删除） |
| `ui/tests.rs` | 50 项测试（`use super::{...}` 照旧） |

约定：子模块之间不互相 `use`，一律 `use super::*` 取公共面词汇表（依赖方向单向）；跨模块使用的原私有条目提为 `pub(super)`——对 `ui` 及其后代可见、不进公共面；`project::ui::*` 的对外路径由 `ui.rs` 末尾的 `pub use ...::*` 保持不变。拆分逐字搬迁、未改逻辑。

## 3. 生命周期与 I/O 位置

| 时点 | 发生什么 | 在哪 |
| --- | --- | --- |
| 启动装配（`WorkbenchView::new(cx)`） | 解析会话（环境变量 → 设置「启动时恢复上次项目」→ 空态）→ **取写锁**（`open`；被另一实例占用则 `open_read_only`）→ 按**最终**作用域载连接 → 装配共享状态。取锁 / 载入失败**退回空态**：会话不保留，错误进欢迎页选择器（`picker.error`） | `crates/workbench/src/view/lifecycle.rs::new` / `StartupLock` |
| 构造（启动副作用） | 项目名册**总是**预载一次（有项目时欢迎 tab 默认隐藏，但标题栏「回到欢迎页」与设置「项目」页都渲染这份列表）；洞察规则目录监听 | `lifecycle.rs::install_startup_side_effects` |
| 构造其余部分 | 装配 host；直读 `settings.json` 取排序 | 构造期（不在 render） |
| render | `render_picker` / `render_current_settings` 只读 state 并返回元素（后者只由设置页的注入面板调用，主窗口不再挂项目设置层）；标题栏菜单用 `build_project_menu` 产 `PopupMenu`（宿主 `Button::dropdown_menu` 只负责下拉宿主） | `WorkbenchView::render` |
| 事件回调 | 动作函数（打开 / 关闭 / 增删改查 / 对话框提交）做 I/O 并 `host.notify` | 各处 `on_click` / `on_action` |
| 对话框 | 由 `window.open_dialog` / `open_alert_dialog` 承载；关闭时机由提交结果决定 | 事件上下文 |
| 对话框内容（builder 闭包） | builder 是**每帧重跑的渲染闭包**（vendor 契约：cheap and idempotent），因此只读 state + 输入框；唯一的 I/O（`location_hint` / `folder_hint` 的目录探测）按「位置 / 名称 / 路径」做**局部 memo**——统一走 `ui::memo_hint`（单测 `memo_hint_recomputes_only_when_key_changes` 锁住"同 key 不重算 / 换 key 必重算 / None 也缓存"） | `ui.rs::open_create_dialog` / `ui.rs::open_folder_dialog` |
| 设置窗口里的项目列表 | 同一份 `render_picker`，作为 `SettingsHost::projects_panel`（`PanelRender`）每帧被设置页骨架调用：只读 state，不加载 / 不写；点击只投递 `HomeAction` 回主窗口 | `settings/src/skeleton.rs` 调面板；`crates/workbench/src/view/actions.rs::open_settings_at` 装配 |
| 设置窗口里的当前项目设置 | `render_current_settings` 作为 `SettingsHost::project_settings_panel`（同类型别名）每帧被调用：有会话才返回块，无项目时宿主给一句占位；快照仍只在事件路径取（进「项目」节 / 保存 / 归档后） | 同上；快照取数在 `open_settings_at`（`section == Some("projects")`）与 `ui/actions.rs` 的保存 / 归档路径 |

### 启动路径的可见状态（2026-11-27）

启动装配只有 `cx`、拿不到 `Window`，所以状态**不能靠 toast**；三个落点各司其职：

| 状态 | 落点 | 为什么是这里 |
| --- | --- | --- |
| 只读（写锁被另一实例占用 / 归档只读） | 标题栏项目槽的「只读」徽标 + tooltip；标题栏项目菜单的「只读：写操作不可用」说明行；设置「项目」页状态行「只读」+「只读（写操作已禁用）」；编辑器状态栏「连接只读」徽标 | `project_ui.notice` **只**在选择器里渲染，而有项目时欢迎 tab 默认隐藏——只写 notice 等于用户看不到 |
| 启动取锁 / 载入失败 | 欢迎页选择器的错误行（`picker.error`，danger 色）；失败时**会话退回空态**，不保留一个"看似打开、实际没锁"的项目 | 启动路径没有交互式打开那套「仍要打开 / 只读打开」对话框，只能退回空态并说清原因 |
| 连接清册加载失败 | 左 Dock「数据源连接」面板底部的 info 行（`Shared::notice`，`Shared::say` 回执） | 与连接相关的提示归连接面板，不混进项目状态 |

## 4. 对话框关闭时机（栈语义）

`Root` 的对话框是**栈**：`window.open_dialog` push，关闭时 `pop` 最后一个。因此：

- `Dialog::on_ok` 一律返回 `false`，不依赖其返回值关闭；
- 提交成功时由回调显式 `window.close_dialog(cx)`；
- 被拦截的动作（打开 / 关闭项目）若会另开对话框（如锁占用），必须**先关本对话框、再推进动作**，否则 `pop` 会关掉新开的那个。

未保存拦截因此拆成两段：`prepare_unsaved`（另存草稿 + 清空编辑区，失败则写 `dialog_error` 并保持打开）→ `advance_pending`（执行被拦截的动作）。

校验错误统一存 `ProjectUiState::dialog_error`：对话框 builder 每帧重读，提交失败时写入并 `notify`，无需重建 builder 闭包状态。

### 位置字段：系统目录选择器

原型的「位置 = 目录选择 + 预览 `位置/名称`」由三件组成：

| 件 | 实现 |
| --- | --- |
| `浏览…` 按钮 | `pick_directory`：`App::prompt_for_paths(files:false, directories:true, multiple:false)` → `Window::spawn` 等 oneshot → `AsyncWindowContext::update` 拿 `&mut Window` 回填 `InputState::set_value`（取消则保持原值） |
| 目录输入行 | `directory_row`：输入框（`flex_1`）+ 浏览按钮，新建 / 打开 / 重定位共用 |
| 目标预览 | `target_preview`：新建对话框展示 `目标：位置/名称`，位置或名称为空时给占位提示 |

要点：`set_value` 需要 `&mut Window`，而异步回调里只有 `AsyncApp`，因此必须走 `Window::spawn`（而非 `Context::spawn`）+ `AsyncWindowContext::update`。测试用 `TestAppContext::simulate_path_prompt_response` 模拟用户选择，注意它**应答已入队的请求**，所以要先用 `pick_directory` 发起、再模拟响应。

## 5. 远程项目（范围说明）

远程项目（`ProjectPath::Remote { url, project_id }`，DuckLake）**只在模型层预留**：`models.rs` 有类型与 `remote()` 构造器，v1 蓝本有对应命令分支；但 `CreateProjectInput` / `ProjectStore::create` / `inspect_target` 全部只处理本地路径，本期明确不做（`project-dev-plan.md`「不做」清单）。新建对话框因此用一行 muted 文案明示范围，避免用户把 URL 填进「位置」。

## 6. 窗口测试方案

位置：`crates/project/src/ui/tests.rs`（`#[cfg(test)] mod tests;`），**50 项测试（31 项 GPUI headless 窗口测试 + 19 项纯函数测试）**（2026-11-24 实测）。

### 骨架

| 部件 | 作用 |
| --- | --- |
| `Recorder` | 记录宿主桥调用：重绘次数 / 写回的排序值 / 草稿脏标记 / 清空次数 / 清脏次数 |
| `TestNotifier` / `TestEditor` | `ProjectUiHost` 的两个桥替身，零副作用 |
| `test_host(&rec)` | 组装 host（state / session 与视图共享同一 `Rc`） |
| `Harness` | 宿主视图替身：按状态渲染选择器 / 设置 / 菜单，并挂 `Root::render_dialog_layer` |
| `open_harness(cx, host)` | `cx.add_window_view(\|window, cx\| Root::new(harness, window, cx))`，返回 host、输入实体与 `VisualTestContext` |

### 覆盖清单（对应 dev-plan §4 场景）

| 测试 | 场景 |
| --- | --- |
| `picker_renders_without_project` | 4：无项目态选择器渲染（空列表不崩） |
| `settings_and_menu_render_for_open_project` | 4 / 13：设置面板 + 菜单内容渲染；只读标记来自宿主 state |
| `create_dialog_opens_and_rejects_empty_name` | 1 / 2：`Dialog` 打开；空名校验失败、错误入 state、对话框保持 |
| `delete_dialog_requires_exact_name` | 6：删除确认名称匹配 |
| `request_close_intercepts_dirty_editor` | 10：脏编辑区 → 关闭被拦截 |
| `request_open_intercepts_dirty_editor` | 10：脏编辑区 → 打开被拦截、不切项目 |
| `cycle_sort_persists_through_host_callback` | 4：排序循环 + 宿主持久化回调 + 重绘请求 |
| `lock_busy_dialog_offers_escape_hatches` | 11：锁占用 `AlertDialog` 打开且不改会话 |
| `cards_and_menus_construct_for_all_states` | 8 / 6：卡片三分支（活跃 / 失效重定位 / 已移除恢复 + 固定）与更多菜单 |
| `browse_fills_location_from_system_picker` | 1：浏览目录 → 回填位置输入框（并断言选择器选项为「仅目录 / 单选」） |
| `browse_cancel_keeps_location` | 1：取消选择器 → 位置输入框保持原值 |
| `create_dialog_inline_hint_updates_while_typing` | 1 / 行内校验：对话框 builder 是渲染闭包，键入非法名 / 不存在位置后**同一帧**就画出提示（`debug_selector` + `debug_bounds` 断言，防止"校验要等下一次无关重绘"） |
| `open_folder_dialog_hint_flags_non_project_dir` | 1 / 行内校验：打开现有项目时非空非项目目录立刻报红，换成空目录提示消失（同时验证 memo 随路径重算） |
| `memo_hint_recomputes_only_when_key_changes` | 行内校验：渲染期目录探测 memo 的语义（同 key 不重算 / 换 key 必重算 / `None` 结果同样缓存）（纯函数，计数断言） |
| `empty_dir_prompts_create_in_place` | 2：空目录 → 询问式创建（不直接当项目打开） |
| `save_project_info_rejects_invalid_name` | 5：改名校验（非法字符 / 空） |
| `read_only_blocks_project_info_save` | 11 / C1：只读下写命令被拦（写 notice、不改会话）且设置面板仍可开 |
| `title_bar_menu_builds_in_both_modes` | A4 / C1：两种模式的标题栏菜单都构造得出来（`PopupMenu::build` + `is_empty`） |
| `picker_controls_activate_from_the_keyboard` | C4：`Tab` 能停靠 + `Enter` 能真的改状态（Tab / 状态筛选 / 排序） |
| `visible_items_filters_by_status_and_needle` | 4 / R4：搜索子串 ∩ 状态筛选（纯函数） |
| `menu_spec_greys_out_write_commands_in_read_only` | C1：只读置灰清单（写命令置灰、读命令与出口可用） |
| `menu_spec_keeps_escape_hatches_without_leading_separator` | C1 / B1：分隔线位置与「逃生口不被藏住」 |
| `description_shows_only_when_present` | B3：描述展示的两个口（卡片行 / 概览文案）只认非空描述（纯函数） |
| `meta_tree_rows_lists_dirs_first_then_files` | B2：结构树序号 / 层级 / 大小 / 不存在目录（纯函数，只读临时目录） |
| `settings_meta_tree_comes_from_snapshot` | B2：设置面板读快照（事件路径取一次）+ 窗口里画得出来 |
| `default_connection_label_reports_missing_option` | A1：默认连接按钮文案三态（含记录值失效如实显示 id） |
| `default_connection_write_respects_read_only` | A1：写默认连接落盘 + 快照跟上；只读模式下被拦且不动磁盘 |
| `read_only_blocks_archive_and_version_snapshot` | E1：只读拦截的另两条路径（归档 / 创建版本），且不写版本台账 |
| `card_menu_spec_branches_and_gates_read_only` | C3：卡片菜单规格（`⋯` 与右键同源）按视图 / 路径状态分支 + 只读置灰 |
| `search_facets_parse_known_keys_only` | C5：facet 解析（未知键 / 空值 / 半截值 / 盘符冒号都回落自由文本） |
| `search_facets_narrow_the_list_with_and_semantics` | C5：facet ∩ 自由文本 ∩ 筛选按钮（AND 语义） |

不覆盖：真实建库与迁移（`crates/project/tests/project_store.rs` 集成测试）、双实例并发（手动清单）、主题视觉（`theme-preview.html` 基准）。

### 三个必须知道的坑

1. **`#[test]` 自相残杀**：gpui 的 `test` 宏展开成裸 `#[test]`。测试模块若 `use gpui_kit::*`（或 `use super::*` 间接引入它），`#[test]` 会解析到 gpui 的宏自身，无限递归 —— 报错 `recursion limit reached while expanding #[test]`，且提高 `recursion_limit` 只会让需求跟着翻倍。解法：测试模块显式列举依赖（含 `AppContext as _` / `StyledExt as _` / `WindowExt as _` 等 trait）。
2. **对话框要有 `Root`**：`window.open_dialog` 依赖窗口根为 `component::Root`；且宿主视图的 `render` 必须自己挂 `Root::render_dialog_layer(window, cx)`，否则对话框存在但不渲染。断言用 `window.has_active_dialog(cx)`。
3. **键盘激活在 KeyUp，不在 KeyDown**：gpui 的可点元素在 `KeyUpEvent` 上派发 `ClickEvent::Keyboard`；`cx.simulate_keystrokes("enter")` 只发 KeyDown，验证「Enter 能点」时必须 `simulate_event(KeyDownEvent)` + `simulate_event(KeyUpEvent)` 各发一次（另：`Tab` 顺序来自上一帧布局，先 `window.draw(cx).clear(cx)` 再 `window.focus_next(cx)`）。

## 7. 实现位置映射

| 设计决策 | 代码文件 |
| --- | --- |
| 项目视图（选择器 / 菜单 / 设置 / 对话框） | `crates/project/src/ui.rs` |
| 宿主契约（`ProjectUiHost` / 三个桥 trait / `OpenProject`） | 同上（文件头「宿主桥」一节） |
| workbench 侧桥接与装配 | `crates/workbench/src/components/project_host.rs` |
| 会话类型与解析 | `crates/project/src/ui.rs`（`OpenProject`）、`crates/workbench/src/services/project_session.rs`（`resolve`） |
| 共享状态字段 | `crates/workbench/src/panels/`（`Shared::{project, project_ui, editor_*}`） |
| 标题栏项目槽 + 菜单 | `crates/workbench/src/view/dock.rs`（`render_title_bar`；项目槽在只读时挂「只读」徽标 + tooltip，有项目时接 `Button::dropdown_menu` + `project::ui::build_project_menu`） |
| 菜单规格（顺序 / 文案 / 可用性） | `crates/project/src/ui.rs`（`project_menu_entries` 纯函数 + `attach_project_menu_handler`） |
| 窗口测试 | `crates/project/src/ui/tests.rs` |
| 启动装配取锁 / 回退（workbench 单测） | `crates/workbench/src/view/lifecycle.rs`（`acquire_startup_project` / `StartupLock` + 文件末尾 `mod tests` 两例） |
| 视图测试规范 | `.agents/skills/gpui-kit-dev/SKILL.md`（「窗口测试」一节） |
| 设置面板的磁盘 / 名册 / 默认连接 / 版本台账快照 | `crates/project/src/ui.rs`（`SettingsSnapshot` / `load_settings_snapshot` / `meta_tree_rows` / `set_default_connection` / `versions_view`；事件路径填充，render 纯读） |
| 卡片命令（`⋯` 下拉与右键菜单） | `crates/project/src/ui.rs`（`card_menu_entries` 规格 + `card_menu` 渲染 + `attach_card_menu_handler`） |
| 搜索 facet（`状态:` / `固定:` / `锁:`） | `crates/project/src/ui.rs`（`parse_search` → `visible_items`；口径对齐 database 导航面板） |
| 默认连接候选来源（全局 + 项目连接） | `crates/workbench/src/components/project_host.rs`（`default_connection_options` → `with_connections`） |
| 位置字段（系统目录选择器 / 目标预览 / 行内校验） | `crates/project/src/ui.rs`（`pick_directory` / `directory_row` / `target_preview` / `location_hint`） |
| 位置记忆（默认新建父目录的读 / 写） | `crates/project/src/ui.rs`（`open_create_dialog` 读 `(host.last_create_location)`、`submit_create` 成功分支调 `(host.save_create_location)`）；宿主侧 `crates/workbench/src/components/project_host.rs`（`with_create_location`，接 `SettingsService::{project_last_create_location, set_project_last_create_location}`） |
| 构建标识落项目（只写不展示） | `crates/project/src/ui.rs`（`apply_opened` → `update_config` 写 `extensions.rds.build_tag`，键 `store::BUILD_TAG_KEY`）；宿主侧 `project_host.rs`（`with_build_tag(Rc::new(\|\| crate::version::info().tag()))`） |
| 项目中心（有项目态的项目列表 + 切换） | 列表本体仍是 `render_picker`，由宿主渲染进**全局设置窗口**的「项目」节（`SettingsHost::projects_panel` + `settings::fields::panel_item`）；动作投递口 `ProjectUiHost::home` / `on_open_hub` 在 `crates/workbench/src/view/workspace.rs::wire_project_home` 注入，设置窗口侧的挂载与切节在 `crates/workbench/src/view/actions.rs::open_settings_at` |
| 窗口测试 50 项（31 窗口 + 19 纯函数） | `crates/project/src/ui/tests.rs` |

## 8. 验证方式

```bash
cargo check --workspace --all-targets          # 零告警
cargo test --workspace -j 2                    # 全量（-j 2 硬性要求）
cargo test -p rds-project -j 1           # 只跑项目 crate（lib 64 项，其中 `ui/tests.rs` 50 项；2026-11-24 实测）
cargo build -p rds-app -j 2                    # codegen 验证（check ≠ 能出机器码）
cargo clippy -p rds-project --all-targets -j 1 # 2026-11-22 实测 2 条，见下
```

`cargo clippy` 不是仓库门禁（门禁是 `cargo check` 零告警），本 crate 2026-11-22 从 22 条清到 2 条；余下两条**有意不修**：

| 告警 | 位置 | 为什么不改 |
| --- | --- | --- |
| `large_enum_variant`（640B vs 16B） | `service.rs` 的 `OpenOutcome` | 改 `Opened(Box<OpenedProject>)` 会把装箱成本推给 workbench 的每个调用点，属跨 crate 的 API 变更；当前只有「打开一次」这一条低频路径，栈上多 640B 无所谓 |
| `too_many_arguments`（8/7） | `ui.rs::dialog_footer_three` | 参数是「三个按钮的 id / 文案 / 回调」的平行三元组，抽结构体只是把参数搬进字段，对读者没有增益；函数体只有一处 `DialogFooter` 组装 |

注意：`cargo test --workspace` **必须带 `-j 2`**（并行链接重型 crate 会耗尽内存（DuckDB 已改动态链接），见 `.cargo/config.toml` 的 `test-all` 别名）。

窗口用例在**默认并行**下会互相锁死：2026-11-27 实测 `cargo test -p rds-workbench --lib` 跑到 `quick_open::tests::palette::*` / `host::opening_the_palette_loads_scratchpad_files` 全部「running for over 60 seconds」且永不返回；同一批用例加 `--test-threads=1` 串行后 0.54s 全过。整套口径（同次实测）：227 条 = 210（`-- --skip quick_open`）+ 17（`quick_open -- --test-threads=1`），全绿。
