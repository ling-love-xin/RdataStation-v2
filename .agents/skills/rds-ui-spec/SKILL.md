---
name: rds-ui-spec
description: RdataStation UI 尺寸与颜色约束：rem 尺寸常量（crates/workbench_shell/src/ui.rs，经 `crate::ui` / `workbench_shell::ui` 引用）、主题 token 取色、组件规格表（标题栏/活动栏/状态栏/树节点/列表/输入框/下拉/tab）。编写或修改任何 UI 布局、控件尺寸、间距、颜色时使用。
---

# RdataStation UI 规范

**硬约束**：视图代码不得出现与布局结构相关的裸 `px(N.)`、裸 `font_size * N.`、裸 hex/rgb 颜色。

三层约束，任何 UI 改动都落在其中一层：

1. 颜色 / 圆角 / 字号 → 主题 token（`cx.theme().colors.*`、`theme.radius`）
2. 尺寸 / 间距 → `crates/workbench_shell/src/ui.rs` 常量（`crate::ui` 是它的重导，路径不变）
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

> **`ui.rs` 住在 `crates/workbench_shell/src/ui.rs`**（2026-09-16 自 `crates/workbench/src/ui.rs` 下沉到外壳 crate，
> 目的：视图下沉特性 crate 时不再反向依赖 workbench）。workbench 侧 `pub use workbench_shell::ui;` 重导，
> 所以 `crate::ui::*` / `rds_workbench::ui::*` 写法全部不变；直接依赖 shell 的 crate 写 `workbench_shell::ui::*`。
> 新增常量仍登在这个文件（单一来源），但需注意 **shell 不得依赖任何特性 crate**。

规则：**结构尺寸**先进 `ui.rs` 登记倍率；**局部间距**直接用 Tailwind 尺度，无需登记；不得出现裸 `px(N.)`。

> 单位陷阱：`rems(x)` 的基准是**主题字号**（默认 16px，`rems(1.) = 16px`），与 Tailwind 方法名里的数字（`gap_1 = 4px`）是两套单位。不要把 px 值手写 `/ 4.` 后交给 `rems()`（会把尺寸放大 4 倍，项目内曾出现两处）。

> 树 / 列表缩进统一用设计公式：`ui::TREE_BASE_PADDING + depth as f32 * ui::TREE_INDENT`（= 0.5rem + 0.875rem × depth），不要各写各的倍率。
>
> **行级共用原语在 `workbench_shell::tree`**（2026-09-20 起，消费方：导航 M4 / 草稿箱 M5 / 资产库 M6 / Mock M7）：
> `active_bar(color)`（选中行左侧 2px 条，调用方行需 `relative()`）、`disclosure_slot()`（固定 `w_2p5` 槽，**槽宽是缩进算式的一部分**）、
> `disclosure_icon(expanded, color)`（12px chevron，**四面板统一用它**；旧的字符载体 `disclosure_glyph` 已删）、
> `indent_spacer(step, depth)` / `indent_rem(base, step, depth)`、
> 以及行高预算 `RowHeight`（基础高 + 附加块）与 `row_sizes(n, rem, f)`（虚拟列表 `item_sizes`）。
> 新增树 / 列表行请**先看这一层有没有**，不要在面板里再写一遍（导航 / 草稿箱 / 资产库此前各写了一份）。
> 行态口径（悬停不覆盖选中 / 选中底 `list_active` / 定高行截断 / 命中区下限）收在
> `docs/architecture/theme/ui-constraints.md` §8.3——**改口径改那一处**，别在各面板各拍一次。

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
| 图标 | `ICON_SIZE_XS` / `ICON_SIZE_SM` / `ICON_SIZE_MD` | 0.75 / 0.875 / 1.0 → 12/14/16 | `size_3` / `size_3p5` / `size_4` |
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
- **树节点**：行高 1.5rem（数据源导航树按虚拟列表用 `NAV_ROW_*`：分组 24 / 连接 26 / 对象 22）；缩进 = 0.5rem + 0.875rem × depth；激活条 2px；标题 `text_sm`（导航树按密度用 `text_xs`）；**悬停不覆盖激活态**；类别靠形状（图标）而非颜色
- **列表行**：行高 1.5rem；`list` / `list_hover` / `list_active`
- **输入框 / 下拉框**：高 2rem（紧凑 1.625rem）；圆角 `theme.radius`；弹层 `popover` + 内距 0.5rem
- **Tab 标签**：头高 2.25rem；内距 `GAP_LG` / `GAP_SM`

完整规格表见 `docs/architecture/ui/ui-design-spec.md`。

## 组件规格总表（上游标准组件 · 0.7.1 源码实测）

下表是 gpui-kit 0.7.1 标准组件的尺寸档位，全部实测自 vendor 源码（非估计）。
写视图时**优先用组件的 `Size` API / 档位方法**（`h_8()`、`px_2()`、`text_sm()`、`size_8()`），
像素值仅作 @16px rem 核对；同一组件内字号、图标、内距、命中区随 Size 一起变。

| 组件 | 维度 | XSmall | Small | Medium | Large | 源码位置 |
| --- | --- | --- | --- | --- | --- | --- |
| Button 常规 | 高度 | `h_5` 20 | `h_6` 24 | `h_8` 32 | `h_8` 32 | button.rs:671-683 |
| Button 常规 | 横内距 | `px_1` 4 | `px_2` 8 | `px_2p5` 10 | `px_3` 12 | 同上 |
| Button 图标 | 尺寸 | `size_5` 20 | `size_6` 24 | `size_8` 32 | `size_8` 32 | button.rs:661-666 |
| Button | 圆角 | `radius*0.5` | `radius*0.5` | `radius` | `radius*2` | button.rs:636-638 |
| Button / 输入 | 字号 | `text_xs` 12 | `text_sm` 14 | `text_sm` 14 | `text_base` 16 | sizing.rs:245-253 |
| Input | 高度 | `h_5` 20 | `h_6` 24 | `h_8` 32 | `h_11` 44 | sizing.rs:281-289 |
| Input | 横内距 | 4 | 8 | 10 | 12 | sizing.rs:147-152 |
| Input | 竖内距 | 0 | 2 | 4 | 8 | sizing.rs:160-163 |
| List / Menu 行 | 竖内距 | 0 | 2 | 4 | 8 | sizing.rs:302-308 |
| Menu 项 | 内距 | `py_1().px_2()` = 4/8 | | | | menu_item.rs:101-102 |
| Table 行 | 行高 | 26 | 30 | 32 | 40 | sizing.rs:57-65 |
| Table 单元格 | padding(上/左右) | 2/4 | 3/6 | 4/8 | 8/12 | sizing.rs:69-96 |
| 通用元素 | `size()` | `size_4` 16 | `size_5` 20 | `size_8` 32 | `size_11` 44 | sizing.rs:312-320 |
| Dialog 头/脚 | 内部间距 | `gap_2` 8 | | | | dialog/header.rs:46 footer.rs:52 |
| Tab 标签 | 内距 | 横 `px(12)`（tab.rs:71 默认） | | | | tab/tab.rs:71 |

> 密度档位（design-guides）：**标准（Medium）为默认**；舒适/Large 用于引导与稀疏表单；
> 紧凑/Small 用于工具栏与数据密集面；XSmall 仅限工具类密度，勿整应用用。
> 按钮帧 20/24/32、输入至 44、表格行 26/30/32/40 —— 即上表的档位跨度。

## 空间语法（关系 → 间距 token）

间距表达关系，选 token 先问"两个东西彼此是什么关系"（design-guides §Spatial grammar）：

| 关系 | token | @16px | 例 |
| --- | --- | --- | --- |
| 光学修正 | `xxs` | 2 | 图标基线、紧凑分隔 |
| 控件内部 | `xs` | 4 | 菜单图标/文案、标题/描述 |
| 紧密控件 | `sm` | 8 | 按钮图标/文案、对话框操作 |
| 一个内容组 | `md` | 12 | 通知列、紧凑表单行 |
| 同节内不同组 | `lg` | 16 | 面板内距、表单组 |
| 不同节 | `xl` | 24 | 页面/检查器大块 |
| 区域边界 | `xxl` | 32 | 空态呼吸、页面带 |

规则：**内距先于外距**（组件内距属组件、组件间间距属父级）；标题与描述的间距小于描述到下一节；
重复行保持图标/文案/值/徽标/尾操作稳定列；垂直间距体现分组（等距=等关系）；
**不重复内距**（放进已有内距的面板里的卡片不再加整档内距）。

## 相关 skill

- 五段布局 / 三模式 / Quick Open 交互规格：`rds-layout`
- 主题资产与语义 token：`rds-theme`
- GPUI-kit API 与编码规范：`gpui-kit-dev`

## 契约测试

改动尺寸后运行 `cargo test -p rds-workbench --test ui_contract`：它会校验
`ui.rs` 常量与设计值一致（契约 1 走 `rds_workbench::ui::*`，不读路径，故下沉后无需改）、
视图层无裸 `px(...)` / 裸色值、边栏隐藏/恢复状态机。

> 范围：尺寸与色值契约目前只扫 `view.rs` / `panels/`（`panels/` 下新增子模块由 `ui_contract.rs` 的 2c 守卫强制登记）；`components/connection_dialog/` 与 `project/ui.rs` 仍是存量欠债（见 `connection-dialog-architecture.md` §14 #14）。新增代码请按本 skill 写，不要以这两个目录的既有写法为样例。
