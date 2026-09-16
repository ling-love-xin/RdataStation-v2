---
name: gpui-kit-dev
description: GPUI-kit 0.6.1 视图开发规范：依赖只向下、主题取色零裸色值、render 权威同步点、事件/Action 模式、布局与滚动陷阱、组件选型（禁止手搓）、0.6.1 API 查证路径与常用组件 API、简体中文注释约定。编写或修改任何 GPUI 视图、元素、组件代码时使用。
---

# GPUI-kit 0.6 视图开发规范

## 依赖与导入

- 视图层只依赖 gpui-kit：`gpui_kit::*`（聚合包）= gpui-pre；`gpui_kit::base` = gpui-base（dock/flex/styled）；`gpui_kit::component` = gpui-component（组件/主题/图标）；`gpui_kit::assets`
- 常用导入（对照 `crates/workbench/src/view.rs`）：

```rust
use gpui_kit::*;
use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dock::{panel_handle, DockArea, DockLayout, DockPlacement, DockSkin};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::{ActiveTheme, Icon, IconName, Root, Theme, ThemeRegistry, TitleBar};
use gpui_kit::prelude::FluentBuilder as _;
```

- 例外：**测试模块**里不要用 `use gpui_kit::*`（详见「窗口测试」一节，会与 `#[gpui_kit::test]` 自相残杀）

## 颜色（硬约束）

- **禁止裸 hex/rgb**；一律 `cx.theme().colors.<字段>`（需 `use gpui_kit::component::ActiveTheme`）
- 常用字段：`background` / `foreground` / `border` / `muted_foreground` / `sidebar` / `sidebar_accent` / `title_bar` / `title_bar_border` / `popover` / `overlay` / `primary` / `danger` / `success` / `warning` / `info`
- 无专用字段的产品角色走语义 token（见 rds-theme skill）
- 不留下调试背景色（如 `bg(theme.colors.danger)` + `DBG-` 注释）；调试用边框/临时标识，完成后清理

## 状态与同步（权威同步点）

- 可交互状态放 `Shared`（模型），渲染时从 Shared 读取
- 副作用（Dock 装配/移除、模式切换）不在点击回调里直接做：回调只更新 Shared 状态 + `cx.notify()`；`render` 是模式同步的权威点（如 `WorkbenchView::render` 中的 `apply_left_mode` / `apply_right_mode`）
- **`cx.theme()` 借用了 `cx`**：同一个函数里既要拿 theme 又要 `entity.update(cx, …)` 时，把 update 放在 `let theme = cx.theme();` **之前**（否则 E0502：不可变借用尚未结束）
- **控件集合与文案随数据语义动态渲染**：同一页面在不同类型下只渲染有意义的控件（如文件型库不显示认证/SSL 卡片）；标签与占位由当前数据推导（如驱动 `url_template`），不要写死某一种类型的示例
- **单输入源**：同一个 `InputState` 只在一处渲染；两处同时可编辑会分不清权威值（需要“只读回显”时用文本，不要复制一个输入框）
- **不要在回调里重入 `update` 当前实体**：方法持有 `&mut Context<T>` 时直接用 `cx.subscribe_in` / `cx.notify()` / 修改 `self`，不要 `entity.update(cx, …)`（同一实体正在被更新 → panic `cannot update … while it is already being updated`）。订阅句柄存到字段（`_xx_sub: Option<Subscription>`，前缀下划线表明“仅持有”）；若订阅需要挂到别的实体上，就把建立动作放到那个实体的入口方法里，而不是被它调用的方法里（实例见 `docs/architecture/connection/connection-dialog-architecture.md` 决策 #30）

## 事件与 Action

- 全局命令用 `actions!(crate_name, [ActionA, ActionB])` 注册 GPUI Action；在 app 层 `cx.bind_keys` 绑定快捷键；视图用 `.on_action(...)` + `.key_context("...")` 处理
- 点击回调模式：更新 Shared → `entity.update(app, |_, cx| cx.notify())`

## 组件选型（0.6.1 已有，禁止手搓）

写控件前先查下表；表中每一项都在 0.6.1 存在（`gpui_kit::component::…`）。手搓 `div().on_click()` 会丢 hover / 键盘 / 焦点 / disabled / a11y，属于回归。

| 需求 | 用 | 不要 |
| --- | --- | --- |
| Tab 条 / 分段控件 | `tab::{Tab, TabBar}`（`TabBar::underline()` / `segmented()`，`selected_index` + `on_click`） | 自绘下划线 tab、`scope_seg` |
| 开关 | `switch::Switch`（`new/checked/label/on_click`，`Disableable::disabled`，`Sizable` 档位） | 自绘轨道 + 滑块 |
| 复选 / 单选 | `checkbox::Checkbox`、`radio::{Radio, RadioGroup}` | 自绘勾选 |
| 按钮（含图标按钮） | `button::Button`：`.ghost()` / `.toggled(bool)` / `.disabled(bool)` / `.icon(IconName::…)` / `.label(…)` / `.size(…)` | 文本 div + `cursor_pointer` + `on_click` |
| 菜单 / 右键菜单 | `menu::{PopupMenu, DropdownMenu}`（项用 `PopupMenuItem`） | `Popover` 里手搓 `menu_item` |
| 折叠分组 | `accordion::Accordion`、`collapsible::Collapsible` | `▸/▾` 字符 + `on_click` |
| 下拉选择 | `select::{Select, SelectState}`、`combobox::{Combobox, ComboboxState}` | 点击循环切换的 div |
| 列表 / 树行 | `list::{List, ListState, ListDelegate}`；虚拟化用 `v_virtual_list` | 手搓 hover/选中态行 |
| 禁用态 | `gpui_kit::base::Disableable as _` 的 `.disabled(bool)`（Input 是本体方法） | 自绘控件漏掉 disabled（实例：对话框三个自绘开关未接 `form_disabled`） |

> 已知反例（技术债，不要照抄）：`crates/workbench/src/components/connection_dialog/render.rs` 的 Tab 条 / 开关、`panels/` 的 `tool_btn` / 树展开字符 / 文本按钮。反向结论（“库没有 Tabs/Switch”）已作废。

## 布局与滚动陷阱（项目内已验证）

- **`h_flex()` = `flex_row().items_center()`，交叉轴居中**。把「列」（带 header / footer / 滚动区的 `v_flex`）放进 `h_flex` 必须 `.items_stretch()` 或给列 `h_full()`，否则列高于行时上下同时被裁。正例：`crates/workbench/src/view.rs` 的 `div().h_flex().items_stretch().flex_1().min_h_0()`。
- **`rems(x)` 的基准是主题字号（默认 16px），不是 Tailwind 的 4px**。`gap_1`（=4px，Tailwind 方法）与 `rems(1.)`（=16px）是两套单位，不要混算；px 意图不要手写 `/ 4.`（曾把树缩进放大 4 倍）。结构尺寸一律引用 `crate::ui::*` 常量。
- **滚动所有权**：Dock 面板的 `#tab-content` 是 `absolute().size_full()` + `overflow_y_scroll`，**不产生滚动**；面板自己要有 `flex_1() + min_h_0()` 的滚动主体并挂 `overflow_y_scrollbar()`。滚动条要贴滚动区域边缘——`Scrollable` 挂在拥有整块内容的元素上，内距放在它内部，不要在外面再包一层 padding。
- **允许收缩的 flex 子元素要 `min_w_0()` / `min_h_0()`**（`flex_1()` 不够）；长文本还要 `text_ellipsis()` 或 `overflow_hidden()`，否则会视觉溢出推挤同行按钮。
- **固定宽度的表格式布局**（每列 `w(rems(N.)).flex_none()`）必须配横向滚动，否则列一多必然溢出。

## 状态与副作用

- `render` 是纯读路径：不做 I/O（`Runtime::new` / `block_on` / `fs` / 打开数据库）、不写 `Shared`、不解析 JSON、不深拷贝大集合。副作用回事件路径（`Shared` 更新 + `cx.notify()`）。
  - 已知反例（技术债）：`panels/editor.rs` 的连接详情卡在 render 内调 `load_navigator_tree` **同步读分析库文件**（真读盘，缓存键 = 当前连接）；另有 render 内**写状态 + 入队后台任务**（`panels/nav.rs` 的 `render_connection_row` / `render_nav_node` → `ensure_nav_loaded`、`panels/editor.rs` 的 `render_property_panel`、`panels/scratchpad_panel.rs` 的 `render_scratchpad`；I/O 在工作线程）；`project/ui.rs` 的 `render_settings` 每帧 `fs::metadata`。更重的一类是**事件路径**同步 I/O（`block_on` 阻塞 UI 线程）：`nav.rs` 4 处 + `scratchpad_panel.rs` 14 处，见 `docs/architecture/layout/panels-modules.md` §4。
- 重复元素（列表 / 树 / tab / 协议链）的 `ElementId` 用业务键（`conn.id` / `node.key` / 稳定名称），**不用下标**——下标在增删与「最新在前」插入后会让 hover 等按 id 记录的状态串行。

## 颜色语义

- 取色一律 `cx.theme().colors.<字段>`；`border` 是描边色，**不当文字色/大面积填充**；对话框内用 `list` / `list_hover` / `list_active` / `popover`，不要拿 `sidebar_accent`。
- 状态文案按语义取色：成功 `success` / 失败 `danger` / 提示 `info`；同一个 `notice` 字段不要一律一个色（项目内已出现「失败用 info」「成功用 danger」两处反向误用）。
- 不要用 `rgb(...)` / `hsla(...)` 造色（`settings_view.rs` 曾用裸 hex）；透明用 `transparent_black()`。

## 注释约定

- 注释与文档一律简体中文，说明非显而易见的意图、约束、取舍；不写复述代码的注释

## 常用组件速查

- **TitleBar**：自带窗口拖拽与窗口控制按钮（Windows 上按钮只需 `window_control_area` hitbox，点击由系统触发，无需 `on_click`）。窗口用 `TitleBar::window_options()` 创建（`appears_transparent`）
- **DockArea**：`set_dock` / `toggle_dock` / `remove_dock` / `is_dock_open` / `has_dock`（三模式用法见 rds-layout skill）
- **Button**：`.icon(...).size(px(28.)).ghost().toggled(bool).label(...)`
- **StatusBar**：`.left(...)` / `.right(...)`
- **Input**：`Input::new(&InputState)`；InputState 用 `cx.new(|cx| InputState::new(window, cx))` 创建

## 窗口测试（GPUI headless）

写视图测试时按 `crates/project/src/ui/tests.rs` 的骨架来：

- 标注 `#[gpui_kit::test]`，参数用 `cx: &mut gpui_kit::TestAppContext`；进入测试先 `cx.update(gpui_kit::init)`（主题/组件/输入）；依赖需在 crate 的 `[dev-dependencies]` 启用 `gpui-kit = { workspace = true, features = ["test-support"] }`
- 需要窗口时 `cx.add_window_view(|window, cx| MyView::new(window, cx))` 拿到 `(Entity<V>, &mut VisualTestContext)`；渲染一帧用 `cx.update(|window, cx| window.draw(cx).clear(cx))`
- **安全模式：不要通配导入**。`use gpui_kit::*` 或 `use super::*` 会把 gpui 的 `test` 属性宏带进作用域，而 `#[gpui_kit::test]` 展开出的裸 `#[test]` 会解析到它自己 → `recursion limit reached while expanding #[test]`（提高 `recursion_limit` 只会让需求跟着翻倍，不要加）。测试模块里显式列举依赖，别忘这些 trait：`AppContext as _`（`cx.new`）、`StyledExt as _`（`v_flex`）、`Styled as _`（`gap_2` / `size_full`）、`FluentBuilder as _`（`when_some`）、`ActiveTheme as _`、`WindowExt as _`（`has_active_dialog`）
- **模态对话框**（`window.open_dialog` / `open_alert_dialog`）要求窗口根是 `gpui_kit::component::Root`：`cx.add_window_view(|window, cx| Root::new(view, window, cx))`；且宿主视图的 `render` 要自己挂 `Root::render_dialog_layer(window, cx)`，否则对话框不渲染。断言用 `window.has_active_dialog(cx)`
- 图标资产未注册时静默渲染为空（不 panic），测试无需 `set_assets`
- 测试里的其他依赖（存储 / 设置服务 / 后端口）用测试桥替身，只记录调用，不接真实宿主
- **宿主交互测试走生产入口**：不要直接调 `state.open(...)` 或自建一份状态，而是通过宿主面板的方法（如 `EditorPanel::request_new_connection`）驱动——否则订阅、宿主通知等副作用不会被覆盖（曾因此漏掉一个必现的重入 panic，仅 `dialog_host_layer` 这类走入口的测试能拦住）；断言需要内部状态时由宿主提供只读访问器（如 `dialog_state()`）

## 查 API（版本对齐，必读）

- **权威源是锁定的 crates.io 0.6.1 解包源码**（与 `Cargo.lock` 一致）：
  `D:/Dev/Rust/.cargo/registry/src/mirrors.tuna.tsinghua.edu.cn-4dc01642fd091eda/gpui-kit-0.6.1/`、同目录下 `gpui-base-0.6.1/`、`gpui-component-0.6.1/`、`gpui-kit-assets-0.6.1/`。
  不确定 API 时 `grep -rn "pub fn xxx" <该目录>/src/`，或直接读 `src/` 对应文件。
- **本地 `D:\ABDM\Compressed\gpui-kit-main\gpui-kit-main` 是 main 分支快照，不是 0.6.1**：与锁定版本有 30+ 文件差异（含新增 `carousel/`、`empty.rs`，删除 `dock/tiles.rs` 等）。它只用于读 `skills/`（coding-guides / design-guides）与 `docs/`（ARCHITECTURE / STYLING-AND-MOTION）等**规范类内容**；**禁止**拿它的函数签名 / 组件清单当 0.6.1 的 API 依据。
- 上游设计规范（选组件、版式、文案、动效）看本地包的 `skills/gpui-kit-design-guides/references/design-guides.md` 与 `skills/gpui-kit/references/coding-guides.md`（规范性文件，优先于任何示例代码）。
- 找不到 API 时：宁可在该组件源码里找等价能力，也不要凭 React / CSS / 旧版 GPUI 经验“推断”一个方法名。
