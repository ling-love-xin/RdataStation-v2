# 数据源管理 / 数据库导航模块（M4）

> 左 Dock `LeftPanel::Database`：**统一管理数据源连接，并浏览其对象树元数据**。
> 本页是模块**入口与特点提炼**；规格细节见文末「文档索引」的四份文档。

## 一句话定位

在左侧「数据库导航」面板里维护数据源（新建 / 编辑 / 分组 / 标签 / 筛选），并逐级展开浏览对象树
（Catalog → [Schema] → 类别文件夹 → 表 / 视图 → 列），双击打开**靠右停靠的属性面板**。

**边界**：本模块**只管理数据源与其元数据**。DuckDB 分析表 / 分析资源归 M6；SQL 编辑器、Mock、
洞察只在本面板提供**入口**，本体属各自模块。

## 核心特点

| # | 特点 | 说明 |
| --- | --- | --- |
| 1 | **一实例一项目** | 启动即绑定当前项目，不存在「未打开项目」状态。连接按**归属域**三分：项目 `P` / 全局 `G` / 共享 `GP`（行内右对齐成列，可关）。 |
| 2 | **分组 = 结构，标签 = 检索** | 分组是项目级自定义结构：连接可属**多个**分组，只在**主组**全亮呈现，其它组以 `∈ 主组名` 引用行出现（点击跳回）。标签多值，**不进树**，只做筛选 / 搜索。 |
| 3 | **行内密度预算** | 常驻仅 **3 元素**（徽标 + 名称 + 归属域列）；`+` 与行操作仅 hover / 选中显。徽标**双通道**：**颜色 = 状态，形状 + 2 字母 = 类型**；驱动 id 不进行（只在 tooltip / 属性面板）。 |
| 4 | **层级随数据库类型动态** | 驱动声明 `MetadataBrowser::has_schema_level()`：MySQL / SQLite / DuckDB **无 Schema 层**（Catalog 直接挂类别文件夹），PostgreSQL 保留 `Catalog(当前库) → Schema`。差异只在服务层决定，`NavPath` / `NavNode` / 渲染层不分叉。 |
| 5 | **连接生命周期自动** | 展开连接根**自动建连**；断开始终**保留缓存**；同一文件型库的多条逻辑连接**共享一个物理连接**（别名，两个 id 均可查到，文件只打开一次）。 |
| 6 | **元数据二级缓存、永不自动删** | L1 内存 + L2 每连接 SQLite，cache-aside；刷新只重建该范围。**唯一删除入口**是 `⋯ → 缓存管理…`。 |
| 7 | **render 期零 I/O** | 树加载 / 属性 / 预热 / 预取全部进后台工作线程 + 结果队列，主线程轮询回填；本地小读（分组 / 标签 / 展开态）走 `cx.defer_in`。 |
| 8 | **facet 筛选 + 搜索语法** | 归属域 chips 常驻；类型 / 驱动 / 标签收敛进「**筛选 ▾ N**」（`N` = 已生效项数）。搜索支持 `scope: / source: / type: / driver: / tag:`，作为**额外 AND 约束**叠加，命中高亮。 |
| 9 | **常驻模块入口** | 连接右键底部固定三项（与连接状态无关）：在 SQL 编辑器中打开 · 生成 Mock 数据 · 查看洞察。 |
| 10 | **零裸色值、零裸 px** | 颜色一律取主题 token（含产品语义角色 `search.match.background` 等）；尺寸走 `ui.rs` 常量。 |

## 能力概览（当前实现）

- **树层级**：连接 → Catalog → [Schema] → 类别文件夹（表 / 视图 / 存储过程·函数 / 序列 / 触发器，**仅显示有对象的**，标题带计数）→ 表 / 视图 → 列（带类型 + `PK`/`FK`）。
- **大 schema 分页**：类别文件夹首批 `ui::NAV_FOLDER_PAGE_SIZE = 200` 条，行尾「加载更多」。
- **邻接预取**：表文件夹首次加载后，后台预取前 `nav_jobs::PREFETCH_BATCH = 20` 张表的列。
- **首连预热**：仅预热 databases / schemas（方案 C），带进度显示与取消。
- **刷新粒度**：单连接（面板头 `⟳`）/ 单节点（右键「刷新元数据」）/ 全部（`⋯`）。
- **属性面板**：双击对象 / 连接，或选中后 `F4`；靠右停靠、分隔条可拖拽、**宽度记忆**。
- **快捷键**：`Ctrl+F` 聚焦搜索 · `↑↓` 移动选中 · `→←` 展开 / 折叠 · `Enter` / `F4` 打开属性。
- **显示开关**：`⋯ → 显示标签`（名称下一行，≤2 chip + `+N`）/ `显示归属域`。

## 已知限制

| 限制 | 说明 |
| --- | --- |
| 系统库 / 系统 schema 不过滤 | `information_schema` / `mysql` / `sys`、`pg_toast` 照常列出；「自定义显示数据库 / Schema」留待后续架构设计。 |
| PostgreSQL 只列当前库 | 一条 PG 连接只绑定一个库，非当前库无法浏览，故不列出；跨库浏览（展开时另开一条连接）后续再做。 |
| Mock / 洞察为占位 | 入口已就绪，面板待 M7 / M8 实装。 |
| SQL 可执行区受限 | 中央编辑区目前仅连接启用 DuckDB 联邦（`use_duckdb_fed`）时显示可执行 SQL 区。 |
| 大 schema 列内联阈值 | 列内联展开阈值待定（规划：> 50 列转属性面板）。 |

## 代码落点

| 层 | 位置 |
| --- | --- |
| 视图 / 面板 | `crates/workbench/src/panels.rs`（`SidebarPanel::render_database_nav` / `render_nav_tree` / `render_connection_row` / `render_nav_node` / `EditorPanel::render_property_panel`）；`crates/workbench/src/view.rs`（`LeftPanel` / `RightPanel` 装配与事件订阅） |
| 后台任务 | `crates/workbench/src/services/nav_jobs.rs`（单工作线程 + 队列）；连接入口 `crates/workbench/src/services/nav_runtime.rs`（进程级 `BRIDGE_RUNTIME`） |
| 视图状态持久化 | `crates/workbench/src/services/nav_store.rs`（`navigator_state`） |
| 导航编排 / 树模型 | `crates/database/src/navigator_service.rs` · `model.rs` · `property_panel.rs` · `cache.rs` |
| 实时内省 | `crates/database/src/metadata_service.rs` + `crates/engine/src/driver/native/{mysql,postgres,sqlite,duckdb}.rs`（`MetadataBrowser`） |
| 组织数据（分组 / 标签） | `crates/engine/src/persistence/connection_org_store.rs`（项目库 / 全局库） |
| 尺寸常量 | `crates/workbench/src/ui.rs` |

## 验证基线

```sh
cargo check --workspace --all-targets -j 2
cargo test -p rds-engine -p rds-database -p rds-workbench --lib -j 2
```

真实端点回归（4 类数据库 × 项目 / 全局 / 共享）：

| 数据库 | 树形 |
| --- | --- |
| MySQL | `mall_business → 表 (7) → order → 16 列` |
| PostgreSQL | `postgres → public → 表 (12) → inventory_ledger → 7 列` |
| SQLite | `main → 表 (25) → attachment → 8 列` |
| DuckDB | `main → 表 (8) → cities → 列` |

## 文档索引

| 文档 | 作用 |
| --- | --- |
| `database-navigator-architecture.md` | **架构与设计理念**：定位与边界 / 心智模型与概念定位表 / 分层与 render 零 I/O / 五条数据流 / 决策取舍 / 降级容错 / 测试策略 / 实现映射 / 已知问题 |
| `database-navigator-prototype-design.md` | **原型设计**：面板布局 / 连接行解剖（v6/v7 降密）/ 双通道徽标 / 树模型 / 三级元数据管线 / 交互 / 主题映射 / GPUI 落点 |
| `database-navigator-prototype.html` | **可交互原型**：RDS Light/Dark 双主题（徽标 / 加载 / 筛选弹层 / 右键菜单 / 密度对比） |
| `database-nav-dev-plan.md` | **开发方案与逐轮记录**：Phase A/B/C + v6/v7 任务逐项状态 / 迁移与表 / 测试场景 / 风险 |
| `database-navigator-user-guide.md` | **使用手册**：入口 / 界面导览 / 典型流程 / 分组与标签分工 / 快捷键 / 显示开关 / FAQ / 验收清单 |
