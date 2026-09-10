# RdataStation v2 布局实现方案

> 状态：已与用户确认（v5 示意）· 关联文件：`docs/architecture/layout/layout-proposal.html`
> 技术栈：gpui-kit 0.6（聚合包：`gpui_kit::*` = gpui-pre / `gpui_kit::base` = gpui-base / `gpui_kit::component` = gpui-component / `gpui_kit::assets`）
> 范围：`crates/workbench` 布局重构 + `crates/settings` 拆分（设置入口归属），不含各 Feature panel 的业务内容（占位先行）

## 1. 布局总览

与 VSCode 的差异布局，共五段：

```
┌────────────────────────────────────────────────────────────────────────┐
│ 标题栏 │ [logo] [项目:营销分析(挖空槽)]        [⌕ 搜索/命令][─ □ ✕]   │
├───┬───────────────────────────────┬──────────────────────┬───┤
│左 │ 左侧边栏                       │     中央编辑区        │右 │
│活 │ (草稿箱 / 数据库导航 /         │   EditorPanel        │活 │
│动 │  资源分析 / 插件)              │   查询编辑器+结果集   │动 │
│栏 │                               │                      │栏 │
│   │                               │                      │   │
│   │  [⚙ 设置]                     │                      │[⚙ 设置]│
├───┴───────────────────────────────┴──────────────────────┴───┤
│ 状态栏 │ « 完全隐藏 | 活动指示 ... 连接/引擎/编码            │
└─────────────────────────────────────────────────────────────────┘
```

| 区域 | 承载组件（gpui-kit 0.6） | 说明 |
| --- | --- | --- |
| 标题栏 | `gpui_kit::component::title_bar::TitleBar` + 自绘挖空槽 | 无窗口标题；左：软件图标+挖空项目名；中：Quick Open 入口；右：窗口控制 |
| 左侧活动栏 | 自绘 48px 竖条（`gpui_kit::base::flex` + `Button` + `Icon`） | 4 图标切换左侧 panel；底部 ⚙ 设置 |
| 右侧活动栏 | 同上（镜像） | 3 图标切换右侧 panel；底部 ⚙ 设置 |
| 左侧边栏 | `DockArea::set_dock(DockPlacement::Left, ...)` 的 Dock | 草稿箱 / 数据库导航 / 资源分析 / 插件 |
| 右侧边栏 | `DockArea::set_dock(DockPlacement::Right, ...)` 的 Dock | 洞察 / Mock 生成 / 历史 |
| 中央编辑区 | DockArea Center（现有 `EditorPanel`） | 保留，不动业务内容 |
| 状态栏 | `gpui_kit::component::status_bar::StatusBar` | 左：完全隐藏按钮 + 活动指示；右：连接/引擎/编码 |

> Dock 布局核心（`DockArea` / `DockLayout` / `DockPlacement` / 拖拽与状态）位于 **`gpui_kit::base::dock`**；
> 外观层（`DockSkin` / panel 表现 trait）位于 **`gpui_kit::component::dock`**，二者均从 `gpui_kit` 可达。

## 2. 各区域设计

### 2.1 标题栏（差异点：无窗口标题）

- 左：`assets/icons/32x32.png` 软件图标（明亮版；暗黑版未设计，dark 主题先复用，见 `theme/theme-design.md`）
- 挖空槽：`title_bar` 背景深一档的圆角槽，槽内 `项目` 标签 + 项目名（当前占位 "营销分析"，后续接 `project` crate）
  - 槽底色为**主题语义角色**（`theme/theme-design.md` §5.4 的 product token，如 `title_bar.slot.background`），代码不写死色值
- 中：Quick Open 入口（见 3.3），宽度 320px 居中
- 右：窗口控制按钮（`TitleBar` 自带：─ □ ✕）
- 无窗口标题文本

### 2.2 活动栏（左右镜像，VSCode 差异点）

- 结构：48px 竖条，上段 4 个（左）/ 3 个（右）panel 图标，底部 `⚙ 设置`
- 图标集合：
  - 左：草稿箱 / 数据库导航 / 资源分析 / 插件（对应 `scratchpad` / `database` / `analytics_resource` / `plugin`）
  - 右：洞察 / Mock 生成 / 历史（对应 `insight` / `mock` / `query_history`）
- 高亮：激活图标左侧（右侧为右侧）2px 白条 + 亮色图标（VSCode 行为）
- 背景色与选中高亮取**主题语义角色**（activity_bar 系列 token），代码不写死色值
- 点击行为：见 3.1

### 2.3 侧边栏（左右对称）

- 左侧边栏：Dock 内单 Tab（不可关闭），头部标题 = 当前 panel 名，内容 = 对应 panel 视图
- 右侧边栏：同上，`set_dock(DockPlacement::Right, ...)` 装配
- 宽度：左 240px / 右 280px 起步（Dock 窗口级尺寸，`DockLayout::child(..., Some(px(240.)))`），Dock 拖拽调宽
- 默认状态：左侧展开、右侧收起（见 3.2）

### 2.4 中央编辑区

- 保留现有 `EditorPanel` 全部能力（连接详情 / 新建连接 / SQL 查询 + 结果集 / 历史），仅外层布局调整

### 2.5 状态栏

- 左：`« 完全隐藏` 按钮（见 3.2）+ 当前活动指示（"数据库导航"）
- 右：连接名 / DuckDB 状态 / 编码

## 3. 交互设计

### 3.1 活动栏点击（VSCode 行为）

```
点击活动栏图标：
  若该侧边栏收起      → 展开并切换到该 panel
  若该侧边栏展开且非当前 panel → 切换 panel 内容
  若该侧边栏展开且是当前 panel  → 收起该侧边栏（toggle）
```

### 3.2 边栏三模式（左右独立，gpui-kit 0.6 Dock API）

| 模式 | 状态 | 触发 | 实现（0.6） | 恢复 |
| --- | --- | --- | --- | --- |
| 展开 | 活动栏 + 边栏可见 | 点击活动栏图标 | `set_dock(placement, layout, window, cx)` 安装（或 dock 已存在且 open） | — |
| 收起 | 边栏隐藏、活动栏保留 | 再次点击当前激活图标 / 边栏头 `×` | `toggle_dock(placement, window, cx)`（dock 保留、离屏，panel 状态不丢） | 点击活动栏图标（`toggle_dock` 再次调用） |
| 完全隐藏 | 活动栏 + 边栏均隐藏，编辑区最大化 | 状态栏 `« 完全隐藏` | `remove_dock(placement, window, cx)` + 隐藏该侧活动栏元素 | 状态栏按钮 / 命令面板 / Quick Open（重新 `set_dock` 同一 layout 恢复） |

- `remove_dock` 为 0.6 新增：dock 与面板整体移除（panel 收到 `on_removed`）；恢复时以同一 `DockLayout`（同一 panel 实体）重新 `set_dock`
- `toggle_dock` 与 `set_center` 为 0.6 兼容保留 API（旧 `set_left_dock / set_right_dock / set_bottom_dock` 已删除，统一走 `set_dock`）
- 左右模式独立，互不影响

### 3.3 Quick Open（搜索 + 命令融合，VSCode Quick Open 模式）

- 入口：标题栏居中输入框（点击或 `Ctrl+P` 唤起）
- 面板：`Dialog` 顶部输入框 + 结果列表（`Input` + `List` 自搭）
- 匹配：默认搜索 文件/表/草稿/连接；输入 `>` 前缀仅匹配命令
- 结果分组：`文件 / 表`、`命令` 两组混排，↑↓ 选择、↵ 执行/打开、Esc 关闭
- 命令注册：`crates/workbench/src/commands.rs`（现为占位）按 `GPUI Action` 注册命令，供 Quick Open 与快捷键复用；其中"打开设置"命令委托给 `crates/settings`（见 §4）

## 4. 代码改动清单

| 文件 | 改动 |
| --- | --- |
| `crates/settings/`（**新增 crate**） | 设置能力拆分：model（通用 / 外观(主题) / 引擎路径 / 连接默认值）、持久化、设置视图、`OpenSettings` 命令。迁入 workbench 的 `Tool::Settings` 占位与 `services/persistence_service.rs`。结构见 `docs/architecture/settings/settings-crate-design.md` |
| `crates/workbench/src/view.rs` | `Tool` 枚举重构为 `LeftPanel`（Draft/Database/Resources/Plugin）+ `RightPanel`（Insight/Mock/History）；`WorkbenchView` 增加右侧活动栏/右侧 dock 装配（`set_dock`）、标题栏重构、Quick Open 状态、三模式状态（`remove_dock`/`toggle_dock`） |
| `crates/workbench/src/panels.rs` | `SidebarPanel` 按新枚举渲染（草稿箱/插件为占位视图）；新增 `RightSidebarPanel`（洞察/Mock/历史占位）；`Shared` 增加 panel 状态、三模式状态 |
| `crates/workbench/src/commands.rs` | 注册基础命令：切换 panel、展开/收起/完全隐藏、打开设置（→ settings）、打开 Quick Open、执行 SQL |
| `crates/app/src/main.rs` | 注册主题加载：`ThemeRegistry::watch_dir(assets/themes, cx, on_load)` |
| `assets/themes/rds-theme.json` | 明暗两套主题（见 `theme/theme-design.md`） |
| `assets/themes/product-tokens.json` | （如采用独立语义 token 文件）活动栏/挖空槽等产品语义角色，见 `theme/theme-design.md` §5.4 |

## 5. 实施步骤

1. **Step 1 数据模型**：`LeftPanel` / `RightPanel` 枚举 + `Shared` 扩展（active panel、三模式状态、Quick Open 状态）
2. **Step 2 布局骨架**：标题栏重构（logo/挖空槽/居中 Quick Open/控制按钮）；活动栏左右两条；`init_workspace` 增加右侧 `set_dock(DockPlacement::Right, ...)`
3. **Step 3 交互**：活动栏 toggle、三模式（`toggle_dock` / `remove_dock`）、Quick Open 面板（Dialog + Input + 分组列表）
4. **Step 4 占位 panel**：草稿箱 / 插件 / 洞察 / Mock / 历史 五个占位视图（复用现有 placeholder 风格）
5. **Step 5 settings crate**：`crates/settings` 骨架（model / 持久化 / 视图 / 命令），迁移 `persistence_service`，workbench 活动栏 ⚙ 接入
6. **Step 6 主题接入**：加载 `rds-theme.json`（+ 产品语义 token），验证明暗切换
7. **验证**：`cargo check --workspace` + `cargo run -p rds-app` 手动核对（布局渲染、toggle、三模式、Quick Open、明暗切换）

## 6. 实现位置映射表（设计决策 → 代码）

| 设计决策 | 实现位置 |
| --- | --- |
| 五段布局 / 标题栏 / 活动栏 | `crates/workbench/src/view.rs`（`WorkbenchView::render_title_bar` / `render_activity_bar` / `init_workspace`） |
| 左右侧边栏（Dock 装配与三模式） | `crates/workbench/src/view.rs` `init_workspace`（`set_dock` / `toggle_dock` / `remove_dock`） |
| panel 枚举与占位视图 | `crates/workbench/src/panels.rs`（`LeftPanel` / `RightPanel` / `SidebarPanel` / `RightSidebarPanel`） |
| Quick Open 与命令 | `crates/workbench/src/commands.rs` + 标题栏入口（view.rs） |
| 设置入口（活动栏 ⚙） | `crates/settings`（视图 + `OpenSettings` 命令），workbench 只保留触发按钮 |
| 主题加载 / 切换 | `crates/app/src/main.rs`（`watch_dir`）+ `crates/settings`（外观节切换命令） |
| 主题资产 | `assets/themes/rds-theme.json`（+ product-tokens.json） |

## 7. 未决项

- 暗黑版软件图标（dark 主题暂复用明亮版）
- 项目名来源（暂占位 "营销分析"，接 `project` crate 后替换）
- 连接入口归属（并入数据库导航栏数据源节点，与现导航树结构一致）
