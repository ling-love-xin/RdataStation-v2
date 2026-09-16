---
name: rds-layout
description: RdataStation 工作台五段布局实现规格：标题栏/左右活动栏/左右 Dock 边栏/状态栏/Quick Open 的尺寸、组件与主题角色，以及边栏三模式 Dock API。修改 crates/workbench 的 view.rs / panels/ 布局代码时使用。
---

# 工作台五段布局实现规格

设计文档：`docs/architecture/layout/layout-design.md`（v5）；可视化示意：`docs/architecture/layout/layout-proposal.html`。

## 结构（自上而下）

```
标题栏
[左活动栏 | 左 Dock(240px) | DockArea 中央编辑区 | 右 Dock(280px) | 右活动栏]
状态栏
```

## 标题栏（差异点：无窗口标题文本）

- 高度 36px，背景取 `theme.colors.title_bar`，底边框 `theme.colors.title_bar_border`
- 左：`assets/icons/32x32.png` logo（20×20 圆角）+ 挖空槽（`项目` 标签 + 项目名），槽背景用语义 token `title_bar.slot.background`（比标题栏深一档）
- 中：Quick Open 入口 320px 居中（点击或 Ctrl+P 唤起）
- 右：窗口控制按钮 ─ □ ✕（gpui-kit TitleBar 自带机制或自绘 `window_control_area` hitbox）
- 窗口创建用 `TitleBar::window_options()`（appears_transparent、app_owns_titlebar_drag）

## 活动栏（左右镜像，VSCode 差异点）

- 48px 竖条；左：草稿箱/数据库导航/资源分析/插件 4 图标；右：洞察/Mock 生成/历史 3 图标；底部 ⚙ 设置
- 激活图标 2px 侧条 + 亮色（activity_bar 语义 token，见 rds-theme skill）
- 点击行为：该侧收起 → 展开并切换；展开且非当前 → 切换 panel；展开且当前 → 收起（toggle）

## 边栏三模式（Dock API，gpui-kit 0.6）

| 模式 | API |
| --- | --- |
| 展开 | `set_dock(placement, layout, window, cx)` |
| 收起 | `toggle_dock(placement, window, cx)`（dock 保留、离屏，panel 状态不丢） |
| 完全隐藏 | `remove_dock(placement, window, cx)` + 隐藏该侧活动栏 |

- 恢复：以同一 `DockLayout`（同一 panel 实体）重新 `set_dock`
- 左右模式独立；状态存 `Shared`，`WorkbenchView::render` 统一同步到 Dock（权威同步点）
- Dock 布局核心在 `gpui_kit::base::dock`；外观层在 `gpui_kit::component::dock`

## 状态栏

- 左：`« 完全隐藏` 按钮 + 活动指示；右：连接名 / DuckDB 状态 / 编码
- 背景品牌色（`theme.colors.primary` 派生），白字

## Quick Open

- 标题栏居中入口（320px 宽）唤起；面板 = Dialog + Input + 分组列表（`文件 / 表`、`命令` 两组混排）
- 输入 `>` 前缀仅匹配命令；↑↓ 选择、↵ 执行/打开、Esc 关闭

## 布局代码位置

- 标题栏/活动栏/Dock 装配/三模式：`crates/workbench/src/view.rs`（`render_title_bar` / `render_left_activity_bar` / `render_right_activity_bar` / `init_workspace` / `apply_left_mode` / `apply_right_mode`）
- panel 枚举：`crates/workbench/src/view.rs`（`LeftPanel` / `RightPanel` / `SidebarMode`）；面板实现：`crates/workbench/src/panels/`（`mod.rs` 装配 + `nav.rs` / `scratchpad_panel.rs` / `editor.rs` / `right.rs` / `shared.rs`）
- 命令：`crates/workbench/src/commands.rs`

## 检查清单

- 布局改动后对照 `layout-proposal.html` 逐段核对尺寸、位置、颜色
- 各区域主题角色见 rds-theme skill；代码零裸色值
