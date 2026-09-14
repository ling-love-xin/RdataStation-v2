# 数据源管理 / 数据库导航模块（M4）· 模块入口

> **一句话**：把「有哪些数据源、它们里面有什么」这件事做成**一眼可读的树**——连接按项目/全局/共享三分，用分组给结构、用标签给检索，展开即自动建连，逐级懒加载对象树，双击开靠右属性面板。
>
> 本文只提炼**特点 / 边界 / 硬约束 / 地图**；细节一律指向下方文档（本目录内），**不复制设计**。
> 状态：Phase A/B/C + v6/v7 主线完成（2026-09-14）。已知缺口与排期**唯一权威**是架构文档 §11。
>
> **边界**：本模块只管理**数据源与元数据**。连接的新建/编辑对话框属 M3 连接模块；SQL 编辑器 / Mock / 洞察只在本面板提供**入口**，本体属各自模块；DuckDB 分析表与分析资产归 M6。

## 1. 模块特点

### 产品行为

| 特点 | 含义 | 出处 |
| --- | --- | --- |
| **一实例一项目 + 三归属域** | 启动即绑定当前项目，不存在「未打开项目」；连接按存储域分 `P` 项目 / `G` 全局 / `GP` 共享，行内**右对齐成列**（可关） | 架构 §4.1、原型 §2.1 |
| **分组 = 结构，标签 = 检索** | 分组是项目级自定义结构且**多对多**：只在**主组**全亮，其它组以 `∈ 主组名` 引用行出现（点击跳回）；标签多值、**不进树**，只做筛选 / 搜索 | 架构 §2.5、§4.3、原型 §2.2 |
| **行内密度预算 3 元素** | 常驻仅「徽标 + 名称 + 归属域列」；`+` 与行操作**仅 hover / 选中显**，不占常驻宽度 | 架构 §2.3、§2.4、原型 §2.1 |
| **双通道徽标** | **颜色 = 状态**（灰 = 不可用 / 彩 = 可用）、**形状 + 2 字母 = 类型**；hover 卡（300ms）补类型 / 状态 / 驱动名 | 架构 §2.4、§4.4、原型 §2.3 |
| **层级随数据库类型动态** | 驱动声明是否有独立 Schema 层：MySQL / SQLite / DuckDB 无（Catalog 直接挂类别文件夹），PostgreSQL 有（`Catalog(当前库) → Schema`） | 架构 §4.2、原型 §3 |
| **展开即建连** | 展开连接根若未建连，**先自动建连**再加载；未连接时不排加载，改显提示，避免 `CONN_NOT_FOUND` | 架构 §5.2、dev-plan §0 |
| **模块入口：通用项 + 表 / 视图专属** | 右键底部**通用项**（所有节点都有，与类型 / 连接状态无关）：在 SQL 编辑器中打开 · 查看洞察；**生成 Mock 数据仅表 / 视图** | 原型 §6.2、手册 §4.8 |
| **属性面板靠右停靠** | 双击对象 / 连接或 `F4`；填充编辑区**右侧**，分隔条可拖拽、**宽度记忆** | 原型 §7、手册 §4.7 |
| **大 schema 不卡** | 类别文件夹首批 200 条 + 行尾「加载更多」；表文件夹首次加载后后台预取前 20 张表的列 | 架构 §8、dev-plan §0（C2/C4） |

### 数据与缓存

| 特点 | 含义 | 出处 |
| --- | --- | --- |
| **元数据缓存分级** | **L1 内存** + **L2 每连接 SQLite**，**cache-aside**（命中即渲染，未命中 L3 实时内省并回写；L3 不是缓存） | 架构 §5.1 |
| **缓存只增不删** | 刷新只重建该范围，**不删**缓存；唯一删除入口是 `⋯ → 缓存管理…`（与 v1 教训一致） | 架构 §6、手册 §7 |
| **组织元数据独立表** | 分组 / 成员（含 `is_primary`）/ 标签落 `connection_groups` / `connection_group_members` / `connection_tags`，按存储域分区（项目库 / 全局库） | 架构 §4.3 |
| **视图状态分区** | 展开 / 选中 / 过滤存 `navigator_state`：项目 / 共享连接 → `project.db`，全局连接 → `global.db` | 架构 §4.5 |
| **同文件多连接共享物理连接** | 文件型库（SQLite / DuckDB）同 URL 的多条逻辑连接**别名**到同一 `Arc<dyn Database>`：文件只打开一次，两个 id 均可查到 | dev-plan §0（v8） |
| **驱动目录一次性加载** | 徽标形状与驱动显示名来自 `drivers` 表，`defer_in` 一次性加载后经 `Shared` 共享（render 无 I/O） | 架构 §4.4 |

### 架构与约束

| 特点 | 含义 | 出处 |
| --- | --- | --- |
| **render 期零 I/O（核心纪律）** | 树加载 / 属性 / 预热 / 预取全部进**后台工作线程 + 结果队列**，主线程轮询回填；本地小读（分组 / 标签 / 展开态）走 `cx.defer_in` | 架构 §3.2 |
| **依赖只向下** | 视图只依赖 `gpui-kit`；M4 领域与编排在 `crates/database`，GPUI 视图在 `crates/workbench`（不反向依赖） | 架构 §3.1 |
| **层级差异只在服务层** | 类型差异落在驱动能力位 + `navigator_service`；`NavPath` / `NavNode` / 渲染层不为数据库类型分叉 | 架构 §4.2 |
| **零裸色值、零裸 px** | 颜色一律主题 token（含产品语义角色 `search.match.background`）；结构尺寸走 `ui.rs` 常量，`px(` 会被契约测试拦下 | 架构 §3 |
| **降级不静默** | L2 不可用时降级实时内省；节点级失败带**可读原因** + 重试入口，不静默吞错 | 架构 §7 |

### 工程与文档

| 特点 | 含义 | 出处 |
| --- | --- | --- |
| **纯函数优先** | 可测判定拍成纯函数：`parse_nav_search` / `nav_type_badge` / `nav_type_short_label` / `nav_qualified_name` / `NavNode::child_key` / `NavSource::{from_conn_id,from_key}` | 架构 §9 |
| **一段一职责** | 面板状态（`DatabaseNavView`）/ 后台任务（`nav_jobs`）/ 运行时入口（`nav_runtime`）/ 视图状态持久化（`nav_store`）各归其位 | 架构 §10 |
| **文档六件套** | README（本文）+ 架构 + 原型 + 交互稿 + 开发方案 + 使用手册 | `../README.md` |
| **真机反馈成文** | 每轮用真实端点（4 类数据库 × P/G/GP）回归，问题 → 结论 → 文档与测试同步 | dev-plan §0 |

## 2. 边界

- **做**：数据源的**组织视图**（分组 / 标签 / 归属域 facet / 筛选 / 搜索）、对象树**懒加载与缓存**、连接 / 断开、刷新、**属性面板**、常驻模块入口。
- **不做**：连接**新建 / 编辑对话框**本体（M3，见 `../connection/README.md`）、SQL 编辑器 / Mock / 洞察**本体**、DuckDB **分析资产**（M6）。

## 3. 代码地图（要改什么去哪）

| 想改 | 去哪 |
| --- | --- |
| 面板布局 / 连接行 / 树渲染 / 右键菜单 | `crates/workbench/src/panels.rs`（`SidebarPanel::{render_database_nav, render_nav_tree, render_connection_row, render_nav_node}`） |
| 属性面板渲染 | `crates/workbench/src/panels.rs::EditorPanel::render_property_panel` + `crates/database/src/property_panel.rs` |
| 左 / 右 Dock 装配、面板事件订阅 | `crates/workbench/src/view.rs`（`LeftPanel` / `RightPanel` / `init_workspace`） |
| 后台加载 / 预热 / 预取队列 | `crates/workbench/src/services/nav_jobs.rs` |
| 连接 / 断开 / 标签 / 分组入口 | `crates/workbench/src/services/nav_runtime.rs` |
| 展开态 / 选中 / 过滤持久化 | `crates/workbench/src/services/nav_store.rs` |
| 树层级 / 懒加载 / 刷新粒度 | `crates/database/src/navigator_service.rs` |
| 树节点模型 / 来源短码 / 路径 | `crates/database/src/model.rs` |
| L2 缓存读写 | `crates/database/src/cache.rs` |
| 内省调用（catalog/schema/table/column…） | `crates/database/src/metadata_service.rs` |
| 各数据库内省实现 / 能力位 | `crates/engine/src/driver/native/{mysql,postgres,sqlite,duckdb}.rs`（`MetadataBrowser`） |
| 分组 / 标签表与读写 | `crates/engine/src/persistence/connection_org_store.rs` |
| 尺寸常量 | `crates/workbench/src/ui.rs` |

数据链路：`SidebarPanel（导航面板）→ nav_jobs（后台工作线程）→ NavigatorService → MetadataService → 驱动 MetadataBrowser → 目标数据库`；组织数据（分组 / 标签）走 `ConnectionOrgStore → project.db / global.db`。**无 HTTP / IPC 层**。

## 4. 改这个模块前必须遵守

1. `render` 期**零 I/O**：新增加载一律走 `nav_jobs` 入队 + 结果回填；禁止在 render / 点击回调里 `block_on` 远程库。
2. 视图只依赖 `gpui-kit`；颜色用 `theme.colors.*` / 产品语义 token，尺寸进 `ui.rs`（裸 `px(` 会被 `ui_contract` 拦下）。
3. **类型差异只加在服务层与驱动能力位**（如 `has_schema_level`），不要给 `NavNode` / 渲染层加数据库类型分支。
4. 新增 UI 数据项先回答「来自哪张表 / 哪个服务方法」；**零 UI 造数据**（驱动目录、分组、标签都来自库）。
5. **缓存只增不删**：断开 / 刷新 / 删除连接都不得删除缓存文件；清理只经显式「缓存管理」。
6. 展开态跨重启保留、运行时连接**不**保留：加载前必须确认运行时已连接，否则只给提示。
7. 注释与文档用简体中文，说明意图与取舍（不复述代码）。
8. `cargo` 命令固定 `-j 2`（DuckDB 静态库并发链接会 OOM）。

## 5. 测试与验证

```sh
# 模块回归（含导航模型 / 缓存单测）
cargo test -p rds-engine -p rds-database -p rds-workbench --lib -j 2

# 尺寸 / 颜色契约（扫描 panels.rs / view.rs 等）
cargo test -p rds-workbench --test ui_contract -j 2

# 全工作区编译守卫（含全部 target）
cargo check --workspace --all-targets -j 2
```

- 基准（2026-09-14 实测）：`rds-engine --lib` 249 / `rds-database --lib` 6 / `rds-workbench --lib` 52 全绿；`check --workspace --all-targets -j 2` 零错误。
- 导航模型单测在 `crates/database/src/{model.rs,cache.rs}`（来源短码、`child_key`、schema 缓存往返）。
- **真机回归**（4 类数据库 × 项目 / 全局 / 共享，均按树的实际层级核对）：

| 数据库 | 树形 |
| --- | --- |
| MySQL | `mall_business → 表 (7) / 存储过程·函数 (4) → order → 16 列` |
| PostgreSQL | `postgres → public → 表 (12) / 视图 (1) → inventory_ledger → 7 列` |
| SQLite | `main → 表 (25) → attachment → 8 列` |
| DuckDB | `main → 表 (8) → cities → 列` |

- 真机验收清单见 `database-navigator-user-guide.md` §9。

## 6. 文档地图

| 文档 | 什么时候读它 |
| --- | --- |
| `database-navigator-architecture.md` | **为什么这样设计 / 怎么运转**：心智模型与边界规则 / 概念定位表 / 分层与 render 零 I/O / 五条数据流 / 决策取舍 / 降级容错 / **§11 已知问题与后续项（权威）** |
| `database-navigator-prototype-design.md` | **长什么样**：面板布局 / 连接行解剖（v6/v7 降密）/ 双通道徽标 / 树模型 / 三级元数据管线 / **§6.2 右键菜单** / 主题映射 / GPUI 落点 |
| `database-navigator-user-guide.md` | **怎么用**：入口 / 界面导览（连接行怎么读 · 状态色 · 类型形状 · 层级）/ 典型流程 / 快捷键 / 显示开关 / FAQ / 验收清单 |
| `database-nav-dev-plan.md` | **做到哪了**：Phase A/B/C 与 v6/v7 任务逐项状态、逐轮实现记录与踩坑、迁移与表、测试场景、风险 |
| `database-navigator-prototype.html` | 可交互示意稿（明暗双主题；密度对比） |
| `../connection/README.md` | 上游：连接的新建 / 编辑与作用域路由（M3） |

## 7. 下一步（摘要，权威见架构 §11）

| 类别 | 项 |
| --- | --- |
| 模块内可做 | 系统库 / 系统 schema 过滤（「自定义显示数据库 / Schema」架构落地）；大 schema 列内联展开阈值 |
| 需协调 | PostgreSQL 跨库浏览（展开兄弟库时另开一条连接）；元数据缓存键切身份指纹（C8）接线 |
| 依赖其他模块 | Mock（M7）/ 洞察（M8）面板实装；SQL 可执行区脱离 `use_duckdb_fed` 限制 |
| 待拍板 | 组内手动排序的交互细节（决策已定）；标签命名规范 `key:value` |
