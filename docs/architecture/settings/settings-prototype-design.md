# 设置页（应用级）· 原型设计

> 状态：**首版（2026-09-16，待迭代）** · 本文回答"设置页长什么样"，"什么能进设置页"归 `settings-architecture.md`
> 关联：`settings-architecture.md`（准入与作用域裁决）、`settings-prototype.html`（交互稿，**示意稿非权威**）、`settings-dev-plan.md`（开发方案）、`settings-crate-design.md`（crate 沿革）、`../layout/layout-design.md`（入口与五段布局）、`../ui/ui-design-spec.md`（三层约束）、`../theme/theme-design.md`（配色）
> 落地进度（2026-09-16）：僵尸项裁撤 + 代码侧登记表 + **两栏页面实体**（`crates/settings/src/settings_page.rs`）已落地；**宿主替换未做**（工作台仍渲染旧的单列 `settings_view`），见 `settings-dev-plan.md` §0/§2。
> 技术栈：gpui-kit 0.6.1，组件只从组件库取（禁止手搓），取色零裸 hex，尺寸只引用 `crates/workbench/src/ui.rs` 常量

## 1. 设计基准

| 维度 | 取值 | 来源 |
| --- | --- | --- |
| 形态 | 居中模态「两栏 Dialog」，与 Quick Open 同级弹层 | `layout-design.md` §4（"`settings_view.rs` 以 Dialog 呈现（与 Quick Open 同级弹层）"） |
| 尺寸 | 61.25 × 35 rem（980 × 560 px），居中；内容区内部滚动 | 宽度复用「新建数据源连接」同档；新增 5 个常量（§7） |
| 配色 | 弹层 `popover` / 导航与行 `list*` / 文字 `foreground`·`muted_foreground` / 危险 `danger` | `theme-design.md`、rds-theme skill |
| 组件 | `Input`（搜索）、`List`（分节导航）、`Switch`·`Select`·`TabBar::segmented`·`Button`（行控件）、`StatusBar`（底栏）、`Collapsible`（备用） | gpui-kit 0.6.1 组件表（rds-ui-spec skill「组件规格速查」） |
| 契约 | 零裸色值 / 零裸 `px`；结构尺寸引用 `ui::*`；本页视图纳入 `ui_contract` 的**尺寸**扫描 | `ui-design-spec.md` §1 |

## 2. 形态选型（为什么是两栏弹层）

| 方案 | 描述 | 裁定 | 理由 |
| --- | --- | --- | --- |
| **A 两栏 Dialog（选定）** | 左分节导航 + 右内容区 | ✅ | 入口（⚙ / `Ctrl+,` / Quick Open）已存在，不必新增通道；设置项会随模块增长，必须先有**定位手段**（分节 + 搜索）；弹层不占常驻版面 |
| B 单列长页（现状实现） | 分节竖排、整页滚动 | ❌ | 项一多就"滚动找项"；分节之间没有层次；无法承载搜索 |
| C 独立窗口 / 中央标签页（VSCode 式设置编辑器） | | ❌ | 中央区已归编辑器（M5 / 编辑器模块）；再开窗口引入窗口生命周期与主题同步成本 |
| D 侧栏 Dock 面板 | | ❌ | 活动栏 4 项已定（含插件占位）；设置属低频、跨模块，不该占常驻位置 |

### 2.1 与其他弹层 / 对话框的关系

| 层 | 归属 | 规则 |
| --- | --- | --- |
| Quick Open | workbench | 与设置页**互斥**：打开其一即关闭另一个（现状**未互斥**，见 §11 落地项 3） |
| **设置页（应用级）** | `settings` 视图 + workbench overlay 宿主 | 与语义 Dialog 可叠加 |
| 项目设置 | `project` crate（`project::ui::render_settings`） | **不同作用域**：项目级设置归项目设置，应用级归设置页（`settings-architecture.md` §2.2） |
| 连接 / 项目对话框 | `Root::render_dialog_layer` | 语义 Dialog 层，位于设置页之上 |

## 3. 页面解剖

```
┌─ 设置（popover 底 · border 描边 · theme.radius 圆角）─── 61.25rem × 35rem ───┐
│ 标题行（2.25rem）：设置 ……………………………………… [⚙] [✕]                       │
├ 1px border ──────────────────────────────────────────────────────────────┤
│ 搜索行： [⌕ 搜索设置…]                                    （py 内距）        │
├────────────────┬─────────────────────────────────────────────────────────┤
│ 分节导航 12.5rem│ 内容区（弹性宽，内部纵向滚动）                              │
│  ▸ 外观         │  § 节标题（text_sm · MEDIUM）        [重置本节]（有非默认项时）│
│  ▸ 数据源导航  ●│  ╭─ 分组（group_box 底 + theme.radius）─────────────────╮ │
│  ▸ 连接默认值   │  │ 行：标签（15rem 标签列）        控件列（右对齐）  ↺   │ │
│  ▸ 项目         │  │     说明行（text_xs · muted，最多 1 行）              │ │
│                 │  ╰───────────────────────────────────────────────────╯ │
├────────────────┴─────────────────────────────────────────────────────────┤
│ StatusBar：左「修改即时生效并持久化到 settings.json」                        │
└─────────────────────────────────────────────────────────────────────────┘
```

- **几何恒定**：弹层宽高固定，只有内容区滚动——切节 / 加行都不改变弹层尺寸（沿用连接对话框"Tab 内容区固定高度"的教训）。
- **分节导航行高**沿用列表行高 `ui::ROW_HEIGHT`（1.5rem = 24px）；导航与内容区之间是 1px `border` 竖线。
- **分节导航行尾 `●`**（`muted_foreground` 小点 + hover 卡「本节有 N 项非默认值」）：非默认项计数，v1 为 P2 可选项。
- **内容区一次只显示一节**（不做"全节连续滚动"）：与连接对话框 Tab 一致，且滚动位置不跨节串味。
- **节内分组用 `group_box` 卡片**；v1 **不做折叠**（每节行数 ≤ 8，折叠多一层状态与焦点管理）。触发重评：某节 > 15 行时改用 `Collapsible`。

## 4. 行规格

### 4.1 四种行形态

| 形态 | 用于 | 控件 | v1 实例 |
| --- | --- | --- | --- |
| 开关行 | 布尔 | `Switch`（默认尺寸） | 显示标签 / 显示归属域 |
| 选择行 | 枚举 | ≤ 4 项用 `TabBar::segmented`；> 4 项用 `Select` | 主题模式（浅色/深色）、来源标识（短码/文字）、建连超时（5/15/30/60s）、LAN 直连 TLS、项目列表排序 |
| 数值行 | **预设档（分段）** | `TabBar::segmented`（档位来自登记表 `presets`） | 建连超时（5/15/30/60s） |
| 动作行 | 不是设置项，是跳转 | `Button` | 缓存管理… |

### 4.2 规格

| 元素 | 规格 |
| --- | --- |
| 行高 | 单行最小 2.5rem（`SETTINGS_ROW_MIN_HEIGHT`）；带说明行自动增高 |
| 标签列 | 15rem（`SETTINGS_LABEL_WIDTH`），`text_sm`，`foreground`，垂直居中 |
| 说明行 | `text_xs`，`muted_foreground`，**最多 1 行**，超出 `text_ellipsis()`；用于解释语义边界（如"仅影响新建连接"） |
| 控件列 | 右对齐；`TabBar::segmented` / `Select` 用 Small 档（1.625rem）与 `Switch`（默认 36×20）保持同一行视觉高度 |
| 行分隔 | `border` 1px（`ui::HAIRLINE`），每行下缘；分组最后一行不画 |
| 圆角 | 弹层用 `theme.radius`（当前 8px）；分组卡与控件用 0.375rem（6px）局部圆角；**不新造 token** |
| hover | 行底 `list_hover`（提示"整行是一件事"） |
| 禁用 | `.disabled(true)`（`Disableable`）**且**说明行给出原因——禁止"只变灰不说为什么" |
| 恢复默认 | 图标按钮（`IconName::RotateCw`），**仅在当前值 ≠ 默认值时出现**；hover 卡显示默认值文本（待接 `HoverCard`，见 §11） |
| 节级重置 | 节标题行右侧 `Button`（ghost，文案「重置本节」），仅在**本节**含非默认值项时出现；**不二次确认**（重置只写回默认值，且逐行仍可单独改回） |
| 键盘 | `Tab` 在控件间移动；`↑/↓` 在分节导航内移动；`Esc` 关闭；搜索聚焦 `Ctrl+F`（`Ctrl+,` 开/关已有） |

### 4.3 搜索行

- **数据来源**：只搜**已登记的设置项**（`settings-architecture.md` §6 登记表）——节名 / 组名 / 行标签 / key；不搜模块内开关（那些不在此页）。
- 命中高亮用产品语义 token `search.match.background`（已落地，无需新增角色）。
- 有结果：内容区切为结果列表，每条为「节 › 行」面包屑 + 控件（**可直接改**，不必跳转）。
- 无结果：空态（图标 + 「没有匹配的设置项」+ 一行说明「这里只列已登记的设置项」）。
- 清空搜索：回到进入搜索前的节与滚动位置。
- **动作行不参与搜索**（它不是设置项，没有 key）；搜索只覆盖登记项。
- v1 **不做**搜索语法（无前缀操作符、无 `@modified`）；触发重评：设置项 > 40 条时补 `@modified`。

### 4.4 生效方式标注

| 档 | 语义 | v1 项 | 呈现 |
| --- | --- | --- | --- |
| 即时 | 改完立刻反映到界面 | 主题模式、来源标识、显示标签、显示归属域 | 无附加标记 |
| 下次操作 | 下一次操作才用到 | 建连超时、LAN 直连 TLS（下次建连）、项目列表排序（下次打开列表） | 无附加标记；说明行写明"下一次…生效" |
| 需重启 | 需重载进程 | v1 无 | 行尾 `warning` 文案「重启后生效」+ 底栏右侧「重启」按钮 |

> 规则：**一项设置如果在界面上看不出生效方式，就必须在说明行写出来**（对齐"没实现就不宣传"）。

> **数值行一律用预设档，不做自由输入**：自由输入要成套的校验与单位处理，而目前没有这种需求（`navigator.property_panel_width` 由拖拽设置、不上页）。
> 触发重评：出现无法穷举的数值项（如保留份数要支持任意值）时，补校验与单位后缀再上。契约测试 `page_rows_are_scalar_and_writable` 会拦住"数值行没给预设档"。

## 5. 第一版内容清单

只收**已有真实消费方**的项；登记表（含未上页的项）见 `settings-architecture.md` §6。

| 节 | 行 | key | 形态 | 生效 | 消费方 |
| --- | --- | --- | --- | --- | --- |
| 外观 | 主题模式 | `appearance.theme_mode` | segmented 浅色 / 深色 | 即时 | `app/src/main.rs`（启动应用）、`SettingsService::set_theme_mode` |
| 数据源导航 | 来源标识 | `navigator.source_short_code` | segmented 短码 / 文字 | 即时 | `panels.rs::render_connection_row` |
| 数据源导航 | 显示标签 | `navigator.show_tags` | Switch | 即时 | `panels.rs::render_connection_row` |
| 数据源导航 | 显示归属域 | `navigator.show_scope` | Switch | 即时 | `panels.rs::render_connection_row` |
| 数据源导航 | 缓存管理… | —（动作行） | `Button` | — | 宿主回调 → `components/cache_dialog.rs`；说明行写「打开缓存对话框，清理元数据缓存」 |
| 连接默认值 | 建连超时 | `connection_defaults.connect_timeout_ms` | segmented 5/15/30/60s | 下次操作 | `workbench/services/connection_service.rs::connect_with_type` |
| 连接默认值 | LAN 直连 TLS | `connection_defaults.lan_disable_tls` | segmented 关 TLS / 保留 TLS | 下次操作 | `connection_service.rs::apply_lan_tls_default` |
| 项目 | 列表排序 | `projects.sort_mode` | segmented 最近 / 名称 / 创建 | 下次操作 | `view.rs::WorkbenchView::new`（读）+ `components/project_host.rs`（写，双入口：选择器内循环按钮） |

> **第一版只有 4 节 7 行 + 1 动作行，这是准入规则的必然结果**：僵尸项被裁（架构 §7.2）、未接线的项不画（§6 待接）。页面框架的价值在于后续模块**按规则往里加**，而不是首版行数。

### 5.1 本期不设的节（及其理由）

| 拟设节 | 裁定 | 理由 |
| --- | --- | --- |
| 通用 | **不设** | `general.language`（无 i18n）与 `general.restore_last_workspace`（无消费方）本期裁撤；不建空壳节 |
| 引擎 | **不设** | `engine.workspace_dir` / `cache_dir` 无消费方（目录权威在 M2 的目录布局）；待 M2 支持路径注入后再评估 |
| **插件（M9）** | **不设（beta3 预留）** | plugin 不在 app 依赖图、无视图、无设置项生产者——见 `settings-architecture.md` §10 |
| 编辑器 / 洞察 / Mock / 草稿箱 | **暂不设** | 目前无准入项（大文件档位是常量、字号来自主题资产、采样与折叠态是会话态或项目级） |

### 5.2 待接线后加入的节（本节只登记，不预先画行）

| 节 | 待接项 | 前置 |
| --- | --- | --- |
| 分析资产 | 保留版本份数（`resources.keepVersions`，`Input` + 说明「0 = 只留元数据；-1 = 全留」） | M6 dev-plan P2.4（现为常量 `DEFAULT_KEEP_VERSIONS = 5`） |
| 分析资产 | 默认排序 / 默认分组 | 同上（需先定"默认"的语义边界） |

## 6. 主题映射

| 元素 | 角色（`cx.theme().colors.*`） |
| --- | --- |
| 弹层底 / 前景 | `popover` / `popover_foreground` |
| 弹层描边 / 行分隔 | `border` |
| 分节导航底 | `popover`（与内容区同底，靠 1px `border` 分栏） |
| 导航行 hover / 选中 | `list_hover` / `list_active` |
| 导航选中左侧条 | `list_active_border`（coral），宽 `ui::TREE_ACTIVE_BAR`（2px） |
| 导航未选中文字 / 选中文字 | `muted_foreground` / `foreground` |
| 节标题 / 行标签 | `foreground` |
| 说明行 / 默认值提示 | `muted_foreground` |
| 分组卡底 | `group_box`（`group_box_foreground` 供其内文字） |
| 命中高亮 | 产品语义 token `search.match.background` |
| 底栏 | `StatusBar` 组件自带（`primary` + `primary_foreground`） |
| 危险动作 / 需重启提示 | `danger` / `warning` |
| 焦点环 | `ring` |

> 对话框内**不用** `sidebar_accent`（rds-ui-spec：对话框内用 `list*` / `popover`）。

## 7. 尺寸常量（页面自持，登记在 `crates/settings/src/ui.rs`）

| 常量 | 倍率 | @16px | 依据 |
| --- | --- | --- | --- |
| `PAGE_WIDTH` | 61.25 | 980 | 与「新建数据源连接」同档宽度；两栏（12.5 导航 + 弹性内容）后仍容得下 15rem 标签列 |
| `PAGE_HEIGHT` | 35.0 | 560 | 固定高：切节 / 加行不跳高（沿用连接对话框 Tab 区的教训） |
| `NAV_WIDTH` | 12.5 | 200 | 与连接对话框侧栏同宽，避免第二套"侧栏宽度" |
| `LABEL_WIDTH` | 15.0 | 240 | 行标签列；再窄则中长标签（如"建连超时"）要换行 |
| `ROW_MIN_HEIGHT` | 2.5 | 40 | 设置行最小高（比列表行高一档，因控件更高） |
| `ROW_HEIGHT` | 1.5 | 24 | 分节导航行高（与 workbench `ROW_HEIGHT` 同值） |
| `SECTION_HEAD_HEIGHT` | 1.75 | 28 | 节标题行 |
| `HEADER_HEIGHT` | 2.25 | 36 | 标题行（与 workbench `PANEL_HEADER_HEIGHT` 同值） |
| `CARD_RADIUS` / `CARD_PADDING` | 0.375 / 0.75 | 6 / 12 | 分组卡圆角与内边距 |
| `HAIRLINE` / `ACTIVE_BAR` | 1px / 2px | — | 固定描边：分隔线 / 导航激活条 |

**为什么不在 `crates/workbench/src/ui.rs`**（初版设计曾这样写）：依赖方向是 `workbench → settings`，
页面在 `settings` 里**不能反向引用**工作台的常量；因此页面自持一份。与 workbench 同名常量
（`ROW_HEIGHT` / `HAIRLINE` / `PANEL_HEADER_HEIGHT`）**保持同值**，调整需两边同步；
尺寸契约扫描覆盖本文件（见 §11 落地项 5）。

局部间距与图标尺寸仍用 gpui 的 Tailwind 尺度方法（`gap_2` / `px_3` / `size_4`）与组件默认尺寸（`Switch` 36×20、`TabBar::segmented` Small 档）。

## 8. GPUI 落点映射

| 元素 | 落点 | 备注 |
| --- | --- | --- |
| 页面尺寸常量 | `crates/settings/src/ui.rs` | 页面自持（依赖方向所限，§7） |
| 条目清单与分节 | `crates/settings/src/registry.rs` | 页面不得自排、自加 |
| 设置项字段与默认值 | `crates/settings/src/model.rs` | 新增字段必先登记 |
| 唯一写入路径 | `crates/settings/src/lib.rs::{apply_by_key, value_by_key}` | 页面不直接改 model |
| 宿主桥 | `SettingsHost { on_close, on_open_cache }`（`settings_page.rs`） | 副作用由宿主实现 |
| 设置页实体 | `crates/settings/src/settings_page.rs`（✅ 已实现；`settings_view.rs` 待退役） | `Entity<SettingsPage>`；分节 / 行 / 下拉全部由 `registry` 驱动 |
| 分节导航 | 同上 | `List` + `ListDelegate`（hover / 选中 / 键盘由组件给）；稳定 id 用**节的 key**，不用下标 |
| 分节与行清单 | `crates/settings/src/registry.rs`（`sections()` / `page_rows()`） | 页面**不得自排顺序、不得自加行**（登记表是权威） |
| 行与控件 | 同上 | 只读 `SettingsService::get_*`；写只经 `SettingsService::set_*` |
| 搜索框 | 同上 | `Input` + `InputState`（`cx.new(|cx| InputState::new(window, cx))`） |
| 宿主 overlay | `crates/workbench/src/view.rs::render_settings_panel`（⬜ 待替换为 `SettingsPage::new(window, host, cx)`） | 互斥规则在宿主实现（§2.1） |
| 缓存管理 | 宿主回调（现状 `on_open_cache`）→ `workbench/src/components/cache_dialog.rs` | 缓存 UI 依赖 engine，不进 `settings`（依赖只向下） |
| 命令 | `settings/src/commands.rs`（`OpenSettings` 已有） | `ToggleThemeMode` 未接线，见 §11 |
| 契约扫描 | `crates/workbench/tests/ui_contract.rs`：把本页视图文件加入**尺寸**扫描（颜色扫描已含） | 见 §11 落地项 5 |

## 9. 状态与空态矩阵

| 状态 | 呈现 |
| --- | --- |
| 首次打开 | 停在「外观」节（会话内记住上次停留节，**不持久化**） |
| 搜索有结果 | 结果列表（节 › 行 + 控件） |
| 搜索无结果 | 空态（图标 + 文案 + "只列已登记的设置项"说明） |
| 值非默认 | 行尾恢复默认按钮 + （P2）导航节小点 |
| 写盘失败 | 底栏 `danger`：「未能写入 settings.json：<原因>；本次改动仅在本进程生效」——现状静默失败，属**待落地**（架构 §9） |
| 无项目 / 只读项目 | 不受影响（应用级设置与项目态无关） |
| 明暗切换 | 即时重绘，无加载态 |

## 10. 与 V1 的对照

| v1 | v2 处置 | 理由 |
| --- | --- | --- |
| `SettingsModal`（4 标签页，前端 `store`） | **重设计**为两栏弹层 + 搜索 | v1 无搜索、无准入门槛；标签页数量随模块增长会失控 |
| 快捷键只读展示（无实现） | **不做** | 只读清单必然漂移；快捷键权威在各模块文档与 Quick Open |
| 一堆显示开关 | **部分迁** | 只迁有真实消费方的三项（来源标识 / 显示标签 / 显示归属域） |
| 主题明暗切换 | **照搬语义** | 已有实现与 `ToggleThemeMode` 命令（键位待定，§11） |

## 11. 已知问题与落地项（本页）

| # | 状态 | 项 | 说明 |
| --- | --- | --- | --- |
| 1 | ✅ | 两栏页面 | 已在 `settings_page.rs` 实现（分节导航 + 内容区 + 搜索 + 恢复默认）；**宿主替换未做**（工作台仍挂旧视图） |
| 2 | ✅ | 分段控件改用组件 | 枚举 / 两态 / 数值预设档统一走 `TabBar::segmented`（`segmented()` 辅助函数）；旧视图里的手搓按钮组随旧视图退役 |
| 3 | ⬜ | Quick Open 与设置页未互斥 | 两个 overlay 可同时为真（`view.rs::render` 分别 append） |
| 4 | ⬜ | `Esc` 关闭 / `Ctrl+F` 聚焦搜索未绑 | 需在页面挂 `key_context("settings")` + app 层 `bind_keys` |
| 5 | ⬜ | 尺寸契约扫描未覆盖 | `ui_contract` 的尺寸扫描要加入 `settings/src/{ui.rs, settings_page.rs}` |
| 6 | 🟡 | `ToggleThemeMode` 未接线 | Action 已定义但无键位、无 `on_action` 处理；**要么接线、要么删除**（见架构 §14 Q2） |
| 7 | ⬜ | 写盘失败静默 | `save_settings` 忽略 I/O 错误，页面无从提示 |
| 8 | ✅ | 搜索 | 搜索行 + 结果列表（节 › 行 面包屑）+ 无结果空态；只搜登记项、动作行不参与 |
| 9 | ⬜ | 「恢复默认」的 hover 卡未接 | 现用 `IconName::RotateCw` 图标按钮 + `tooltip` 缺位；默认值文本无处看（接 `HoverCard` 后补） |
| 10 | ⬜ | 页面无窗口测试 | 目前只有 3 项纯函数测试（搜索命中 / 行集一致 / 默认快照无改动）；切节与点击写入待补 |

> 进度：僵尸行已随 `model.rs` 裁撤删除（2026-09-16）；其余项见 `settings-dev-plan.md` §2 的阶段任务。

## 12. 待拍板

| # | 问题 | 候选 |
| --- | --- | --- |
| Q1 | 底栏是否提供「打开 settings.json」/「打开配置目录」？ | A：不提供（避免鼓励手改）· B：提供（报障与高级用户） |
| Q2 | 节内分组何时引入折叠？ | 触发条件已定：某节 > 15 行 |
| Q3 | 是否要「导出 / 导入设置」（团队共享配色与偏好）？ | 与连接模块 C4 模板导入导出同源，可一起排期 |
| Q4 | 节数 > 8 时导航是否分组（界面 / 数据 / 引擎）？ | 触发条件写入本节，先行不实现 |
| Q5 | 「非默认值」节小点是否必要？ | 项数少时价值低，可延后 |
| Q6 | 界面缩放的将来落点 | 主题资产字号倍率 vs 复活 `appearance.font_size`（架构 §14 Q4） |
