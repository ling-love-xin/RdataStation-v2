---
name: rds-ui-spec
description: RdataStation UI 尺寸与颜色约束：rem 尺寸常量（crates/workbench/src/ui.rs）、主题 token 取色、组件规格表（标题栏/活动栏/状态栏/树节点/列表/输入框/下拉/tab）。编写或修改任何 UI 布局、控件尺寸、间距、颜色时使用。
---

# RdataStation UI 规范

**硬约束**：视图代码不得出现与布局结构相关的裸 `px(N.)`、裸 `font_size * N.`、裸 hex/rgb 颜色。

三层约束，任何 UI 改动都落在其中一层：

1. 颜色 / 圆角 / 字号 → 主题 token（`cx.theme().colors.*`、`theme.radius`）
2. 尺寸 / 间距 → `crates/workbench/src/ui.rs` 常量
3. 组合方式 → `docs/architecture/ui/ui-design-spec.md` §4 组件规格表

## 尺寸：rem 基准 + Tailwind 尺度 + 固定描边

三种表达，按场景选：

```rust
use crate::ui;

// 1) 结构性尺寸（ui.rs 声明倍率）——样式场景用 gpui 原生 rems()
div().h(rems(ui::TITLE_BAR_HEIGHT))
// 需要 Pixels 的 API（如 set_dock_size）：乘主题字号
area.set_dock_size(placement, cx.theme().font_size * ui::LEFT_DOCK_WIDTH, window, cx);

// 2) 局部间距 / 内距——用 Tailwind 尺度方法（取值天然受限）
div().gap_1().px_2().h_6()

// 3) 固定描边（不随字号缩放）
div().border_b(ui::HAIRLINE)
```

规则：**结构尺寸**先进 `ui.rs` 登记倍率；**局部间距**直接用 Tailwind 尺度，无需登记；不得出现裸 `px(N.)`。

常用常量（倍率 → @16px 实际值 / Tailwind 对照）：

| 用途 | 常量 | 值 | Tailwind |
| --- | --- | --- | --- |
| 标题栏高 | `TITLE_BAR_HEIGHT` | 2.25 → 36 | `h_9` |
| 活动栏宽 | `ACTIVITY_BAR_WIDTH` | 3.0 → 48 | `w_12` |
| 活动栏单项 | `ACTIVITY_ITEM_WIDTH` / `_HEIGHT` | 2.75 / 2.5 → 44/40 | `w_11` / `h_10` |
| 活动栏图标 | `ACTIVITY_ICON_SIZE` | 1.75 → 28 | `size_7` |
| 边栏起步宽 | `LEFT_DOCK_WIDTH` / `RIGHT_DOCK_WIDTH` | 15 / 17.5 → 240/280 | — |
| 列表/树行高 | `ROW_HEIGHT` | 1.5 → 24 | `h_6` |
| 树缩进 | `TREE_INDENT` / `TREE_BASE_PADDING` | 0.875 / 0.5 → 14/8 | — / `pl_2` |
| 面板头 | `PANEL_HEADER_HEIGHT` | 2.25 → 36 | `h_9` |
| 控件高 | `CONTROL_HEIGHT_MD` / `_SM` | 2.0 / 1.625 → 32/26 | `h_8` / `rems(1.625)` |
| 图标 | `ICON_SIZE_SM` / `ICON_SIZE_MD` | 0.875 / 1.0 → 14/16 | `size_3p5` / `size_4` |
| 间距 | `GAP_SM` / `GAP_MD` / `GAP_LG` | 0.25/0.5/0.75 → 4/8/12 | `gap_1` / `gap_2` / `gap_3` |
| 固定描边 | `HAIRLINE` / `ACTIVITY_ACCENT_BAR` / `TREE_ACTIVE_BAR` | 1px / 2px / 2px | `h_px` / `w_0p5` / `w_0p5` |

新增控件尺寸：**结构尺寸先在 `ui.rs` 登记倍率，再在视图换算引用**；局部间距直接用 Tailwind 尺度方法。

## 颜色

一律 `cx.theme().colors.<字段>`（需 `use gpui_kit::component::ActiveTheme`），禁止裸 hex。

| 场景 | 角色 |
| --- | --- |
| 侧边栏 | `sidebar` / `sidebar_border` / `sidebar_accent` / `sidebar_accent_foreground` |
| 活动栏 | `secondary`（背景）+ `sidebar_accent_foreground`（激活条 / 激活图标） |
| 标题栏 | `title_bar` / `title_bar_border`；挖空槽 `sidebar`（深一档） |
| 状态栏 | `primary`（背景）+ `primary_foreground`（文字，成对保证对比度） |
| 列表 / 表格 | `list*` / `table*` |
| 弹层 | `popover` / `overlay` / `border` |
| 焦点环 | `ring` |

## 组件规格速查

- **标题栏**：高 2.25rem，无窗口标题；logo 1.25rem；挖空槽 1.625rem 高 + 0.875rem 内距；Quick Open 入口 20rem×1.625rem；窗口控制由 `TitleBar` 组件自带
- **活动栏**：宽 3rem；单项 2.75×2.5rem；激活条 2px（左栏在左、右栏在右）
- **Dock 边栏**：左 15rem / 右 17.5rem 起步（`set_dock_size`，用户可拖拽）
- **状态栏**：品牌色 + 白字；左右各一个自绘「完全隐藏 / 恢复」开关
- **树节点**：行高 1.5rem；缩进 = 0.5rem + 0.875rem × depth；激活条 2px；标题 `text_sm`
- **列表行**：行高 1.5rem；`list` / `list_hover` / `list_active`
- **输入框 / 下拉框**：高 2rem（紧凑 1.625rem）；圆角 `theme.radius`；弹层 `popover` + 内距 0.5rem
- **Tab 标签**：头高 2.25rem；内距 `GAP_LG` / `GAP_SM`

完整规格表见 `docs/architecture/ui/ui-design-spec.md`。

## 相关 skill

- 五段布局 / 三模式 / Quick Open 交互规格：`rds-layout`
- 主题资产与语义 token：`rds-theme`
- GPUI-kit API 与编码规范：`gpui-kit-dev`

## 契约测试

改动尺寸后运行 `cargo test -p rds-workbench --test ui_contract`：它会校验
`ui.rs` 常量与设计值一致、视图层无裸 `px(...)` / 裸色值、边栏隐藏/恢复状态机。
