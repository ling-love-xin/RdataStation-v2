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

## 颜色（硬约束）

- **禁止裸 hex/rgb**；一律 `cx.theme().colors.<字段>`（需 `use gpui_kit::component::ActiveTheme`）
- 常用字段：`background` / `foreground` / `border` / `muted_foreground` / `sidebar` / `sidebar_accent` / `title_bar` / `title_bar_border` / `popover` / `overlay` / `primary` / `danger` / `success` / `warning` / `info`
- 无专用字段的产品角色走语义 token（见 rds-theme skill）
- 不留下调试背景色（如 `bg(theme.colors.danger)` + `DBG-` 注释）；调试用边框/临时标识，完成后清理

## 状态与同步（权威同步点）

- 可交互状态放 `Shared`（模型），渲染时从 Shared 读取
- 副作用（Dock 装配/移除、模式切换）不在点击回调里直接做：回调只更新 Shared 状态 + `cx.notify()`；`render` 是模式同步的权威点（如 `WorkbenchView::render` 中的 `apply_left_mode` / `apply_right_mode`）

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

## 查 API

- gpui-kit 0.6 源码在 cargo registry（如 `gpui-component-0.6.0/src/`），不确定 API 时直接读源码或 `grep` registry 目录
