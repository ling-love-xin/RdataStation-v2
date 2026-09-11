# 数据源管理 / 数据库导航模块 · 开发方案（Phase A/B/C）

> 状态：**Phase A 已实现（2026-09-11）** · `cargo check --workspace --all-targets` 零错误；`cargo test -p rds-database` 全绿 · 关联文件：`database-navigator-prototype-design.md`（原型设计）、`database-navigator-prototype.html`（可交互原型）
> 设计基线：作用域来源短码 `P/G/GP`、项目级自定义分组（多对多）+ 多值标签、三级缓存与增量刷新、缓存永不自动删除、属性面板填充编辑区右侧、预热方案 C。
> 技术栈：GPUI（gpui-kit 0.6）；M4 领域/服务在 `crates/database`（非 UI），视图在 `crates/workbench`。
> 前置：M3 连接模块 Phase A/B 已实现；engine 元数据缓存（含增量/预热索引/FTS/分页/版本迁移）已迁移。

## 0. 进度

| 阶段 | 状态 |
| --- | --- |
| Phase A（骨架与核心闭环） | ✅ 已实现 |
| Phase B（分组/标签/搜索/属性面板/状态） | 🟡 进行中（B5 属性面板已实现） |
| Phase C（预热/增量/收尾） | ⬜ 待办 |

**Phase A 实现位置**

| 交付 | 文件 |
| --- | --- |
| 导航领域模型（NavNode/NavSource/NavScope/NavFolder/NavPath/NavState/ConnectionEntry/分组/标签） | `crates/database/src/model.rs` |
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
- 连接 URL 未做密码百分号编码——Phase C 统一处理；
- 只渲染数据源与其对象树，**不含分析资产**（符合范围边界）。

**Phase B 实现进度**

| 任务 | 状态 | 落点 |
| --- | --- | --- |
| B5 属性面板（类型信息 + 属性网格 + 子实体表格） | ✅ | `crates/database/src/property_panel.rs`、`crates/database/src/navigator_service.rs`（`load_properties`） |
| B5 属性面板视图（编辑区右侧停靠，双击对象/连接打开） | ✅ | `crates/workbench/src/panels.rs`（`EditorPanel::render_property_panel`、`Shared::property_target`） |
| B4 搜索（本地筛选：连接/对象名，命中自动展开） | ✅ | `crates/workbench/src/panels.rs`（`InputState` + `nav_node_matches`） |
| B6 状态持久化（展开态，`navigator_state`） | ✅ | `crates/workbench/src/services/nav_store.rs`、`nav_runtime.rs`、`panels.rs` |
| B1 分组服务（CRUD + 多对多 + 排序） | ⬜ | 表已建（迁移 017） |
| B2 标签服务（多值 + 检索） | 🟡 存储层就绪 | `nav_store.rs`（`list_tags`/`set_tags`） |
| B3 分组/标签视图（拖拽/右键/对话框） | ⬜ | — |
| B7 上下文菜单动作（生成 SQL / 复制名 / 查看数据） | ⬜ | — |
| B8 缓存管理入口 + 短码⇄文字开关 | ⬜ | — |

**Phase B 已知限制**

- 属性面板为**堆叠分区**（列/索引/约束），暂未做子实体 Tab 切换；
- 双击节点打开属性（gpui `click_count >= 2`）；右键菜单、复制/生成 SQL 待 B7；
- 属性加载为阻塞式（与 Phase A 同），后续随缓存编排迁后台；
- 属性面板宽度固定 392px（拖拽与记忆待 B8）。

## 1. 现状结论（盘点摘要）

| 层 | 状态 |
| --- | --- |
| M3 数据源（`DataSourceService` list/get/save/update/delete/test、连接对话框、作用域 G_/P_/GP_） | ✅ 已实现 |
| M4 实时内省（`database::MetadataService`：catalog/schema/table/column/index/constraint/routine/trigger/sequence） | ✅ 已实现 |
| engine 元数据缓存（`MetadataCacheManager` / `MetadataCacheOps`：L1/L2、增量同步、FTS、分块、同步状态、`CacheVersionManager`） | ✅ 已实现 |
| 运行时连接（`workbench::ConnectionService`：connect/close/switch/has/list） | ✅ 已实现（遗留 85KB，Phase C 收敛） |
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
| B1 | 分组服务：CRUD + 多对多成员 + 排序（手动优先，未排按名称）| `crates/database/src/group.rs`（新增） | 一连接可属多组；排序持久化 |
| B2 | 标签服务：多值增删改 + 按标签检索 | 同上 + `connection_tags` | `tag:x` 检索命中 |
| B3 | 分组/标签视图：拖拽归组、右键「分组 ▸ / 标签 ▸」、分组对话框（名称/描述） | `database_nav_panel.rs` + `Dialog` | 拖拽与对话框走通；分组头统一配色 |
| B4 | 搜索：本地筛选 + FTS（`search_fts`）+ 结果落编辑区 + 高亮 | `navigator_service.rs` + `database_nav_panel.rs` + `crates/workbench/panels.rs` | 300ms 防抖；命中高亮；Enter 打开 |
| B5 | 属性面板：类型注册表（connection/table/view/column/index/constraint/routine/…）+ 右侧停靠面板（属性/数据 Tab） | `crates/database/src/property_panel.rs` + workbench 编辑区右侧面板 | 双击/右键打开；字段随类型变化；宽度记忆 |
| B6 | 状态持久化：`navigator_state` 读写 + 800ms 防抖；分组展开态 | `navigator_service.rs` + engine `persistence` | 重启恢复展开/选中/过滤 |
| B7 | 上下文菜单动作：查看数据、复制名/限定名、生成 SELECT/INSERT/UPDATE/DELETE → 编辑器 | `database_nav_panel.rs` + `crates/workbench/src/commands.rs` | 生成 SQL 落到编辑器 |
| B8 | 缓存管理入口（设置 + 面板头「更多」）+ 短码⇄文字开关 | `crates/settings` + 导航面板 | 查看占用 / 显式清理；开关生效并持久化 |

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
| 遗留 `ConnectionService`（85KB）职责重叠 | Phase A 只接线，Phase C6 按 connection-dev-plan C3 收敛 |
| L2 缓存陈旧 | TTL 标记（非删除）+ 手动刷新 + 增量 diff |
| 分组/标签越界到其他项目 | 分组存项目库；标签随连接所在库（project/global）分区 |

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
| 分组与标签服务（M:N + 多值） | `crates/database/src/group.rs` |
| 实时内省 | `crates/database/src/metadata_service.rs`（已有） |
| 属性面板注册表 | `crates/database/src/property_panel.rs` |
| 导航视图面板（标签页/分组/树/搜索） | `crates/workbench/src/components/database_nav_panel.rs` |
| 左 Dock 装配 | `crates/workbench/src/panels.rs` |
| 属性面板（编辑区右侧） | `crates/workbench`（编辑区分栏 + `crates/database` 数据） |
| 连接 / 断开 | `crates/workbench/src/services/connection_service.rs` |
| 数据源 CRUD + 标签读取 | `crates/workbench/src/services/data_source_service.rs` |
| 新增迁移（global 018 / project_meta 017） | `crates/engine/migrations/{global,project_meta}/` |
| 新表访问（navigator_state / groups / members / tags） | `crates/engine/src/persistence/`（新增 store） |
| 缓存占用 / 清理 | engine `MetadataCacheManager::{size,delete}`（仅设置入口调用） |
| 主题 token `search.match.background` | `assets/themes/rds-theme.json` |
| 视图开关（短码⇄文字、面板宽度） | `crates/settings` |
