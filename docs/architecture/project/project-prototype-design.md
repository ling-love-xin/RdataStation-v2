# 项目管理模块 · 原型设计（项目生命周期 · CRUD）

> 状态：**已实现；2026-11-22 按 `crates/project/src/ui.rs` 逐条核对过一遍**。
> 本文是**设计与取舍的唯一口径**；`project-prototype.html` 是 2026-09-11 的**设计稿**，
> 其中卡片、新建对话框等已与当前实现有出入——差异集中在 **§13 落地核对**，两处不一致时以 §13 为准。
> 关联文件：`project-prototype.html`（可交互设计稿）、`project-dev-plan.md`（开发方案与进度）
> 技术栈：gpui-kit **0.10.0**（`vendor/gpui-component` / `vendor/gpui-base` 均为 0.10.0；2026-11-22 由 0.9.0 升级，见 `docs/architecture/dependencies/core-packages-2026-11-22-upgrade-assessment.md`）
> 前置：v1 蓝本 `v1/backend/src/commands/project_commands.rs`（创建/打开/校验/重命名/删除/最近项目）；v2 后端 `crates/project`（`ProjectStore` / `ProjectManager` / `models.rs`）与 P0 会话 `workbench/src/services/project_session.rs` 已就绪
> 关联文档：`overview.md`（M1 双层数据架构）、`connection/connection-dev-plan.md`（作用域 `G_/P_/GP_`）、`scratchpad/scratchpad-prototype-design.md`（草稿箱根 = 项目目录）
>
> **范围界定**：本模块只负责**项目的增删改查与生命周期**。系统级 ↔ 项目级的**提升（promote）/ 引用（snapshot）** 属于 M1 双层资产能力，**不在本模块范围**，另立设计（见 §12）。

## 1. 设计基准与语义

### 1.1 核心语义：一实例一项目

v2 采纳「**每一个应用实例即一个项目**」：一次启动只有**一个当前项目**，标题栏项目槽、草稿箱根、项目作用域连接、分析库全部指向同一项目根。由此确定四条硬约束：

- **不存在「多项目并列打开」的工作台**——切换项目 = 关闭当前项目 + 打开目标项目（同一窗口内完成，编辑区状态重置）。
- **不支持多窗口打开同一个项目**——打开项目时在 `.RSmeta/` 下获取**项目锁**（`project.lock`，记录 pid/时间），已被占用时默认拒绝，但提供**逃生口**（只读打开 / 仍要打开，见 §8）；进程退出/关闭项目时释放，陈旧锁按 pid 存活检测回收。
- **切换 / 关闭前拦截未保存草稿**——编辑区存在脏缓冲区时，先弹「取消 / 放弃更改并继续 / 保存并继续」，取消则中止切换。
- **项目名册状态需持久化**——固定（pin）与排序方式跨会话保留（见 §2.2 R5/R8），落全局库。

项目管理 UI 因此不是左 Dock 的一个 panel，而是**窗口级状态**（无项目 → 选择器，覆盖中央区，保留五段外壳）**加**标题栏项目槽上的弹层（有项目 → 项目菜单 / 设置）。

### 1.2 项目目录结构（UI 只读展示，不写入裸路径）

```
{project}/
├── <用户文件>                     # 草稿箱根 = 项目目录
└── .RSmeta/                       # 内部元数据（UI 隐藏，不参与草稿箱树）
    ├── project.db                 # 项目统一 SQLite（project_info / connections / settings）
    ├── project.json               # 项目配置（可迁移 JSON）
    ├── analytics.duckdb           # 项目分析库（DuckDB）
    ├── project.lock               # 项目锁（防同项目多实例）
    ├── config/settings.json       # ProjectConfig：settings / default_connection_id / extensions（含 rds.build_tag）
    ├── queries/                   # SQL 文件
    └── project_metadata/          # 每个连接一个元数据 .db（introspection 缓存）
```

> 常量统一取 `crates/project` 的 `RS_META_DIR_NAME`（`.RSmeta`）；UI/服务不得各写各的。

## 2. 项目 CRUD 全览（本模块主线）

> 统一收口在 `ProjectService`；后端能力来自 `crates/project` + 名册库 `crates/store/src/persistence/global_db.rs`（`engine::persistence::global_db` 是重导出薄壳，实现已迁至 `rds-store`）。全局库是**项目名册**（recent/all/removed），`.RSmeta` 是**项目本体**——二者需保持同步。

### 2.1 Create（创建）

| # | 操作 | 入口 | 语义 / 后端 |
| --- | --- | --- | --- |
| C1 | **新建空项目** | 选择器「＋ 新建项目」→ §6 对话框 | `ProjectStore::create` + `save_project_info*`；建 `.RSmeta` 并迁移 `project.db` / `analytics.duckdb` |
| C2 | **从现有目录创建 / 导入** | 选择器「🗀 打开文件夹…」 | 含 `.RSmeta` → 直接登记并打开；空目录 → 询问「在此创建项目」；非空非项目 → **阻止** |
| C3 | **从示例项目开始** | 选择器「从示例项目开始」 | 固定路径 `sample_project_dir()`：已是有效项目则直接打开；否则按 `CreateProjectInput::new("示例项目", path).with_sample_drafts(true)` 创建，再写一份 `welcome.sql`（写草稿失败不阻断打开） |

创建校验：名称非空、≤64 字、不含 `\ / : * ? " < > |`；目标目录可写；冲突按上表分支。

### 2.2 Read（读取 / 查询）

| # | 操作 | 入口 | 语义 / 后端 |
| --- | --- | --- | --- |
| R1 | **启动会话解析** | 应用启动 | `RDS_PROJECT_PATH` → 全局库最近项目（路径仍存在）→ 空态（`resolve()`） |
| R2 | **最近项目列表** | 选择器「最近」 | `get_recent_projects(limit)`：`removed_at IS NULL` **且** `last_opened_at` 非空（从未打开过的不进最近），`ORDER BY is_pinned DESC, last_opened_at DESC` |
| R3 | **全部项目列表** | 选择器「全部」 | `get_all_projects`（过滤 `removed_at IS NULL`） |
| R4 | **搜索 / 过滤** | 选择器顶部输入 + `状态：<当前>` 下拉 | 输入框按**名称 / 描述 / 路径**子串过滤（大小写不敏感）；另支持 facet 语法 `状态:<active\|archived\|offline\|syncing\|全部>`、`固定:<是\|否>`、`锁:<是\|否>`，与状态下拉**叠加（AND）**；状态下拉候选固定为「全部 / Active / Archived / Offline / Syncing」，属临时视图状态、**不持久化** |
| R5 | **排序** | 选择器排序菜单 | 最近打开 / 名称 / 创建时间；**排序方式持久化**（`settings` 通用设置 `projects.sort_mode`） |
| R6 | **打开项目** | 卡片 / `open_by_path` / `open_by_id` | 校验 `.RSmeta` → 获取项目锁 → `open_project*` → 更新 `last_opened_at` → 写会话 |
| R7 | **元信息读取** | 卡片 / 设置 | `ProjectInfo` + 路径有效性 + `check_project_missing_drivers` |
| R8 | **固定 / 取消固定** | 卡片 `Star` 星标 / `⋯` | 写 `project_info.is_pinned`（+ `pinned_at`），固定项在最近/全部置顶；**跨会话保留** |
| R9 | **已移除项目找回** | 选择器「已移除」视图 | `list_removed_projects`（`removed_at` 非空）→「恢复」清空 `removed_at` 并回到名册 |

> 失效路径（磁盘已移走）：卡片降为**失效态**，菜单由「在资源管理器中显示 / 删除数据…」换成「重新定位…」（`card_menu_entries` 的路径分支），保留 打开 / 固定·取消固定 / 移出列表；不报裸错误。

### 2.3 Update（更新）

| # | 操作 | 入口 | 语义 / 后端 |
| --- | --- | --- | --- |
| U1 | **重命名** | 项目菜单「重命名…」/ 设置「基本信息」 | 仅改**显示名**（`name`）：`update_project_info` + 同步 `project.json`/`project.db` + 标题栏即时更新；**不改磁盘目录名**（移动目录超出范围，见 §12） |
| U2 | **编辑描述** | 设置 · 概览 | 同 U1 管道（`description`） |
| U3 | **默认连接** | 设置 · 默认项 | 写 `ProjectConfig.default_connection_id` → `save_config` |
| U4 | **归档 / 取消归档** | 项目菜单 / 设置 · 危险区 | `ProjectStatus::Active ↔ Archived`；归档不删除，仍在名册 |
| U5 | **重新定位** | 失效卡片 | 指向新目录：校验 `.RSmeta` → 改全局库 `path` + `save_info` |
| U6 | **固定状态** | 卡片 `Star` 星标 | 见 R8，属名册属性更新（跨会话） |
| U7 | **重命名后同步** | — | 名册 / `.RSmeta` / 会话三处一致由服务保证（单一写入入口） |

### 2.4 Delete（删除）

| # | 操作 | 入口 | 语义 / 后端 | 可逆 |
| --- | --- | --- | --- | --- |
| D1 | **移出名册**（软删） | 卡片 `⋯`（或右键）→「移出列表」 | 置 `project_info.removed_at = now`，**保留磁盘**；从最近/全部隐藏（可经 R9 找回）。路径失效的卡片走 `forget`（直接清名册记录，不可找回） | 可找回（R9） |
| D2 | **删除磁盘数据**（硬删） | 卡片 `⋯` →「删除数据…」/ 设置 · 危险区「删除数据…」 | 删除 `{project}/.RSmeta` 及项目文件（`delete_project_disk` 语义）；**输入项目名二次确认**；先释放项目锁；随后从全局库移除登记 | 不可逆 |
| D3 | **删除前置校验** | — | 若为当前项目 → 先关闭（走未保存拦截）；确认文案展示将删除的绝对路径 | — |

> 归档（U4）≠ 删除；软删（D1）≠ 硬删（D2）。三者入口分开，避免误操作。

## 3. 信息架构与三态

| 态 | 条件 | 主视图 | 标题栏项目槽 |
| --- | --- | --- | --- |
| **无项目** | 会话解析为空且未被用户关闭 | 项目选择器（覆盖中央区） | 显示「未打开项目」占位 |
| **有项目** | 会话存在 | 正常五段工作台 | 显示 `Folder` 图标 + 「项目」+ 项目名（**无状态点**）；点击项目槽弹**项目菜单** |
| **切换/关闭中** | 未保存拦截 / 锁竞争 / 加载 | 选择器（带加载态）或拦截对话框 | 加载指示 |

**关键帧（状态迁移）**：

```
启动 → resolve()
  ├─ Some → 有项目（标题栏槽填名；草稿箱/连接/导航按项目根加载）
  └─ None → 无项目（选择器）
选择器 ──[C1 新建 / C3 示例]──► 对话框 ──[成功]──► 有项目
选择器 ──[C2 打开文件夹]──────► 校验/询问 ──► 有项目
选择器 ──[R6 最近项]─────────► 校验路径 + 抢锁 ──► 有项目
有项目 ──[切换项目]──► 未保存拦截? ──► 释放锁 → 选择器 ──► 目标项目
有项目 ──[关闭项目]──► 未保存拦截? ──► 释放锁 → 无项目
打开被锁项目 ──► 提示「已在另一实例打开」──► [只读打开 / 仍要打开 / 取消]
```

## 4. 项目选择器（无项目态 · Welcome，覆盖中央区）

### 4.1 布局

保留左右活动栏与状态栏，选择器占据中央编辑区。左右分栏：

```
┌──────────────────────────────────────────────────────────────┐
│  [R] RdataStation        [最近][全部][已移除]      [◐ 主题]    │
├───────────────────────────────────┬──────────────────────────┤
│ [搜索名称 / 描述 / 路径…] 状态：全部▾ 排序：最近打开▾ │ [＋ 新建项目]  主按钮 │
│  ★ 营销分析  active   打开 ⋯       │  [📂 打开文件夹…] 次按钮  │
│  ┌──────────────────────────────┐ │  [⧉ 从示例项目开始]        │
│  │ D:\work\marketing            │ │                          │
│  │ 最后打开：2 小时前 · 状态：active │ │  ── 快速开始 ──           │
│  └──────────────────────────────┘ │  · 新建空项目            │
│  ┌──────────────────────────────┐ │  · 从示例项目开始        │
│  │ 风控看板   ⚠ 缺失驱动：sqlite  │ │  · 打开现有目录（.RSmeta）│
│  │ …                            │ │                          │
│  └──────────────────────────────┘ │                          │
│  （空态 / 无匹配态 / 已移除列表）   │                          │
└───────────────────────────────────┴──────────────────────────┘
```

顶部三个视图 Tab：**最近**（R2）/ **全部**（R3）/ **已移除**（R9）。搜索框旁边是 `状态：<当前>` 与 `排序：<当前>` 两个下拉按钮（**不是**一个下拉），排序方式（R5）跨会话保留。

### 4.2 最近项目卡片

**当前实现（2026-11-22 核对，`ui.rs` 卡片渲染 + `meta_line`）**：

| 层 | 内容 |
| --- | --- |
| 第一行 | 项目名（`text_sm` 中等字重）+ 状态**纯文本**（按状态着色，不是徽标）+ 固定项星标（`Star`，`primary` 色）+ 右侧 `打开` / `⋯` 两个操作 |
| 路径行 | `text_xs` 弱化色，**完整路径原文**（不省略中段） |
| 描述行 | 有描述才显示（`text_xs` + `text_ellipsis`） |
| 元信息行 | `最后打开：{相对时间或"从未打开"} · 状态：{status}` |
| 附加警示 | 路径失效 → `⚠ 路径已失效，可能已移动或删除`；缺失驱动 → `⚠ 缺失驱动：a、b`；被占用 → `● 已在另一实例打开` |

- 卡片**不显示**连接数 / 版本数（`ProjectSummary` 没有这两个字段；版本数在 `ProjectInfo.version_count`，但未进卡片）。
- 悬停操作：`打开`、`⋯`（右键同源 `card_menu_entries`：打开 / 固定·取消固定 / 在资源管理器中显示 / 移出列表 / 删除数据…；路径失效时「在资源管理器中显示」「删除数据…」换成「重新定位…」；「已移除」视图只有 打开 / 固定 / 恢复）——**不含「重命名」**（改名走项目菜单 / 设置「基本信息」）。
- 双击/回车 = 打开；↑↓ 移动选择；固定项置顶（排序第一键是 `is_pinned`，见 `sort_items`）。
- 失效态与缺失驱动都在卡片内加一行警示，不弹裸错误。

### 4.3 空态与边界态

| 态 | 文案与操作 |
| --- | --- |
| 无项目 | 图标 + 「还没有项目」+ 新建 / 打开文件夹 / 从示例开始 |
| 搜索无匹配 | 「无匹配项目」+ 清空搜索 |
| 全部列表为空 | 同「无项目」 |
| 已移除为空 | 「没有已移除的项目」 |
| 全局库未就绪 | 「项目服务未就绪」+ 重试（不暴露裸错误串） |
| 路径失效 | 卡片置灰 + 「路径已失效」+ 移出 / 重新定位（U5） |
| 被其他实例占用 | 打开时弹三项：**只读打开 / 仍要打开 / 取消**（§8） |

## 5. 标题栏项目槽与项目菜单（有项目态）

```
[📁] 项目 营销分析        ← 文件夹图标 + 「项目」 + 名称（实测：没有状态点）
```

点击弹出**项目菜单**（`popover` token），条目来自 `project::ui::project_menu_entries(read_only)`（**非只读 8 项；只读时顶部多一条置灰说明，共 9 项**，顺序如下）：

```
┌────────────────────────────┐
│ 只读模式：写操作不可用      │  ← 仅只读打开时出现，禁用态
│ ──────────────────────────  │
│ ＋ 新建项目…               │  ← 作用于别的目录，只读时仍可用
│ 🗀 打开现有目录…            │  ← 同上
│ ──────────────────────────  │
│ ← 项目中心…                │  ← 跳到全局设置的「项目」页，不关闭当前项目
│ ⚙ 项目设置…                │
│ ▤ 重命名…                  │  ← 只读时禁用
│ ↗ 在资源管理器中显示         │
│ ──────────────────────────  │
│ ▥ 归档 / 取消归档           │  ← 只读时禁用
│ ──────────────────────────  │
│ ✕ 关闭项目                  │
└────────────────────────────┘
```

> **两份菜单都覆盖「有项目时怎么进别的项目」**：`new` / `open-folder` 走 `act_create_project` / `act_open_folder`（有草稿先拦，确认后直达目标对话框，见 §8），`switch` 走 `act_open_hub`（跳到全局设置的「项目」页，见 §5.1）。菜单项**不显示快捷键**（`PopupMenuItem` 只有 label + icon + 禁用态）；快捷键：`Ctrl+Shift+P` = 打开项目中心（= 设置「项目」页）、`Ctrl+Shift+W` = 关闭项目（`crates/app/src/main.rs` 的 `KeyBinding::new("ctrl-shift-p", SwitchProject, Some("workbench"))` 与 `"ctrl-shift-w"`，context `"workbench"`）。两者已彻底分工：`SwitchProject` **不再**等价于 `CloseProject`。`重命名…` 是 `项目设置…` 的直达别名。提升/引用**不在此菜单**（范围外，见 §12）。固定/取消固定为选择器卡片级操作，不进标题栏菜单。缺失驱动只在**卡片**上以 `⚠ 缺失驱动：…` 一行展示，菜单里没有警告行。

### 5.1 项目中心 = 全局设置的「项目」页（有项目态）

「有项目时，别的项目在哪里管理」——答案是**全局设置窗口的「项目」页**，不是要求先关项目，也不再在 Dock / 覆盖层里另开一块：

```
设置（独立窗口）
┌────────────────────────────────────────────────────────────┐
│ 侧栏：[常规] [外观] … [项目] …                             │
├────────────────────────────────────────────────────────────┤
│ 项目列表   选择、切换、新建与管理项目；点「打开」直接切到该项目  │
│  （与选择器同一份列表：Tab / 搜索 facet / 状态 / 排序 / 卡片菜单）  │
│  营销分析  active 已固定 [当前]        [打开] [⋯]            │
│  流量看板  archived                   [打开] [⋯]            │
│                                        [新建项目] [打开文件夹…] [示例] │
├────────────────────────────────────────────────────────────┤
│ 列表排序   [最近打开 ▾]                                     │
└────────────────────────────────────────────────────────────┘
```

- **复用选择器**：设置页只加一个分组卡片，内容直接由宿主注入 `render_picker`——列表、搜索、facet、排序、卡片右键/`⋯`、只读置灰**没有第二套实现**（`SettingsHost::projects_panel` + `fields::panel_item`）。
- **当前标记**：卡片按「`item.path == host.root()`」打 `Tag::primary().child("当前")`（判据是纯函数 `is_current_project`）。
- **切换语义**：卡片「打开」与 `⋯` 的「打开」都走 `act_switch` → `request_switch`：无当前项目 → 等价 `request_open`；目标 == 当前根 → no-op；脏草稿 → `open_unsaved_dialog(..., PendingAction::SwitchTo(path))`；否则 `switch_now` = `do_close`（还锁 + `on_closed` 清连接作用域）→ `open_path`。**先释放再打开**，不把新锁盖在旧会话上。
- **动作投回主窗口**：设置窗口里点这些入口，动作经 `ProjectUiHost::home`（`HomeDispatch`）投回主窗口执行——草稿拦截对话框、`do_close` 与 `open_path` 的副作用都属于主窗口；宿主未注入投递口时（单测 / 无宿主）就地执行，行为与拆分前一致。
- **入口**：`Ctrl+Shift+P` / 标题栏菜单「项目中心…」/ 选择器菜单「项目中心…」→ `act_open_hub` → `open_settings_at(Some("projects"))`（窗口已开着则切节，不抢焦点）。
- **为什么不做 Dock 常驻 tab / 覆盖层**：常驻 tab 在每次打开项目后都占着中央区、与编辑器 tab 争位置；覆盖层是新窗口之上又一层盖，与设置窗口互相遮挡。放进设置窗口后只有一个入口、一套列表；无项目态不需要它——中央区本来就是完整选择器。
- **原型文件**：`project-prototype.html` **未改**（它本就没有画过项目中心，只画选择器 / 项目设置 / 逃生口），要视觉示意请以 `settings-prototype-design.md` 的「项目」节与本文本节为准。

## 6. 新建项目对话框（Create）

**当前实现（2026-11-22 核对，`ui.rs::open_create_dialog` / `submit_create`）**：模态 `Dialog`，
不设宽 → 用组件默认 **448px**（不是早期设计稿写的 560）。

| 字段 | 控件 | 校验（行内即时提示，判据与提交同源） |
| --- | --- | --- |
| 项目名称 `*` | 单行 `Input` | `service::validate_project_name`：非空、≤64 字、不含 `\ / : * ? " < > \|`；名称未填时不提示（避免开框即红） |
| 位置 `*` | 目录输入 + 浏览按钮（系统目录选择器） | `location_hint`：空 → 「请选择项目位置」；不存在 → 「位置不存在：…」；目标已是项目 → 「该目标目录已是项目，请直接打开」；目标非空 → 「目标目录非空，请换一个名称或选择空目录」；**名称不合法时整行不提示**（目标路径还不成立，且含分隔符的名称会把 `join` 拼成子路径判出错误结论） |
| 目标预览 | 静态文本 | `目标：{位置}/{名称}`，**位置非空且名称合法**后才显示真实路径，否则退回占位文案 |
| 描述（可选） | 单行 `Input` | — |

- **没有**「默认连接」「初始化分析库」「创建示例草稿」复选——这三项只存在于设计稿；分析库恒建（`ProjectStore::create`），示例草稿只在「从示例项目开始」那条路径写入（`create_sample`）。
- **位置默认值**＝上次成功新建用的父目录（`settings.projects.last_create_location`），没记过才回退用户主目录；创建成功后写回（`submit_create` 的成功分支）。
- 底部只有 `取消` / `创建项目`（`Primary`）。按钮**不置灰**：先按下，失败时把原因写进提交错误行。

**提交期冲突处理**（`service::create` → `inspect_target`）：

| 场景 | 行为 |
| --- | --- |
| 目标目录已是有效项目（含 `.RSmeta`） | 报错「该目录已是项目，请直接打开」 |
| 目标目录存在但非空、且非项目 | 报错「目录非空，请选择空目录」 |
| 目标目录为空 / 不存在 | 正常创建（目录不存在则连同 `.RSmeta` 一起建） |

- 空目录这条在「打开现有目录」路径上另有分支：先弹确认「在空目录创建项目？」（原型 C2）。
- **「打开现有项目」对话框（2026-11-22 补齐）**：单字段「项目目录（含 .RSmeta）」+ 浏览按钮，宽 448px；行内提示与 `submit_open_folder` 同源（空 / 不存在 / 非空且非项目当场报红；**空目录不报错**，按下「打开」才问是否在此创建），因此用户选完目录就能看到判断，不必先点按钮被拒。见 `project-user-guide.md` §4.3。
- 创建即打开：成功后直接走统一 `open_path`（取锁、刷新连接/导航），不另开一步。

## 7. 项目设置视图（Read / Update / Delete）

**当前实现（2026-11-24 核对，`ui/settings.rs::render_current_settings`）**：不再有主窗口覆盖式面板——
从项目菜单「项目设置…」/「重命名…」进来的是**全局设置窗口的「项目」页**（handler 预填名称与描述后
走 `act_open_hub`）。该页有两块宿主注入内容：上面是项目列表（§4/§5），下面就是本节这个
**「当前项目设置」块**——有会话就显示，无项目时整块不出现。块内为**单栏纵向分节**（7 节）：

| 分节 | 内容 | 数据来源 |
| --- | --- | --- |
| 概览 | 四项只读 kv：名称 / 路径 / 描述 / 状态（「活跃 / 已归档 / …」+「可写 / 只读」，如「已归档 · 可写」） | `SettingsSnapshot`（事件路径取一次；状态含名册 `status` 与本地读写态） |
| 基本信息 | 名称输入（**仅改显示名**）+ 描述输入 + 「保存项目信息」按钮 | `save_project_info`：校验名 → `service::update_meta` → 改 `session.name`（标题栏槽随之变）→ toast「已保存项目信息」→ 刷新选择器与快照 |
| 默认项 | 默认连接（U3）：下拉 + 说明「新建查询 / 打开项目时的预选连接；不设则用系统当前连接」 | `ProjectConfig.default_connection_id` |
| 存储（.RSmeta） | 只读结构树：树头 `.RSmeta/ · 合计 {human_size}`，每行「缩进 + 图标 + 名称 + 大小 + 复制路径」（点击 toast「已复制路径：…」）+ 说明 +「打开 `.RSmeta`」 | `meta_tree_rows`（`fs::metadata` 快照） |
| 依赖 | 缺失驱动列表与处理 | `check_project_missing_drivers` |
| 版本 | 手动快照台账列表（时间 + 说明，行尾「删除」两段式：先切「确认删除 / 取消」）+ 消息输入 +「创建版本快照」（成功后清空输入） | `project.db` 的 `project_versions`（`list_versions` / `create_version` / `delete_version`） |
| 危险区 | 归档/取消归档（U4）、移出名册（D1）、删除磁盘数据（D2，输入项目名确认） | `GlobalDatabaseManager` |

- 名称/描述不是即输即改：点「保存项目信息」后写名册并把 `session.name` 同步给标题栏槽。
- 存储结构树只读；「打开 `.RSmeta`」用系统文件管理器打开（排障用）。
- 只读打开时，写类入口按 `read_only_blocked` 禁用（U2 描述、归档、默认连接等）。
- 快照（路径 / 描述 / 状态 / 结构树 / 依赖 / 版本）只在**事件路径**取：进入「项目」页时宿主调
  `load_settings_snapshot`，保存 / 归档 / 建版本后各自刷新。未取到时块头提示「尚未读取项目信息」，
  底部「刷新列表与结构树」一次性补齐——render 不读盘、不查库。
- 块内没有「关闭」按钮（设置窗口自带 ✕ / `Esc`），标题是「当前项目设置 · {name}」；内容为流式
  布局，随设置页整页滚动（不嵌套滚动）。

## 8. 未保存拦截与锁竞争

| 场景 | 行为 |
| --- | --- |
| 切换/关闭项目且编辑区有脏缓冲区 | 弹 `Dialog`「未保存的草稿」，三按钮 **取消 / 放弃更改并继续 / 保存并继续**（`open_unsaved_dialog`）；取消则中止，「保存并继续」写回草稿文件后继续 |
| 打开被本项目其他实例锁定的项目 | 弹 `Dialog`「项目「{name}」已在另一实例打开」，正文给出持有者 pid，三按钮 **取消 / 仍要打开 / 只读打开**（`open_lock_busy_dialog`） |
| ↳ 只读打开 | 打开项目但不获取写锁，进入只读模式（写操作按钮禁用，`ui.notice` 标「只读打开：该项目已被另一实例占用」，状态栏标注「只读」） |
| ↳ 仍要打开 | 清除锁文件后重新抢锁（仅在确认对方已退出 / 陈旧锁时使用） |
| 陈旧锁（持有进程已退出） | 按 pid 存活检测自动回收，不阻塞打开 |
| 关闭项目 | 释放锁 → 会话置空 → 回选择器 |

## 9. 关键状态与校验清单

| 场景 | 期望 |
| --- | --- |
| 创建重名项目（同路径） | 提示打开现有项目，不覆盖 |
| 创建到非空非项目目录 | 阻止 + 明确文案 |
| 打开路径缺 `.RSmeta` | 拦截；`.rdata-station` 旧结构尝试迁移 |
| 打开最近项目但路径已移走 | 失效态，不崩溃 |
| 打开被锁项目 | 三项选择（只读/仍要/取消）；只读模式有明确标注 |
| 项目名非法字符 | 行内提示（`location_hint` 之外由 `validate_project_name` 给出）；提交按钮**不置灰**，按下后再报错 |
| 重命名 | 仅改显示名，磁盘目录不变；三处（名册/.RSmeta/会话）一致 |
| 固定 / 排序持久化 | 重启后固定项仍置顶、排序方式保持 |
| 缺失驱动 | 卡片上 `⚠ 缺失驱动：{列表}` 一行 + 设置 · 依赖节；不阻塞打开 |
| 切换项目 | 编辑区重置；草稿箱/连接/导航全部指向新根，无跨项目串数据 |
| 软删 vs 硬删 | 软删可从「已移除」找回；硬删输入项目名确认后方可执行 |

## 10. 主题映射（token → 视觉，零 hex）

| 区域 | 主题角色 |
| --- | --- |
| 选择器底 / 卡片 | `background` / `sidebar.background`，卡片边 `border`，悬停 `list.hover.background` |
| 卡片选中 | `list.active.background` + 左边条 `list.active.border` |
| 主按钮 / 次按钮 | `primary.background`+`primary.foreground` / `background`+`input.border` |
| 状态文字 | active→`success`，syncing→`info`，archived→`muted.foreground`，offline→`warning`；缺失驱动 / 路径失效→`warning` |
| 固定 / 只读标记 | `muted.foreground`（弱化）；只读态状态栏用 `warning` |
| 标题栏项目槽 | `title_bar.slot.background`（产品语义角色，见主题方案 §5.4） |
| 项目菜单 / 对话框 | `popover.background`+`popover.foreground`，危险项 `danger` |
| 存储结构树 | 复用数据库导航树缩进与 `muted.foreground` 元信息 |

## 11. GPUI 落点映射

> ⚠️ **2026-10-01 更新（A3 迁移后）**：本表已与 `project-dev-plan.md` §6 对齐。项目视图与
> 服务自 A3 批次起迁入 `crates/project/src/`（视图与 model/service 同 crate），不再位于
> workbench。唯一权威落点以 **dev-plan §6 为准**，此表仅为速查。

| 组成 | 类型/位置 |
| --- | --- |
| 项目管理 UI（选择器 / 项目中心 / 菜单 / 对话框 / 设置） | `crates/project/src/ui.rs`（公共面：类型、`ProjectUiHost`、UI 状态与输入实体；视图与 model/service 同 crate）+ 五个子模块 `ui/{host,actions,dialogs,picker,settings}.rs`（`pub use` 重导出，`project::ui::*` 路径不变）。选择器 = `render_picker`（挂载在 `crates/workbench/src/panels/project_picker.rs` 的 `ProjectPickerPanel`，仅无项目态可见）；**项目中心** = 全局设置的「项目」页（列表由 workbench 注入 `render_picker`，`SettingsHost::projects_panel`；动作经 `HomeDispatch` 投回主窗口）；**项目设置** = 同页的「当前项目设置」块（`render_current_settings`，`SettingsHost::project_settings_panel`，无项目时不出现）；菜单 = 语义 `Button::dropdown_menu` + `PopupMenu`；对话框 = `Dialog` / `AlertDialog` |
| 项目服务编排（列表 / 创建 / 打开 / 关闭 / 更新 / 删除 / 找回 / 版本） | `crates/project/src/service.rs` |
| 项目视图 ↔ 宿主桥（重绘 / 编辑区 / 排序偏好 / 打开后刷新） | `crates/workbench/src/components/project_host.rs`（`ProjectUiHost` 注入） |
| 会话解析（env → 最近 → 空态） | `crates/workbench/src/services/project_session.rs` |
| 标题栏项目槽 + 项目菜单 | `crates/workbench/src/view/dock.rs`（`render_title_bar`；菜单规格 / 内容由 `project::ui::project_menu_entries` / `build_project_menu` 提供） |
| 会话与刷新信号 | `crates/workbench/src/panels/shared.rs:314`（`Shared.project: Rc<RefCell<Option<project::ui::OpenProject>>>`） |
| 项目锁（OS 文件锁 + 占用者信息） | `crates/project/src/lock.rs`（`.RSmeta/project.lock` / `project.lock.owner`） |
| 名册持久化（固定 / 软删） | `crates/store/src/persistence/global_db.rs`（`project_info` 表：`is_pinned` / `pinned_at` / `removed_at`）+ 迁移 `crates/store/migrations/global/019_add_project_ui_state.sql`；`engine::persistence::global_db` 仅为重导出 |
| 动作 / 快捷键 | `crates/workbench/src/commands.rs`（`SwitchProject` / `CloseProject`；project crate 不再有 commands 文件） |
| 依赖接线 | 根 `Cargo.toml`（`project` 别名）+ `crates/workbench/Cargo.toml` |

> `project → engine, shared, gpui-kit`（符合硬约束，不依赖 `workbench` / `settings`）；视图为
> `Entity`，由 `workbench`/`app` 组合，`project` 不反向依赖 `workbench`。
> `promote_dialog.rs` / `snapshot_dialog.rs` 不再属于本模块（见 §12）。

## 12. 范围外与遗留确认

**明确排除（本期不做）**：

- **提升（promote）/ 引用（snapshot）/ 系统级共享资产**：属 M1 双层资产能力，另立设计。
- **移动 / 另存项目目录**：重命名只改显示名（U1）；移动目录影响全局库 path、连接快照、外部引用，风险高，延后。
- **项目模板市场 / 云端（DuckLake）项目**：`ProjectPath::Remote` 已预留，本期不实现入口。

**已确认（v2）**：

1. 选择器覆盖中央区，保留五段外壳。
2. 切换/关闭拦截未保存草稿（取消 / 放弃更改并继续 / 保存并继续）。
3. 不支持同一项目多窗口；被锁项目给「只读打开 / 仍要打开」逃生口。
4. 提升/引用移出本模块范围。
5. 删除磁盘数据输入项目名二次确认。
6. 存储结构树提供「打开 `.RSmeta`」入口。
7. 「从示例项目开始」首版必需。
8. 固定与排序方式持久化到全局库 / 设置。
9. 软删提供「已移除项目」找回入口。

## 13. 落地核对（2026-11-22）

以 `crates/project/src/ui.rs` 为准，对设计稿逐条核对后修正的差异。**两处不一致时以本节为准**：

> 证据列只记**符号名**，不记行号：行号随每次编辑漂移（2026-11-22 复核时表内的 `main.rs` / `ui.rs` 行号已全部失效），故本表统一按符号查。

| # | 设计稿（`project-prototype.html`，2026-09-11） | 当前实现 | 证据 |
| --- | --- | --- | --- |
| 1 | gpui-kit 0.7.1 | gpui-kit **0.10.0**（2026-11-22 由 0.9.0 升级） | `Cargo.toml` 的 `gpui-kit = { version = "0.10.0", features = ["gpui-fast"] }`；`vendor/gpui-component` / `vendor/gpui-base` 的 `Cargo.toml` 均为 0.10.0 |
| 2 | 卡片状态是徽标（Active/Syncing/…） | 状态为**纯文本**，按状态着色；文案经 `status_label` 由名册 key 翻成中文（`active`→活跃、`archived`→已归档…，未知值原样保留） | `project_card` + `status_label` |
| 3 | 固定标记 📌 | `Star` 图标 + `primary` 色 | `Icon::new(IconName::Star)` |
| 4 | 元信息「最后打开 · 连接数 · 版本数」 | 只有「最后打开：…」（状态已在首行带色显示，不再重复一遍）；`ProjectSummary` 无连接数 / 版本数字段 | `meta_line` |
| 5 | 路径「等宽、中间省略」 | `text_xs` 弱化色，**完整路径原文** | 卡片路径行 |
| 6 | 行内操作 📂 / ✕ | `打开` + `⋯`（右键菜单同源） | `card_menu_entries` |
| 7 | 新建对话框宽 560 | 组件默认 **448**（未显式设宽） | `open_create_dialog` 无 `.w()`；`dialog.rs` 默认 448 |
| 8 | 新建对话框含默认连接 / 初始化分析库 / 创建示例草稿 | **没有**这三项；只有名称、位置、目标预览、描述 | 同左 |
| 9 | 「校验未过时创建按钮禁用」 | 按钮**不置灰**；改为行内即时提示 + 提交后报错 | `submit_create` / `location_hint` |
| 10 | 项目设置为「中央编辑区 Tab」、6 节 | 全局设置「项目」页里的**「当前项目设置」块**、**7 节**（多「基本信息」；「依赖自检」实际叫「依赖」） | `ui/settings.rs::render_current_settings`；7 处 `section(theme, …)` |
| 11 | 「版本」= `Version` 版本链 | 实际是**手动快照台账**（`project_versions` 一行一条 message，不承载内容），支持记录 / 列出 / 按 id 删除（行内两段式确认） | `service::{list_versions, create_version, delete_version}`；`models.rs` 对 `Versioned` 的「未接入」标注 |
| 12 | 标题栏项目槽带状态点（`●`，警告=缺失驱动/归档） | 槽内是 `Folder` 图标 + 「项目」+ 名称，**没有状态点** | `view/dock.rs::render_title_bar`（`Button::new("project-slot")` + `CatalogIcon::Folder`） |
| 13 | 项目菜单 6 项，「切换项目…」带 `Ctrl+⇧P`，顶部一条缺失驱动黄色警告行 | **8 项**（非只读）：「新建项目…」/「打开现有目录…」/──/「**项目中心…**」（原「切换项目…」）/「项目设置…」/「重命名…」/「在资源管理器中显示」/──/「归档 / 取消归档」/──/「关闭项目」；只读时顶部多插一条置灰说明（共 9 项），`new` / `open-folder` **保持可用**、`rename` / `archive` 置灰；菜单**不显示快捷键**、**无缺失驱动警告行**（快捷键本身仍在：`crates/app/src/main.rs` 的两条 `KeyBinding` 分别绑 `Ctrl+Shift+P`（打开项目中心）/ `Ctrl+Shift+W`（关闭项目）） | `ui/picker.rs::project_menu_entries` / `build_project_menu` |
| 14 | 未保存拦截按钮「保存 / 不保存 / 取消」 | 「取消 / 放弃更改并继续 / 保存并继续」（标题「未保存的草稿」） | `ui.rs::open_unsaved_dialog` |
| 15 | 锁对话框按钮顺序「取消 / 只读打开 / 仍要打开」 | 标题「项目「{name}」已在另一实例打开」、正文给 pid，按钮「取消 / 仍要打开 / 只读打开」 | `ui.rs::open_lock_busy_dialog` |
| 16 | 选择器顶部只有一个「排序: 最近打开 ▾」 | 搜索框（placeholder 含 facet 说明）+ `状态：<当前>` + `排序：<当前>` 两个下拉 | `ui.rs::render_picker`（搜索行 / 两个下拉在 `render_picker` 内组装）；`ui.rs::StatusFilter` |
| 17 | 空态带「新建项目 / 打开文件夹」按钮 | `Empty` 组件只有图标 + 标题 + 描述，**不带按钮**；标题随 Tab 变化，未就绪时显示「加载中…」 | `ui.rs::empty_state` |
| 18 | 名册持久化在 `crates/engine/src/persistence/global_db.rs` | 实现已迁至 **`crates/store`（rds-store）**，`engine::persistence` 只是重导出薄壳；迁移脚本同在 `crates/store/migrations/global/` | `crates/engine/src/persistence/mod.rs`（`pub use rds_store::persistence::*`） |

本次（2026-11-22）新增、设计稿里没有的三项：

- **行内即时校验**：名称/位置在输入时给出错误，判据与提交同源（`validate_project_name` / `inspect_target`）。
- **位置记忆**：默认位置取 `settings.projects.last_create_location`（登记为 `SettingEntry::Module`，不上设置页）。
- **构建标识落项目**：每次非只读打开把 `version::info().tag()` 写进 `.RSmeta/config/settings.json` → `extensions.rds.build_tag`，**只写不展示**（避免与这里的「版本」概念混淆）。

**本轮核对范围**：§2 CRUD 全览（R4 搜索 facet、D1/D2 入口）、§3 三态与项目槽、§4 布局与卡片、§5 项目菜单、§6 新建对话框、§7 设置视图、§8 未保存拦截与锁、§9 校验清单、§10 token 名（抽查 `vendor/gpui-component/src/theme/schema.rs`：`list_hover` / `list_active` / `list_active_border` / `description_list.label.*` / `muted_foreground` 均存在；`title_bar.slot.background` 属产品 token）、§11 落点表中的文件路径（逐条 `Test-Path` 核对，已修正 `crates/store` 两处），均已与 `crates/project/src/ui.rs` / `crates/workbench/src/view/dock.rs` 对照修正。
**仍未逐条比对**：§10 只核到 token 名存在、未核每个组件实际取色调用（值以 `assets/themes/rds-theme.json` 为准，见 `theme/theme-design.md`）；§11 表内的组件选型描述（组件名以 `vendor/gpui-component` 0.10.0 源码为准，未逐行核到每个 view 文件）；§12 范围外。涉及实现细节时以 `project-wiring-matrix.md` 与源码为准，不要引用本文当接口文档。

### 13.1 2026-11-24 增量（项目管理入口：四刀 + 只读判据收窄）

| 改动 | 落点 |
| --- | --- |
| 菜单 6 → 8 项（+「新建项目…」/「打开现有目录…」，只读仍可用）；「切换项目…」→「项目中心…」 | `ui/picker.rs::project_menu_entries` / `attach_project_menu_handler`；期望值在 `ui/tests.rs::menu_spec_*` |
| **真正的切换**：`PendingAction::SwitchTo`；`request_switch` 判三分（无当前 → `request_open`；同根 → no-op；脏草稿 → 拦截后 `switch_now`）；`switch_now` = `do_close` → `open_path` | `ui.rs`（`PendingAction`）、`ui/actions.rs`、`ui/dialogs.rs::advance_pending` |
| **项目中心 = 全局设置的「项目」页**（不关当前项目也能浏览 / 管理别的项目）：列表由 workbench 注入 `render_picker`，套在「项目列表」分组卡片里；当前项目卡片打「当前」`Tag` | `settings/src/{settings_page.rs,skeleton.rs,fields.rs}`（`SettingsHost::projects_panel` / `fields::panel_item`）、`crates/workbench/src/view/actions.rs::open_settings_at`、`ui/picker.rs::is_current_project` / `project_card` |
| **动作投回主窗口**：设置窗口里点「打开 / 新建 / 打开文件夹 / 示例 / 关闭」经 `HomeDispatch` 投回主窗口执行（对话框与 `do_close`/`open_path` 的副作用都属于主窗口）；未注入时就地执行 | `ui.rs`（`HomeAction` / `HomeDispatch` / `HubOpener` + `ProjectUiHost::{home,on_open_hub}`）、`ui/actions.rs::act_*`、`crates/workbench/src/view/workspace.rs::wire_project_home` |
| `Ctrl+Shift+P` / 菜单「项目中心…」改为打开设置页的「项目」节（窗口已开着则切节，不抢焦点） | `crates/workbench/src/view/render.rs`（`SwitchProject` 处理器）、`ui/picker.rs::attach_project_menu_handler`（`switch` 分支 → `act_open_hub`） |
| **覆盖层退役**：`ui/hub.rs` 删除（`open_project_hub` / `close_project_hub` / `render_project_hub`）；`ProjectUiState` 9 → 8 字段（去掉 `hub_open`） | `crates/project/src/ui/`（子模块回到五个）、`ui/actions.rs`（`apply_opened` / `do_close` 不再复位） |
| 测试 50 → 50（31 窗口 + 19 纯函数；lib 64）：删 2 项覆盖层测试，加 3 项（入口委派宿主 / 未注入 no-op / 投递接管切换） | `ui/tests.rs`（`hub_entry_*`、`home_dispatch_takes_over_switch`、`current_marker_only_for_current_root`） |
| 示例项目入口改走切换：`create_sample` 末尾由 `open_path` 改 `request_switch`（设置「项目」页里点「从示例项目开始」也不再盖锁） | `ui/actions.rs::create_sample` |
| **只读判据收窄**：卡片与动作的只读拦截按**目标项目**判定——当前项目只读时，设置「项目」页里别的项目仍可固定 / 恢复 / 移出 / 删除；`read_only_blocked` 拆出 `write_blocked(host, target, …)`，`card_menu_entries` 第三参由「全局只读」改为「这张卡是否被拦」 | `ui/host.rs::write_blocked`、`ui/picker.rs::item_write_locked` / `card_menu_entries`、`ui/dialogs.rs::{toggle_pin, soft_remove, restore, forget, open_delete_dialog, confirm_delete}` |

| **项目设置浮层退役**：`ProjectUiState` 8 → 7 字段（去掉 `settings_open`）；`render_settings` → `render_current_settings`（有会话就返回，不再判开关；去掉 `absolute`/`inset_0`/关闭按钮），成全局设置「项目」页第二块 | `crates/project/src/ui.rs`（`ProjectUiState`）、`ui/settings.rs::render_current_settings`、`ui/picker.rs`（`settings`/`rename` → `act_open_hub`，预填不丢弃） |
| 设置侧新增第二注入槽 `SettingsHost::project_settings_panel`；「项目」节 = 「项目列表」+「当前项目设置」两个分组 | `crates/settings/src/{settings_page.rs,skeleton.rs}`、`crates/workbench/src/view/actions.rs`（进「项目」节时 `load_settings_snapshot`） |
| 主窗口不再挂项目设置层（`render.rs` 的挂载段删除） | `crates/workbench/src/view/render.rs` |
| 测试 50 → 50（lib 64）；settings 49 → 49 | `ui/tests.rs`、`crates/settings/src/settings_page.rs` 测试 |

> 仍未做（与设计稿的既有差异，非本轮引入）：标题栏槽**无状态点**（#12）、空态**无按钮**（#17）、卡片缺连接数与版本数（#4）、路径中间省略（#5）、新建对话框宽 448（#7）与其三项复选框缺失（#8）、菜单不显示快捷键（#13 后半）。
>
> **只读判据已收窄（原「跨项目误拦」已修）**：只读锁保护的是**当前项目**自己的 `.RSmeta`；设置「项目」页里别的项目不在锁的范围。判据落到 `ui/host.rs::write_blocked(host, target, …)`（`read_only_blocked` 是它的「目标=当前项目」薄包装），卡片侧用 `ui/picker.rs::item_write_locked` 决定置灰、`card_menu_entries(item, tab, locked)` 的第三参语义由「全局只读」改为「这张卡是否被拦」。于是当前项目只读时，别的项目的固定 / 恢复 / 移出列表 / 删除数据照常可用；当前项目自己的写命令（卡片与标题栏菜单）仍被置灰 + 动作内兜底，双保险不变。
>
> **迁入设置后已知取舍**（① 列表固定高度 `rems(30.0)`，因为 `render_picker` 根是 `size_full`、放进自适应卡片会塌成 0 且内部滚动失效；② 切节靠换 keyed-state id，搜索框内容与滚动位置会重置；③ 设置窗口若在首次 render 前被打开，列表位显示「尚未装配」占位；④ 设置页里点入口时若有未保存草稿，拦截对话框弹在**主窗口**）。机制细节见 `../settings/settings-architecture.md` §3.1 / §3.2。
