# RdataStation 全量主题色板（RDS Light / RDS Dark）

> 状态：**已落地** · 资产 `assets/themes/rds-theme.json`（明暗各 140/140 token）· 校验脚本 `tools/theme_audit.py` · 预览页生成 `tools/gen_theme_preview.py`
> 技术栈：gpui-kit 0.8.1，字段名与 JSON 键名逐项对齐 `vendor/gpui-component/src/theme/schema.rs::ThemeConfigColors`（140 个可配键）
> 本文件是**色值权威来源**；`theme-design.md` 保留设计意图说明，凡与本文件冲突以本文件为准。

---

## 1. 本次修订：审计结论与处置

对原资产（89/140）实测后修的问题，全部有数据支撑：

| # | 问题 | 实测 | 处置 |
| --- | --- | --- | --- |
| 1 | token 覆盖不全 | 明暗各缺 51 键（全部 `button.*`、语义色 hover/active、`drag.border`、`drop_target`、`table.foot.*`、`base.*.light`） | 补齐到 140/140，杜绝回退到 gpui-kit 默认蓝主题 |
| 2 | 主色对比度不足 | `#C25B46` + 白字 = **4.30:1** | 主色明度下压一档 → `#BC5340`（**4.72:1**），hover/active 同步 |
| 3 | 语义色明暗语义打架 | light 是"饱和底+浅字"、dark 是"暗底+浅字"，同一 token 两套角色 | 统一为**实心底 + 高对比文字**（light 700 档+白字 / dark 400 档+极深字） |
| 4 | 表头文字看不清 | light `#8E8E8E` on `#F5F5F5` = **3.01:1**；dark **4.44:1** | 改 `#616161` / `#969696` |
| 5 | 选中态几乎不可见 | `list.active.background` 实心 `#E4E4E4` 被 schema `clamp_alpha` 压到 α=0.2 → 白底上 ≈ `#FAFAFA` | 改为带品牌色调的 α 值 `#BC534033`，压完仍是 0.2 且可辨识 |
| 6 | 纯黑违反自家规范 | light 语法高亮 `editor.foreground #000000`、`operator #000000` | 改 `#1E1E1E` / `#333333`（`rds-design` 反模式 #10） |
| 7 | 图表系列色撞车 | `chart.3` 与 `chart.bullish` 同为绿色 | `chart.3` 回归品牌珊瑚，`chart.2` 改青，系列色与涨跌色分离 |
| 8 | 状态栏配色不可行 | 珊瑚底 + `muted_foreground` 灰字 = **1.08:1** | 改中性 chrome 底，见 §6 |

**结果**：140/140 覆盖、0 未知键、106 项对比度配对 **0 不达标**、`clamp_alpha` 全部落在上限内。

---

## 2. 品牌色阶（coral）

源自 `assets/icons/app-icon-coral-clean.png` 渐变，色相不变、明度重排以通过 AA。

| 档位 | 色值 | 角色 |
| --- | --- | --- |
| coral-300 | `#F5A188` | dark 主色 hover |
| coral-400 | `#E8846F` | **dark 主色**、link、ring、caret、active 左边条 |
| coral-500 | `#D9705B` | dark 主色 active |
| coral-600 | `#BC5340` | **light 主色**（白字 4.72:1）、状态栏强调、选中底 |
| coral-700 | `#A8452F` | light 主色 hover / link.hover |
| coral-800 | `#943B29` | light 主色 active / link.active |

> 旧 `coral-600 #C25B46` 作废（4.30:1）。图标资产仍用原渐变，不受影响。

---

## 3. 布局灰度骨架

沿用 VS Code Dark/Light Modern：**外层 chrome 比内容区暗**（暗色）。

| 区域 | Light | Dark | 说明 |
| --- | --- | --- | --- |
| 编辑器 / 内容区 | `#FFFFFF` | `#1F1F1F` | `background` |
| 侧边栏 / 活动栏 / 标题栏 | `#F8F8F8` | `#181818` | chrome 同底 |
| 挖空槽 | `#FFFFFF` | `#1F1F1F` | 比 chrome 亮/暗一档，内嵌感 |
| 弹层 | `#FFFFFF` | `#202020` | `popover`，比内容区高一档 |
| 状态栏 | `#F3F3F3` | `#181818` | 中性，见 §6 |
| 表头 / 斑马纹 | `#F8F8F8` / `#FAFAFA` | `#394248` / `#252526` | 结果集表格；深色表头是 DBeaver css 原值，见 `result-panel.md` §8.38 |

---

## 4. 全量 Token 对照表（140 键）

### 4.1 基础 · 文本 · 链接

| 键 | Light | Dark |
| --- | --- | --- |
| `background` | `#FFFFFF` | `#1F1F1F` |
| `foreground` | `#333333` | `#CCCCCC` |
| `border` | `#D4D4D4` | `#3C3C3C` |
| `input.border` | `#B0B0B0` | `#4F4F4F` |
| `caret` | `#BC5340` | `#E8846F` |
| `selection.background` | `#ADD6FF4D` | `#264F784D` |
| `overlay` | `#00000073` | `#00000080` |
| `ring` | `#BC5340` | `#E8846F` |
| `window.border` | `#D4D4D4` | `#3C3C3C` |
| `link` / `.hover` / `.active` | `#BC5340` / `#A8452F` / `#943B29` | `#E8846F` / `#F5A188` / `#D9705B` |

> ⚠️ **`overlay` 是一键两用**：Quick Open 遮罩（`workbench/src/quick_open/palette.rs:715`）与模态对话框遮罩
> （`result_panel/src/panel/dialog.rs`）共用同一个 token。gpui-kit 0.8.1 的 `ThemeConfigColors`
> **没有 scrim 键**，colors 里写未知键会被 serde 忽略并回退透明，所以拆不成两档。
> light 已由 α=0.2（`#00000033`）提到 **0.45**（`#00000073`）以表达模态强度，
> **代价是 Quick Open 遮罩同步变暗** —— 需真机确认观感，若嫌重可回退该值。

### 4.2 布局 chrome

| 键 | Light | Dark |
| --- | --- | --- |
| `sidebar.background` | `#F8F8F8` | `#181818` |
| `sidebar.foreground` | `#616161` | `#CCCCCC` |
| `sidebar.border` | `#E7E7E7` | `#2B2B2B` |
| `sidebar.accent.background` | `#E8E8E8` | `#37373D` |
| `sidebar.accent.foreground` | `#333333` | `#FFFFFF` |
| `sidebar.primary.background` | `#BC5340` | `#E8846F` |
| `sidebar.primary.foreground` | `#FFFFFF` | `#1E1E1E` |
| `title_bar.background` / `.border` | `#F8F8F8` / `#D4D4D4` | `#181818` / `#2B2B2B` |
| `status_bar.background` / `.border` | `#F3F3F3` / `#D4D4D4` | `#181818` / `#2B2B2B` |
| `tab_bar.background` | `#F8F8F8` | `#181818` |
| `tab_bar.segmented.background` | `#F3F3F3` | `#252526` |
| `tab.background` / `.foreground` | `#F8F8F8` / `#616161` | `#181818` / `#969696` |
| `tab.active.background` / `.foreground` | `#FFFFFF` / `#333333` | `#1F1F1F` / `#FFFFFF` |

### 4.3 容器 / 列表 / 表格

| 键 | Light | Dark |
| --- | --- | --- |
| `popover.background` / `.foreground` | `#FFFFFF` / `#333333` | `#202020` / `#CCCCCC` |
| `group_box.background` / `.foreground` | `#F5F5F5` / `#333333` | `#171717` / `#CCCCCC` |
| `group_box.title.foreground` | `#616161` | `#969696` |
| `accordion.background` | `#F5F5F5` | `#171717` |
| `list.background` | `#FFFFFF` | `#1F1F1F` |
| `list.hover.background` | `#F0F0F0` | `#2A2D2E` |
| `list.active.background` | `#BC534033` | `#E8846F33` |
| `list.active.border` | `#BC5340` | `#E8846F` |
| `list.even.background` / `list.head.background` | `#FAFAFA` | `#252526CC` |
| `table.background` / `.hover.background` | `#FFFFFF` / `#F0F0F0` | `#1F1F1F` / `#2A2D2E` |
| `table.active.background` / `.border` | `#BC534033` / `#BC5340` | `#E8846F33` / `#E8846F` |
| `table.even.background` | `#FAFAFA` | `#252526CC` |
| `table.head.background` / `.foreground` | `#F8F8F8` / `#616161` | `#394248` / `#969696` |
| `table.foot.background` / `.foreground` | `#F5F5F5` / `#616161` | `#252526CC` / `#969696` |
| `table.row.border` | `#E1E1E1` | `#3C3C3C33` |
| `description_list.label.background` / `.foreground` | `#F5F5F5` / `#616161` | `#252526` / `#969696` |

### 4.4 按钮（原资产全缺，现补齐 28 键）

| 键 | Light | Dark |
| --- | --- | --- |
| `button.background` / `.foreground` | `#F3F3F3` / `#333333` | `#333333` / `#CCCCCC` |
| `button.hover.background` / `.active.background` | `#E8E8E8` / `#DCDCDC` | `#3A3D41` / `#454545` |
| `button.primary.*` | `#BC5340` / 白 `#FFFFFF` / hover `#A8452F` / active `#943B29` | `#E8846F` / 字 `#1E1E1E` / hover `#F5A188` / active `#D9705B` |
| `button.secondary.*` | 同 `button.*` | 同 `button.*` |
| `button.danger.*` | `#B91C1C` / 白 / `#991919` / `#7F1515` | `#F14C4C` / `#2A0C0C` / `#F26A6A` / `#E84545` |
| `button.success.*` | `#15803D` / 白 / `#116932` / `#0E5328` | `#89D185` / `#0C1F14` / `#A3DE9F` / `#6FBF6B` |
| `button.warning.*` | `#A16207` / 白 / `#854F06` / `#6B3F05` | `#E3B341` / `#241C02` / `#ECC457` / `#C99A2E` |
| `button.info.*` | `#1D4ED8` / 白 / `#1A43B8` / `#163796` | `#75BEFF` / `#0A1626` / `#94CDFF` / `#5AA8EE` |

（顺序均为 `background` / `foreground` / `hover.background` / `active.background`）

### 4.5 语义色（三层模型）

**统一规则**：`xxx.background` = 实心底；`xxx.foreground` = 配在该底上的文字；`xxx.hover` / `xxx.active` = 按下反馈（浅色主题加深、暗色主题加深但保底对比度）。

| 语义 | Light 底 / 字 / hover / active | Dark 底 / 字 / hover / active |
| --- | --- | --- |
| `primary` | `#BC5340` / `#FFFFFF` / `#A8452F` / `#943B29` | `#E8846F` / `#1E1E1E` / `#F5A188` / `#D9705B` |
| `secondary` | `#F3F3F3` / `#333333` / `#E8E8E8` / `#DCDCDC` | `#333333` / `#CCCCCC` / `#3A3D41` / `#454545` |
| `success` | `#15803D` / `#FFFFFF` / `#116932` / `#0E5328` | `#89D185` / `#0C1F14` / `#A3DE9F` / `#6FBF6B` |
| `danger` | `#B91C1C` / `#FFFFFF` / `#991919` / `#7F1515` | `#F14C4C` / `#2A0C0C` / `#F26A6A` / `#E84545` |
| `warning` | `#A16207` / `#FFFFFF` / `#854F06` / `#6B3F05` | `#E3B341` / `#241C02` / `#ECC457` / `#C99A2E` |
| `info` | `#1D4ED8` / `#FFFFFF` / `#1A43B8` / `#163796` | `#75BEFF` / `#0A1626` / `#94CDFF` / `#5AA8EE` |
| `accent` | `#BC53401A` / `#333333` | `#E8846F1A` / `#CCCCCC` |
| `muted` | `#F3F3F3` / `#6E6E6E` | `#3A3D41` / `#8A8A8A` |

> 淡底提示条（告警条 / 徽章）不新增 token：用 `cx.theme().colors.danger.opacity(0.12)` 这类派生写法，代码仍是零裸 hex。

### 4.6 控件 · 图表 · 基础色

| 键 | Light | Dark |
| --- | --- | --- |
| `scrollbar.background` / `thumb` / `thumb.hover` | `#FAFAFA00` / `#A3A3A3E6` / `#8A8A8A` | `#17171700` / `#525252E6` / `#6B6B6B` |
| `skeleton.background` | `#E8E8E8` | `#3A3D41` |
| `slider.background` / `slider.thumb.background` | `#D4D4D4` / `#BC5340` | `#4F4F4F` / `#E8846F` |
| `switch.background` / `switch.thumb.background` | `#D4D4D4` / `#FFFFFF` | `#4F4F4F` / `#FFFFFF` |
| `progress.bar.background` | `#BC5340` | `#E8846F` |
| `drag.border` | `#BC5340A6` | `#E8846FA6` |
| `drop_target.background` | `#BC534033` | `#E8846F33` |
| `chart.1` … `chart.5` | `#2563EB` / `#0891B2` / `#BC5340` / `#B45309` / `#9333EA` | `#3794FF` / `#4EC9B0` / `#E8846F` / `#CCA700` / `#C586C0` |
| `chart.bullish` / `chart.bearish` / `chart.grid` | `#16A34A` / `#DC2626` / `#E1E1E1` | `#89D185` / `#F14C4C` / `#3C3C3C` |
| `base.blue` / `.light` | `#2563EB` / `#60A5FA` | `#3794FF` / `#9CDCFE` |
| `base.cyan` / `.light` | `#0891B2` / `#22D3EE` | `#4EC9B0` / `#7FE3D0` |
| `base.green` / `.light` | `#16A34A` / `#4ADE80` | `#89D185` / `#B5CEA8` |
| `base.magenta` / `.light` | `#9333EA` / `#C084FC` | `#C586C0` / `#DDB6D6` |
| `base.red` / `.light` | `#DC2626` / `#F87171` | `#F14C4C` / `#FF8A8A` |
| `base.yellow` / `.light` | `#B45309` / `#FBBF24` | `#CCA700` / `#E3D18B` |

> 涨跌维持**涨绿跌红**（2026-11-01 确认）。若将来做 A 股行情视图，需按区域切换为涨红跌绿——那时候再动这两个值，不要提前改。

### 4.7 产品语义角色（`product-tokens.json`）

gpui-kit 固定语义面装不下的产品角色，仍走独立资产，代码零 hex：

| 角色 | Light | Dark |
| --- | --- | --- |
| `activity_bar.background` | `#F8F8F8` | `#181818` |
| `activity_bar.active_border` / `icon.active` | `#333333` | `#FFFFFF` |
| `activity_bar.icon.inactive` | `#616161` | `#858585` |
| `title_bar.slot.background` | `#FFFFFF` | `#1F1F1F` |
| `quick_open.group.header` | `#ECECEC` | `#3A3D41` |
| `search.match.background` | `#FFF3C4` | `#4A3F00` |
| `staged.drop` | `#B91C1C` | `#F87171` |
| `result.local_row_background` | `#E4FFB5` | `#2A3A1E` |
| `result.odd_row.background` | `#EBF5FF` | `#1C2129` |
| `result.cell.modified.background` | `#FFE4B5` | `#3A3120` |
| `result.cell.error.background` | `#FF80FF` | `#4A2131` |

**结果集四槽的取值来源（DBeaver 结果集 colorDefinition，逐条实读 `plugin.xml:1166-1207`）**：

| 产品角色 | DBeaver 对应 id（前缀 `org.jkiss.dbeaver.sql.resultset.color.`） | 默认值 |
| --- | --- | --- |
| `result.odd_row.background` | `cell.odd.background` | 235, 245, 255 |
| `result.local_row_background` | `cell.new.background` | 228, 255, 181 |
| `result.cell.modified.background` | `cell.modified.background` | 255, 228, 181 |
| `result.cell.error.background` | `cell.error.background` | 255, 128, 255 |

浅色四槽取 DBeaver 原值。**深色一套是推导值**：DBeaver 该批 colorDefinition 只有一套浅色值，深色主题 css（`e4-dark-data-editor.css`）实测零覆盖，故按深色背景等亮度比推导（非照抄），结论已登记为待用户确认的取舍（见 `result-panel.md` §8.33）。

**结果集工具栏的 chrome 底色不新增产品角色**：直接复用固定语义面的 `tab_bar.background`（`#F8F8F8` / `#181818`），让工具栏与上方结果集标签条同底、连成一条 chrome 带。语义对应 DBeaver 的「非活动标签 / widget 背景」层（`UIStyles.getDefaultWidgetBackground()` = 主题 `INACTIVE_TAB_BG_START` → 回退系统 `COLOR_WIDGET_BACKGROUND`），与网格的列表底（`getDefaultTextBackground()` → `COLOR_LIST_BACKGROUND`，白）分层；取证与取舍见 `result-panel.md` §8.34。

---

## 5. 语法高亮（编辑器）

沿用 VS Code Dark+ / Light+，仅剔除纯黑：light `editor.foreground #1E1E1E`、`operator` / `punctuation #333333`、`editor.active_line_number #333333`。

### 5.1 两条高亮路径，别混为一谈

| 路径 | 消费者 | token 来源 | 覆盖 |
| --- | --- | --- | --- |
| **自研 DSL/SQL** | 主编辑器（`engine::sql::highlight_spans`） | `TokenClass` → 14 个名字（`crates/editor/src/view/highlight.rs:88`） | 资产已全覆盖 |
| **tree-sitter** | notebook 的 markdown 单元、json / text / python 文件预览（`host/mod.rs:602`、`notebook.rs:46`） | vendor `HIGHLIGHT_NAMES`（**41** 个名字，`highlighter/registry.rs`） | 见下 |

### 5.2 tree-sitter 侧：41 名 → 31 个定义，其余靠前缀回退

`SyntaxColors` 是**具名结构体**（与 `colors` 同款陷阱）：写未知键会被 serde **静默丢弃**，所以键名必须来自那 41 个合法名。

点号名会自动回退到前缀（`keyword.return` → `keyword`，`registry.rs` 的 `style()`），所以不需要把 41 个全写一遍；但**没有前缀可退的名字会完全不上色**。2026-11-02 补齐了原先缺的 16 个（此前 15 个定义 → 有 16 个名字无定义也无回退）：

| 名字 | Light | Dark | 归属家族 |
| --- | --- | --- | --- |
| `attribute` / `label` | `#0451A5` | `#9CDCFE` | property / variable |
| `constructor` | `#795E26` | `#DCDCAA` | function |
| `enum` / `variant` | `#267F99` | `#4EC9B0` | type |
| `preproc` | `#AF00DB` | `#C586C0` | 独立紫（VSCode 同档） |
| `embedded` / `primary` | `#1E1E1E` | `#D4D4D4` | 显式继承前景色 |
| `emphasis` | 斜体 | 斜体 | 只给 `font_style`，不给色 |
| `emphasis.strong` | `font_weight: 700` | 同 | 只给字重 |
| `hint` / `predictive`（补全预览） | `#616161` | `#969696` | 次级灰 |
| `link_text` | `#BC5340` | `#E8846F` | 品牌链接色 |
| `link_uri` | `#616161` | `#969696` | URL 段弱化 |
| `text.literal` / `text.code.span` | `#A31515` | `#CE9178` | string（行内代码） |

> **坑**：`FontStyle::Underline` 在 vendor 里被映射成 `Normal`（`registry.rs:190`）——下划线实际无效，别指望它。

资产当前明暗各 **31 个定义**，41 个合法名**全覆盖**（含前缀回退），0 非法键；代码文字对 `editor.background` 的对比度全部达标（注释按 3:1 档，其余 4.5:1）。

契约 5（`crates/workbench/tests/ui_contract.rs`）要求 14 个 token 名在明暗各声明一次——当前资产 31 个，全部满足。

---

## 6. 状态栏：为什么是中性色而不是品牌珊瑚

`theme-design.md` §3 原写"状态栏 = coral"，**实际不可实现**，原因是组件约束：

```
vendor/gpui-component/src/status_bar.rs:92-95
    .bg(cx.theme().tokens.status_bar)
    .text_color(cx.theme().muted_foreground)   // ← 文字色硬编码
```

`StatusBar` 组件的文字固定取 `muted_foreground`（中性灰），没有 `status_bar.foreground` 字段可覆盖。而中性灰压在珊瑚底上是数学上无解的：

- 白字要 ≥4.5 需底色亮度 ≤ 0.1833；黑字要 ≥4.5 需底色亮度 ≥ 0.175
- `#BC5340` 亮度 0.1725 → 白字 4.72 ✅，但 `muted_foreground #6E6E6E` 只有 **1.08:1** ❌

所以默认给**中性 chrome 底**（light `#F3F3F3` / dark `#181818`），配 `muted_foreground` 分别 4.59:1 / 5.14:1 ✅。

**若要品牌色状态栏**，必须自绘并显式指定文字色（不能复用 `StatusBar` 组件），此时用：

| 主题 | 底 | 文字 | 对比度 |
| --- | --- | --- | --- |
| Light | `#BC5340` | `#FFFFFF`（`primary.foreground`） | 4.72:1 |
| Dark | `#A84A38` | `#FFFFFF` | 5.67:1 |

---

## 7. 守门与验收

改色后必须跑的通关卡：

```bash
python tools/theme_audit.py                      # 覆盖率 + 对比度（含产品 token + 语法高亮）+ clamp_alpha 合规
python tools/gen_theme_preview.py                # 由主题资产重新生成色卡预览页
python tools/gen_theme_keys.py                   # gpui-kit 升级后重建键名白名单（assets/themes/theme-keys.json）
cargo test -p rds-workbench --test ui_contract   # 尺寸/裸色值契约 + highlight token 契约（仅 workbench）
cargo test -p rds-ui-guard                       # 全工作区守门（4 项，见下）
```

`cargo test -p rds-ui-guard` 的四项（**零产品依赖，1.5 秒**，CI 的 `ui_guard` job 会跑）：

1. `no_new_hardcoded_color_or_pixel` —— 25 个 crate 的裸色值 / 裸 `px` 基线（冻结基线、只拦新增）
2. `theme_asset_colors_only_use_schema_keys` —— 主题 `colors` 键名合法性 + `theme-keys.json` 与 vendor schema 一致
3. `product_tokens_use_declared_role_names` —— `product-tokens.json` 角色名合法性
4. `shared_constants_do_not_diverge` —— **跨 crate 同名 `pub const` 取值不得分叉**（2026-11-02 新增，见 §7.2）

判定标准：

- token 覆盖 **140/140**，未知键 0
- 对比度：正文/控件文字 ≥ 4.5:1，大文字与图形 ≥ 3:1，全部配对 0 不达标
- 产品语义角色（`product-tokens.json`，9 个 × 明暗）对比度一并校验——它们**不在** 140 键内，容易漏
- 语法高亮：41 个合法名全覆盖（含前缀回退）、0 非法键、代码文字对 `editor.background` 达标
- `list.active` / `table.active` 的 alpha ≤ 0.2、`selection` ≤ 0.3（超出会被 schema 的 `clamp_alpha` 静默压缩，表现为"选中态看不见"）
- 视图代码零裸 hex（契约 2），新增颜色一律先加 token 再消费
- 启动期若主题 JSON 含未知 `colors` 键，stderr 会告警（**不再静默回退默认蓝主题**）

### 7.2 尺寸常量的单一来源（2026-11-02 收敛）

同名 `pub const` 在两个 crate 各定义一份时，**改其中一个不会报错也不会生效**，是纯静默缺陷。

- 已收敛：`RESULT_*` 13 个常量原先在 `result_panel/src/ui.rs` 与 `editor/src/support/ui.rs` 各一份，现统一由 **result_panel 持有、editor re-export**（依赖方向 `editor → result_panel`，正向引用成立）。
  - 其中 `RESULT_PANE_HEIGHT` 当时**已分叉**：result_panel `18.0` / editor `20.0`。全仓核查后确认 result_panel 那份无人引用（死值）、真正生效的是 editor 的 `20.0`，故统一为 **20.0**，注释已记录 8rem→10rem→20rem 的两次教训。
- 守门：`shared_constants_do_not_diverge` 会拦新增分叉；确需同名不同值的要登记在 `CONST_DIVERGENCE_ALLOWED` 并写明理由（当前 2 条：`ALL`、`HISTORY_LIMIT`）。取值一致的重复定义只提示、不失败（当前 5 组收敛候选）。

### 7.1 已解决：Quick Open 组头（2026-11-02）

组头文字原先取 `muted_foreground`，实测 light **4.32:1** / dark **3.16:1**（需 4.5）。

两个候选方案里选了**② 改文字色**（`delegate.rs:354` → `theme.colors.foreground`）：

| 方案 | 做法 | 结果 |
| --- | --- | --- |
| ① 改组头底色 | light `#ECECEC`→`#F3F3F3` | 4.60 ✓，但 dark 要改到 `#242424`（4.50 ✓）时与 `background` 只剩 **1.06:1** 落差，**分区感基本消失** |
| ② **（采用）** 改文字色 | `muted_foreground` → `foreground` | light **10.69:1** / dark **6.8:1**；底色不动、分区感保留，弱化层级交给 `text_xs` + 底色 |

组头另一处文字：note「搜索中…」取 `theme.colors.info`（= `info.background`），light 5.67:1 / dark 5.49:1，本来就达标。

至此主题审计 **186 项校验 0 不达标**。

---

## 8. 平台约束备忘（改色前必读）

1. **`clamp_alpha`**：`list.active.background` / `table.active.background` 强制 α ≤ 0.2，`selection.background` ≤ 0.3。给实心值没有意义，会被静默压掉。
2. **未定义键会回退 gpui-kit 默认色**（内置蓝主题），与珊瑚品牌混用——所以宁可多定义，不要留空。
3. **`button.*` 系列 fallback 有坑**：未定义时 `button_danger_foreground` 回退为 `danger` 色本身（红底红字）。现已全部显式定义。
4. **`StatusBar` 文字色** = `muted_foreground`，见 §6。
5. 主题切换：`Ctrl+Shift+L` 或设置 → 外观；资产热更新由 `ThemeRegistry::watch_dir` 负责，改 JSON 无需重编译即可看效果。
6. **`overlay` 一键两用**（模态遮罩 / Quick Open 遮罩），0.8.1 无 `scrim` 键可拆档，见 §4.1。
7. **产品语义 token 独立于 140 键**：9 个角色在 `assets/themes/product-tokens.json` 由 `workbench_shell`
   单独加载为 GPUI global；该文件缺失 = 整块回退标准字段，且**不在**主题覆盖率检查范围内。
8. **键名写错是静默失败**：`ThemeConfigColors` 无 `deny_unknown_fields`，未知键被忽略后该字段回退默认色。
   防法：启动期 `assets/themes/theme-keys.json` 比对（stderr 告警）+ `cargo test -p rds-ui-guard` 编译期校验。
   注意 `base.*` 是**私有**字段（无 `pub`），按 `pub` 提取键名会漏掉 12 个。
