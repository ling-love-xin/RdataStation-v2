# RdataStation v2 明暗主题配色设计方案

> 状态：已与用户确认 · 落地文件：`assets/themes/rds-theme.json`（可直接加载）· 色卡预览：`docs/architecture/theme/theme-preview.html`
> 技术栈：gpui-kit 0.6（`ThemeRegistry::watch_dir` + `Theme::change(mode, window, cx)`，字段名对齐 0.6 `ThemeConfigColors`）
> 约束：按编码指南，**应用代码不写裸 hex/rgb**——所有颜色来自主题 JSON 资产与语义 token（见 §5）

## 1. 设计原则

1. **结构取自 VSCode**：Dark+/Light+ 的灰度分层（编辑器 / 侧边栏 / 活动栏 / 标题栏 / 状态栏），保证数据库工作台的长时间可读性
2. **品牌色注入 coral**：图标为橙红（coral）渐变，主色/状态栏采用品牌珊瑚色系，形成识别度
3. **语义色沿用 VSCode 经典**：成功绿 / 危险红 / 警告黄 / 信息蓝
4. **语法高亮直接用 VSCode Dark+/Light+ 色板**（久经考验，无需自创）

## 2. 品牌色（coral，取自 `assets/icons/app-icon-coral-clean.png` 渐变）

| 名称 | 色值 | 用途 |
| --- | --- | --- |
| coral-500 | `#E8735A` | 品牌主色（渐变中值） |
| coral-400 | `#F5A188` | 渐变亮端 / dark 强调 |
| coral-600 | `#C25B46` | light 主按钮 / 状态栏 |
| coral-700 | `#A84A38` | dark 状态栏 / hover |

## 3. 布局灰度分层（核心骨架）

| 区域 | Dark | Light | 说明 |
| --- | --- | --- | --- |
| 编辑器背景 | `#1E1E1E` | `#FFFFFF` | 中央编辑区 |
| 侧边栏背景 | `#252526` | `#F3F3F3` | 左右侧边栏 |
| 活动栏背景 | `#333333` | `#ECECEC` | 左右活动栏（产品语义 token，见 §5.4） |
| 标题栏背景 | `#323233` | `#DDDDDD` | 标题栏 |
| 挖空槽背景 | `#252526` | `#F3F3F3` | 标题栏项目槽（title_bar 深一档，产品语义 token） |
| 状态栏背景 | `#A84A38` | `#C25B46` | 品牌色，白字（`primary` 派生） |

> 活动栏与挖空槽在 `ThemeConfigColors` 无专用字段，定义为**产品语义角色**（§5.4），色值只存在于主题 JSON 资产，代码通过 `Theme::semantic_tokens()` 读取，不写裸 hex。

## 4. ThemeConfig 关键 token 对照表

### 4.1 Dark（`RDS Dark`）

| 字段 | 色值 | 用途 |
| --- | --- | --- |
| `background` | `#1E1E1E` | 编辑器/窗口底 |
| `foreground` | `#CCCCCC` | 默认文字 |
| `border` | `#3C3C3C` | 通用边框 |
| `input.border` | `#4F4F4F` | 输入框边（2026-09-12 由 `#3C3C3C` 提亮：与原 `border` 同色时输入框在深底上不易辨） |
| `caret` | `#E8846F` | 光标 |
| `selection.background` | `#264F78` | 文本选区 |
| `sidebar.background` | `#252526` | 侧边栏底 |
| `sidebar.foreground` | `#CCCCCC` | 侧边栏文字 |
| `sidebar.border` | `#3C3C3C` | 侧边栏边 |
| `sidebar.accent.background` | `#37373D` | 侧边栏选中/激活项底 |
| `sidebar.accent.foreground` | `#FFFFFF` | 选中项文字 |
| `list.background` | `#1E1E1E` | 列表底 |
| `list.hover.background` | `#2A2D2E` | 列表 hover |
| `list.active.background` | `#37373D` | 列表选中底 |
| `list.active.border` | `#E8846F` | 选中项左边条（品牌） |
| `table.background` | `#1E1E1E` | 结果集表格底 |
| `table.head.background` | `#252526` | 表头 |
| `table.row.border` | `#3C3C3C33` | 行边 |
| `popover.background` | `#252526` | Quick Open / 弹层 |
| `popover.foreground` | `#CCCCCC` | 弹层文字 |
| `title_bar.background` | `#323233` | 标题栏 |
| `title_bar.border` | `#323233` | 标题栏边 |
| `primary.background` | `#E8846F` | 主按钮/强调（品牌 coral） |
| `primary.foreground` | `#1E1E1E` | 主按钮文字 |
| `accent.background` | `#E8846F1A` | hover 强调底 |
| `muted.background` | `#3A3D41` | 禁用底 |
| `muted.foreground` | `#8A8A8A` | 弱文字 |
| `tab.background` | `#2D2D30` | 编辑器标签底 |
| `tab.active.background` | `#1E1E1E` | 激活标签底 |
| `tab.active.foreground` | `#FFFFFF` | 激活标签字 |
| `tab_bar.background` | `#252526` | 标签栏底 |
| `overlay` | `#00000080` | 遮罩 |

### 4.2 Light（`RDS Light`）

| 字段 | 色值 | 用途 |
| --- | --- | --- |
| `background` | `#FFFFFF` | 编辑器/窗口底 |
| `foreground` | `#333333` | 默认文字 |
| `border` | `#D4D4D4` | 通用边框 |
| `input.border` | `#B0B0B0` | 输入框边（2026-09-12 由 `#D4D4D4` 加深：输入框底 = `background` = 白，极浅边框会“看不见”） |
| `caret` | `#C25B46` | 光标 |
| `selection.background` | `#ADD6FF` | 文本选区 |
| `sidebar.background` | `#F3F3F3` | 侧边栏底 |
| `sidebar.foreground` | `#616161` | 侧边栏文字 |
| `sidebar.border` | `#E7E7E7` | 侧边栏边 |
| `sidebar.accent.background` | `#E4E4E4` | 侧边栏选中/激活项底 |
| `sidebar.accent.foreground` | `#333333` | 选中项文字 |
| `list.background` | `#FFFFFF` | 列表底 |
| `list.hover.background` | `#F0F0F0` | 列表 hover |
| `list.active.background` | `#E4E4E4` | 列表选中底 |
| `list.active.border` | `#C25B46` | 选中项左边条（品牌） |
| `table.background` | `#FFFFFF` | 结果集表格底 |
| `table.head.background` | `#F5F5F5` | 表头 |
| `table.row.border` | `#E1E1E1` | 行边 |
| `popover.background` | `#FFFFFF` | Quick Open / 弹层 |
| `popover.foreground` | `#333333` | 弹层文字 |
| `title_bar.background` | `#DDDDDD` | 标题栏 |
| `title_bar.border` | `#D4D4D4` | 标题栏边 |
| `primary.background` | `#C25B46` | 主按钮/强调（品牌 coral） |
| `primary.foreground` | `#FFFFFF` | 主按钮文字 |
| `accent.background` | `#C25B461A` | hover 强调底 |
| `muted.background` | `#F3F3F3` | 禁用底 |
| `muted.foreground` | `#8E8E8E` | 弱文字 |
| `tab.background` | `#ECECEC` | 编辑器标签底 |
| `tab.active.background` | `#FFFFFF` | 激活标签底 |
| `tab.active.foreground` | `#333333` | 激活标签字 |
| `tab_bar.background` | `#F3F3F3` | 标签栏底 |
| `overlay` | `#00000033` | 遮罩 |

### 4.3 语义色（两套一致，取自 VSCode）

| 语义 | Dark | Light |
| --- | --- | --- |
| success（成功/已连接） | `#89D185` / 底 `#14532D` | `#16A34A` / 底 `#F0FDF4` |
| danger（危险/错误） | `#F14C4C` / 底 `#5A1D1D` | `#DC2626` / 底 `#FEF2F2` |
| warning（警告） | `#CCA700` / 底 `#5A4D08` | `#B45309` / 底 `#FFFBEB` |
| info（信息） | `#3794FF` / 底 `#1A3D5C` | `#2563EB` / 底 `#EFF6FF` |

### 4.4 语法高亮（VSCode 色板，仅列常用）

| 语法 | Dark | Light |
| --- | --- | --- |
| keyword | `#569CD6` | `#0000FF` |
| string | `#CE9178` | `#A31515` |
| function | `#DCDCAA` | `#795E26` |
| comment | `#6A9955` | `#008000` |
| number | `#B5CEA8` | `#098658` |
| type | `#4EC9B0` | `#267F99` |
| property | `#9CDCFE` | `#0451A5` |

## 5. 落地方式

### 5.1 主题文件

- `assets/themes/rds-theme.json`（ThemeSet 格式，含 `RDS Light` / `RDS Dark`），标准 token 对齐 0.6 `ThemeConfigColors`
- 扩展产品语义角色（活动栏背景 / 挖空槽背景 / 活动栏激活条等）**已定：落独立 `assets/themes/product-tokens.json`**（0.6 schema 不接受 `rds-theme.json` 里的未知字段，已实测）；色值仍只存在于 JSON 资产，代码零 hex

### 5.2 加载

- `app` 启动时 `ThemeRegistry::watch_dir(assets/themes, cx, on_load)`（支持热更新）

### 5.3 切换

- 命令 `theme.toggle.mode` 注册于 `crates/settings`（"外观"节）→ `Theme::change(mode, window, cx)`（0.6 签名，`window: Option<&mut Window>`）

### 5.4 产品语义 token（替代旧"代码常量"做法）

> 编码指南禁止应用代码写死裸色值。此前方案中"活动栏背景常量 `RDS_ACTIVITY_BAR`（dark `#333333` / light `#ECECEC`）"**废止**，改为产品语义角色：

| 产品角色 | Dark | Light | 消费方 |
| --- | --- | --- | --- |
| `activity_bar.background` | `#333333` | `#ECECEC` | 左右活动栏背景 |
| `activity_bar.active_border` | `#FFFFFF` | `#333333` | 激活图标侧条 |
| `activity_bar.icon.active` | `#FFFFFF` | `#333333` | 激活图标色 |
| `activity_bar.icon.inactive` | `#858585` | `#616161` | 未激活图标色 |
| `title_bar.slot.background` | `#252526` | `#F3F3F3` | 标题栏项目挖空槽 |
| `quick_open.group.header` | `#3A3D41` | `#ECECEC` | Quick Open 分组头 |
| `search.match.background` | `#4A3F00` | `#FFF3C4` | 搜索命中文本底（数据源导航连接行 / 对象行） |

- **已落地**（2026-09-12）：资产 `assets/themes/product-tokens.json`（明暗各 7 角色）；加载设施 `crates/settings/src/product_tokens.rs`（`ProductTokens: Global`、`get(cx)`、`apply_from_str` / `apply_from_path`，缺角色回退语义最接近的标准字段）；app 启动 + `ThemeRegistry::watch_dir` 热更新（`crates/app/src/main.rs::attach_product_tokens`）
- 消费：`settings::product_tokens::get(cx).<role>(cx.theme())`（如 `activity_bar_background`）——**代码零 hex**；消费方：`view.rs`（活动栏背景 / 激活条 / 标题栏挖空槽 / Quick Open 分组头）、`panels.rs::nav_name_highlight`（命中底色）
- 状态栏背景：`theme.colors.primary` 派生（已有 token，无需新增）

### 5.5 软件图标

- 明亮版：`assets/icons/32x32.png`
- 暗黑版：未设计，dark 主题暂复用明亮版，后续按 coral 渐变设计暗底版本后替换

## 6. 对比验证

- 打开 `docs/architecture/theme/theme-preview.html` 可直观对比两套配色（明暗色卡）
- 实现后 `cargo run -p rds-app` 内切换主题核对：文字对比度（正文前景 vs 各背景 ≥ 4.5:1）、选中态、hover、表格斑马纹、Quick Open 弹层

## 7. 实现位置映射表

| 设计决策 | 实现位置 |
| --- | --- |
| 主题资产（明暗 token + 产品语义角色） | `assets/themes/rds-theme.json` + `assets/themes/product-tokens.json`（独立资产，已落地） |
| 主题加载（watch_dir 热更新） | `crates/app/src/main.rs`（`load_theme_assets` + `attach_product_tokens`） |
| 主题切换命令（外观节） | `crates/settings`（`commands.rs` / `model.rs` 外观节） |
| 产品语义 token 加载与读取 | `crates/settings/src/product_tokens.rs`（`ProductTokens: Global`）+ 各组件 `settings::product_tokens::get(cx).<role>(theme)` 消费 |
| 图标资产 | `assets/icons/32x32.png`（明亮版）/ 暗黑版待补 |
| 色卡预览 | `docs/architecture/theme/theme-preview.html` |
