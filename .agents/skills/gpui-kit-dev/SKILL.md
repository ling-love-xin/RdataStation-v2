---
name: gpui-kit-dev
description: GPUI-kit 0.6 视图开发规范：依赖只向下、主题取色零裸色值、render 权威同步点、事件/Action 模式、常用组件 API 与简体中文注释约定。编写或修改任何 GPUI 视图、元素、组件代码时使用。
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

## 查 API

- gpui-kit 0.6 源码在 cargo registry（如 `gpui-component-0.6.0/src/`），不确定 API 时直接读源码或 `grep` registry 目录
