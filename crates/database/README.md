# rds-database

数据源管理 / 数据库导航（M4）的**服务层** crate。它回答「连上后怎么浏览、怎么检索、怎么取元信息」——导航树、三级缓存、属性面板取数、表数据预览都在这里；树的 UI 渲染在 `rds-workbench` 的 `panels/nav.rs`，连接的新建 / 编辑 / 测试在 M3（`rds-connection` + workbench `connection_service`）。

- 分类：Feature crate（M4 数据源管理 / 导航）
- 上游消费方：`rds-workbench`（导航面板、属性面板、Quick Open `ShowProperties`、拖拽落点）
- 依赖：内部依赖 `engine`（驱动内省 / L2 持久化 / `ConnectionOrgStore`）、`shared`（`NavSource` 等模型）、`property`（`rds-property` 描述模型 / 数据容器）、`workbench_shell` + `gpui-kit`（UI 基础设施）；不依赖 `app`
- 测试：crate 内 98 项单测 + `crates/workbench/tests/nav_property_real.rs` 真机全链路（SQLite / DuckDB，2026-10-03）

---

## 职责与边界

**本 crate 管：**
- 导航树：连接 → Catalog →（Schema 层随驱动有无）→ 类别文件夹（带计数）→ 对象 → 列；懒加载 + 分页（`load_children_page`，首批 200 + 加载更多）
- 元数据缓存：**L1 内存 + L2 每连接 SQLite，cache-aside**（命中即渲染，未命中实时内省回写；刷新只重建不删除）
- 连接生命周期衔接：展开即建连（未连接自动建连）；连接 / 断开状态与徽标数据
- 分组 / 标签服务接口：`ConnectionOrgStore`（`engine::persistence`）的读写编排（归组 / 排序 / 打标签 / 主组）
- 搜索：面板名称档（树内过滤）与 `SearchIndex` 后台任务（跨连接索引）
- 属性面板取数：`load_properties`（实时内省 + 属性 L2 缓存）+ provider 分派（`property_providers::global_registry()` 按驱动 type_id 选源；通用骨架取数 + 各驱动 provider 组装）
- 表数据预览：`table_data` 分页抓取（排序 + 主键补齐 / `limit+1` 探测 / 真 COUNT / NULL 置灰）
- 后台任务队列：`nav_jobs` 单工作线程（render 期零 I/O 的纪律底座）

**本 crate 不管（也不要管）：**
- 不建连接——新建 / 编辑 / 测试 / 保存是 M3 职责；这里只消费 `ConnectionManager` 连接池
- 不执行 SQL——编辑 / 执行 / 结果集是 `editor` 模块；导航只投递「打开查询」请求（双击表 = `SELECT * … LIMIT 200`）
- 不渲染树 / 面板 UI——视图在 `rds-workbench` 的 `panels/nav.rs`（`editor.rs` 属性面板）；本 crate 只出状态与数据
- 不编辑元数据——改名 / 建索引 / 授权不在本模块
- 不做权限 / 依赖内省——驱动能力未实现，属性面板对应页签灰置（`rds-property` 注册表 `Unsupported`）
- 不含全文检索引擎——内容档走 Quick Open `#`（`scratchpad` 全文索引）；面板搜索框只有名称档

## 公开接口

```text
pub mod navigator_service;  // 导航服务：load_children / load_children_page(分页) / object_position
                            //   / warm_schemas(预热) / prefetch_columns(表列预取) / load_properties 入口
pub mod metadata_service;   // 内省取数：list_catalogs / has_schema_level / list_schemas / list_tables
                            //   / list_columns / list_indexes / list_constraints / list_procedures
                            //   / list_functions / list_sequences / list_triggers / get_routine_source
                            //   / get_table_ddl / execute_query（数据预览复用）
pub mod nav_jobs;           // 后台任务队列：LoadChildren / LoadProperties / SearchIndex / Warm
                            //   / PrefetchColumns / GenerateDml / TestConnection
pub mod model;              // NavPath / NavNode / NavSource / PropertyRef / PropertyRequest(含 data)
pub mod nav_view;           // 视图状态（展开 / 选中 / 过滤 / 分页游标）与行交互载荷
pub mod property_panel;     // load_properties（属性 + 数据同一次往返）+ provider 分派骨架
pub mod property_providers; // 各驱动 provider 注册（pg/sqlite/duckdb/mysql）
pub mod table_data;         // 数据预览分页核心：TableDataRequest / quote_ident / load_table_data

pub use model::{NavPath, NavSource};
```

## 关键设计判断

- **三级缓存 cache-aside（L1 内存 / L2 连接级 SQLite / L3 实时内省）**：命中即渲染、未命中取数回写；刷新只重建该范围**不删其他缓存**（唯一删除入口「缓存管理…」）。新增数据源写入连接表即启动缓存——首次展开走 L3 回写，之后命中 L2 不再打库。
- **展开即建连**：连接根未建连先自动建连再加载，未连接时不排加载、改显提示，避免 `CONN_NOT_FOUND` 噪音。
- **render 期零 I/O**：所有取数走 `nav_jobs` 队列；`render` 只读状态、`Shared` 更新 + `cx.notify()` 走事件路径（既有欠债见 `docs/architecture/layout/panels-modules.md` §4，新代码不得再犯）。
- **属性面板抽独立 crate（`rds-property`）**：描述驱动 + 能力灰置；数据预览复用同一链路（`table_data` 只读分页，编辑回写待 `table_editor` 能力位）。注：**编辑器结果集的就地写回已于 2026-10-12 在 `crates/editor` 侧落地（Phase 2a，双击脏格 → 提交变更，独立事务逐条 UPDATE），不经 `table_editor` 能力位**；该能力位仍为 `NotBuilt`，语义待定，本条说的是 M4 属性面板/表数据预览这条独立链路，两者不要混为一谈（详见 `docs/architecture/editor/editor-architecture.md` §6 D23、§13）。
- **数据预览从 Arrow `batches` 提取行**（`rows_as_json`）：native 驱动 `query` 只填 `batches`、`rows` 字段恒空（引擎已知坑）；文本单元格经 `serde_json::Value::String` 取内部值（避免 JSON 序列化自带引号）。
- **分组 / 标签分轨**：分组是结构（多对多 + 主组），标签是检索（多值、不进树）；归组 / 排序整容器写 `0..n`。

## 接线关系

- **workbench → database**：`panels/nav.rs` 消费 `nav_view` 状态 + `navigator_service`（经 `nav_jobs` 入队）；`panels/property_panel.rs`（`PropertyHostPanel`）经 `Shared` 桥请求属性；Quick Open `ShowProperties` 构造 `PropertyRequest`（`data: None`，数据页懒加载）。
- **database → property（rds-property）**：`load_properties` 是数据生产者（通用骨架取数 + provider 分派），provider（`property_providers/`）实现 `PropertyProvider` trait；渲染方（workbench）只消费 `ObjectProperties`，三方互不依赖视图。
- **database → engine**：驱动内省走 `MetadataBrowser`；L2 缓存与分组 / 标签落库走 `engine::persistence`；`engine` 不回依赖本 crate。

## 测试

- crate 内单测：`nav_view` 状态机 / 分页 / 失败重试、`property_ref_of` 映射、`table_data` 转义与分页探测、`property_panel` 骨架 / provider（含 PG roles/tablespaces 候选与编辑 SQL）。
- 真机全链路（`crates/workbench/tests/nav_property_real.rs`）：SQLite 3.53.2 / DuckDB v1.5.5 各跑同一参数化链路——测试连接 → 真实连接 → Catalog / 类别文件夹（带计数）/ 表 / 视图 / 列 → 属性面板 → 数据页（分页 / 排序 / NULL）→ 注册表灰置 → 二次加载 cache-aside。结果见 `docs/architecture/database/database-navigator-test-report-2026-10-03.md`。
- 已知缺口：MySQL / PG 无端点（本环境）；大 schema（>500 对象）真机未做；GUI 像素级冒烟未做。

## 文档入口

- 设计文档不复制在本 crate——统一在 `docs/architecture/database/`（README 入口 / 架构 / 规格书 / 原型 / 手册 / 开发方案 / 测试报告），与本文件是同一套事实的不同讲法。
