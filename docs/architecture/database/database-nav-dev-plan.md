# 数据源管理 / 数据库导航模块 · 开发方案（Phase A/B/C）

> 状态：**Phase A/B 完成；Phase C 进行中（C1–C7 已实现；C8 由连接侧推进）；v6/v7 降密与徽标语义已实现（V1–V10）· 2026-09-14** · 测试基线（2026-09-19 复跑，`-j 2`）：`cargo test -p rds-engine --lib`（449）/ `-p rds-database --lib`（45）/ `-p rds-workbench --lib`（110）全绿 · 关联文件：`database-navigator-prototype-design.md`（原型设计）、`database-navigator-prototype.html`（可交互原型）
> 设计基线：作用域来源短码 `P/G/GP`、项目级自定义分组（多对多）+ 多值标签、三级缓存与增量刷新、缓存永不自动删除、属性面板填充编辑区右侧、预热方案 C。
> 技术栈：GPUI（gpui-kit 0.6）；M4 领域/服务在 `crates/database`（非 UI），视图在 `crates/workbench`。
> 前置：M3 连接模块 Phase A/B 已实现；engine 元数据缓存（含增量/预热索引/FTS/分页/版本迁移）已迁移。

## 0. 进度

| 阶段 | 状态 |
| --- | --- |
| Phase A（骨架与核心闭环） | ✅ 已实现 |
| Phase B（分组/标签/搜索/属性面板/状态/菜单/缓存） | ✅ 已实现（B1–B8） |
| Phase C（预热/增量/收尾） | 🟡 进行中（C1–C7 已实现；C8 由连接侧推进） |
| **v6/v7 降密与徽标语义** | ✅ **已实现（V2–V10，2026-09-13）** |

**v6/v7 降密与概念定位（2026-09-13）**

设计依据：`database-navigator-prototype-design.md` §1.1（定位表 + 3 条边界规则）、§2（行内解剖 / 双通道徽标 / 归属域列 / 分组头）、§6.1。

| # | 任务 | 落点 | 状态 |
| --- | --- | --- | --- |
| V1 | 概念定位与命名（文档） | `database-navigator-prototype-design.md` §1.1/§2/§6；「来源 / 作用域」→「归属域」 | ✅ 2026-09-13 |
| V2 | 连接行瘦身：**移除行内 🗂 与驱动文本**；名称 `text_ellipsis` | `database/src/nav_view.rs::render_connection_row` | ✅ 2026-09-13 |
| V3 | **双通道徽标**：颜色 = 状态，形状 = 类型（内叠 2 字母） | `database/src/nav_view.rs::{nav_type_badge, NavBadgeStatus}` + `Shared::driver_catalog`（`nav_runtime::driver_catalog()` 一次性加载） | ✅ 2026-09-13（hover 卡已接：`nav_badge_hover_card` 用 `HoverCard`，300ms 显类型 / 状态 / 驱动） |
| V4 | **归属域右对齐固定列** + `⋯ → 显示归属域` | `database/src/nav_view.rs::render_connection_row`（`.justify_end()` 定宽列）+ `settings::SettingsService::{show_scope,set_show_scope}` | ✅ 2026-09-13 |
| V5 | 分组头**聚合健康度** + **全折叠** | `database/src/nav_view.rs::render_group_header`（`已连接/总数` + 失败计数 + 全折叠）+ `render_nav_tree` 预计算连接 / 错误集 | ✅ 2026-09-13 |
| V6 | **多组引用样式 + 主组** | `database/src/nav_view.rs::{render_nav_tree, render_connection_row, render_reference_row}`；主组显式存储 `connection_group_members.is_primary`（右键 `设为主组 ▸`），未指定回退 `membership[conn][0]`（组排序最前） | ✅ 2026-09-13 |
| V7 | **facet 入口**（归属域 chips + 「筛选 ▾」承载类型 / 驱动 / 标签） | `database/src/nav_view.rs::{render_database_nav, build_facet_items, nav_facet_candidates, apply_facet}` + `DatabaseNavView` facet 状态 + `settings::model::NavigatorFilters`（持久化）；搜索语法 `scope:/source:/type:/driver:/tag:` | ✅ 2026-09-13（搜索 token 作额外 AND 约束，与 chips 叠加，不互相回写） |
| V8 | 行操作**悬停 / 选中显隐**（`+`、`✎`、连接/断开） | `database/src/nav_view.rs::render_connection_row`（`.group("nav-conn-row")` + `.group_hover` + `.opacity`） | ✅ 2026-09-13（右键 + 键盘仍为全量入口） |
| V9 | 行尾 **`+` = 标签快捷入口** | `database/src/nav_view.rs::render_connection_row`（复用行内组织编辑器） | ✅ 2026-09-13 |
| V10 | **`⋯ → 显示标签`**（默认关）+ 标签行内「≤2 chip + `+N`」 | `database/src/nav_view.rs::render_connection_row` + `settings::SettingsService::{show_tags,set_show_tags}` | ✅ 2026-09-13 |

> 新增常量：`ui.rs::{NAV_BADGE_SIZE, NAV_SCOPE_COL_SHORT, NAV_SCOPE_COL_TEXT, NAV_ADD_TAG_SIZE}`。
> 单测：`panels::tests::{type_badge_maps_known_types_and_falls_back, search_facets_parse_tokens_and_free_text, type_short_label_strips_category_suffix}`、`database::model::tests::source_key_roundtrip`、`engine::persistence::connection_org_store::tests::primary_group_is_exclusive_and_falls_back`、`settings::model::tests::{legacy_config_without_navigator_uses_defaults, navigator_filters_roundtrip}`。
> **驱动目录**：`nav_runtime::driver_catalog()`（同步读全局 `drivers` 表；随组织数据在 `defer_in` 一次性加载）→ `Shared::driver_catalog` 跨面板共享，**render 期零 I/O**。
> **属性面板**：连接项新增「数据库类型」行（`property_panel::load_properties` 接 `db_type`）、「驱动」行显示目录友好名（`PostgreSQL (Official) · postgres_native`）。
> **仍待做**：工作线程优先级队列；大 schema 列内联阈值；标签命名规范 `key:value`。

**Phase A 实现位置**

| 交付 | 文件 |
| --- | --- |
| 导航领域模型（NavNode/NavSource/NavFolder/NavPath/NavState/ConnectionEntry/分组/标签） | `crates/database/src/model.rs` |
| 导航编排服务（三级缓存预留、懒加载、来源过滤） | `crates/database/src/navigator_service.rs` |
| 迁移：`navigator_state` / `connection_tags`（global） | `crates/engine/migrations/global/018_add_navigator_state.sql` |
| 迁移：`navigator_state` / `connection_tags` / `connection_groups` / `connection_group_members`（project） | `crates/engine/migrations/project_meta/017_add_navigator_groups_tags_state.sql` |
| 连接 / 断开运行时（保留缓存） | `crates/workbench/src/services/nav_runtime.rs` |
| 数据源导航面板（标签页 + 对象树懒加载 + 连接/断开） | `crates/workbench/src/panels/`（`render_database_nav` 等） |
| 断开不删缓存 | `crates/workbench/src/services/connection_service.rs`（`close_connection`） |
| 依赖声明 workbench → database | `Cargo.toml` / `crates/workbench/Cargo.toml` |

**Phase A 已知限制（已收敛，保留作历史记录）**

- 加载为阻塞式 → ✅ 已迁后台（`nav_jobs` 工作线程 + 结果队列，见「导航加载迁后台（收尾）说明」）；
- 未接 L2 每连接缓存 → ✅ 已接（`database::cache::NavCache`，C3）；
- 展开态未持久化到 `navigator_state` → ✅ 已持久化（`nav_store`，B6）；
- 面板头按钮（新建连接 / 刷新 / 断开）与搜索输入、右键菜单——✅ 已实现（新建/刷新/断开/搜索/右键均落地，见 Phase B 表）；
- 连接 URL 未做密码百分号编码——✅ 已修复（2026-09-11，随 M3 C3 第一批收敛：URL 组装下沉 `connection::url::build_connection_url`，userinfo 统一百分号编码）；
- 只渲染数据源与其对象树，**不含分析资产**（符合范围边界）。

**M3 连接侧协作能力（2026-09-11，已可供导航消费）**

- 运行态连接状态：`workbench::services::workspace_loader::fill_connected`（列表项 `connected` 来自连接管理器）；
- 作用域可见性：`load_connections_for_scope(project_root)` 合并全局 + P_/GP_（未打开项目仅全局）；
- 运行时连接入口：`nav_runtime::{connect_entry, disconnect_entry, is_connected}`（缓存断开不删）；
- 来源短码 `P/G/GP` 展示：归属本模块 B8（短码⇄文字开关），连接侧仅保证 ID 前缀语义稳定；
- **标签 / 分组权威存储**（2026-09-11 上提，连接域数据）：`engine::persistence::ConnectionOrgStore`（`open_global` / `open_project` / `open_at`）；M3 `DataSourceService` 保存/更新时同步 tags、删除时一致性清理；导航侧 `nav_runtime::{list_tags,set_tags}` 已接线，分组视图待 B3。
- **M3 C3 传输层收敛完成**（2026-09-11）：URL 处理族 / URL 参数注入改写 / 协议链执行与隧道生命周期已全部下沉 `crates/connection`（`url.rs` / `url_params.rs` / `chain.rs`），`ConnectionService` 仅剩依赖 engine 的会话生命周期编排（50KB），原「85KB 遗留」风险解除。

**Phase B 实现进度**

| 任务 | 状态 | 落点 |
| --- | --- | --- |
| B5 属性面板（类型信息 + 属性网格 + 子实体表格） | ✅ | `crates/database/src/property_panel.rs`、`crates/database/src/navigator_service.rs`（`load_properties`） |
| B5 属性面板视图（编辑区右侧停靠，双击对象/连接打开） | ✅ | `crates/workbench/src/panels/`（`EditorPanel::render_property_panel`、`Shared::property_target`） |
| B4 搜索（本地筛选：连接/对象名，命中自动展开） | ✅ | `crates/workbench/src/panels/`（`InputState` + `nav_node_matches`） |
| B6 状态持久化（展开态，`navigator_state`） | ✅ | `crates/workbench/src/services/nav_store.rs`、`nav_runtime.rs`、`panels/` |
| B1 分组服务（CRUD + 多对多 + 排序） | ✅ 服务层就绪（2026-09-11） | `engine::persistence::ConnectionOrgStore`：create/update/delete/list_groups、add/remove_member、set_member_order、list_group_members、list_groups_for_connection |
| B2 标签服务（多值 + 检索） | ✅ 服务层就绪（2026-09-11） | `ConnectionOrgStore`：set_tags/list_tags/list_connections_by_tag/list_all_tags；`nav_runtime::{list_tags,set_tags}` 已接线；M3 保存/更新同步、删除清理 |
| B3 分组/标签视图（拖拽/右键/对话框） | ✅ 已实现（2026-09-12） | `crates/workbench/src/panels/`：`render_nav_tree`（分组一级 + 「未分组」）、`render_group_header`（统一色条 + 计数 + 折叠）、`render_group_editor`（右键「移动到分组…」：行内分组多选 + 新建分组）、`render_tag_editor`（行尾 `+`：仅标签输入）、`nav_source_chip`（来源筛选 chips）、`ensure_nav_org` / `reload_nav_org` |
| B1/B2 视图接线（分组多对多 + 标签多值） | ✅ 已实现 | `nav_runtime::{list_groups,create_group,list_group_members,add_to_group,remove_from_group,list_all_tags}`；`ConnectionOrgStore::list_tag_pairs`（一次性映射） |
| B1 组内排序口径落地（未排按名称） | ✅ 已实现（2026-09-16） | `ConnectionOrgStore::{MEMBER_ORDER_UNSET, list_group_members_detailed}` + 迁移 `022_normalize_group_member_order.sql`；视图 `database/src/nav_view.rs::nav_order_members`（纯函数，分组内与「未分组」共用） |
| B7 上下文菜单动作（查看数据 / 复制名 / 查看属性 / 刷新） | ✅ 已实现（2026-09-12） | `crates/workbench/src/panels/`：`ContextMenuExt::context_menu` 挂到连接行 / 对象节点 / 分组头；`Shared::editor_set` + `SidebarEvent::EditorSqlRequest`（生成 SELECT → 编辑区）；`toggle_connection` / `refresh_node` / `delete_group` / `create_group_interactive`；分组删除带 `AlertDialog` 确认 |
| B8 缓存管理入口 + 短码⇄文字开关 + 属性面板宽度记忆 | ✅ 已实现（2026-09-12） | `crates/settings/src/model.rs`（`Navigator` 分区）+ `settings_view.rs`（数据源导航节）；`crates/workbench/src/components/cache_dialog.rs`（两处入口）；`database/src/nav_view.rs::{refresh_all, render_connection_row}` + `panels/editor.rs::render_property_panel` + `EditorPanel::render`（`h_resizable`） |
| B7 拖拽（表 / 视图 → 编辑区插入限定名） | ✅ 已实现（2026-09-16） | `crates/workbench/src/panels/`：`NavDragPayload` + `NavDragGhost`（拖拽幽灵）、`render_nav_node` 挂 `on_drag`（仅表 / 视图）、SQL 区容器 `drag_over` + `on_drop`、编辑区 `content` 兜底 `on_drop`、`EditorPanel::apply_nav_drag`（`NavDropMode::{AtCursor, Append}`） |
| B3 归组拖拽 + 组内外手动排序 | ✅ 已实现（2026-09-16） | `panels/`：`NavConnDragPayload` / `ConnDropTarget` / `nav_reorder`（纯函数）、连接行与引用行挂 `on_drag`+`on_drop`、分组头挂 `on_drop`（归组 / 移出全部）、`SidebarPanel::{apply_conn_drop, container_order, container_label}`；落库 `ConnectionOrgStore::{set_member_order_all, list_ungrouped_order, set_ungrouped_order}` + `nav_runtime::set_container_order` |
| B3 分组描述表单（名称 + 描述） | ✅ 已实现（2026-09-16） | `crates/workbench/src/components/group_form_dialog.rs`（新建 / 编辑共用；名称必填 + 内联提示）；入口：面板头 `🗂＋`、分组右键「新建分组 / 编辑分组…」、组内联编辑器「新建分组」；落库 `nav_runtime::{create_group_with, update_group}`（`rename_group` 不再洗掉描述） |
| B3 分组本身排序 + 连接重排快捷键 | ✅ 已实现（2026-09-16） | `panels/`：`NavGroupDragPayload` / `group_ids` / `apply_group_drop` / `step_group`（分组排序）、`nav_step`（纯函数）+ `nav_step_selected`（`Alt+↑/↓`）；落库 `ConnectionOrgStore::set_group_order` + `nav_runtime::set_group_order`；命令 `NavReorderUp/Down` 绑在 `database-nav` context |

**Phase B 已知限制**

- 属性面板为**堆叠分区**（列/索引/约束），暂未做子实体 Tab 切换；
- 双击节点打开属性（gpui `click_count >= 2`）；右键菜单已实现（见下方 B7 范围）；复制 / 生成 SQL 已支持、INSERT/UPDATE/DELETE 待后续；
- 属性加载已迁后台（`nav_jobs::enqueue_properties` + `apply_props_results`），与 Phase A 的阻塞式问题一并收敛；
- 归组：入口=右键「**移动到分组…**」或**把连接行拖到分组头**（v6 已移除行内 `🗂`）；组内外手动排序已接线（见下方「B3 归组拖拽与排序实现说明」）；
- 新建分组与编辑分组都走**表单**（名称 + 描述，`group_form_dialog.rs`）；行内重命名（分组头右键 → 行内输入）仍保留，只改名称；
- **facet 筛选**（归属域 + 类型 / 驱动 / 标签）持久化在 `settings.json` 的 `Navigator::filters`（UI 偏好）；**展开 / 选中**仍走 `navigator_state`；分组关系走组织存储；无单独的面板级 `navigator_state` 行。
- 搜索支持**连接名 + 标签**子串匹配，并支持 `scope:` / `source:` / `type:` / `driver:` / `tag:` 结构化 token（作额外 AND 约束）；命中高亮已实现（C5）。

**Phase B7 上下文菜单已实现范围**

| 节点 | 菜单项 |
| --- | --- |
| 连接 | 连接 / 断开 · **测试连接** · 编辑连接… · 查看属性 · 移动到分组… · 设为主组 ▸ · **复制连接（模板）…** · **共享至项目**（仅 `G_`）/ **取消共享**（仅 `GP_`） · **删除连接**（二次确认） · 复制名称 · 刷新元数据 · **通用项** |
| 表 / 视图 | 查看属性 · 查看数据（`SELECT * … LIMIT 200` 注入编辑区） · **生成 SQL ▸**（INSERT / UPDATE / DELETE） · 复制名称 · 复制限定名 · 刷新元数据 · **通用项** · **生成 Mock 数据**（表 / 视图专属） |
| 其他对象（列 / Catalog / Schema / 文件夹 / 例程 / 序列 / 触发器） | 查看属性 · 复制名称（*限定名仅在有 catalog/schema 时出现*） · 刷新元数据（*仅可展开节点*） · **通用项** |
| 分组头 | 重命名分组（行内输入） · **编辑分组…**（表单：名称 + 描述） · 新建分组（表单） · 删除分组（`AlertDialog` 二次确认）；「未分组」仅「新建分组」 |

> **通用项**（所有节点都有，位于菜单底部、分隔线之后）：**在 SQL 编辑器中打开** · **查看洞察**；
> **生成 Mock 数据**仅**表 / 视图**有，夹在两者之间。

**B7 待办（后续阶段）**

- 连接右键「查看洞察」与各节点右键「生成 Mock 数据」目前仅切换到右 Dock 占位面板（M7 / M8 待实现）；「在 SQL 编辑器中打开」仅选中连接 + 聚焦编辑区（SQL 可执行区目前受 `use_duckdb_fed` 限制，见架构 §11#16）。

**B7 拖拽实现说明（2026-09-16）**

- 只有**表 / 视图**行可拖；拖拽幽灵显示短名，插入的是**限定名**（`nav_qualified_name`，`db.db.name` 已去重）。
- 载荷（`NavDragPayload`）只带两个字符串，**不带连接句柄与执行计划**：拖拽不承诺语义，落点决定动作。
- 落点两条路径（gpui 的 drop 按 bubble 派发 + `stop_propagation`，**内层落点优先**，不会双插）：
  - **SQL 区容器**（`use_duckdb_fed` 连接才渲染）：插到**光标处**（`TextareaState::insert`，尊重选区）；拖入时描边高亮（`drag_over::<NavDragPayload>`，颜色走主题 token）。
  - **编辑区内容区**（兜底）：追加到草稿末尾；拖到编辑区任意位置都不会“没落点”。
- 未聚焦的 SQL 输入光标停在 0，直接插入会把表名顶到用户语句前面 → 只有**SQL 区可见且已聚焦**时才走光标插入，其余一律追加（与右键「查看数据」同策略走 `set_value`，不发 `InputEvent`，手动同步 `editor_dirty` / `editor_sql`）。
- 两种落点都**聚焦 SQL 区**；SQL 区不可见（非联邦连接）时额外给一条面板通知，说明名字已进草稿。

**B3 归组拖拽与排序实现说明（2026-09-16）**

- 连换行（含引用行）可拖，载荷 `NavConnDragPayload { conn_id, name }`（与表 / 视图的载荷**不同类型**：落点只认自己的类型，拖到不相干的元素上自然什么都不发生）。
- 落点语义（`ConnDropTarget`）：
  - **分组头**（`Container`）：自定义组 = 加入并**保留**其它归属（多对多）；「未分组」头 = `remove_from_all_groups`（移出全部）；已在容器内且落组头→**不改位置**（避免“只是归组”把行拽到末尾）。
  - **连接行 / 引用行**（`BeforeRow`）：先归组，再插到该行**之前**（跨组拖 = 归组 + 定位一步到位）。
- 顺序计算是纯函数 `nav_reorder(ids, moving, before)`：拖到自己身上 / 已就位 → `None`（不写库）；目标行被筛选掉不在列表里 → 退化为追加。
- 落库**每次写整个容器的 `0..n`**（`set_member_order_all` / `set_ungrouped_order`），不做相对插入：序号不会出现空洞，也不依赖拖拽前的快照。写库前先 `reload_nav_org()` 拿**变更后**的成员表（否则刚加入的成员会缺席）。
- **「未分组」没有真实分组行**，成员是推导出来的（不属于任何分组），故顺序单开 `navigator_ungrouped_order`（迁移 021）；写库是**整体替换**（先清后写），避免连接重新回到未分组时“复活”旧位置。
- 渲染顺序：手动排序在前，未排过的按名称升序（`container_order` / `render_nav_tree` 同源）；排序落库时用**未筛选**的全量成员，避免被搜索过滤掉的行丢位置。
- 「未分组」头在**已有自定义分组时也渲染**（即使为空）：它是「拖拽移出分组」的常驻落点；右键菜单也能移出，两条路都在。
- 未做：见架构 §11#20（缓存首版限制）与 §11#21（C8 接线）；组内「未排按名称」已落到存储侧（§11#22）。

**Phase B8 已实现范围**

| 能力 | 实现 |
| --- | --- |
| 缓存管理（占用 / 逐条与全部清理） | `crates/workbench/src/components/cache_dialog.rs`；入口：设置面板「数据源导航 → 缓存管理…」+ 导航面板头 `⋯` →「缓存管理…」；**仅此处**删除 `conn_{id}.sqlite` |
| 来源短码 ⇄ 文字开关 | `settings::model::Navigator::source_short_code`；设置面板「数据源导航 → 来源标识（短码/文字）」；`render_connection_row` 消费；切换后 `refresh_windows()` 即时生效 |
| 属性面板宽度记忆 | `settings::model::Navigator::property_panel_width`（默认 24.5 rem）；`EditorPanel::render` 用 `h_resizable` 包裹，拖拽实时更新，**关闭面板时**写 `settings.json` |
| 刷新全部元数据 | 导航面板头 `⋯` →「刷新全部元数据」（`refresh_all`，不删磁盘缓存） |
| 新建数据源入口（2026-09-12 补齐） | 导航面板头 `＋` + 空态「还没有数据源」引导按钮（对齐原型 §2.1 / §2.4）；`Shared::new_connection_request`（`panels/` 置位 + `SidebarEvent::NewConnectionRequest`，`view.rs` 级联通知，`EditorPanel::render` 消费并 `request_new_connection`） |
| 面板头工具栏补齐（2026-09-12） | 对齐原型 §2.1 `[＋][🗂＋][⟳][断开][⋯]`：`⟳ 刷新元数据` = 刷新当前选中连接（§4.2「单连接 = 工具栏 ⟳」，全部在 `⋯`）；`断开当前连接` = 断开当前选中连接（仅运行时已连接时可用，缓存保留）；作用目标由 `SidebarPanel::nav_current_connection`（选中节点为连接根）推导，未选中 / 未连接时 `disabled` |

**B8 说明**

- 属性面板宽度在关闭面板时持久化（拖拽过程不写盘，避免每帧 I/O）；窗口直接退出而未关闭面板时，保留上次持久值。
- 缓存列表按当前可见连接采集；`P_`/`GP_` 缓存落项目 `meta/connection_metadata/`、`G_` 落系统 `global_metadata/`，与 `MetadataCacheManager` 路径规则一致。

**Phase C 实现进度**

| 任务 | 状态 | 落点 |
| --- | --- | --- |
| C4 大 schema 客户端分页（「加载更多」） | ✅ 已实现（2026-09-12） | `crates/workbench/src/panels/`（`render_more_row`、`folder_limit`、`page_limit`）+ `ui.rs::NAV_FOLDER_PAGE_SIZE`（200/页） |
| C6 收敛遗留：移除死代码 | ✅ 部分（2026-09-12） | `database/src/nav_view.rs::render_connection_list` 已删（无调用点）；`db_navigator.rs` **保留**（仍被 Mock 面板与 `mock_generator` 消费，属 M5/M6 范围） |
| C7 快捷键：Ctrl+F 聚焦搜索 + 导航树键盘操作 | ✅ 已实现（2026-09-12） | `workbench::commands::{FocusNavSearch, NavUp, NavDown, NavExpand, NavCollapse, NavOpenProperties}` + `app/main.rs` 绑定（`database-nav` context）+ `database/src/nav_view.rs::{nav_move, nav_expand, nav_collapse, nav_open_properties, nav_order}`（选中高亮 + 行点击聚焦面板） |
| C1 预热方案 C（仅 catalogs/schemas）+ 进度 + 取消 | ✅ 已实现（2026-09-12） | `crates/database/src/navigator_service.rs::warm_schemas`；`crates/workbench/src/services/nav_jobs.rs`（后台任务）；面板头显示「预热 d/t + 取消」 |
| C2 邻接节点预加载（列） | ✅ 已实现（2026-09-12） | `navigator_service::prefetch_columns` + `nav_jobs::prefetch_columns`；`database/src/nav_view.rs::ensure_nav_loaded` 在「表」文件夹首次加载后排队前 20 张表（`nav_jobs::PREFETCH_BATCH`） |
| 导航后台任务基建（阻塞 → 工作线程） | ✅ 已实现（2026-09-12） | `services/nav_jobs.rs`：单工作线程 + tokio 运行时 + mpsc 串行队列；原子量进度/取消；面板用主线程 async 任务 300ms 轮询重绘 |
| C-收尾 导航加载全部迁后台（消除 render 期 I/O） | ✅ 已实现（2026-09-12） | `nav_jobs::{enqueue_load, enqueue_properties, drain_load_results, drain_props_results}`；`panels/`：树加载/属性加载改为入队 + 主线程轮询回填（`apply_load_results` / `apply_props_results`），render 只读内存；本地 SQLite 一次性读取（分组/标签、展开态）改用 `cx.defer_in` 在渲染后执行 |
| C3 增量刷新接入 | ✅ 已实现（首版，2026-09-12） | `crates/database/src/cache.rs`（新增 `NavCache` cache-aside）+ `navigator_service.rs`（`with_context(project_root, fresh)`）；范围：schema / 表 / 视图 / 列；刷新（`fresh`）先 `prune_schema` 再重写；**修复 engine 既有缺陷** `list_columns_normalized` 引用了不存在的 `fkc.table_id` |
| C5 产品语义 token（含 `search.match.background`）+ 命中高亮 | ✅ 已实现（2026-09-12） | `crates/workbench_shell/src/product_tokens.rs`（`ProductTokens: Global`、`get`、`apply_from_str` / `apply_from_path`；缺失角色回退最接近的标准字段；2026-09-16 自 settings 迁入壳层）+ `assets/themes/product-tokens.json`（明暗各 7 角色：`activity_bar.*` / `title_bar.slot.background` / `quick_open.group.header` / `search.match.background`）；`crates/app/src/main.rs::attach_product_tokens`（启动 + `ThemeRegistry::watch_dir` 热更新）；消费方 `view.rs`（活动栏背景+激活条、标题栏挖空槽、Quick Open 分组头）、`database/src/nav_view.rs::nav_name_highlight`（命中底色：连接行 + 对象行）；单测 `product_tokens::tests::parses_roles_and_tolerates_missing` |
| C8 元数据缓存键切身份指纹 | ⚙️ engine 侧部分落地 | `engine::persistence::metadata_identity`（纯函数 + 测试，已提交 `fd1ffdb`）**尚未接线**（由连接侧任务推进） |

**C3 首版说明（已知限制）**

- 缓存命中判据为「非空即有数据」，空对象列表（如某 schema 无视图）会回落实时内省；
- 刷新（`fresh`）会先删该 schema 的缓存行再重写（外键级联删表 / 列），兄弟文件夹缓存随之失效 → 下次展开回落实时（可接受）；
- **列级删除不会剪枝**（`save_column` 为 upsert，无 per-table 清空 API）；删除列后直到「缓存管理 → 清理」或整 schema 刷新才会消失；
- 每次 `NavCache::open` 都新建一条缓存 SQLite 连接并跑幂等迁移；后续可改为每服务实例复用一份句柄。

**C7 说明**

- `Ctrl+F` 聚焦搜索（宿主 action）；`↑↓` 移动选中、`→` 展开、`←` 折叠、`Enter` / `F4` 打开属性；
- 可见序列由渲染顺序每帧重建（`nav_order`），避免与树的过滤 / 分组 / 分页逻辑重复实现；
- 仅当焦点在导航面板内时生效（点击行会聚焦面板）；搜索框获得焦点时 `↑↓` 仍由输入框处理优先（未消费才冒泡）。

**连接池运行时生命周期（修复，2026-09-13）**

- 现象：真实端点（MySQL/PostgreSQL）在导航里连不上 / 连了却加载不出对象树；SQLite/DuckDB 正常。
- 根因：`nav_runtime::{connect_entry, disconnect_entry, is_connected, load_entry_with}` 每次调用都 `Runtime::new()` 再丢弃；sqlx / 原生驱动连接池建在该运行时上，运行时销毁后池的后台任务死亡，池**永久不可用**（取用挂起）。`app::init_global_system` 早已用 `OnceLock` 常驻运行时（并有注释），`nav_runtime` 是漏网特例。
- 修复：`nav_runtime` 引入进程级 `BRIDGE_RUNTIME: OnceLock<Runtime>`，四个入口统一改用它。
- 实测证据：建池后 drop 运行时 → 新运行时上查询恒超时；改为进程级共享运行时 → 即时返回 `PostgreSQL 18.6`。
- 排查提示：该问题是**环境无关的确定性 bug**；偶发的 LAN 建连慢（池 `acquire_timeout` 默认 30s）会与其症状叠加，勿混为一谈。
- 鲁棒性加固（2026-09-13）：`ConnectionService::connect_with_type` 增加「可配建连超时 + 失败重试一次」；未配置 SSL 档案且目标为 LAN / 本机时，对 sqlx 驱动（`mysql` / `postgres`）显式关 TLS。设置项：`connection_defaults.{connect_timeout_ms, lan_disable_tls}`（设置面板「连接默认值」可改）。单测：`connection_service::tests::{lan_host_detection_covers_private_and_loopback, lan_tls_default_only_touches_sqlx_direct_lan}`。
- 展开即建连 + 行内操作收敛（2026-09-13）：连接根展开前先 `ensure_connected_for_browse`（未连则先建连），避免 `MetadataService` 取不到运行时句柄而冒泡 `CONN_NOT_FOUND`；行尾 hover 仅 `+` / `✎`，**连接 / 断开仅右键菜单**；标签改为显示在**名称下一行**（不在名称行内，字号取最初版 `text_xs`）。
- 展开态恢复防护（2026-09-13）：`render_connection_row` 仅在**运行时已连接**时才排后台加载——展开态跨重启恢复但运行时连接不跨重启，否则启动即报 `CONN_NOT_FOUND`；未连接时改显提示「未连接 · 右键「连接」或再次展开」。实测复现：未连接直接 `load_children("G_real_mysql", Connection)` → `[CONN_NOT_FOUND]`；`connect_entry` 后再加载 → 正常返回 catalogs/schemas。
- MySQL 内省空修复（2026-09-13）：`driver/native/mysql.rs::mysql_rows_to_arrow` 把 VARCHAR/TEXT 列误判为 Arrow `Binary`（`Vec<u8>` 探测先于 `String`），导致 `StringArray` 下转全失败、catalog/schema/table/column 全空。修复：先按声明类型名识别文本族；TEXT 与真 BLOB 在协议层同名 `BLOB`，改用「字节可否 UTF-8 解码」区分，并在 Utf8 建数组时回退 lossy 解码。实测全深度：MySQL `mall_business → 表 7 → order → 16 列`。
- 入口职责拆分（2026-09-13）：行尾 `+` **只做标签**（`tag_editor_for` → `render_tag_editor`）；归组改为右键「**移动到分组…**」（`group_picker_for` → `render_group_editor`，仅多选组 + 新建分组）。切换连接时重建标签输入并重新预填（避免把 A 的标签写到 B）。
- **树层级按数据库类型动态渲染（2026-09-13）**：旧实现里 MySQL 的 `get_schemas` 回退为 catalog 列表，导致 `catalog(db) → schema(同名 db) → 表` 出现同名重复层。新增驱动能力位 `MetadataBrowser::has_schema_level()`（默认 `true`）；MySQL / SQLite / DuckDB 为 `false`，PostgreSQL 保留 `true`。`MetadataService::has_schema_level` 透出，`navigator_service::load_children` 在 `NavPath::Catalog` 分支判定：有 Schema 层走 `load_schemas`，否则直接 `load_folders(conn_id, catalog, catalog)`。`NavPath` / `NavNode` / 渲染层不分叉；无 Schema 层驱动的 `get_schemas` 改为返回空（**不再回退 catalog 列表**）。实测（真实端点）：MySQL `mall_business → 表 (7) → order → 16 列`；PG `postgres → public → 表 (12)`；SQLite `main → 表 (25)`；DuckDB `main → 表 (8)`。
- **文件型库同文件多 id 别名（2026-09-13）**：旧实现 `ConnectionService::connect_with_type` 发现同 URL 已有文件型连接时**直接返回旧连接 id**（避免文件锁重复打开），导致 `connect_entry("P_real_sqlite")` 返回 Ok 但管理器里只有 `G_real_sqlite`，随后按请求 id 加载报 `[CONN_NOT_FOUND]`。修复：命中同 URL 时把同一 `Arc<dyn Database>` **再挂到请求的 conn_id**（别名，改写 `ConnectionInfo` 的 id / 作用域 / 名称；重连配置沿用权威连接），返回请求 id；文件只打开一次，断开只摘该 id 映射，最后一个引用释放才真正关连。实测：先连 `G_real_sqlite` / `G_real_duckdb` 再连 `P_real_sqlite` / `P_real_duckdb`，四条 id 均可加载 `main → 表 (25) / 表 (8)`。
- **PostgreSQL 只列当前库（2026-09-13）**：一条 PG 连接只绑定一个数据库，`information_schema` 仅暴露当前库；旧 `get_catalogs` 用 `pg_database` 列出服务器全部库，非当前库展开恒空（`get_schemas(catalog)` 0 行 → 回退 `main` → 表空）。修复：`get_catalogs` 改为 `SELECT current_database()::text`（sqlx 与 native 两驱动），消除兄弟库假节点。实测：`G_real_pg` / `P_real_pg` 均只列 `postgres → pg_toast, public`。跨库浏览（展开时另开一条连接）留待后续。
- **右键模块入口：通用项 + 表 / 视图专属（2026-09-14）**：菜单底部固定**通用项**（分隔线之后、与节点类型 / 连接状态无关）——「在 SQL 编辑器中打开」（选中该节点所属连接 + 清空导航 / 结果残留 + 聚焦编辑区）、「查看洞察」（右 Dock `RightPanel::Insight` 展开）；**所有节点都有**（连接 / Catalog / Schema / 文件夹 / 表 / 视图 / 列 / 例程）。**「生成 Mock 数据」仅表 / 视图**（`RightPanel::Mock` 展开），夹在两个通用项之间；连接菜单不含该项。经 `SidebarEvent::{OpenSqlEditor, OpenRightPanel}` 由 `WorkbenchView` 订阅处理（宿主是布局状态的唯一权威）。
- **序列 / 触发器解蔽 + 例程源码接入 + 属性面板覆盖扩展（2026-09-14）**：
  - `MetadataService::{list_sequences,list_triggers}` 在浏览器层返回空时回退 `Database::list_*`——此前 `MetadataBrowser::{get_sequences,get_triggers}`（四驱动均未实现，trait 默认空）遮蔽了 PG 的真实实现，「序列」文件夹永不出现；实测 PG `public` 序列 0 → 12 条。
  - `get_routine_source` 接入属性面板「源码」分区（先按存储过程取、未命中再按函数取）；顺带修复 MySQL / MySQL(native) `SHOW CREATE` **取列 1（sql_mode）而非 DDL**：改为按列名 `Create …` 定位。
  - `PropertyKind` 新增 `Routine` / `Sequence` / `Trigger`，`load_objects` 对所有类别对象挂 `property`（此前仅表 / 视图有 → 例程 / 序列 / 触发器无「查看属性」）。
  - 限定名去重：无独立 Schema 层的驱动把 schema 传成 catalog，`db.db.name` 折为 `db.name`（`property_panel::qualify` + `panels::nav_qualified_name`）。
- **导航侧「测试连接」与「生成 SQL ▸」（2026-09-14）**：
  - 测试连接：新增 `DataSourceService::test_saved`（回读记录 → 组装 `DataSourceSaveInput`（密文列解密）→ 走既有 `test`，与对话框同源）与 `nav_runtime::test_entry`；结果由导航后台任务回传，落面板提示（成功含版本 / 耗时）。
  - 生成 SQL：新增 `database::sql_gen`（纯函数 `dml_template` / `qualified_name` + 7 个单测）与 `nav_jobs::{enqueue_generate_dml, enqueue_test_connection}`；列由后台任务取（导航同一套 cache-aside，命中 L2 不发查询），生成后注入编辑区、**不自动执行**；`UPDATE` / `DELETE` 的 `WHERE` 取主键，无主键退化为 `WHERE 1 = 0` 并附注释。
- **连接右键：复制模板 / 共享 / 取消共享 / 删除（2026-09-14）**：
  - `DataSourceService::{duplicate_as_template, share_to_project}` + 作用域同名检查 `ensure_name_available_scoped`（全局 + 当前项目，大小写不敏感——`save` 原先只拦全局侧）；UI 侧新增行内「复制为模板」输入器（`nav_copy_input`）与导航侧删除二次确认（`AlertDialog`）。
  - **踩坑**：`connection::url::build_connection_url` 会**解密并内联密码**（供真实连接用），模板直接用它会导致「无密码」失效（保存时又从 URL 解析回密码）——改为先清空 `password_encrypted` 再组装 URL。
  - 「已共享」判定按**来源全局 id** 匹配（`id_prefix::source_global_id`）而非当天快照 id：`to_snapshot_id` 带日期，跳天再共享会得到不同 id → 会漏判。
  - 实测（真实数据，跑完完全回滚）：复制 `G_real_pg` → `G_conn_zz_tmp_src_pg`（用户名保留、密码为空）、同名拦截 ✓；共享 → `GP_conn_zz_tmp_src_pg_20260914`（含凭据密文）、重复共享拦截 ✓；取消共享后 `G_` 保留且可再次共享 ✓；删除副本 ✓；全局连接数回到基线 4 条。

**导航加载迁后台（收尾）说明**

- **render 现在的 I/O 为零**：树子节点、对象属性、预热、预取全部进工作线程；结果通过结果队列回传，面板用主线程 async 任务（60ms）轮询回填并 `notify`。
- 本地 SQLite（分组 / 成员 / 标签、各连接展开态）的一次性读取改用 `cx.defer_in`，在渲染的效果周期后执行（不再在 render 内读盘）；未就绪时树区域显示「加载中…」。
- 展开 / 刷新改为**先入队 + 显示占位**（`DatabaseNavView::loading`），不再阻塞首帧。
- 已知限制：工作线程串行——大预取（`PREFETCH_BATCH`）可能延迟用户展开的响应；后续可做优先级队列或双队列。
- 不在本模块范围、仍为同步的 render 期 I/O：`M5 草稿箱`（`load_scratchpad`）与 `M3 连接`（`connect_entry` 在事件路径 `block_on`，非 render）。

- 内省驱动依赖 tokio，不能在 UI 线程上跑；`services/nav_jobs.rs` 用**单工作线程 + tokio 运行时 + mpsc 串行队列**执行，面板只提交任务；
- 预热进度为原子量，面板用主线程 async 任务 300ms 轮询重绘（连续两次非活动才结束，避开入队→启动的竞态）；「取消」在 catalog 之间生效；
- C2 仅在「表」文件夹首次加载后排队一次（每连接重新连接时重置），最多 20 张（`PREFETCH_BATCH`）；命中 L2 的表不再内省；
- 已知限制：面板自身的展开加载仍是同步 `block_on`（仅预热/预取进了后台）；将全部加载迁后台属后续收尾。

## 1. 现状结论（盘点摘要）

> 下表为 Phase A 前的盘点快照； M4 已于 Phase A–C 落地（进度见上方表格）。

| 层 | 状态 |
| --- | --- |
| M3 数据源（`DataSourceService` list/get/save/update/delete/test、连接对话框、作用域 G_/P_/GP_） | ✅ 已实现 |
| M4 实时内省（`database::MetadataService`：catalog/schema/table/column/index/constraint/routine/trigger/sequence） | ✅ 已实现 |
| engine 元数据缓存（`MetadataCacheManager` / `MetadataCacheOps`：L1/L2、增量同步、FTS、分块、同步状态、`CacheVersionManager`） | ✅ 已实现 |
| 运行时连接（`workbench::ConnectionService`：connect/close/switch/has/list） | ✅ 已实现（C3 收敛后 50KB：传输/协议层已下沉 `crates/connection`，仅剩依赖 engine 的会话编排） |
| M4 领域模型 / 视图 / 命令 / 属性面板 | ✅ 已实现（`crates/database/src/{model,property_panel}.rs` + `panels/editor.rs::EditorPanel::render_property_panel`） |
| 分组 / 标签 / 导航状态表 | ✅ 已实现（`connection_groups` / `connection_group_members` / `connection_tags` / `navigator_state`） |
| 导航视图与装配 | ✅ 已实现（`database/src/nav_view.rs::{render_database_nav, render_nav_tree, render_connection_row, render_nav_node}`；原 `render_connection_list` / `render_navigation_placeholder` 占位已删） |

**核心缺口**：导航领域模型、导航编排服务、视图面板、分组/标签/状态表、运行时连接接入、属性面板注册表。

## 2. 阶段划分

### Phase A — 骨架与核心闭环（目标：真实对象树 + 连接/断开）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| A1 | 领域模型：`NavNode` / `NavNodeKind`（Connection/Schema/Folder/Table/View/Column/Routine/Trigger/Sequence）/ `NavSource`（P/G/GP）/ `NavState` | `crates/database/src/model.rs` | 编译通过；节点类型覆盖既有 `SchemaObjectKind` |
| A2 | 迁移：新增表 `navigator_state`、`connection_tags`（global）；`connection_groups`、`connection_group_members`、`connection_tags`、`navigator_state`（project） | `crates/engine/migrations/global/018_*.sql`、`crates/engine/migrations/project_meta/017_*.sql` | 迁移幂等；表结构见原型设计 §2.2 / §6.4 |
| A3 | 导航服务：三级缓存读取（L1→L2→L3）+ 懒加载 + 按来源过滤 + 刷新粒度 | `crates/database/src/navigator_service.rs`（新增） | 展开节点返回真实 schema/表/列；命中 L2 时不走网络 |
| A4 | 视图面板：面板头 + 来源标签页 + 搜索 + 树 + 底部状态；Dock 装配 | `crates/workbench/src/components/database_nav_panel.rs`（新增）、`crates/workbench/src/panels/` | `LeftPanel::Database` 渲染真实面板；标签切换与展开正常 |
| A5 | 连接 / 断开：接入 `ConnectionService`；`close_connection` 去掉删缓存；状态点四态 | `crates/workbench/src/services/connection_service.rs`、导航面板 | 双击连接可连接/断开；状态点正确；断开后 L2 仍在 |
| A6 | 依赖声明：`workbench → database` | `crates/workbench/Cargo.toml` | 无依赖环 |
| A7 | 验证：`cargo check` + 手动走通 §3 场景 1–4 | 全仓 | 编译零告警；真实连接可用 |

### Phase B — 分组 / 标签 / 搜索 / 属性面板 / 状态

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| B1 | 分组服务：CRUD + 多对多成员 + 排序（手动优先，未排按名称）| ✅ `crates/engine/src/persistence/connection_org_store.rs`（连接域共用，2026-09-11 上提；2026-09-16 补 `set_member_order_all` / `list_ungrouped_order` / `set_ungrouped_order`） | 一连接可属多组；排序持久化 |
| B2 | 标签服务：多值增删改 + 按标签检索 | ✅ 同上 + `connection_tags`（权威检索表；M3 保存同步 / 删除清理） | `tag:x` 检索命中 |
| B3 | 分组/标签视图：拖拽归组、右键「分组 ▸ / 标签 ▸」、分组对话框（名称/描述） | `database_nav_panel.rs` + `Dialog` | ✅ 归组拖拽 + 连接排序 + 分组排序（拖拽 / 右键）+ 分组表单（名称/描述）均走通；分组头统一配色 |
| B4 | 搜索：本地筛选 + 索引名称搜索（结果落**树顶结果区**）+ 高亮 | `navigator_service.rs` + `nav_jobs.rs` + `nav_view.rs` | ✅ 已实现（2026-09-18）：本地过滤命中已加载节点；跨连接索引搜索（中缀、≥ 2 字、无缓存不建文件）命中树顶结果区，单击开属性面板。**内容档（注释 / 数据类型）在 Quick Open 的 `#` 档**（2026-09-19），不在本面板；「结果落编辑区」未采（当年原型设想，已由树顶结果区 + Quick Open 取代） |
| B5 | 属性面板：类型注册表（connection/table/view/column/index/constraint/routine/…）+ 右侧停靠面板（属性/数据 Tab） | `crates/database/src/property_panel.rs` + workbench 编辑区右侧面板 | 双击/右键打开；字段随类型变化；宽度记忆 |
| B6 | 状态持久化：`navigator_state` 读写 + 800ms 防抖；分组展开态 | `navigator_service.rs` + engine `persistence` | 重启恢复展开/选中/过滤 |
| B7 | 上下文菜单动作：查看数据、复制名/限定名、生成 SELECT/INSERT/UPDATE/DELETE → 编辑器；表 / 视图**拖拽**插入限定名 | `database_nav_panel.rs` + `crates/workbench/src/commands.rs` | 生成 SQL 落到编辑器；拖拽落 SQL 区插光标处、落其它位置追加 |
| B8 | 缓存管理入口（设置 + 面板头「更多」）+ 短码⇄文字开关 | `crates/settings` + 导航面板 | 查看占用 / 显式清理；开关生效并持久化 |
| B9 | **契约面补齐（M3↔M4 审计，2026-09-12）**：① 导航行 / 右键**删除入口**（调同一 `workspace_loader::delete_connection(conn_id, project_root)`，删除后清 `DatabaseNavView` 缓存与状态）；② **标签 / 分组视图接线**（消费 `nav_runtime::{list_tags,set_tags,list_groups,create_group,rename_group,delete_group,*_member}`，权威源为 `connection_tags` / `connection_group_members`）；③ `NavSource::from_conn_id` 改依赖 `engine::persistence::id_prefix`（废弃自实现前缀推导）；④ 行点击同步 `shared.selected` | `crates/workbench/src/panels/`、`crates/database/src/model.rs` | 导航行可删除（作用域路由正确）；标签 / 分组可读可改且与对话框一致；遗留 `conn-` ID 归库与 M3 一致（审计详表见 `connection-dialog-architecture.md` §16） |

### Phase C — 预热 / 增量 / 收尾

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| C1 | 预热方案 C（仅 databases/schemas）+ 进度 + 取消 | `navigator_service.rs`（`build_metadata_index` / `is_syncing` / `get_sync_status` / `cancel_sync`） | 首连后台预热；进度可见可取消 |
| C2 | 邻接节点预加载 | `navigator_service.rs` | 展开表后相邻预取，失败静默 |
| C3 | 增量刷新接入（`detect_all_changes` / `incremental_sync` + 快照） | `navigator_service.rs` | 二次刷新只落变更 |
| C4 | 大 schema 分页 + 虚拟列表 | `navigator_service.rs` + `nav_view.rs`（视图已下沉到 `crates/database`） | ⏳ **分页已接**（2026-09-18）：`get_objects_chunk` 索引分块 + `offset` 追加 + 「加载更多」；**虚拟列表未做**（今天靠分页限制条数，不是虚拟滚动） |
| C5 | 主题 token：注册 `search.match.background`；明暗核对 | `assets/themes/rds-theme.json`、`app` | 两套主题对比度达标 |
| C6 | 收敛遗留：移除 `panels/` 导航占位；`db_navigator.rs` 移出本模块 | `crates/workbench` | 无死代码残留 |
| C7 | 快捷键与无障碍（↑↓/→←/Enter/F4/Ctrl+F） | `crates/workbench/src/commands.rs`、导航面板 | 键位走通 |
| C8 | **元数据缓存键切身份指纹**（规则已冻结，见 `connection-dialog-architecture.md` §3.6）：L2 路径 `conn_{id}.sqlite` → `meta_{fp}.sqlite`（`engine::persistence::metadata_identity`）；新增 `metadata_cache_index`（`canonical_desc` 可读描述 / `ref_conn_ids` 引用计数 / `last_used_at` / `size_bytes`）；同指纹并发预热互斥（进程内 + WAL / `busy_timeout`）；旧 `conn_*.sqlite` 按 legacy 保留（copy 不 move） | `crates/engine/src/persistence/{metadata_identity.rs,metadata_cache.rs,metadata_cache_pool.rs}`、`crates/workbench/src/services/connection_service.rs` | 改名 / 改密 / 换驱动实现后命中同一份 L2；同库两条连接不重复预热；孤儿缓存（引用为 0）可见且不自动删 |

## 3. 测试场景清单

1. 新建连接 → 出现在对应来源标签 → 展开 schema → 表 → 列（真实 PG/MySQL/SQLite/DuckDB）
2. 来源过滤：项目标签含 `P`/`GP`；全局标签含 `G`
3. 连接 / 断开：状态点四态切换；断开后 L2 缓存文件仍在（可离线浏览）
4. 三级缓存：首次走 L3 写入 L2；重开面板命中 L2（<100ms）
5. 分组：新建 → 拖入连接 → 同一连接出现在两个分组 → 折叠/计数/排序（手动优先，未排按名称）
6. 标签：连接打多个标签 → 搜索 `tag:x` 命中
7. 搜索：本地筛选 + 索引名称搜索（树顶结果区）+ 高亮 + 单击开属性面板；内容档在 Quick Open `#` 档（≥ 3 字）
8. 属性面板：双击表/列/索引/约束 → 字段正确；属性/数据 Tab 切换；宽度记忆
9. 状态持久化：展开/选中/过滤 → 重启恢复；`navigator_state` 分区（project/global）正确
10. 刷新：单表 / 单 schema / 单连接 / 全部；增量只落变更
11. 预热：首连后台预热 schema、进度显示、可取消
12. 缓存管理：查看占用 → 显式清理单连接 / 全量；断开/删除不触发删除
13. 明暗主题切换：面板各区域 token 正确（对照原型）
14. 依赖红线：`cargo check --workspace` 零告警；Feature 间无 view 依赖、无环

## 4. 风险与对策

| 风险 | 对策 |
| --- | --- |
| 缓存只增不删导致磁盘膨胀 | 「缓存管理」显示占用 + 孤儿缓存提示（**不自动删**）；`CacheVersionManager` 治理 |
| M:N 分组 + 多标签查询性能 | `connection_group_members(group_id, connection_id)` 与 `connection_tags(connection_id, tag)` 建唯一索引 |
| 大 schema（10 万+ 表）渲染卡顿 | `get_tables_chunk` 分页 + 虚拟列表；列内联仅在 ≤50 列时展开 |
| 连接/内省阻塞 UI | 后台任务 + 进度/取消；复用 `get_sync_status`；L2 命中先渲染 |
| 遗留 `ConnectionService` 职责重叠 | 传输/协议层已按 connection-dev-plan C3 全部下沉 `crates/connection`（url / url_params / chain）；服务内仅剩依赖 engine 的会话编排（50KB），职责边界清晰 |
| L2 缓存陈旧 | TTL 标记（非删除）+ 手动刷新 + 增量 diff |
| 分组/标签越界到其他项目 | 分组存项目库；标签随连接所在库（project/global）分区 |
| 标签双源（JSON 字段 vs `connection_tags`） | `connection_tags` 为权威检索表（M3 保存/更新同步、删除清理）；连接记录 `tags` JSON 仅作兼容投影 |

## 5. 与 v1 对齐表

| v1 设计 / 实现 | v2 现状 | 本方案 |
| --- | --- | --- |
| `database-navigator.vue` + 虚拟树 + 右键菜单 | GPUI 占位 | 按原型重写 |
| `useGroupManager`（localStorage 分组） | 无 | 分组/标签落库（新增表） |
| `use-cache-warming.ts`（智能预热） | engine 有索引/队列能力 | 采用方案 C（仅 databases/schemas） |
| `06-CACHE-OPTIMIZATION.md`（三层缓存 / 增量同步 / 版本迁移） | engine 已迁移 | 导航服务编排接入 |
| `database-navigator-store.ts`（refresh/clearCache/disconnect） | `db_navigator.rs` 只读分析库 | 移出；新服务接入连接 + 刷新 |
| `properties-registry.ts` + `properties-panel-dbeaver.html` | 无 | 属性面板注册表 + 右侧停靠面板 |
| localStorage 导航状态 | 无 | SQLite `navigator_state` |

## 6. 验证方式

- 每阶段：`cargo check -p rds-workbench -p rds-app -p rds-database --all-targets` 零告警
- 服务层集成测试：`crates/database/tests/`（缓存读取、分页、搜索、分组/标签 CRUD）、`crates/workbench/tests/`（连接/断开、面板装配）
- UI：`cargo run -p rds-app` 手动走通 §3 场景
- 主题：明暗切换核对 token（`theme-preview.html` 为基准）
- 架构红线：Feature 间无对方 view 依赖、无依赖环

## 7. 实现位置映射（设计决策 → 代码文件）

| 设计决策 | 代码文件 |
| --- | --- |
| 导航领域模型（NavNode/NavSource/NavState） | `crates/database/src/model.rs` |
| 导航编排服务（缓存/刷新/预热/搜索/分页） | `crates/database/src/navigator_service.rs` |
| 分组与标签服务（M:N + 多值；连接域共用） | ✅ `crates/engine/src/persistence/connection_org_store.rs`（2026-09-11 上提；原计划 `crates/database/src/group.rs` 取消） |
| 实时内省 | `crates/database/src/metadata_service.rs`（已有） |
| 属性面板注册表 | `crates/database/src/property_panel.rs` |
| 导航视图面板（标签页/分组/树/搜索） | `crates/workbench/src/components/database_nav_panel.rs` |
| 左 Dock 装配 | `crates/workbench/src/panels/` |
| 属性面板（编辑区右侧） | `crates/workbench`（编辑区分栏 + `crates/database` 数据） |
| 连接 / 断开 | `crates/workbench/src/services/connection_service.rs` |
| 数据源 CRUD + 标签读取 | `crates/workbench/src/services/data_source_service.rs` |
| 标签读写入口（导航侧） | `crates/workbench/src/services/nav_runtime.rs`（`list_tags`/`set_tags`） |
| 新增迁移（global 018 / project_meta 017） | `crates/engine/migrations/{global,project_meta}/` |
| 新表访问（navigator_state） | `crates/workbench/src/services/nav_store.rs`（导航视图状态私有） |
| 新表访问（groups / members / tags） | `crates/engine/src/persistence/connection_org_store.rs`（连接组织元数据，M3/M4 共用） |
| 协议链执行 / 隧道生命周期（`connection::chain::TunnelRegistry`，M3 C3） | `crates/connection/src/chain.rs` |
| 缓存占用 / 清理 | engine `MetadataCacheManager::{size,delete}`（仅设置入口调用） |
| 主题 token `search.match.background` | `assets/themes/rds-theme.json` |
| 视图开关（短码⇄文字、面板宽度） | `crates/settings` |
