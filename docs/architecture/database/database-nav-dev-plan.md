# 数据源管理 / 数据库导航模块 · 开发方案（Phase A/B/C）

> 状态：**Phase A/B 完成；Phase C 进行中（C1/C2/C3/C4/C6/C7 已实现）· 2026-09-12** · `cargo check --workspace --all-targets` 零错误；`cargo test -p rds-workbench --lib`（37）/ `-p rds-database`（5）/ `-p rds-settings`（1）/ `-p rds-engine --lib`（242）全绿 · 关联文件：`database-navigator-prototype-design.md`（原型设计）、`database-navigator-prototype.html`（可交互原型）
> 设计基线：作用域来源短码 `P/G/GP`、项目级自定义分组（多对多）+ 多值标签、三级缓存与增量刷新、缓存永不自动删除、属性面板填充编辑区右侧、预热方案 C。
> 技术栈：GPUI（gpui-kit 0.6）；M4 领域/服务在 `crates/database`（非 UI），视图在 `crates/workbench`。
> 前置：M3 连接模块 Phase A/B 已实现；engine 元数据缓存（含增量/预热索引/FTS/分页/版本迁移）已迁移。

## 0. 进度

| 阶段 | 状态 |
| --- | --- |
| Phase A（骨架与核心闭环） | ✅ 已实现 |
| Phase B（分组/标签/搜索/属性面板/状态/菜单/缓存） | ✅ 已实现（B1–B8） |
| Phase C（预热/增量/收尾） | 🟡 进行中（C1/C2/C3/C4/C6/C7 已实现；C5 待条件；C8 由连接侧推进） |

**Phase A 实现位置**

| 交付 | 文件 |
| --- | --- |
| 导航领域模型（NavNode/NavSource/NavFolder/NavPath/NavState/ConnectionEntry/分组/标签） | `crates/database/src/model.rs` |
| 导航编排服务（三级缓存预留、懒加载、来源过滤） | `crates/database/src/navigator_service.rs` |
| 迁移：`navigator_state` / `connection_tags`（global） | `crates/engine/migrations/global/018_add_navigator_state.sql` |
| 迁移：`navigator_state` / `connection_tags` / `connection_groups` / `connection_group_members`（project） | `crates/engine/migrations/project_meta/017_add_navigator_groups_tags_state.sql` |
| 连接 / 断开运行时（保留缓存） | `crates/workbench/src/services/nav_runtime.rs` |
| 数据源导航面板（标签页 + 对象树懒加载 + 连接/断开） | `crates/workbench/src/panels.rs`（`render_database_nav` 等） |
| 断开不删缓存 | `crates/workbench/src/services/connection_service.rs`（`close_connection`） |
| 依赖声明 workbench → database | `Cargo.toml` / `crates/workbench/Cargo.toml` |

**Phase A 已知限制（后续阶段处理）**

- 加载为阻塞式（点击时 `block_on`），Phase C 迁移到后台任务；
- 未接 L2 每连接缓存（目前直连实时内省）——Phase C；
- 展开态未持久化到 `navigator_state`（表已建）——Phase B；
- 面板头按钮（新建连接 / 刷新）、搜索输入、右键菜单——Phase B；
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
| B5 属性面板视图（编辑区右侧停靠，双击对象/连接打开） | ✅ | `crates/workbench/src/panels.rs`（`EditorPanel::render_property_panel`、`Shared::property_target`） |
| B4 搜索（本地筛选：连接/对象名，命中自动展开） | ✅ | `crates/workbench/src/panels.rs`（`InputState` + `nav_node_matches`） |
| B6 状态持久化（展开态，`navigator_state`） | ✅ | `crates/workbench/src/services/nav_store.rs`、`nav_runtime.rs`、`panels.rs` |
| B1 分组服务（CRUD + 多对多 + 排序） | ✅ 服务层就绪（2026-09-11） | `engine::persistence::ConnectionOrgStore`：create/update/delete/list_groups、add/remove_member、set_member_order、list_group_members、list_groups_for_connection |
| B2 标签服务（多值 + 检索） | ✅ 服务层就绪（2026-09-11） | `ConnectionOrgStore`：set_tags/list_tags/list_connections_by_tag/list_all_tags；`nav_runtime::{list_tags,set_tags}` 已接线；M3 保存/更新同步、删除清理 |
| B3 分组/标签视图（拖拽/右键/对话框） | ✅ 已实现（2026-09-12） | `crates/workbench/src/panels.rs`：`render_nav_tree`（分组一级 + 「未分组」）、`render_group_header`（统一色条 + 计数 + 折叠）、`render_org_editor`（行内分组多选 + 标签输入 + 新建分组）、`nav_source_chip`（来源筛选 chips）、`ensure_nav_org` / `reload_nav_org` |
| B1/B2 视图接线（分组多对多 + 标签多值） | ✅ 已实现 | `nav_runtime::{list_groups,create_group,list_group_members,add_to_group,remove_from_group,list_all_tags}`；`ConnectionOrgStore::list_tag_pairs`（一次性映射） |
| B7 上下文菜单动作（查看数据 / 复制名 / 查看属性 / 刷新） | ✅ 已实现（2026-09-12） | `crates/workbench/src/panels.rs`：`ContextMenuExt::context_menu` 挂到连接行 / 对象节点 / 分组头；`Shared::editor_set` + `SidebarEvent::EditorSqlRequest`（生成 SELECT → 编辑区）；`toggle_connection` / `refresh_node` / `delete_group` / `create_group_interactive`；分组删除带 `AlertDialog` 确认 |
| B8 缓存管理入口 + 短码⇄文字开关 + 属性面板宽度记忆 | ✅ 已实现（2026-09-12） | `crates/settings/src/model.rs`（`Navigator` 分区）+ `settings_view.rs`（数据源导航节）；`crates/workbench/src/components/cache_dialog.rs`（两处入口）；`panels.rs::{refresh_all, render_connection_row, render_property_panel}` + `EditorPanel::render`（`h_resizable`） |

**Phase B 已知限制**

- 属性面板为**堆叠分区**（列/索引/约束），暂未做子实体 Tab 切换；
- 双击节点打开属性（gpui `click_count >= 2`）；右键菜单已实现（见下方 B7 范围）；复制 / 生成 SQL 已支持、INSERT/UPDATE/DELETE 待后续；
- 属性加载为阻塞式（与 Phase A 同），后续随缓存编排迁后台；
- 归组尚未支持**拖拽**与组内外手动排序，目前通过行内 `🗂` 编辑器的多选切换（B3 视图首版）；排序服务 `set_member_order` 已就绪；
- 新建分组用默认名「新建分组」（自动去重）；重命名已支持（分组头右键 → 行内输入），描述表单待后续；
- 「来源筛选 + 分组」状态尚未持久化到 `navigator_state`（仅展开态已持久化）；
- 搜索目前为**连接名 + 标签**子串匹配；`source:global` / `tag:prod` 结构化语法、以及对象树层命中高亮待 B7。

**Phase B7 上下文菜单已实现范围**

| 节点 | 菜单项 |
| --- | --- |
| 连接 | 连接 / 断开 · 编辑连接… · 查看属性 · 分组 / 标签… · 复制名称 · 刷新元数据 |
| 表 / 视图 | 查看属性 · 查看数据（`SELECT * … LIMIT 200` 注入编辑区） · 复制名称 · 复制限定名 · 刷新元数据 |
| 其他对象（列 / schema / 文件夹 / 例程） | 查看属性 · 复制名称（*限定名仅在有 catalog/schema 时出现*） · 刷新元数据 |
| 分组头 | 重命名分组（行内输入） · 新建分组 · 删除分组（`AlertDialog` 二次确认）；「未分组」仅「新建分组」 |

**B7 待办（后续阶段）**

- 生成 INSERT / UPDATE / DELETE、生成 Mock 数据、测试连接、复制连接（模板）、共享至项目 / 取消共享、删除连接；
- 拖拽表到编辑器插入限定名（需 `on_drag`）；「查看数据」目前仅注入 SQL，未自动执行（自动执行需抽出 `EditorPanel` 的执行管线）。

**Phase B8 已实现范围**

| 能力 | 实现 |
| --- | --- |
| 缓存管理（占用 / 逐条与全部清理） | `crates/workbench/src/components/cache_dialog.rs`；入口：设置面板「数据源导航 → 缓存管理…」+ 导航面板头 `⋯` →「缓存管理…」；**仅此处**删除 `conn_{id}.sqlite` |
| 来源短码 ⇄ 文字开关 | `settings::model::Navigator::source_short_code`；设置面板「数据源导航 → 来源标识（短码/文字）」；`render_connection_row` 消费；切换后 `refresh_windows()` 即时生效 |
| 属性面板宽度记忆 | `settings::model::Navigator::property_panel_width`（默认 24.5 rem）；`EditorPanel::render` 用 `h_resizable` 包裹，拖拽实时更新，**关闭面板时**写 `settings.json` |
| 刷新全部元数据 | 导航面板头 `⋯` →「刷新全部元数据」（`refresh_all`，不删磁盘缓存） |

**B8 说明**

- 属性面板宽度在关闭面板时持久化（拖拽过程不写盘，避免每帧 I/O）；窗口直接退出而未关闭面板时，保留上次持久值。
- 缓存列表按当前可见连接采集；`P_`/`GP_` 缓存落项目 `meta/connection_metadata/`、`G_` 落系统 `global_metadata/`，与 `MetadataCacheManager` 路径规则一致。

**Phase C 实现进度**

| 任务 | 状态 | 落点 |
| --- | --- | --- |
| C4 大 schema 客户端分页（「加载更多」） | ✅ 已实现（2026-09-12） | `crates/workbench/src/panels.rs`（`render_more_row`、`folder_limit`、`page_limit`）+ `ui.rs::NAV_FOLDER_PAGE_SIZE`（200/页） |
| C6 收敛遗留：移除死代码 | ✅ 部分（2026-09-12） | `panels.rs::render_connection_list` 已删（无调用点）；`db_navigator.rs` **保留**（仍被 Mock 面板与 `mock_generator` 消费，属 M5/M6 范围） |
| C7 快捷键：Ctrl+F 聚焦搜索 + 导航树键盘操作 | ✅ 已实现（2026-09-12） | `workbench::commands::{FocusNavSearch, NavUp, NavDown, NavExpand, NavCollapse, NavOpenProperties}` + `app/main.rs` 绑定（`database-nav` context）+ `panels.rs::{nav_move, nav_expand, nav_collapse, nav_open_properties, nav_order}`（选中高亮 + 行点击聚焦面板） |
| C1 预热方案 C（仅 catalogs/schemas）+ 进度 + 取消 | ✅ 已实现（2026-09-12） | `crates/database/src/navigator_service.rs::warm_schemas`；`crates/workbench/src/services/nav_jobs.rs`（后台任务）；面板头显示「预热 d/t + 取消」 |
| C2 邻接节点预加载（列） | ✅ 已实现（2026-09-12） | `navigator_service::prefetch_columns` + `nav_jobs::prefetch_columns`；`panels.rs::ensure_nav_loaded` 在「表」文件夹首次加载后排队前 20 张表（`nav_jobs::PREFETCH_BATCH`） |
| 导航后台任务基建（阻塞 → 工作线程） | ✅ 已实现（2026-09-12） | `services/nav_jobs.rs`：单工作线程 + tokio 运行时 + mpsc 串行队列；原子量进度/取消；面板用主线程 async 任务 300ms 轮询重绘 |
| C-收尾 导航加载全部迁后台（消除 render 期 I/O） | ✅ 已实现（2026-09-12） | `nav_jobs::{enqueue_load, enqueue_properties, drain_load_results, drain_props_results}`；`panels.rs`：树加载/属性加载改为入队 + 主线程轮询回填（`apply_load_results` / `apply_props_results`），render 只读内存；本地 SQLite 一次性读取（分组/标签、展开态）改用 `cx.defer_in` 在渲染后执行 |
| C3 增量刷新接入 | ✅ 已实现（首版，2026-09-12） | `crates/database/src/cache.rs`（新增 `NavCache` cache-aside）+ `navigator_service.rs`（`with_context(project_root, fresh)`）；范围：schema / 表 / 视图 / 列；刷新（`fresh`）先 `prune_schema` 再重写；**修复 engine 既有缺陷** `list_columns_normalized` 引用了不存在的 `fkc.table_id` |
| C5 `search.match.background` token | ⬜ 受阻 | gpui-kit 0.6.1 `ThemeColor` 无该字段，且仓库尚无产品语义 token 注册设施；命中高亮未实现，提前加 token 无消费方 |
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
| M4 领域模型 / 视图 / 命令 / 属性面板 | ⬜ 占位（`crates/database/src/{model,commands,database_view,property_panel}.rs`） |
| 分组 / 标签 / 导航状态表 | ⬜ 缺失（`connections.tags` 列存在，但需独立表） |
| 导航视图与装配 | ⬜ 占位（`panels.rs::render_connection_list` + `render_navigation_placeholder`） |

**核心缺口**：导航领域模型、导航编排服务、视图面板、分组/标签/状态表、运行时连接接入、属性面板注册表。

## 2. 阶段划分

### Phase A — 骨架与核心闭环（目标：真实对象树 + 连接/断开）

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| A1 | 领域模型：`NavNode` / `NavNodeKind`（Connection/Schema/Folder/Table/View/Column/Routine/Trigger/Sequence）/ `NavSource`（P/G/GP）/ `NavState` | `crates/database/src/model.rs` | 编译通过；节点类型覆盖既有 `SchemaObjectKind` |
| A2 | 迁移：新增表 `navigator_state`、`connection_tags`（global）；`connection_groups`、`connection_group_members`、`connection_tags`、`navigator_state`（project） | `crates/engine/migrations/global/018_*.sql`、`crates/engine/migrations/project_meta/017_*.sql` | 迁移幂等；表结构见原型设计 §2.2 / §6.4 |
| A3 | 导航服务：三级缓存读取（L1→L2→L3）+ 懒加载 + 按来源过滤 + 刷新粒度 | `crates/database/src/navigator_service.rs`（新增） | 展开节点返回真实 schema/表/列；命中 L2 时不走网络 |
| A4 | 视图面板：面板头 + 来源标签页 + 搜索 + 树 + 底部状态；Dock 装配 | `crates/workbench/src/components/database_nav_panel.rs`（新增）、`crates/workbench/src/panels.rs` | `LeftPanel::Database` 渲染真实面板；标签切换与展开正常 |
| A5 | 连接 / 断开：接入 `ConnectionService`；`close_connection` 去掉删缓存；状态点四态 | `crates/workbench/src/services/connection_service.rs`、导航面板 | 双击连接可连接/断开；状态点正确；断开后 L2 仍在 |
| A6 | 依赖声明：`workbench → database` | `crates/workbench/Cargo.toml` | 无依赖环 |
| A7 | 验证：`cargo check` + 手动走通 §3 场景 1–4 | 全仓 | 编译零告警；真实连接可用 |

### Phase B — 分组 / 标签 / 搜索 / 属性面板 / 状态

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| B1 | 分组服务：CRUD + 多对多成员 + 排序（手动优先，未排按名称）| ✅ `crates/engine/src/persistence/connection_org_store.rs`（连接域共用，2026-09-11 上提） | 一连接可属多组；排序持久化 |
| B2 | 标签服务：多值增删改 + 按标签检索 | ✅ 同上 + `connection_tags`（权威检索表；M3 保存同步 / 删除清理） | `tag:x` 检索命中 |
| B3 | 分组/标签视图：拖拽归组、右键「分组 ▸ / 标签 ▸」、分组对话框（名称/描述） | `database_nav_panel.rs` + `Dialog` | 拖拽与对话框走通；分组头统一配色 |
| B4 | 搜索：本地筛选 + FTS（`search_fts`）+ 结果落编辑区 + 高亮 | `navigator_service.rs` + `database_nav_panel.rs` + `crates/workbench/panels.rs` | 300ms 防抖；命中高亮；Enter 打开 |
| B5 | 属性面板：类型注册表（connection/table/view/column/index/constraint/routine/…）+ 右侧停靠面板（属性/数据 Tab） | `crates/database/src/property_panel.rs` + workbench 编辑区右侧面板 | 双击/右键打开；字段随类型变化；宽度记忆 |
| B6 | 状态持久化：`navigator_state` 读写 + 800ms 防抖；分组展开态 | `navigator_service.rs` + engine `persistence` | 重启恢复展开/选中/过滤 |
| B7 | 上下文菜单动作：查看数据、复制名/限定名、生成 SELECT/INSERT/UPDATE/DELETE → 编辑器 | `database_nav_panel.rs` + `crates/workbench/src/commands.rs` | 生成 SQL 落到编辑器 |
| B8 | 缓存管理入口（设置 + 面板头「更多」）+ 短码⇄文字开关 | `crates/settings` + 导航面板 | 查看占用 / 显式清理；开关生效并持久化 |
| B9 | **契约面补齐（M3↔M4 审计，2026-09-12）**：① 导航行 / 右键**删除入口**（调同一 `workspace_loader::delete_connection(conn_id, project_root)`，删除后清 `DatabaseNavView` 缓存与状态）；② **标签 / 分组视图接线**（消费 `nav_runtime::{list_tags,set_tags,list_groups,create_group,rename_group,delete_group,*_member}`，权威源为 `connection_tags` / `connection_group_members`）；③ `NavSource::from_conn_id` 改依赖 `engine::persistence::id_prefix`（废弃自实现前缀推导）；④ 行点击同步 `shared.selected` | `crates/workbench/src/panels.rs`、`crates/database/src/model.rs` | 导航行可删除（作用域路由正确）；标签 / 分组可读可改且与对话框一致；遗留 `conn-` ID 归库与 M3 一致（审计详表见 `connection-dialog-architecture.md` §16） |

### Phase C — 预热 / 增量 / 收尾

| # | 任务 | 落点 | 验收 |
| --- | --- | --- | --- |
| C1 | 预热方案 C（仅 databases/schemas）+ 进度 + 取消 | `navigator_service.rs`（`build_metadata_index` / `is_syncing` / `get_sync_status` / `cancel_sync`） | 首连后台预热；进度可见可取消 |
| C2 | 邻接节点预加载 | `navigator_service.rs` | 展开表后相邻预取，失败静默 |
| C3 | 增量刷新接入（`detect_all_changes` / `incremental_sync` + 快照） | `navigator_service.rs` | 二次刷新只落变更 |
| C4 | 大 schema 分页（`get_tables_chunk`）+ 虚拟列表 | `navigator_service.rs` + `database_nav_panel.rs` | 万级表不卡顿；「加载更多」 |
| C5 | 主题 token：注册 `search.match.background`；明暗核对 | `assets/themes/rds-theme.json`、`app` | 两套主题对比度达标 |
| C6 | 收敛遗留：移除 `panels.rs` 导航占位；`db_navigator.rs` 移出本模块 | `crates/workbench` | 无死代码残留 |
| C7 | 快捷键与无障碍（↑↓/→←/Enter/F4/Ctrl+F） | `crates/workbench/src/commands.rs`、导航面板 | 键位走通 |
| C8 | **元数据缓存键切身份指纹**（规则已冻结，见 `connection-dialog-architecture.md` §3.6）：L2 路径 `conn_{id}.sqlite` → `meta_{fp}.sqlite`（`engine::persistence::metadata_identity`）；新增 `metadata_cache_index`（`canonical_desc` 可读描述 / `ref_conn_ids` 引用计数 / `last_used_at` / `size_bytes`）；同指纹并发预热互斥（进程内 + WAL / `busy_timeout`）；旧 `conn_*.sqlite` 按 legacy 保留（copy 不 move） | `crates/engine/src/persistence/{metadata_identity.rs,metadata_cache.rs,metadata_cache_pool.rs}`、`crates/workbench/src/services/connection_service.rs` | 改名 / 改密 / 换驱动实现后命中同一份 L2；同库两条连接不重复预热；孤儿缓存（引用为 0）可见且不自动删 |

## 3. 测试场景清单

1. 新建连接 → 出现在对应来源标签 → 展开 schema → 表 → 列（真实 PG/MySQL/SQLite/DuckDB）
2. 来源过滤：项目标签含 `P`/`GP`；全局标签含 `G`
3. 连接 / 断开：状态点四态切换；断开后 L2 缓存文件仍在（可离线浏览）
4. 三级缓存：首次走 L3 写入 L2；重开面板命中 L2（<100ms）
5. 分组：新建 → 拖入连接 → 同一连接出现在两个分组 → 折叠/计数/排序（手动优先，未排按名称）
6. 标签：连接打多个标签 → 搜索 `tag:x` 命中
7. 搜索：本地筛选 + FTS + 高亮 + Enter 落编辑区
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
| 左 Dock 装配 | `crates/workbench/src/panels.rs` |
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
