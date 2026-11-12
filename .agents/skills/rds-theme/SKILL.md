---
name: rds-theme
description: RdataStation 主题系统：明暗配色 token 对照、产品语义角色注册与消费、主题资产加载与切换。新增/调整语义 token、修改配色或主题相关代码时使用。
---

# RdataStation 主题系统

设计文档：`docs/architecture/theme/theme-design.md`（设计意图）；**色值权威来源：`docs/architecture/theme/theme-tokens-complete.md`（140/140 全量色板，冲突以它为准）**；色卡预览：`docs/architecture/theme/theme-preview.html`。

## 布局灰度分层（核心骨架）

> 2026-10-12 对齐 VS Code Dark/Light Modern 灰阶骨架（来源 `docs/architecture/ui/vscode-ui-design-system.md` §5.4）；
> 品牌强调色（coral）保留。层级规则：**外层 chrome 比内容区暗**（暗色）——标题栏/侧栏/活动栏/状态栏/tab 栏同底 `#181818`，
> 编辑器/活动 tab/挖空槽 `#1F1F1F`，弹层 `#202020`；亮色 chrome `#F8F8F8`、编辑器白。

| 区域 | Dark | Light | 说明 |
| --- | --- | --- | --- |
| 编辑器背景 | `#1F1F1F` | `#FFFFFF` | 中央编辑区 |
| 侧边栏背景 | `#181818` | `#F8F8F8` | 左右侧边栏 |
| 活动栏背景 | `#181818` | `#F8F8F8` | 产品语义 token（与侧栏同底，靠激活条/图标区分） |
| 标题栏背景 | `#181818` | `#F8F8F8` | 标准字段 `title_bar.background` |
| 挖空槽背景 | `#1F1F1F` | `#FFFFFF` | 产品语义 token（比标题栏亮一档，内嵌感） |
| 状态栏背景 | `#181818` | `#F3F3F3` | 中性 chrome（**不是 coral**：`StatusBar` 组件文字硬编码 `muted_foreground`，珊瑚底配灰字仅 1.08:1，详见色板 §6） |

> 品牌主色 2026-11-01 起为 **coral-600 `#BC5340`**（light）/ coral-400 `#E8846F`（dark）。
> 旧 `#C25B46` 配白字仅 4.30:1 已作废。完整色阶见 `theme-tokens-complete.md` §2。

## 取色规则（硬约束）

- 标准字段：`theme.colors.<字段>`（`title_bar` / `title_bar_border` / `sidebar` / `sidebar_accent` / `border` / `background` / `popover` / `overlay` / `primary` 等，字段名对齐 gpui-kit 0.7.1 `ThemeColor`——0.6.1 → 0.7.0 字段集无变化（2026-10-02 实测）、0.7.0 → 0.7.1 亦无变化（141 字段逐项对比，2026-10-11 实测））
- 无专用字段的产品角色 → 产品语义 token（代码零裸色值）：

| 产品角色 | Dark | Light | 消费方 |
| --- | --- | --- | --- |
| `activity_bar.background` | `#181818` | `#F8F8F8` | 左右活动栏背景（与侧栏同底，靠激活条/图标区分） |
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

## 主题方案（包）

- 一包 = 一个 Light + 一个 Dark + 一套产品 token，落在 `assets/themes/packs/<id>/`（子目录，registry 只扫第一层 json，不会注册）。
- 值域：`crates/settings/src/theme_pack.rs::BUILTIN`（当前 `rds` = 现役、`qinghua`）；默认 `rds`，行为与历史一致。
- 选择：设置页「外观 → 主题方案」（`appearance.theme_pack`）→ `settings::install_theme_pack_sink`
  → `crates/app/src/main.rs::apply_theme_pack_state` 重装「主题 + 产品 token」（只换一半会残留上一包的语义色）。
- 临时覆盖：环境变量 `RDS_THEME_PACK=<id>`（优先于设置值、不落盘，试色 / CI 用）。
- 审包：`python tools/audit_theme_packs.py`（`theme_audit.py` 只审现役两套，路径硬编码）。

## 产品语义 token 消费（已落地）

0.6.1 / 0.7.0 / 0.7.1 的语义面（`SemanticThemeTokens` / `ColorTokens`）是**固定 18 角色**（`ColorTokens` 0.7.0/0.7.1 均 18 项；`SemanticThemeTokens` 23 项，2026-10-02 实测 0.7.0、2026-10-11 复核 0.7.1），装不下上表产品角色；因此落到独立资产 + 独立加载设施：

- **资产**：`assets/themes/product-tokens.json`（`light` / `dark` 各一映射，键名 = 上表角色名，值为 hex；缺失角色回退语义最接近的标准字段）
- **注册**：`crates/workbench_shell/src/product_tokens.rs`——`ProductTokenSet: Global`（`apply_from_str` / `apply_from_path` / `get(cx)`）；app 启动与主题目录热更新时调用（`crates/app/src/main.rs::attach_product_tokens`）。`settings` 侧 `pub use workbench_shell::product_tokens;` 重导，`settings::product_tokens::*` 路径不变
- **消费**（代码零 hex）：

```rust
settings::product_tokens::get(cx).activity_bar_background(cx.theme())
settings::product_tokens::get(cx).search_match_background(cx.theme())
```

- 角色→访问器：`activity_bar_background` / `activity_bar_active_border` / `activity_bar_icon_active` / `activity_bar_icon_inactive` / `title_bar_slot_background` / `quick_open_group_header` / `search_match_background`
- 消费方：`view.rs`（活动栏背景 / 激活条 / 标题栏挖空槽 / Quick Open 分组头）、`database/src/nav_view/primitives.rs::nav_name_highlight`（命中底色）
- **新增产品角色时必须同步改**：`product-tokens.json`（明暗两套）→ `TokenMap` / `ProductTokens` 字段 + `from_map` + 访问器（含回退）→ 消费方

## 检查清单

- 新增颜色一律先确认主题 JSON 已定义，代码零 hex
- 明暗切换后对照 `theme-preview.html` 核对：文字对比度（≥ 4.5:1）、选中态、hover、Quick Open 弹层
- **改色后跑** `python tools/theme_audit.py`：校验 140/140 覆盖、对比度（**含 product-tokens 的 9 个产品角色**）、`clamp_alpha` 合规（选中态 α 超 0.2 会被 schema 静默压缩，表现为"选中看不见"）
- **改任何 crate 的视图代码后跑** `cargo test -p rds-ui-guard`：全工作区守门（25 个 crate，不只 workbench/insight）。口径是「冻结基线、只拦新增」——存量命中登记在 `BASELINE` 里（多为 `ui.rs` 的合法常量定义），**命中数上升即失败**，新增豁免必须同步上调基线并写明理由
- **gpui-kit 升级后跑** `python tools/gen_theme_keys.py`：重建 `assets/themes/theme-keys.json`（启动期键名白名单）。键名写错是静默失败（无 `deny_unknown_fields`），未知键会让该 token 回退默认蓝主题
- `overlay` 是**一键两用**（模态遮罩 + Quick Open 遮罩，无 `scrim` 键可拆档），改它前先看色板 §4.1
