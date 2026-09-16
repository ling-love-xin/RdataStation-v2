---
name: rds-theme
description: RdataStation 主题系统：明暗配色 token 对照、产品语义角色注册与消费、主题资产加载与切换。新增/调整语义 token、修改配色或主题相关代码时使用。
---

# RdataStation 主题系统

设计文档：`docs/architecture/theme/theme-design.md`；色卡预览：`docs/architecture/theme/theme-preview.html`。

## 布局灰度分层（核心骨架）

| 区域 | Dark | Light | 说明 |
| --- | --- | --- | --- |
| 编辑器背景 | `#1E1E1E` | `#FFFFFF` | 中央编辑区 |
| 侧边栏背景 | `#252526` | `#F3F3F3` | 左右侧边栏 |
| 活动栏背景 | `#333333` | `#ECECEC` | 产品语义 token |
| 标题栏背景 | `#323233` | `#DDDDDD` | 标准字段 `title_bar.background` |
| 挖空槽背景 | `#252526` | `#F3F3F3` | 产品语义 token |
| 状态栏背景 | `#A84A38` | `#C25B46` | 品牌 coral，`primary` 派生 |

## 取色规则（硬约束）

- 标准字段：`theme.colors.<字段>`（`title_bar` / `title_bar_border` / `sidebar` / `sidebar_accent` / `border` / `background` / `popover` / `overlay` / `primary` 等，字段名对齐 gpui-kit 0.6 `ThemeColor`）
- 无专用字段的产品角色 → 产品语义 token（代码零裸色值）：

| 产品角色 | Dark | Light | 消费方 |
| --- | --- | --- | --- |
| `activity_bar.background` | `#333333` | `#ECECEC` | 左右活动栏背景 |
| `activity_bar.active_border` | `#FFFFFF` | `#333333` | 激活图标侧条 |
| `activity_bar.icon.active` | `#FFFFFF` | `#333333` | 激活图标色 |
| `activity_bar.icon.inactive` | `#858585` | `#616161` | 未激活图标色 |
| `title_bar.slot.background` | `#252526` | `#F3F3F3` | 标题栏项目挖空槽 |
| `quick_open.group.header` | `#3A3D41` | `#ECECEC` | Quick Open 分组头 |
| `search.match.background` | `#4A3F00` | `#FFF3C4` | 搜索命中文本底（导航连接行 / 对象行） |

## 主题资产与加载

- 资产：`assets/themes/rds-theme.json`（RDS Light / RDS Dark 两套）
- 加载：app 启动 `ThemeRegistry::watch_dir(themes_dir, cx, on_load)`（热更新）
- 切换：`Theme::change(mode, window, cx)`；命令 `ToggleThemeMode` 在 `crates/settings`，经 `SettingsService` 即时生效并持久化

## 产品语义 token 消费（已落地）

0.6 的语义面（`SemanticThemeTokens` / `ColorTokens`）是**固定 18 角色**，装不下上表产品角色；因此落到独立资产 + 独立加载设施：

- **资产**：`assets/themes/product-tokens.json`（`light` / `dark` 各一映射，键名 = 上表角色名，值为 hex；缺失角色回退语义最接近的标准字段）
- **注册**：`crates/settings/src/product_tokens.rs`——`ProductTokenSet: Global`（`apply_from_str` / `apply_from_path` / `get(cx)`）；app 启动与主题目录热更新时调用（`crates/app/src/main.rs::attach_product_tokens`）
- **消费**（代码零 hex）：

```rust
settings::product_tokens::get(cx).activity_bar_background(cx.theme())
settings::product_tokens::get(cx).search_match_background(cx.theme())
```

- 角色→访问器：`activity_bar_background` / `activity_bar_active_border` / `activity_bar_icon_active` / `activity_bar_icon_inactive` / `title_bar_slot_background` / `quick_open_group_header` / `search_match_background`
- 消费方：`view.rs`（活动栏背景 / 激活条 / 标题栏挖空槽 / Quick Open 分组头）、`panels/nav.rs::nav_name_highlight`（命中底色）
- **新增产品角色时必须同步改**：`product-tokens.json`（明暗两套）→ `TokenMap` / `ProductTokens` 字段 + `from_map` + 访问器（含回退）→ 消费方

## 检查清单

- 新增颜色一律先确认主题 JSON 已定义，代码零 hex
- 明暗切换后对照 `theme-preview.html` 核对：文字对比度（≥ 4.5:1）、选中态、hover、Quick Open 弹层
