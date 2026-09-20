# RdataStation v2 UI 设计规范

> 状态：已落地 · 关联代码：`crates/workbench_shell/src/ui.rs`（尺寸常量，经 `crate::ui` 重导）、`assets/themes/rds-theme.json`（颜色 token）
> 关联设计：`docs/architecture/layout/layout-design.md`（布局）、`docs/architecture/theme/theme-design.md`（配色）
> 参考：navop（GPUI 同栈项目）的「集中常量 + 就近常量 + 布局契约」实践

## 1. 目的

解决「组件尺寸没有约束、布局经常乱跑」的问题。核心思路是建立**三层约束**，任何 UI 改动都必须落在其中一层：

```
第 1 层  主题 token     颜色 / 圆角 / 字号         → assets/themes/rds-theme.json
第 2 层  尺寸常量       栏宽栏高 / 控件高 / 间距   → crates/workbench_shell/src/ui.rs
第 3 层  组件规格       各组件如何组合上述两者     → 本文档 §5
```

**硬约束**：视图代码中不得出现与布局结构相关的裸 `px(N.)` 或裸 `font_size * N.`。

## 2. 尺寸基准（rem + 固定描边）

尺寸分三类，统一以 **rem** 为基准（1rem = 主题正文字号，默认 16px）：

| 类型 | 声明位置 | 表达方式 | 适用 |
| --- | --- | --- | --- |
| 结构尺寸倍率 | `crates/workbench_shell/src/ui.rs`（`f32`，经 `crate::ui` 重导） | 样式：`rems(ui::TITLE_BAR_HEIGHT)`（gpui 原生）；需 `Pixels` 的 API（如 `set_dock_size`）：`cx.theme().font_size * ui::LEFT_DOCK_WIDTH` | 栏宽栏高、控件高、Quick Open 尺寸 |
| 局部间距 | 无需声明 | gpui 的 Tailwind 尺度方法：`gap_1` / `px_2` / `h_9` | 间距、内距等局部尺寸（取值天然受限） |
| 固定描边 | `crates/workbench_shell/src/ui.rs`（`Pixels`） | `ui::HAIRLINE` | 1px 边框、2px 激活条等不应缩放的细线 |

> `ui.rs` 只声明**设计倍率**（数值依据，可追溯到设计文档）；rem → px 换算交给 gpui 原生 `rems()` 或主题字号，自动随字号 / DPI 缩放。

### 2.1 尺寸常量表（倍率 → @16px 实际值，Tailwind 方法对照）

| 分组 | 常量 | 倍率 | @16px | Tailwind |
| --- | --- | --- | --- | --- |
| 五段布局 | `TITLE_BAR_HEIGHT` | 2.25 | 36 | `h_9` |
| | `ACTIVITY_BAR_WIDTH` | 3.0 | 48 | `w_12` |
| 活动栏 | `ACTIVITY_ICON_SIZE` | 1.75 | 28 | `size_7` |
| | `ACTIVITY_ITEM_WIDTH` | 2.75 | 44 | `w_11` |
| | `ACTIVITY_ITEM_HEIGHT` | 2.5 | 40 | `h_10` |
| Dock 边栏 | `LEFT_DOCK_WIDTH` | 15.0 | 240 | — |
| | `RIGHT_DOCK_WIDTH` | 17.5 | 280 | — |
| 标题栏 | `TITLE_LOGO_SIZE` | 1.25 | 20 | `size_5` |
| | `TITLE_BAR_PADDING_X` | 0.625 | 10 | `pl_2p5` |
| | `TITLE_SLOT_HEIGHT` | 1.625 | 26 | `rems(1.625)` |
| | `TITLE_SLOT_PADDING_X` | 0.875 | 14 | `px_3p5` |
| | `QUICK_OPEN_ENTRY_WIDTH` | 20.0 | 320 | `w_80` |
| | `QUICK_OPEN_ENTRY_HEIGHT` | 1.625 | 26 | `rems(1.625)` |
| | `QUICK_OPEN_ENTRY_PADDING_X` | 0.625 | 10 | `px_2p5` |
| | `QUICK_OPEN_PANEL_WIDTH` | 35.0 | 560 | — |
| | `QUICK_OPEN_PANEL_TOP` | 2.625 | 42 | — |
| | `QUICK_OPEN_PANEL_MAX_HEIGHT` | 26.25 | 420 | — |
| 列表 / 树 | `ROW_HEIGHT` | 1.5 | 24 | `h_6` |
| | `ROW_HEIGHT_COMPACT` | 1.375 | 22 | `h(rems(…))` |
| | `TREE_INDENT` | 0.875 | 14 | — |
| | `TREE_BASE_PADDING` | 0.5 | 8 | `pl_2` |
| | `PANEL_HEADER_HEIGHT` | 2.25 | 36 | `h_9` |
| 数据源连接行（v7） | `NAV_BADGE_SIZE` | 1.125 | 18 | — |
| | `NAV_SCOPE_COL_SHORT` | 2.4 | 38 | — |
| | `NAV_SCOPE_COL_TEXT` | 3.4 | 54 | — |
| | `NAV_ROW_ACTION_SIZE` | 1.25 | 20 | `size_5` |
| 导航树行高（虚拟列表） | `NAV_ROW_GROUP` | 1.5 | 24 | `h(rems(…))` |
| | `NAV_ROW_CONNECTION` | 1.625 | 26 | `h(rems(…))` |
| | `NAV_ROW_TREE` | 1.375 | 22 | `h(rems(…))` |
| | `NAV_SUBLINE` | 1.125 | 18 | `h(rems(…))` |
| | `NAV_EDITOR_TAG` | 3.5 | 56 | `h(rems(…))` |
| | `NAV_EDITOR_COPY` | 4.5 | 72 | `h(rems(…))` |
| | `NAV_EDITOR_GROUP` | 5.5 | 88 | `h(rems(…))` |
| 草稿箱行（虚拟列表） | `SCRATCHPAD_ROW_CHIPS` | 1.625 | 26 | `h(rems(…))` |
| 控件 / 图标 | `CONTROL_HEIGHT_SM` | 1.625 | 26 | `rems(1.625)` |
| | `CONTROL_HEIGHT_MD` | 2.0 | 32 | `h_8` |
| | `ICON_SIZE_XS` | 0.75 | 12 | `size_3` |
| | `ICON_SIZE_SM` | 0.875 | 14 | `size_3p5` |
| | `ICON_SIZE_MD` | 1.0 | 16 | `size_4` |
| 间距 | `GAP_SM` / `GAP_MD` / `GAP_LG` | 0.25 / 0.5 / 0.75 | 4 / 8 / 12 | `gap_1` / `gap_2` / `gap_3` |
| | `PANEL_PADDING` | 0.5 | 8 | `p_2` |
| 属性面板（编辑区右侧） | `PROPERTY_PANEL_MIN_WIDTH` | 13.75 | 220 | — |
| | `PROPERTY_PANEL_MAX_WIDTH` | 47.5 | 760 | — |
| 固定描边 | `HAIRLINE` | — | 1px | `h_px` |
| | `ACTIVITY_ACCENT_BAR` | — | 2px | `w_0p5` |
| | `TREE_ACTIVE_BAR` | — | 2px | `w_0p5` |
| | `NAV_GROUP_BAR_WIDTH` | — | 2px | `w_0p5` |

## 3. 颜色约束

一律取 `cx.theme().colors.<字段>`（需 `use gpui_kit::component::ActiveTheme`），**禁止裸 hex/rgb**。

### 3.1 常用角色

| 场景 | 角色 |
| --- | --- |
| 窗口 / 编辑器底 | `background`、`foreground` |
| 侧边栏 | `sidebar`、`sidebar_border`、`sidebar_accent`、`sidebar_accent_foreground` |
| 活动栏 | `secondary`（背景）、`sidebar_accent_foreground`（激活条 / 激活图标） |
| 标题栏 | `title_bar`、`title_bar_border`、`sidebar`（挖空槽，深一档） |
| 状态栏 | `primary`（背景）+ `primary_foreground`（文字，成对保证对比度） |
| 列表 / 表格 | `list`、`list_hover`、`list_active`、`table_*` |
| 弹层 | `popover`、`overlay`、`border` |
| 语义 | `danger` / `success` / `warning` / `info` |
| 焦点环 | `ring` |

### 3.2 本轮补齐的 token

`rds-theme.json` 新增（此前缺失导致组件回退默认色）：`ring`、`window.border`、`secondary.active.background`、`sidebar.primary.background`、`sidebar.primary.foreground`。

> 状态栏**不新增** `status_bar.background`：设计决策为 `primary` 派生（`theme-design.md` §5.4），`primary`/`primary_foreground` 为成对调校色。

## 4. 组件规格

| 组件 | 尺寸 | 颜色 / 其他 |
| --- | --- | --- |
| **标题栏** | 高 `TITLE_BAR_HEIGHT`；logo `TITLE_LOGO_SIZE`；挖空槽高 `TITLE_SLOT_HEIGHT` + 水平内距 `TITLE_SLOT_PADDING_X`；Quick Open 入口 `QUICK_OPEN_ENTRY_WIDTH`×`QUICK_OPEN_ENTRY_HEIGHT` | 背景 `title_bar`，底边 `title_bar_border`；挖空槽 `sidebar`；窗口控制由 `TitleBar` 组件自带 |
| **活动栏** | 宽 `ACTIVITY_BAR_WIDTH`；单项 `ACTIVITY_ITEM_WIDTH`×`ACTIVITY_ITEM_HEIGHT`；图标 `ACTIVITY_ICON_SIZE`；激活条 `ACTIVITY_ACCENT_BAR` | 背景 `secondary`；激活条 / 激活图标 `sidebar_accent_foreground` |
| **Dock 边栏** | 起步宽左 `LEFT_DOCK_WIDTH` / 右 `RIGHT_DOCK_WIDTH`（`set_dock_size`，用户拖拽可调） | 面板背景 `background`，分隔 `border` |
| **状态栏** | 高由内容（约 1.5rem）；开关图标 `ICON_SIZE_SM` | 背景 `primary` + 文字 `primary_foreground`；左右各一个「完全隐藏 / 恢复」自绘开关 |
| **树节点** | 行高 `ROW_HEIGHT`（数据源导航树专用值见 §2.1 `NAV_ROW_*`：分组 24 / 连接 26 / 对象 22，字号沿用 `text_xs` 档）；缩进 = `TREE_BASE_PADDING` + `TREE_INDENT` × depth；激活条 `TREE_ACTIVE_BAR` | 标题 `text_sm`（导航树按密度预算取 `text_xs`）；激活态 `list_active` + `list_active_border`；**悬停不得覆盖激活态**（`list_hover` 只在未激活时生效）；类别靠**形状**区分，颜色只承载状态 |
| **列表行** | 行高 `ROW_HEIGHT` | `list` / `list_hover` / `list_active` |
| **面板底状态行** | 高 `ROW_HEIGHT`（单行 `text_xs`），水平内距 `px_2p5` | 无底色（面板底）+ `muted_foreground`；上缘 1px `border` 分隔；内容为空时**整行不渲染**（不占高） |
| **输入框** | 高 `CONTROL_HEIGHT_MD`（紧凑 `CONTROL_HEIGHT_SM`） | 圆角 `theme.radius`；边 `border` / `input_border`；底 `background` |
| **下拉框** | 同输入框 | 弹层 `popover` + 圆角 `theme.radius` + 内距 `PANEL_PADDING`，最大高 ≈ `ROW_HEIGHT` × 10 |
| **Tab 标签** | 头高 `PANEL_HEADER_HEIGHT`；内距 `GAP_LG` / `GAP_SM` | 激活 `tab_active` + `tab_active_foreground`；非激活 `tab` + `tab_foreground`；底 `tab_bar` |
| **图标（微标）** | `ICON_SIZE_SM` / `ICON_SIZE_MD` | 取色由调用点定（文字色 / 语义色）；**通用图标用组件库自带的 Lucide 全量集**，数据库类型图标与品牌标口径见 `db-icons.md` |
| **Quick Open 弹层** | 宽 `QUICK_OPEN_PANEL_WIDTH`；顶部 `QUICK_OPEN_PANEL_TOP`；最大高 `QUICK_OPEN_PANEL_MAX_HEIGHT` | `popover` + `border` + `shadow_lg` |

### 4.1 动效（2026-09-20 起）

动效只用来**解释变化**，不做常驻装饰——默认不加；只有「过程进行中」这类**暂态**才挂（上游规范：Motion 一节）。

- 一律用 gpui-kit 自带的 `Animation` + `AnimationExt::with_animation`（值补间 / 关键帧走 `gpui_base::motion`；入场退场用 `EffectTransition`）；**不手搓 `request_animation_frame`**（会绕过 `reduce_motion` 兜底）。
- 循环动效的缓动必须**首尾同值**（`pulsating_between` / `bounce`），否则每圈接缝会跳一下；动画 id 用**业务键**（不是下标），循环动画**只挂暂态**（挂稳态 = 窗口每帧重绘）。
- 只能改变透明度 / 颜色 / 位移：gpui 的样式没有 transform，`Transformation` 仅服务 SVG（`div` 缩不了）。
- 已知动效常量：`NAV_BADGE_PULSE_MS`（1.5s，「连接中」徽标呼吸）。细节与坑见 skill `gpui-kit-dev` §动效。

## 5. 落地方式

| 决策 | 位置 |
| --- | --- |
| 尺寸常量定义 | `crates/workbench_shell/src/ui.rs`（外壳 crate；workbench 侧 `pub use workbench_shell::ui;` 重导，路径不变） |
| 颜色 token | `assets/themes/rds-theme.json` |
| 五段布局应用 | `crates/workbench/src/view.rs`（`render_title_bar` / `render_*_activity_bar` / `render_status_bar` / `render_quick_open` / `apply_*_mode`） |
| 面板 / 树 / 列表应用 | `crates/workbench/src/panels/` |

## 6. 检查清单

- [ ] 视图里没有裸 `px(N.)`（除 `ui::` 常量与纯装饰的 `HAIRLINE`）
- [ ] 没有裸 hex / rgb 颜色
- [ ] 新增尺寸先进 `ui.rs`，再引用
- [ ] 动效走现成 API（§4.1），循环动效只挂暂态、缓动首尾同值、id 用业务键
- [ ] 组件高度/内距取自本文档 §4 表格
- [ ] 明暗两套主题下都核对对比度与可见性

## 7. 布局契约测试

关键不变式已固化为测试：`crates/workbench/tests/ui_contract.rs`（`cargo test -p rds-workbench --test ui_contract`）。

> **为什么常量不在 workbench 里**（2026-09-16 下沉）：视图要下沉到特性 crate（`database` / `scratchpad`）时，
> 它们需要这套常量，而又不能反向依赖 `workbench`（会成环）。`crates/workbench_shell` 只依 `gpui-kit`、
> 零内部依赖，两边都可依赖它——详见 `docs/architecture/layout/panels-coupling-plan.md` §9。

| 契约 | 覆盖内容 |
| --- | --- |
| 尺寸常量 | `ui.rs` 倍率 × 16 等于本文档 §2.1 设计值（标题栏 36 / 活动栏 48 / 边栏 240 与 280 等） |
| 源码扫描 | `view.rs` / `panels/` 不出现裸 `px(...)` 与裸 `rgb(...)` / `hsla(...)` |
| 状态机 | 「完全隐藏 / 恢复」往返还原隐藏前模式（不丢失「收起」）+ 异常快照兜底 |

> 新增结构尺寸或改变设计值时，同步修改 `ui.rs`、本文档 §2.1 与契约测试三处；测试失败即回调未同步。

## 8. 参考：navop 的可借鉴点

navop 与本题技术栈同源（GPUI + gpui-component），值得借鉴的是**工程做法**而非组件代码（其 gpui-component 为自维护 fork，`LayoutSizeTokens`、`Tree` 的 `base_padding/indent` 等在 longbridge 0.6.1 中不存在）：

1. **集中常量**：`crates/core/src/layout.rs` 统一布局尺寸（sidebar / toolbar）。
2. **就近常量**：组件尺寸常量定义在组件文件顶部（如 `TAB_MIN_WIDTH`、`TREE_PANEL_DEFAULT_SIZE`）。
3. **经验沉淀**：`AGENTS.md` 记录 GPUI 布局踩坑（如「Windows 标题栏按钮必须截断后方 Drag hitbox」——与本项目标题栏问题同源）。
4. **布局契约测试**：用测试锁死布局不变式（如 tab 容器高度、侧栏边界）。

> 数值不照抄：navop 侧边栏 320px，本项目为左 240 / 右 280（见 `layout-design.md` §2.3）。
