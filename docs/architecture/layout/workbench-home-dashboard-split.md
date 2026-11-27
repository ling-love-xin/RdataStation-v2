# 工作台首页拆分：欢迎页 / 统计看板

> 状态：设计已拍板，分批落地（2026-11-25，main 工作区）。
> 范围：中央 Dock 面板组里原 `EditorPanel`（tab 名「工作台」）职责混杂问题的拆分方案。

## 1. 背景与问题

原中央「工作台」tab（`crates/workbench/src/panels/editor.rs`，`EditorPanel`）在演进中叠加了四类互不相干的内容：

1. **统计概览**：连接 / 项目元数据 / 洞察画像 / 分析库四张卡（取数服务 `services/overview.rs`）；
2. **选中连接详情卡**：键值行 + 删除按钮 + 分析库导航树；
3. **功能入口**：生成 Mock、新建连接；
4. **遗留宿主职责**：连接对话框状态与订阅、草稿冲突 Diff 浮层、导航拖拽兜底转查询请求。

定位尴尬的根因（代码实证）：

- 欢迎 / 入口职责已由 `ProjectPickerPanel`（`panels/project_picker.rs`）承担：无项目时可见并激活，横幅 + 三入口（新建连接 / 打开项目 / 最近使用）+ 项目列表；`EditorPanel` 里的「新建连接」是第二套入口。
- 连接详情本质是对象属性视图，但属性面板（`PropertyHostPanel`）目标类型不含连接；左栏数据库导航也没有连接详情键值卡。详情卡夹在概览页里两边不靠。
- 只读看板与写操作按钮同屏，信息层级混杂。
- 宿主壳（对话框 / Diff / 拖拽）与页面内容绑死在同一个面板实体上。

## 2. 目标架构

中央 Dock 面板组拆为两个职责清晰的面板，外加既有宿主面板：

```
中央 Dock 面板组
├── WelcomePanel（由 ProjectPickerPanel 演进）
│     欢迎横幅 + 引导位（占位，后续做 onboarding）+ 项目选择器
├── DashboardPanel（新建）
│     统计概览四卡 + 刷新；挂载在中央组，默认不激活；状态栏 chip 唤起
├── EditorHostPanel（活动 SQL 文档宿主，不动）
└── 懒建面板：属性 / 搜索结果 / 元数据搜索 / 洞察工作页 / Mock 详情 / 插件管理 / 配置中心
```

### 2.1 WelcomePanel（欢迎页）

- **职责**：首次使用引导位 + 项目 / 连接入口 + 最近项目继续。
- **可见性**：无项目时可见并激活（沿用现 `ProjectPickerPanel::visible()` 口径）；有项目时隐藏。
- **回家入口**：有项目后仍可从标题栏 / Quick Open 回到欢迎页（入口位置落地时二选一，见 §5）。
- **引导位**：本轮只留占位区块（空态组件或预留注释），步骤引导（onboarding）后续迭代填充，不在本批实现。

### 2.2 DashboardPanel（统计看板）

- **职责**：只读业务统计概览。四张卡（连接 / 项目元数据 / 洞察画像 / 分析库）+ 刷新按钮；取数完全复用 `services/overview.rs`（后台线程纯函数，面板只持快照）。
- **默认形态**：挂载在中央 tab 组，但**默认不激活**；入口收进状态栏 chip。
- **唤起**：状态栏 chip 点击 → `DockArea::select_panel` 唤起 / 切回（复用现有 `select_panel` 幂等路径）。
- **彻底关闭**：设置项关闭后，状态栏 chip 不渲染、面板 `visible() = false` 不挂树、后台概览取数任务不发起（面板 `ensure_overview` 开头再判一次开关兜底）。

## 3. 设置项

新增布尔项，挂现有「布局」节（与「锁定布局」同组；看板显隐本质是界面 / 布局偏好）：

| 键 | 类型 | 默认 | 节 | 说明 |
| --- | --- | --- | --- | --- |
| `layout.dashboard_enabled` | Bool | `true` | layout | 显示统计看板入口与面板；关闭后状态栏不显示看板 chip，也不发起概览取数 |

接线按 `layout.locked` 先例五处（契约测试三条不变式把关）：

1. `model/layout.rs`：`Layout` struct 加 `dashboard_enabled: bool`（`#[serde(default = "default_true")]`），`Default` 补字段；
2. `registry/entries/layout.rs`：加 `SettingSpec`（`SettingKind::Bool`，`SettingEffect::Immediate`，`SettingEntry::Page`）；
3. `registry/mod.rs`：`Slot::LayoutDashboardEnabled` + `slot_for` arm + `slot_kind`（Bool）；
4. `service/layout.rs`：`apply_slot` arm + 访问器 `dashboard_enabled(cx)` / setter `set_dashboard_enabled(on, cx)`；
5. `service/mod.rs`：`value_by_key` arm；`apply_by_key` 的 layout 分组 match 列表补新 Slot。

## 4. 旧 EditorPanel 内容去向

| 现有内容 | 去向 | 阶段 |
| --- | --- | --- |
| 概览四卡 + 刷新 | 搬入 `DashboardPanel`；`services/overview.rs` 不动 | 一 |
| 连接详情卡（键值 / 删除）+ 分析库导航树 | 键值走属性面板（连接行 → `PropertyKind::Connection` provider）；删除走导航行右键 / 面板头 ⋯ 菜单；内嵌导航树缓存删除。待补主机/端口/库名/联邦开关/时间戳字段缺口 | 二 |
| 「新建连接」入口 | 收口到欢迎页横幅 + 导航栏「＋」；看板不放主操作 | 二 |
| 「生成 Mock」入口 | 主入口在右栏活动栏；看板不背 | 二 |
| 连接对话框状态 / 订阅 | 上移 WorkbenchView / Shared 桥层（对话框本就走 EditorBridge 事件路径） | 二 |
| 草稿 Diff 浮层、拖拽兜底转查询 | 同上移桥层，不随面板删除 | 二 |

阶段一交付后，`EditorPanel` 暂保留详情卡与入口（避免概览重复）；阶段二完成落点建设后瘦身并删除旧面板实体。

## 5. 落地批次（一刀一 commit）

1. **批次 1 — 设置项**：§3 五处接线 + 契约测试通过；面板消费点暂留读取位。
2. **批次 2 — DashboardPanel**：新建面板，搬概览卡片与后台取数；挂中央组默认不激活；状态栏 chip 唤起；关闭态不渲染 chip、不取数。
3. **批次 3 — WelcomePanel 演进**：`ProjectPickerPanel` 改名 / 演进为欢迎页，引导位占位；补「回家」入口（标题栏 House 图标按钮，对齐主题 / 语言按钮先例）。
4. **批次 4 — 旧面板瘦身**：详情卡 / 树 / 入口迁出，宿主职责上移，删除 `EditorPanel`。
5. **批次 5 — 收口**：`cargo check --workspace` 0 error；随迁移补齐 i18n 词条与出口包裹；同步本文档与 skill。

### 5.0 实现归属（2026-11-26 收口，看这段比看分支名可靠）

本任务的全部落地都在 `main` 这条线上完成，提交号见 §5.1 进度表（`2bae2306` / `25701a74` /
`290c1ad5` / `8c62eb43` / `4d81c73f` / `7c751f54` / `b64114d7` 等）。

同期另有一条隔离分支 `feat/workbench-dashboard-split`，是**同一设计在另一条线上做的第二份实现**，
已于 2026-11-26 废弃删除。判据：① main 已具备 `panels/dashboard.rs`、`layout.dashboard_enabled`
五处接线、`dashboard_panel` 共享字段、`ensure_dashboard_panel`、`ui_contract` 白名单；② 该分支
唯一看着像未合的 `bb5c138c`（分组计数改纯文本 + 导航分组重命名原位编辑）在 main 上由
`workbench_shell/src/tree.rs::group_count`（纯文本计数）与 `nav_view/rows.rs::group_rename_for`
（原位重命名）等价实现，`git cherry-pick -n bb5c138c` 应用结果为空。

**以后改这条线直接在 `main` 上改**，不要从历史分支找回；批次 4 的 2a / 2b / 2c 仍未落地，
形态待拍板（见下）。

### 5.1 落地进度（main 工作区，2026-11-25）

| 批次 | 状态 | main commit |
| --- | --- | --- |
| 1 设置项 | 已完成 | `2bae2306` |
| 2 DashboardPanel + 状态栏 chip | 已完成 | `25701a74` |
| 3 欢迎页演进 + 回家入口 | 已完成 | `290c1ad5` |
| 4-1 摘除旧面板概览 | 已完成（纯去重，−93 行） | `8c62eb43` |
| 4-2b Diff 浮层状态上移 Shared | 已完成（挂载形态待拍板） | `4d81c73f` |
| 4-2a 对话框状态 / 订阅 / 桥上移宿主 | 待办（阻塞，见下） | — |
| 4-2c 拖拽兜底请求逻辑沉 Shared | 已完成（下沉 `Shared::request_nav_drag`，落点仍在面板 on_drop） | `b088f99f` |
| 4-3 连接详情卡 / 导航树落点 | 摸底完成（落点 = 属性面板 + 导航右键删除；待补字段缺口）；旧卡删除待阶段二 | — |
| 5 收口校验 / i18n / 文档同步 | 部分完成（白名单 + 契约清单 + 耦合表已同步；全量 check 0 error；locale 词条待随并行 WIP 收口） | `7c751f54`、`b64114d7` |

**批次 5 收口明细（2026-11-25）**：

- 白名单契约 `tests/ui_contract.rs::shared_fields_are_whitelisted` 已补登本任务四字段：
  `dashboard_panel` / `open_dashboard_request` / `welcome_forced`，以及批次 2b 漏登的
  `scratchpad_diff`；`panels-modules.md` §3 耦合表、`shared-ownership-plan.md` §2.1 清单同步。
- 契约测试通过（1 passed）；`rds-shared` 68 测试全过（含 zh/en 键对齐）；`cargo check -p rds-workbench` 0 error。
- `dashboard.rs` 已纳入尺寸（2a）/ 颜色（2b）两份源码契约扫描清单（`b64114d7`）；`ui_contract` 10 项全过，`rds-settings` 50 测试全过。
- **遗留 i18n**：`"回到欢迎页"` zh/en 词条与并行自动刷新 WIP 同处 `locales/{zh,en}.json`，
  按 WIP 隔离口径本批不摘 hunk，留工作区待并行线收口时一并入库；未登记项 `tr` 回退中文原文，不阻断编译。
- `cargo check --workspace` 0 error（仅两条既有 warning，非本批）；剩余 EditorPanel 出口 tr 包裹待批次 4 主体落定后复核。

**2026-11-26 main 复测（HEAD `26d5344a`）**：`rds-settings` 50 passed、`rds-shared` 68 passed（含 zh/en 键对齐）、
`ui_contract` 10 passed（白名单 / 尺寸 / 颜色 / 面板清单登记全过）、`cargo check --workspace -j 1` 0 error
（仅 `editor/format.rs` 与 `data_objects/mod.rs` 两条既有 warning）。注意：workbench 测试编译须在 **cargo 层**
`-j 1` 单线程；多 crate 并发曾触编译期 OOM / E0786 rmeta 损坏并连锁出 E0432/E0463，属环境资源 / 缓存问题，非源码错误。

**2026-11-26 4-2c 后复测（HEAD `114fdee5`）**：`rds-settings` 50 passed、`rds-shared` 68 passed、
`ui_contract` 10 passed、`cargo check --workspace -j 1` 0 error（仅 `editor/format.rs` 与
`data_objects/mod.rs` 两条既有 warning）。

跨线适配（非本任务路径，单独 commit）：

- `f19a6064` 适配 `AnyWindowHandle::update` 闭包三参签名；
- `0e5cc71d` 补导出 `open_source_picker_for_connection`（source_picker 并行线接线遗漏）。

**批次 4 阻塞与未决点**：

- 连接详情卡（键值 / 删除）+ 分析库导航树的落点在 `database` crate 的 `NavView`
  （左栏 `DatabaseNavPanel` 只是薄壳）。2026-11-25 只读摸底实证（无需新建行内详情卡）：
  - **键值展示已由属性面板承接**：导航连接行映射 `PropertyKind::Connection` 的 `PropertyRef`
    （`nav_view/rows.rs:171-179`），点击 / Enter 经 `host.show_properties` 打开中央属性 tab；
    provider 侧 `connection_base_rows` / `connection_base_overview` / `connection_meta_sections`
    已出 名称 / 数据库类型 / 归属域 / 驱动 / 版本 / 文件路径 / 连接串 / 描述 + 状态 chip + 认证 / 网络 / 环境分节。
  - **删除连接已有导航入口**：连接行右键「删除连接」（`rows.rs:2104`，确认对话框 → `host.delete_connection`）
    与面板头 ⋯ 菜单「删除当前连接」（`chrome.rs:766`）同一动作；宿主端口 `NavHost::delete_connection` 已实现。
  - **分析库导航树**本就是左栏导航树职责，详情卡里的内嵌 `nav_tables` 缓存是重复出口。
  - **迁移待补字段缺口**：旧卡片的 主机 / 端口 / 数据库名 / Schema / DuckDB 联邦开关 /
    创建时间 / 更新时间，未全部进 `connection_base_rows`；迁卡时需逐项核对——有的并入
    连接串 / 网络分节，有的（联邦开关、时间戳）需补 provider 行或确认不再展示。展示形态
    据此定为**走属性面板**，不在 NavView 行内另建键值卡。
  - **2026-11-26 字段级核证**（2026-11-27 修订：`ConnectionInfo` 已删 `use_duckdb_fed`）：属性 counts 由 `property_panel.rs:287` 从
    `ConnectionManager::get_connection_info`（`engine/connection_manager.rs:62` 的
    `ConnectionInfo`）喂入。该结构**已有** `created_at: Instant`、
    `description`、`url`、`server_version`；**缺**独立的 `host` / `port` / `database` /
    `schema` / `updated_at`（主机·端口·库名编码在 `url` 里，可解析或补字段；`created_at`
    是 `Instant` 非展示串，且无 `updated_at`）。主机/端口/
    库名需从 url 解析或回补 `ConnectionInfo`；Schema、更新时间当前数据层无对应值。
- 批次 4 再拆三步，按风险从低到高：
  1. **摘除旧面板概览**（已完成 `8c62eb43`）：看板已接管四卡，删除 `EditorPanel`
     的 overview 字段 / 取数方法 / 概览渲染出口（低风险，纯去重）；
  2. **宿主职责上移**：连接对话框状态与订阅、草稿 Diff 浮层、导航拖拽兜底转发
     从面板移到 WorkbenchView / Shared 桥层（独立于详情落点）。2026-11-25 核证细化：
     - **2a 对话框**：`ConnectionDialogState<EditorPanel>` 泛型绑面板实体，环境下拉
       订阅挂 `cx.entity()`；集成测试 `tests/dialog_host_layer.rs` 的同构 `HostView`
       直绑 `EditorPanel::new` / `request_new_connection` / `install_editor_bridge`
       并观察 editor 实体级联。上移需把泛型改 WorkbenchView、桥改持宿主实体，并同步
       改写该测试；必须保留 `install_host_redraw_bridge` 的 defer 防双租口径。
       `ConnectionDialogHost` 已由 `Shared` 实现，不动。
       2026-11-25 只读摸底补充实证：
       - 泛型约束是 `H: 'static`（`state.rs` / `mod.rs`），**不要求 `H: Host`**；
         Host 能力另以 `Rc<dyn ConnectionDialogHost>`（Shared 实现）传入。故上移只需把
         实体句柄类型 `EditorPanel` 换成宿主类型，crate 内泛型不用改。
       - 实体绑定点：`dialog.open(cx.entity(), …)` 第一参、`subscribe_env_confirm(&cx.entity(), …)`；
         项目下拉订阅 `subscribe_project_confirm` 走 trait 对象，不绑实体。
       - 直读 `editor.dialog_state()` / `harness.dialog` 的集成测试面比单测广，至少含
         `connection_type_driver` / `connection_staging` / `connection_render_matrix` /
         `connection_project_picker` / `connection_edit_backfill` / `connection_dialog_ui` /
         `connection_template` / `dialog_host_layer`——上移需逐份改 harness 绑定与访问路径。
       - 装配点：`install_editor_bridge`（`panels/mod.rs:580`）闭包内 `editor.update` 调
         `request_*`；`install_host_redraw_bridge`（`:558`）defer 防双租，口径保留。
     - **2b Diff 浮层**：状态已搬 Shared（`scratchpad_diff: Option<ScratchpadDiffView>`，
       与 `scratchpad_search` / `scratchpad_file_info` 同型）；**挂载形态待拍板**。
       2026-11-26 三形态先例实证（渲染函数 `render_scratchpad_diff_pane` 三形态复用）：
       - **① 独立中央 Dock tab（非模态，推荐）**：先例 `SearchResultsHostPanel`
         （`search_results.rs`，吃 `ScratchpadSearchView` 快照；装配 `workspace.rs:391`
         `cx.new` 入中央 DockArea，Shared 存 `WeakEntity`，show 复用聚焦或新建）、
         `PropertyHostPanel`（`property_panel/mod.rs`，`workspace.rs:872`）。
         改动：新增 `DiffHostPanel` 模块 + 装配 + Shared 弱句柄 / show 方法，new 时从
         `scratchpad_diff` 快照，tab 标题「对比: 文件名」；与文档 tab 并列、可切走再回。
         风险中（新面板实体 + Dock 装配 + 多开判定），但有同型先例。
       - **② 宿主 Dialog/Popover 模态弹层**：先例 `render_scratchpad_file_info_popover`
         （`actions.rs:252`，`Dialog::new(cx)` 包 content，内置 overlay / Esc / 遮罩点击
         关闭；`render.rs:355` `root.child(info)` 叠加）。改动：宿主加
         `render_scratchpad_diff_popover` 读 `scratchpad_diff`，content 调 diff 渲染函数，
         on_close 清状态，render.rs 挂载；不新增面板实体。风险低-中（套现成 Dialog），
         但 Diff 较长时弹窗滚动 / 尺寸需调；模态遮挡，与现状非模态差异最大。
       - **③ 宿主 overlay 内嵌层（非模态，层归属上移）**：现状出口内嵌 `editor.rs:472`
         内容列；③ 把这段渲染挪到宿主 `render.rs` 的 root overlay 层（仿 `quick_open` /
         `notification_list` 的 `root.child(absolute 层)`，`render.rs:352-366`），不套
         Dialog。改动：editor.rs 删 Diff 出口，宿主 render.rs 加 absolute overlay 层读
         `scratchpad_diff` 渲染。风险低（纯挪出口），但 overlay 定位 / 遮挡口径需对齐现有层。
       - 现状：出口内嵌编辑区内容列、非模态；状态在 Shared，三形态都只动挂载出口。
     - **2c 拖拽兜底**（已完成 `b088f99f`）：原 `EditorPanel::send_nav_drag` 下沉为
       `Shared::request_nav_drag(qualified, cx)`——解析 `selected_connection`、构造
       `QueryRequest{conn_id, sql: qualified+" ", run:false}` 走 `request_query`，再
       `notify_host`；面板 on_drop 闭包只持 `shared` 调它，删除私有方法与不再使用的
       `QueryRequest` 导入，去掉原方法末尾冗余的面板重绘 `cx.notify()`。落点交互不变，
       请求归属收在 Shared 一处，旧面板瘦身时不必再搬这段逻辑。
  3. **详情卡落点**：待 `database` 导航线 WIP 落定后，在 NavView 行模型上实现
     连接详情，再删除旧 `EditorPanel` 实体。

## 6. 约束与风险

- **render 纯读路径**：概览取数必须留在后台线程（`services/overview.rs` 既有口径），面板 render 只读快照；不得在 render 内打开库 / 读盘。
- **主题取色**：新面板一律 `cx.theme().colors.<字段>`，禁止裸 hex / rgb；字体族走主题键。
- **组件选型**：按钮用 `Button`，空态用 `empty::Empty`，不手搓可点 div；图标用 `gpui_kit::assets::IconName` 完整目录或 `component::IconName` 子集。
- **借用顺序**：`let theme = cx.theme();` 之前完成 `entity.update` / 面板读取（E0502 约束）。
- **i18n**：新写显示位一律 `tr(...)`；`tr` 只接 `&'static str`，String 返回值用 `tr_dyn`；静态表存原文，只在渲染出口包。
- **WIP 隔离**：主工作区未提交改动集中在 result_panel / scratchpad / data_objects，本批不改这些文件；commit 只 add 本批涉及路径。
- **未核证项**：连接详情迁入左栏导航后的具体展示形态（键值列表 vs 选中行展开），阶段二开工前先核证导航面板现有行模型再定。

## 7. 相关文件

- 现状面板：`crates/workbench/src/panels/editor.rs`、`panels/overview.rs`、`panels/project_picker.rs`、`panels/dashboard.rs`
- 取数服务：`crates/workbench/src/services/overview.rs`
- 装配：`crates/workbench/src/view/workspace.rs`（中央 tab 组懒建面板）
- 状态栏：`crates/workbench/src/view/actions.rs`、`view/status.rs`
- 标题栏：`crates/workbench/src/view/dock.rs`
- 设置先例：`crates/settings/src/model/layout.rs`、`registry/entries/layout.rs`、`service/layout.rs`
- 契约 / 耦合表：`crates/workbench/tests/ui_contract.rs`（Shared 字段白名单）、`docs/architecture/layout/panels-modules.md` §3、`shared-ownership-plan.md` §2.1
