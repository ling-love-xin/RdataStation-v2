# RdataStation v2・数据源管理 / 数据库导航模块（M4）完整规格书

> 对齐体例：`docs/architecture/connection/connection-module-spec.md`（M3 规格书）。
> 权威设计文档：`database-navigator-architecture.md`（理念 / 边界 / 数据流 / 决策取舍，本文件只作工程口径摘要，冲突以架构文档为准）。
> 验证：`database-navigator-test-report-2026-10-03.md`（真机全链路 + 回归基线）。

## 1. 模块总览

M4 数据源管理 / 数据库导航：把已建立的连接（M3 产出）变成**可浏览、可检索、可操作的对象树**，并提供对象属性与**表数据预览**。

### 1.1 模块边界（做什么 / 不做什么）

**做什么**

| 项 | 说明 |
| --- | --- |
| 导航树 | 连接 → Catalog →（Schema 层视驱动有无）→ 类别文件夹（表 / 视图 / 例程…，带计数）→ 对象 → 列；懒加载 + 分页（首批 200，行尾「加载更多」） |
| 连接生命周期 | 展开即建连（未连接自动建连）；连接 / 断开状态徽标；右键复制 / 共享 / 删除 |
| 组织能力 | 分组（项目级自定义结构，多对多，`is_primary` 主组）/ 标签（多值，不进树，只做筛选 / 搜索） |
| 检索 | 面板搜索（名称档）+ Quick Open `#` 全文档；跨连接搜索走 `SearchIndex` 后台任务 |
| 属性面板 | 双击 / `F4` 打开对象属性（DBeaver 式 tab，独立 crate `rds-property`，见 `../property/property-module-spec.md`） |
| 表数据预览 | 属性面板「数据」页签：分页 / 排序 / NULL 置灰（只读，见 `../property/property-module-spec.md` §9） |
| 拖拽 | 表 / 视图拖进 SQL 编辑器（载荷只带限定名 + 短名） |
| 元数据缓存 | L1 内存 + L2 每连接 SQLite（cache-aside），L3 实时内省非缓存；刷新只重建不删除 |

**不做什么**

| 项 | 说明 |
| --- | --- |
| 不建连接 | 连接的新建 / 编辑 / 测试 / 保存是 M3 职责；M4 只消费 `ConnectionService` 与 `ConnectionManager` |
| 不做 SQL 执行 | 编辑 / 执行 / 结果集是 `editor` 模块职责；导航只投递「打开查询」请求 |
| 不编辑元数据 | 改名 / 建索引 / 授权等操作不在本模块范围（属性面板只读） |
| 不做权限 / 依赖 | 驱动内省能力未实现，属性面板对应页签灰置（`rds-property` 注册表 `Unsupported`） |
| 不含全文检索引擎 | 搜索内容档走 Quick Open `#`（`scratchpad` 全文索引），面板搜索框只有名称档 |

## 2. 技术栈与依赖

- **表现层**：GPUI + `gpui-kit` 0.7.0（树行原语 `workbench_shell::tree`：`active_bar` / `disclosure_icon` / `indent_spacer` / `RowHeight`）；零裸 px / 裸色值（`ui_contract` 扫描 `panels/`）。
- **服务层**：`crates/database`（rds-database，Feature crate）——`navigator_service`（内省 + 缓存编排）、`nav_jobs`（后台单工作线程）、`metadata_service`（内省取数）、`property_panel`（属性生产者）、`table_data`（数据预览分页）。
- **数据层**：`engine`（驱动 + L2 持久化 + `ConnectionOrgStore` 分组 / 标签）、`shared`（`NavSource` 归属域等模型）、`paths`（运行时数据路径，`RDS_HOME`）。
- **依赖方向（硬约束）**：`database → engine / shared / gpui-kit / workbench_shell / property`；不反向依赖 workbench `panels/`（协作走 command / event / `Shared` 桥）。

## 3. 代码地图

### 3.1 `crates/database/`（rds-database，模块主体）

| 文件 | 职责 |
| --- | --- |
| `src/navigator_service.rs` | 导航服务：`load_children` / `load_children_page`（分页）/ `object_position`（选中定位）/ `warm_schemas`（后台预热）/ `prefetch_columns`（表列预取）/ `load_properties` 入口；**三级缓存编排**（L1 内存 / L2 连接级 SQLite / L3 实时内省） |
| `src/nav_jobs.rs` | 后台任务队列（单工作线程）：`LoadChildren` / `LoadProperties` / `SearchIndex` / `Warm` / `PrefetchColumns` / `GenerateDml` / `TestConnection`；render 期零 I/O |
| `src/metadata_service.rs` | 内省取数封装：`list_catalogs` / `has_schema_level` / `list_schemas` / `list_tables` / `list_columns` / `list_indexes` / `list_constraints` / `list_procedures` / `list_functions` / `list_sequences` / `list_triggers` / `get_routine_source` / `get_table_ddl` / `execute_query`（数据预览复用） |
| `src/model.rs` | `NavPath` / `NavNode` / `NavSource` / `PropertyRef` / `PropertyRequest`（数据页参数透传） |
| `src/nav_view.rs` + `nav_view/rows.rs` | 视图状态（展开 / 选中 / 过滤 / 分页游标）；行交互（右键菜单、拖拽载荷、属性请求构造） |
| `src/property_panel.rs` | 属性取数 `load_properties`（第 6 参 `data_req`）+ `PropertyRegistryImpl`（页签集合 + 灰置） |
| `src/table_data.rs` | 数据预览分页核心：`TableDataRequest` / `quote_ident` / `load_table_data`（排序 + 主键补齐 / `limit+1` 探测 / 真 COUNT） |

### 3.2 `crates/workbench/`（装配与渲染）

| 文件 | 职责 |
| --- | --- |
| `src/panels/nav.rs` | 左侧导航面板（树渲染 / 搜索框 / 显示开关 `⋯`） |
| `src/panels/editor.rs` | 属性面板（`render_property_panel` / `render_data_page`）+ 拖拽落点（`request_query`） |
| `src/view.rs` | Dock 装配（左 Dock 240px）与三模式（展开 / 收起 / 完全隐藏） |
| `src/commands.rs` | 导航相关命令（Quick Open `ShowProperties` 等） |

### 3.3 `crates/property/`（rds-property，独立 crate）

描述模型 / 注册表 / 数据容器（`ObjectProperties` + `TableDataPage`）；详见 `../property/property-module-spec.md`。

## 4. 核心概念模型

### 4.1 归属域（存储域）与 ID 前缀

连接按存储域分 `P` 项目 / `G` 全局 / `GP` 共享（独立副本，显式同步）。前缀判定收归 `id_prefix`（服务层与导航侧同一来源）。行内归属域**右对齐成列**（可关）。

### 4.2 树模型

```
连接（根）
└─ Catalog（SQLite / DuckDB 只有这一层；PG 有 Catalog(当前库) → Schema 两层）
   └─ 类别文件夹（表 (n) / 视图 (n) / 例程…，带计数）
      └─ 对象（表 / 视图 / 例程…）
         └─ 列（表 / 视图展开时加载，首批后台预取前 20 张表）
```

- **层级随数据库类型动态**：驱动声明是否有独立 Schema 层（`has_schema_level`）；无 Schema 层时 Catalog 直接挂类别文件夹（`FALLBACK_CONTAINER` 退化）。
- **类别文件夹计数**：文件夹名带对象数（`表 (2)`），来源实时内省，不静态缓存。

### 4.3 分组 / 标签（`engine::persistence::ConnectionOrgStore`）

- **分组 = 结构，标签 = 检索**：分组是项目级自定义结构且**多对多**；只在**主组**（`is_primary`）全亮，其它组以 `∈ 主组名` 引用行出现（点击跳回）。标签多值、**不进树**，只做筛选 / 搜索。
- 落库：`connection_groups` / `connection_group_members`（含 `is_primary`）/ `connection_tags`，按存储域分区（项目库 / 全局库）。
- 归组 / 排序：拖拽到分组头 = 归组（「未分组」头 = 移出全部）；拖到连接行 = 插到该行之前；分组头拖分组头 = 分组排序；`Alt+↑/↓`（连接）与右键上下移（分组）配套；每次整容器写 `0..n`。

### 4.4 驱动目录（徽标映射）

驱动名 → 徽标（形状 + 2 字母类型码）与状态色（灰 = 不可用 / 彩 = 可用）；hover 卡（300ms）补类型 / 状态 / 驱动名。数据来源 `drivers` 目录（能力矩阵），UI 只做「键 → 显示名」映射。

### 4.5 导航状态（视图状态分区）

展开 / 选中 / 过滤 / 分页游标存 `navigator_state`：项目 / 共享连接 → `project.db`，全局连接 → `global.db`（按存储域分区持久化）。

## 5. 数据模型（实证）

```text
// 导航路径（树节点定位键）
pub enum NavPath {
    Connection,
    Catalog { catalog: String },
    Schema { catalog: String, schema: String },
    Folder { catalog: String, schema: Option<String>, kind: NavFolderKind }, // 类别文件夹
    Table { catalog: String, schema: Option<String>, table: String },
    View { … }, Column { … }, Routine { … },
}

// 树节点（含类别文件夹计数）
pub struct NavNode { … } // kind / name / 计数 / 展开状态等

// 归属域（三态）
pub enum NavSource { Global, Project, Shared } // id_prefix 判定来源唯一

// 分页页（类别文件夹首批 200 + 行尾「加载更多」）
pub struct NavPage { items: Vec<NavNode>, has_more: bool, … }

// 属性请求（属性面板 + 数据预览参数透传）
pub struct PropertyRequest {
    property: PropertyRef,      // conn_id / source / catalog / schema / parent / name / kind
    conn_label: String,
    driver: String,
    data: Option<TableDataRequest>, // None = 导航默认不带数据（「数据」页签懒加载）
}
```

缓存存储（L2，每连接 SQLite）：键 = 连接 id + 路径 + 格式版本；命中即渲染，未命中 L3 实时内省并回写（cache-aside）。

## 6. 关键流程

### 6.1 展开节点的三级缓存读取（核心）

```text
展开连接根 / 节点
  → 已建连？否 → 自动建连（展开即建连）
  → L1 内存命中？是 → 直接渲染
  → L2 连接级 SQLite 命中？是 → 回填 L1、渲染
  → L3 实时内省（MetadataService）→ 写 L1 + L2（cache-aside 回写）
```

- **新增数据源写入连接表即启动缓存**：连接建立并落库后，该连接的元数据缓存自动可写——首次展开走 L3 内省并回写，之后展开命中 L2（不再打库）。
- 刷新语义：`fresh` 刷新以实时内省为准，先清该连接 L1（L2 由各路径重写），**只重建该范围、不删其他缓存**；唯一删除入口 `⋯ → 缓存管理…`。
- 类别文件夹分页：`load_children_page(path, offset, limit)`，首批 200 条 + 行尾「加载更多」；表文件夹首次加载后后台 `prefetch_columns` 预取前 20 张表的列。

### 6.2 连接 / 断开

展开连接根未建连 → 自动建连再加载；未连接时不排加载、改显提示，避免 `CONN_NOT_FOUND`。断开 = 释放物理连接（文件型库同 URL 多条逻辑连接别名共享同一 `Arc<dyn Database>`，文件只打开一次）。

### 6.3 归组 / 标签写入

拖拽归组 / 分组排序 / 打标签 → `ConnectionOrgStore` 整容器写 `0..n`（分组顺序持久化）；标签不进树，只影响筛选 / 搜索。

### 6.4 搜索与筛选

- 面板搜索框：**名称档**（节点名匹配），按当前展开范围过滤；
- Quick Open `#`：**内容档**（全文索引，`scratchpad` 模块）；跨连接检索走 `SearchIndex` 后台任务；
- 标签筛选：多标签 AND / OR（按分组 / 标签选择器）。

### 6.5 属性面板与数据预览

双击 / `F4` / 搜索行单击 → `PropertyRequest`（含 `data: Option<TableDataRequest>`）→ `nav_jobs::enqueue_properties` → `load_properties`（属性 L2 缓存 + 数据按新参重查）→ `rds-property` 渲染。「数据」页签**懒加载**（首切才带默认分页参数重发）；列头排序（升 → 降循环，回到第一页）、上一页 / 下一页、NULL 置灰、无主键视图如实显示「本页无定义顺序」。

### 6.6 拖拽进编辑器

表 / 视图拖拽载荷只带限定名 + 短名；落 SQL 区插到光标处、落其他位置追加；两处都聚焦 SQL 区。

## 7. 缓存体系（与 DBeaver / DataGrip 对照见 `metadata-cache-vs-dbeaver-datagrip.md`）

| 层 | 载体 | 语义 |
| --- | --- | --- |
| L1 | 进程内存 | 当前会话命中；刷新时按连接清空 |
| L2 | 每连接 SQLite 文件 | 跨会话命中；**只增不删**（刷新只重建范围）；唯一删除入口「缓存管理」 |
| L3 | 无（实时内省） | 不是缓存：cache-aside 的未命中源，取到即回写 L1/L2 |

- **缓存身份指纹**：同一物理库的多条连接共享 L2——身份 = 数据库族 + 规范化地址 + principal + 格式版本；驱动实现 / 密码 / 连接参数**不进身份**；指纹只作缓存键、**不作主键**。
- **cache-aside 约定**：命中即渲染、未命中 L3 取数回写；绝不静默返回空（取数失败如实报错）。
- **触发条件（用户口径）**：新增数据源写入连接表 → 该连接缓存即启动（首次展开自动预热回写），无需手动开启。

## 8. 测试体系

- 真机全链路：`crates/workbench/tests/nav_property_real.rs`（SQLite / DuckDB 各跑同一参数化链路：测试连接 → 真实连接 → Catalog / 文件夹 / 表 / 视图 / 列 → 属性面板 → 数据预览分页 / 排序 / NULL → 注册表灰置 → 二次加载 cache-aside）。详见 `database-navigator-test-report-2026-10-03.md`。
- 单元（`crates/database` lib，98 项）：`nav_view` 状态机 / 分页 / 失败重试 / `property_ref_of` 映射 / `table_data` 转义与分页 / 缓存路径。
- 窗口级（`#[gpui_kit::test]`）：树行渲染、右键菜单、tab 切换（`rds-workbench` lib，131 项）。
- 已知测试缺口：MySQL / PG 无端点（环境）；大 schema（>500 对象）真机未做（分页逻辑由单测覆盖）；GUI 像素级冒烟未做。

## 9. 文档体系（`docs/architecture/database/`）

| 文档 | 什么时候读 |
| --- | --- |
| `README.md` | 模块入口：特点矩阵 / 真机验收 / 文档地图 / 下一步 |
| `database-navigator-architecture.md` | **为什么这样设计 / 怎么运转**：心智模型与边界规则 / 概念定位表 / 分层与 render 零 I/O / 五条数据流 / 决策取舍 / 降级容错 / §11 已知问题（权威） |
| `database-navigator-module-spec.md` | **工程口径**：边界 / 代码地图 / 数据模型 / 关键流程 / 缓存体系 / 测试 / 复现命令（本文件） |
| `database-navigator-prototype-design.md` | **长什么样**：面板布局 / 连接行解析 / 双通道徽标 / 树模型 / 三级数据管线 / 右键菜单 / 主题映射 |
| `database-navigator-user-guide.md` | **怎么用**：入口 / 界面导览 / 典型流程 / 快捷键 / 显示开关 / FAQ / 验收清单 |
| `database-nav-dev-plan.md` | **做到哪了**：Phase A/B/C 任务逐项状态、轮次实现记录与踩坑、迁移与表、测试场景、风险 |
| `database-navigator-prototype.html` | 可交互示意（明暗双主题；密度对比） |
| `database-navigator-showcase.html` / `.md` | 宣传版（视觉 / 可贴）：一页看懂特点 / 流程 / 架构 / 三级缓存与大库 |
| `metadata-cache-vs-dbeaver-datagrip.md` | **缓存为什么这样设计**：与 DBeaver / DataGrip 10 维度对照、三条硬约束、分页与搜索落地记录 |
| `../connection/README.md` | 上游：连接的新建 / 编辑与作用域路由（M3） |

## 10. 关键设计决策（速查；详情见架构文档）

1. **分组 / 标签分轨**：结构（分组，多对多 + 主组）与检索（标签，多值）不合并——避免「既当树又当筛选器」的混浊。
2. **颜色给状态、形状给类型**：徽标双通道，hover 才补文字；行内密度预算 3 元素（徽标 + 名称 + 归属域列）。
3. **三级缓存 cache-aside**：L1/L2 命中即渲染，L3 实时内省回写；缓存只增不删。
4. **展开即建连**：避免未连接时的 `CONN_NOT_FOUND` 噪音。
5. **render 期零 I/O**：所有取数走 `nav_jobs` 后台队列，render 只读状态。
6. **属性面板抽独立 crate**（`rds-property`）：描述驱动 + 灰置能力位；数据预览走同一链路（`table_data`）。
7. **大 schema 分页**：首批 200 + 加载更多 + 表列后台预取（前 20 张）。

## 11. 已知问题与待办（权威：`database-navigator-architecture.md` §11）

摘要（完整含每项状态与实测证据见架构文档）：

| 类别 | 项 |
| --- | --- |
| 模块内可做 | 系统库 / 系统 schema 过滤（「自定义显示数据库 / Schema」架构落地）；大 schema 列内联展开阈值；标签命名规范 `key:value` |
| 需协调 | PostgreSQL 跨库浏览（展开兄弟库时另开一条连接）；元数据缓存键切身份指纹（C8）接线 |
| 依赖其他模块 | Mock（M7）/ 洞察（M8）面板装配；SQL 可执行区脱离 `use_duckdb_fed` 限制 |

## 12. 复现命令速查（Windows + PowerShell）

```powershell
# 编译（必须 -j 2）
cargo check -p rds-database -p rds-workbench -j 2

# 全链路真机验证（SQLite + DuckDB，自建临时库）
cargo test -p rds-workbench --test nav_property_real -j 2 -- --nocapture

# 单元回归
cargo test -p rds-database -j 2 --lib
cargo test -p rds-workbench -j 2 --lib
cargo test -p rds-property -j 2

# 应用编译
cargo check -p rds-app -j 2

# 启动
cargo run -p rds-app
```

> 注意：提交前 `git status` 核对，勿卷入他人在途改动（如 `engine/driver/declaration.rs`、`engine/migration/global_init.rs`）。

## 13. 模块关系（M3 / M7 / M8）

| 相邻模块 | 关系 |
| --- | --- |
| M3 连接（`connection`） | 上游：连接的新建 / 编辑 / 测试 / 保存与作用域路由；M4 消费连接池 + 存储域 |
| M7 Mock（`mock`） | 下游：导航表 / 视图右键「生成 Mock 数据」投递对象；M7 面板装配待接 |
| M8 洞察（`insight`） | 下游：导航右键「查看洞察」；M8 面板装配待接 |
| 编辑器（`editor`） | 下游：双击表 = 注入 `SELECT * … LIMIT 200`；拖拽投递限定名 |
| 属性面板（`property`） | 下游：`load_properties` 数据生产者 + `PropertyRegistryImpl` 注册表 |
