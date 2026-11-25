# RdataStation v2 UI 设计规范

> 状态：已落地 · 关联代码：`crates/workbench_shell/src/ui.rs`（尺寸常量，经 `crate::ui` 重导）、`assets/themes/rds-theme.json`（颜色 token）
> 关联设计：`docs/architecture/layout/layout-design.md`（布局）、`docs/architecture/theme/theme-design.md`（配色）
> 关联迁移：`docs/architecture/ui/gpui-kit-0.7-migration.md`（GPUI-kit 0.6.1 → 0.7.0 差异实测 + 0.7.0 → 0.7.1 追加段，多轨开发必读）
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
| 面板工具栏（`PanelToolbar`） | `PANEL_TOOLBAR_PADDING` | 0.5 | 8 | 容器左右内距 |
| | `PANEL_TOOLBAR_TEXT_INSET` | 0.75 | 12 | 8 + `Toolbar` `p_1`=4，面板内列表行 / 状态栏 / chips 同一条线 |
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
| | `ROW_ACTION_SIZE` | 1.25 | 20 | `rems(1.25)` |
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
| **标题栏项目下拉菜单** | 菜单最小宽 **200**（`PopupMenu::min_w(px(200.))`，`crates/project/src/ui/picker.rs::build_project_menu`）；行高 26（组件 `item_height`）；行数 = 规格条数（分隔线不算）。**宽度必须自己给**：`PopupMenuItem::Item` 的标签在 `w_full()` 容器里，不参与菜单宽计算，默认只有 `min_w(rems(8.))`=128、文本区约 84px —— 超过 5 个全角字的文案会画到菜单右缘之外（弹层不裁剪，2026-11-26 实测「在资源管理器中显示」溢出约 60px） | **条目语义**：当前项目读写命令（`重命名…` / `归档`）在只读时置灰，顶部插**一行说明**（`PopupMenuItem::label`，不是菜单项：无图标 / 无 hover / 不占 26 高行），文案压到 9 个全角字内（「只读：写操作不可用」）；**「归档」按状态出文案**（未归档「归档」、已归档「取消归档」+ 对勾）——不再用一条「归档 / 取消归档」让人猜；**同一目的地只留一个入口**（「项目设置…」已删，与「项目中心…」都落设置「项目」页）；条目配色沿用组件默认（禁用 `muted_foreground`），选中态由键盘 / hover 决定，禁用项不得进键盘序 |
| **Dock 边栏** | 起步宽左 `LEFT_DOCK_WIDTH` / 右 `RIGHT_DOCK_WIDTH`（`set_dock_size`，用户拖拽可调） | 面板背景 `background`，分隔 `border` |
| **状态栏** | 高由内容（约 1.5rem）；开关图标 `ICON_SIZE_SM` | 背景 `primary` + 文字 `primary_foreground`；左右各一个「完全隐藏 / 恢复」自绘开关 |
| **树节点** | 行高 `ROW_HEIGHT`（数据源导航树专用值见 §2.1 `NAV_ROW_*`：分组 22（`ROW_HEIGHT_COMPACT`，见下一行的原语）/ 连接 26 / 对象 22，字号沿用 `text_xs` 档）；缩进 = `TREE_BASE_PADDING` + `TREE_INDENT` × depth；激活条 `TREE_ACTIVE_BAR` | 标题 `text_sm`（导航树按密度预算取 `text_xs`）；激活态 `list_active` + `list_active_border`；**悬停不得覆盖激活态**（`list_hover` 只在未激活时生效）；类别靠**形状**区分，颜色只承载状态 |
| **树分组头** | 唯一原语 = `workbench_shell::tree::GroupHeader`（草稿箱四个分组头 + 数据源导航分组头共用）：行高 `ROW_HEIGHT_COMPACT`（22）；左右内距 `px_1p5`（6）；行级 `text_xs`；折叠指示 `disclosure_slot()`（10px，属缩进算式）+ `disclosure_icon()`；组名 `MEDIUM` + 省略号（`label_max_width()` 供导航按 `NAV_NAME_MAX_W` 压缩）；槽位 `leading`（组标识，导航 2px 主色竖条）、`label_icon`（组名前语义图标，如书签）、`trailing`（常驻尾部：健康点 / 计数 / 批量动作）、`hover_action()`（hover 才显，逐次累加）；**改这里就是改全部五个分组头**；**底色必须通栏**（2026-11-25）：分组头是分区条不是可点行，`group_box` 色带要贴到面板/列表左缘，行级左右内距下沉在**每行自己的根**上（导航 body 不得带 `px_1`——带上之后色带左缘会留一道 4px 面板底色竖缝，旧草稿箱【草稿】组即此病） | 底色 `group_box`（凹槽档，比面板底暗一档）；**不用 `sidebar_accent`**——它在各主题包里与 `list.hover` 同值，分组头会常态像被 hover；`hover_background()` 给导航沿用 `list.hover` 提亮；**计数是纯文本、不是胶囊**（2026-11-25 用户口径）：`GroupHeader::count(n, color)` 用 `tree::group_count` 把数字排在**组名之后**，间隔 `GROUP_COUNT_GAP`（0.5rem = 8px，与行 `gap_1` 合计 ≈ 两个字宽），无底色 / 无圆角 / 无最小宽——`badge::Badge` 只留状态语义角标（活动栏角标 / 「连接中」呼吸点），分组计数不用；**重命名在原组名那一格就地编辑**（2026-11-25）：`GroupHeader::label_editor` 挂 `Input`（`XSmall` = 20 高，落在 22 的行里垂直居中），打开即聚焦 + 全选，Enter / Blur 提交——不得另起一行或在弹窗里输入；右键**只有一个改名入口**（「重命名分组」= 上句的原地编辑；原「编辑分组…」弹窗项已删，两项做的是同一件事，连描述一起改走面板头「管理分组」）；导航的拖拽 / 两种落点 / 右键菜单留在外层 `Stateful<Div>`，视觉与点击都在原语内 |
| **列表行** | 行高 `ROW_HEIGHT` | `list` / `list_hover` / `list_active` |
| **表格（结果集 / Mock 预览）** | 密度 `RESULT_TABLE_SIZE` / `PREVIEW_TABLE_SIZE` = 组件 `XSmall`（行高 26px；单元格字号 `text_xs`，左右内距 `px_2` + 组件内距）；行号槽宽 48px钉左（`RESULT_ROW_NUMBER_WIDTH` / `PREVIEW_ROW_NUMBER_WIDTH`）；列宽可拖（默认 100px，结果集另给 `RESULT_COLUMN_MIN_WIDTH`） | 选中 / 悬停 / 斑马纹 / 表头底色走主题的 `table.*`（与 `list.*` 同值：选中底 `table.active.background` = `list.active.background`）；`NULL` 等字串渲染为灰 + 斜体（与字符串 `"NULL"` 区分）；单元格取值与**列名**都截断并挂**悬停全文**（长 JSON / 长文本 / 长列名没有别的地方能看全；宽度 `RESULT_TOOLTIP_MAX_WIDTH` / `PREVIEW_TOOLTIP_MAX_WIDTH` = 24rem，超长折行；行号槽不挂——它不是数据）；**排序入口是表头右端的箭头**（组件自带方向循环，跟视图内排序），要全局排序走右键（结果集「下发源库」/ 预览「重查」）；右键复制给 此值 / 此列 / 整行（TSV，含制表符 / 换行 / 引号时按 `shared::string::tsv_row` 转义——**两张表共用同一份实现**）；网格里 `Ctrl+C` = 复制选中的一格（原文）/ 一整行（TSV），编辑区里 `Ctrl+C` 仍是内核的文本复制（两个 `key_context` 互不覆盖）；行把手只有 `#` 一列（组件自带的窄行头关掉，选整行走“再点一下已选中的格子”） |
| **面板工具栏（面板头）** | 唯一容器 = `workbench_shell::panel_toolbar::PanelToolbar`；高 `PANEL_HEADER_HEIGHT`（面板内第二行工具条用 `.height()` 传 32 / 28 / 30）；左右内距 `PANEL_TOOLBAR_PADDING`（文字左缘 `PANEL_TOOLBAR_TEXT_INSET` = 12）；左槽可伸缩、右槽固定、`secondary` 次行；**三槽全空整条不画**（零高）；**面板第一行（面板头 / 工具栏）紧贴 dock 标签条下缘**（2026-11-25）：面板根零顶距（`v_flex().size_full().min_h_0()` 后直接 `.child(toolbar)`），工具栏不得当 `p_2` / `pt_2` 容器的孩子——`inner_padding` 覆写为 `false` 只去掉 dock 那层 8px，面板根自己再写一层就是第二条缝 | 底色 `toolbar.background`（比内容区 `background` 高一档，面板头与内容区靠底色分开）+ 下缘 1px `border.variant`（弱一档，不用面板间的 `border`，避免每个面板都像被硬切一刀）；两者都是本仓 vendor patch 新增的主题键（对齐 Zed `toolbar_background` / `border_variant`）；下面紧接同底色容器时 `.no_background()` 关掉底色；槽内控件自己做 size 档（容器不托管 size），左缘不再由各面板各写 `px_2` / `px_3` / `p_0` |
| **面板底状态行** | 高 `ROW_HEIGHT`（单行 `text_xs`），水平内距 `px_2p5` | 无底色（面板底）+ `muted_foreground`；上缘 1px `border` 分隔；内容为空时**整行不渲染**（不占高） |
| **输入框** | 高 `CONTROL_HEIGHT_MD`（紧凑 `CONTROL_HEIGHT_SM`；单行 `Input` 的 `.h()` 只对多行生效，行内高度取组件 `Size` 档——`small` = 24） | 圆角 `theme.radius`；边 `border` / `input_border`；底 `background`；**面板内搜索框统一一档**（2026-11-25）：`bordered(false)`（只去边框、**保留** `input.background` 填充底；`appearance(false)` 才会连底一起去）+ `focus_bordered(true)`（聚焦才出框）——导航 / 草稿箱 / 数据对象 / 资产库四处同款，不再有的填充底、有的 1px 亮边框；**同一搜索面板里的多行输入同档**（2026-11-25）：搜索 / 替换 / 包含 / 排除四行共用同一个容器内距（`px_1p5`）与间距（`gap_1`），不得只给其中一行另加一层更暗的底色容器——`input.background` 是 `input` 混 30% 透明的**半透明**底（`vendor/gpui-component/src/theme/mod.rs::input_background`），叠在不同底色上会显出两种深浅（实测 `#282828` vs `#2D2D2D`） |
| **下拉框** | 同输入框 | 弹层 `popover` + 圆角 `theme.radius` + 内距 `PANEL_PADDING`，最大高 ≈ `ROW_HEIGHT` × 10 |
| **Tab 标签** | 头高 `PANEL_HEADER_HEIGHT`；内距 `GAP_LG` / `GAP_SM` | 激活 `tab_active` + `tab_active_foreground`；非激活 `tab` + `tab_foreground`；底 `tab_bar` |
| **图标（微标）** | `ICON_SIZE_SM` / `ICON_SIZE_MD` | 取色由调用点定（文字色 / 语义色）；**通用图标用组件库自带的 Lucide 全量集**，数据库类型图标与品牌标口径见 `db-icons.md` |
| **徽标（计数徽章）** | 最小 `NAV_BADGE_SIZE`（1.125rem = 18px）；内距 3/5px（Tailwind `px_1p25` / `py_0p75`）；**圆角 11px 胶囊**；字号 11px 基准 → @16px 取 `text_sm`（13–14px）；长文本变体 `.long` 圆角 2px | 背景 = 状态语义色（`info` / `danger` / `success` 等）+ 前景 `primary_foreground` 类；**活动栏角标特例**：16px 高、圆角 20px、字号 ≥10–11px（不随 VS Code 取 9px——16px 基准下不可读）；**分组头的条数不用它**（2026-11-25：纯文本紧跟组名，见「树分组头」行）；**树行上的计数也不用它**（2026-11-25 用户口径「统一收成纯文本」：草稿箱内容命中数（过滤态树行）/ 搜索结果每文件命中数都改 `tree::group_count` 纯文本数字紧跟文件名，原 `tree::count_badge` 胶囊助手随之删除——`badge::Badge` 只留状态点与活动栏角标）；规格来源：VS Code `countBadge`（2026-10-12 实证，见 `vscode-ui-design-system.md` §4.5） |
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
| 五段布局应用 | `crates/workbench/src/view/`：`dock.rs`（`render_title_bar` / `render_*_activity_bar` / `apply_*_mode`）、`actions.rs`（`render_status_bar` / `toggle_quick_open`）、`workspace.rs`（`init_workspace`） |
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
| 源码扫描 | `view/` / `panels/` 不出现裸 `px(...)` 与裸 `rgb(...)` / `hsla(...)` |
| 状态机 | 「完全隐藏 / 恢复」往返还原隐藏前模式（不丢失「收起」）+ 异常快照兜底 |

> 新增结构尺寸或改变设计值时，同步修改 `ui.rs`、本文档 §2.1 与契约测试三处；测试失败即回调未同步。

## 8. 参考：navop 的可借鉴点

navop 与本题技术栈同源（GPUI + gpui-component），值得借鉴的是**工程做法**而非组件代码（其 gpui-component 为自维护 fork，`LayoutSizeTokens`、`Tree` 的 `base_padding/indent` 等在 longbridge 0.6.1 中不存在）：

1. **集中常量**：`crates/core/src/layout.rs` 统一布局尺寸（sidebar / toolbar）。
2. **就近常量**：组件尺寸常量定义在组件文件顶部（如 `TAB_MIN_WIDTH`、`TREE_PANEL_DEFAULT_SIZE`）。
3. **经验沉淀**：`AGENTS.md` 记录 GPUI 布局踩坑（如「Windows 标题栏按钮必须截断后方 Drag hitbox」——与本项目标题栏问题同源）。
4. **布局契约测试**：用测试锁死布局不变式（如 tab 容器高度、侧栏边界）。

> 数值不照抄：navop 侧边栏 320px，本项目为左 240 / 右 280（见 `layout-design.md` §2.3）。
