# 导航 B 档架构项与未决项立项方案（讨论稿 · 2026-10-24）

> **会话约束**：本稿只讨论方案、**不改任何代码**。文中每条现状结论都带本地源码行号或竞品分析文档行号；「建议」是我方取舍，「待拍板」是需要用户决策的开放项，二者分开标注。
>
> **取证口径**：本地源码以 `crates/...:行号` 引用；竞品证据以 `docs/analysis/competitor-analysis/<文件>.md:行号` 引用（该目录 gitignore 不入库，PowerShell 读取需 `-Encoding UTF8`）。GPUI 组件 API 一律走本地 vendor 与本地 MCP 索引，不引用在线资料。

## 0. 范围、基线与编号约定

### 0.1 本稿覆盖四个议题

| 议题 | 简称 | 结论性质 |
| --- | --- | --- |
| 大 schema 两阶段回填：表名单先上屏，行估计后台补填 | **S3-④** | 本稿给「做不做、怎么做」的结论 |
| 连接过滤器：include/exclude 双向过滤 + 正则 mask + 全局过滤器 | **B-1** | 架构决策后立项 |
| 按连接分片并发：每连接信号量 = 1、跨连接并行替代全局 `LOAD_CONCURRENCY=1` | **B-2** | **先真机验证连接池/并发容量，再决定是否改调度** |
| 树交互六项：右键先选中、Home/End/PageUp/PageDown、前缀跳转、菜单能力门控、行内重命名、staged 提交模型 | **S4-N** | 现状盘点 + 规格草案 + 分期建议 |

### 0.2 编号对照（避免与旧清单混淆）

- 旧 `S4`：`docs/architecture/database/database-nav-dev-plan.md` §2.6「S4 真机验收清单」19 条鼠标/拖拽/行内编辑器走查项（`database-nav-dev-plan.md:459-490`），属**真机核验清单**，与本稿 `S4-N` 不是同一编号。
- 本稿 `S4-N`：新交互架构批次（键盘/门控/重命名/staged），下文逐项记为 `S4-N1 … S4-N6`。
- `S3-④`：承接既有 S3 批次（S3-① 可见性过滤、S3-② 分区/继承折叠、S3-③ 列内联阈值）的第四项；不另起编号。

### 0.3 已实现基线（避免重复立项，本稿只列缺口）

| 能力 | 现状 | 证据 |
| --- | --- | --- |
| 搜索正则 / 大小写 / 仅已连接三开关 | 已实现，面板级持久化（仅已连接会话级不落库） | `crates/database/src/nav_view.rs:272-277`；`crates/database/src/nav_view/chrome.rs:783-872` |
| 索引搜索防抖 250ms + 签名判变 + 批次序号过期丢弃 | 已实现 | `crates/workbench_shell/src/ui.rs:140`；`crates/database/src/nav_view/actions.rs:189-236,242-293` |
| 右键先选中（连接行 / 对象行） | 已实现 | `crates/database/src/nav_view/rows.rs:1611-1624`、`:2218-2231` |
| Home / End / PageUp / PageDown | 已实现（固定 10 行步长） | `crates/database/src/commands.rs:29-35`；`crates/app/src/main.rs:286-289`；`crates/database/src/nav_view/chrome.rs:948-968`；`crates/database/src/nav_view.rs:657-691` |
| 前缀跳转基础版（单字符首字母、循环下一个） | 已实现，缺累积匹配与超时重置 | `crates/database/src/nav_view/chrome.rs:973-983`；`crates/database/src/nav_view.rs:696-726` |
| 系统 catalog / schema 过滤 + 可见库白名单 | 已实现（S3-①） | `crates/database/src/navigator_service.rs:87-114`；`crates/settings/src/model.rs:403-409` |
| 列预取独立信号量（并发 2，让位语义） | 已实现 | `crates/database/src/nav_jobs.rs:67-72,1032-1035` |
| 索引搜索独立并行（8 块 scoped 线程，不占导航信号量） | 已实现 | `crates/database/src/nav_jobs.rs:170,1095-1128` |

### 0.4 工程硬约束（立项方案同样受其约束）

- `render` 期零 I/O：新增回填 / 过滤 / 任务全部走 `nav_jobs` 入队 + 结果队列回填（README §4 第 1 条；`crates/database/src/nav_jobs.rs:787-836` worker 模型）。
- 视图只依赖 `gpui-kit`；颜色一律主题 token，结构尺寸登记 `workbench_shell/src/ui.rs`，裸 `px(` / 裸 hex 被 `ui_contract` 拦下。
- 类型差异落在服务层与驱动能力位，`NavNode` / 渲染层不为数据库类型分叉。
- 注释与文档简体中文；过程产物（探针、一次性脚本、验证日志）落 `.rds/tmp/` 或 `tools/_archive/`，不入库；架构文档进 `docs/architecture/database/`。
- 后续每批独立 `cargo check` + 单测 + commit；`cargo` 固定 `-j 2`。

---

## 1. 议题 S3-④：大 schema 两阶段回填（行估计后台补填）

### 1.1 现状取证（本地事实）

**字段已存在，但全仓硬编码 `None`**：

| 位置 | 内容 | 证据 |
| --- | --- | --- |
| 视图模型 | `NavNodeKind::Table { row_estimate: Option<u64> }` 字段已定义 | `crates/database/src/model.rs:143` |
| 真实树构建点 | 表节点 `row_estimate: None` 硬编码 | `crates/database/src/navigator_service.rs:706` |
| 搜索命中合成节点 | `row_estimate: None` | `crates/database/src/nav_view/primitives.rs:521` |
| 测试夹具 | `row_estimate: None` | `crates/database/src/nav_view/primitives.rs:1015`；`crates/database/src/nav_view/tests.rs:263,1439,1556,1632,1850` |
| 内省 trait | `NodeInfo`（`name/kind/comment/parent_name/is_partition`）**没有**行估计字段；`SchemaObjects` 同样没有；`NodeDetail.row_count_estimate: Option<u32>` 属懒详情路径 | `crates/engine/src/driver/traits.rs:125-138,145-151,186-191,227-232` |
| 六个原生驱动 `get_table_detail` | `row_count_estimate: None` 全部未实现 | `crates/engine/src/driver/native/postgres.rs:1520`、`native/postgres_native.rs:1178`、`native/mysql.rs:1563`、`native/mysql_native.rs:995`、`native/sqlite.rs:1212`、`native/duckdb.rs:1254` |
| 属性面板 PG 路径 | 打开表属性时懒取 `reltuples` 推一张「行估计 · reltuples」卡片；这是**属性面板路径**，不是树路径 | `crates/database/src/property_providers/pg.rs:227,233-235,247-248`；测试 `:669` |
| 树内行尾徽标 | 只有「分区 N / 继承 N」，无行计数徽标 | `crates/database/src/nav_view/rows.rs:2148-2166` |
| 结果网格行数 | 真 `COUNT(*)` 独立一问，拿不到如实留空（A12/A13 口径），不编 0 | `crates/database/src/table_data.rs:11-12` |
| L2 索引列 | `metadata_index.row_count_estimate` 列已存在（迁移 004 / 006），导航内省链路不回填；现有写值链路属统计同步：schema / 列条目写 `None`（`persistence/metadata_cache.rs:3058,3087`），表条目透传 `table.row_count_estimate`（`:3073`），`tables` 表 upsert 覆盖该列（`:1836`），读回 i64→u32 转换（`:2321`）；`crates/engine/src/cache/metadata_cache.rs` 中 `row_count_estimate` 零命中 | `crates/engine/migrations/connection_metadata/004_refactor_to_normalized.sql:43`、`006_add_metadata_index.sql:30` |

**两阶段现状对照**：名单先上屏已经做到——`collect_objects` 批量内省一次建连（`navigator_service.rs:1027-1037`）；`CHUNK_THRESHOLD = 500` 以下才进 L1（`:44,1060-1065`）；索引重建只在 Tables 趟（`:1083-1085`）；扫描完成标记 `mark_schema_complete`（`:1088`）；缺的是**第二阶段的轻元数据补填**（行估计 / 列计数 / 定义扫描）。

### 1.2 竞品证据

- **rustgrid 两阶段发布**：表名单先上屏，`table_statuses`（含 `COUNT(*)` 概览，大库可能耗时数秒）由后台独立任务返回后回填，期间列表保持可交互；补填完成后通知对象面板并刷新滚动条。证据：`docs/analysis/competitor-analysis/nav-analysis-cellar-rustgrid-sqlab.md:151`（源码 `tree.rs:1306-1372`）；任务粒度参照 `:174-176`。
- **任务分级建议**：展开行优先 > 分类预热 > 列补填 > 概览统计；重元数据（COUNT、列计数、定义扫描）拆第二阶段任务先发布名单。证据：`nav-analysis-cellar-rustgrid-sqlab.md:320`。
- **DBeaver 统计后台化**：`StatReadJob` 用 `IdentityHashMap<DBSObject, StatReadJob>` 按父去重，未读渲染占位 `"..."`，完成后 `asyncExec` 写 `TREE_DATA_STAT_MAX_SIZE` + 父树 redraw。证据：`nav-analysis-dbeaver.md:106`（源码行号实现时回读 DBeaver 仓库再落码）。

### 1.3 结论：做不做

**建议（B 档 v1 立项范围）：做「统计估计后台回填」，不做真 `COUNT(*)` 进树。**

| 方案 | 结论 | 理由 |
| --- | --- | --- |
| v1：统计估计（优化器统计信息）后台补填 | **做** | 单次目录级查询即可拿到整 schema 估计值，不逐表 `COUNT`；与 rustgrid「名单先上屏、轻元数据第二阶段」一致（`nav-analysis-cellar-rustgrid-sqlab.md:151,320`）；复用 `NavNodeKind::Table.row_estimate` 现有字段，不新增模型骨架 |
| 真 `COUNT(*)` 上树 | **不做（v1 排除）** | 大库逐表 `COUNT` 秒级以上（rustgrid `table_statuses` 自己也承认「大库可能耗时数秒」，`nav-analysis-cellar-rustgrid-sqlab.md:151`）；与「名单先上屏、列表保持可交互」目标冲突；现状结果网格真 `COUNT` 是独立一问（`table_data.rs:11-12`），语义归属属性/数据预览路径，不属树路径 |
| 文件夹子对象计数补填 | **暂缓** | 现有 `child_total` 已由索引计数 / 全量长度给出（`crates/database/src/nav_view.rs:260-268`）；行估计只画在表/视图叶子行，不扩展到文件夹计数 |

**待拍板 S3-④-A**：v1 默认开启还是设置开关默认关？建议默认开启、仅对已连接 schema 触发；开关项写进 `Navigator` 设置（仿 `show_tags` 先例 `crates/settings/src/model.rs:380-382`）。

### 1.4 怎么做（架构方案草案，未动代码）

#### 1.4.1 数据落点

- 视图层：回填 `NavNodeKind::Table.row_estimate: Option<u64>`（字段已存在，`model.rs:143`）；`None` = 未回填 / 无估计可用 / 失败留空，三态不细分（UI 只画「占位 `…`」与「≈ N」两种）。
- 传输层：`NodeInfo` **不扩**行估计字段——内省名单路径保持一次批量轻查询；估计值由新的第二阶段任务单独取回，避免把重元数据塞回首屏批次。
- 缓存层：估计值回写 L2 `metadata_index.row_count_estimate`（列已存在，`006_add_metadata_index.sql:30`；upsert 落点 `crates/engine/src/persistence/metadata_cache.rs:1836`）；L1 内存状态在结果队列回填时刷新；断开 / 刷新不删缓存（README §4 第 5 条「缓存只增不删」）。

#### 1.4.2 任务族与状态机

- 新增 `Job::RowEstimate { conn_id, project_root, catalog, schema, tables: Vec<(name, qualified)> }`，`JobPriority::Background`（用户展开任务插队优先，`nav_jobs.rs:526-537` 既有优先级模型复用）。
- 触发时机：表/视图文件夹名单首屏回填完成后，对该 schema 入队一批估计任务；大 schema 按 `PREFETCH_BATCH` 同类分批（`nav_jobs.rs:133` 先例），不在一个任务里扫整库。
- 单任务超时沿用 `JOB_TIMEOUT`（`nav_jobs.rs:150`，30s）；失败 / 超时 / 驱动不支持 → `None` 留空，**不显示 0、不报错红字**（对照 `table_data.rs:12`「拿不到如实留空」口径）。
- 回填路径：任务线程 → 结果队列 → 主线程 `pump` 回填 `children` 对应节点 → `cx.notify()`；render 不直接读远程状态。
- 去重：同一 `(conn_id, schema)` 的在途估计任务复用 `pending_*` 原子计数范式（`nav_jobs.rs:389-395`）登记在途，重复展开不重复入队；刷新元数据时作废该 schema 的在途批次。

#### 1.4.3 驱动支持矩阵（v1 口径）

| 驱动 | 估计来源 | v1 口径 |
| --- | --- | --- |
| PostgreSQL | `pg_class.reltuples`（属性面板已有同款 SQL，`property_providers/pg.rs:247-248`） | 回填；文案标「估计」；统计未 `ANALYZE` 时 reltuples 失真属已知口径，不二次修正 |
| MySQL | `information_schema.TABLES.TABLE_ROWS`（InnoDB 为优化器估计，MyISAM 近似精确） | 回填；v1 不区分引擎口径 |
| SQLite | 无存储统计信息 | 留空，不发真 `COUNT` |
| DuckDB | 无存储统计信息 | 留空，不发真 `COUNT` |
| 视图 / 分区表 | 视图无独立统计；分区父表 `reltuples` 口径单列说明 | 视图留空；分区父表取父表估计 |
| 例程 / 序列 / 触发器 | 不适用 | 不参与 |

#### 1.4.4 UI 表现（草案，尺寸 / 颜色落码时再登记 `ui.rs` 与主题 token）

- 表/视图行尾：未回填显示弱色占位 `…`；回填显示 `≈ N`（千分位格式化，数字过长截断，与「分区 N / 继承 N」徽标同槽位，`rows.rs:2148-2166` 既有布局旁挂）。
- 列表在第二阶段回填期间保持可交互（rustgrid 范式，`nav-analysis-cellar-rustgrid-sqlab.md:151`）；不阻塞滚动、不重排行序（排序权重另案，不在本批）。
- 估计值不得参与名称排序或默认分组（`sort_weight` 列存在但本批不写，`006_add_metadata_index.sql:33`）。

#### 1.4.5 未决拍板项（S3-④）

| 编号 | 问题 | 候选选项 | 我方建议 | 阻塞 |
| --- | --- | --- | --- | --- |
| S3-④-A | 默认开启 / 默认关闭 | 开（仅已连接 schema 触发）/ 关（设置开启） | 默认开，可关 | v1 落码前 |
| S3-④-B | 触发阈值：表数 > N 才排队估计，还是全量排队 | 全量 / 阈值（建议 20 张以上才后台估计，小 schema 首屏顺手取） | 阈值 20，复用 `PREFETCH_BATCH` 量级 | 任务粒度设计 |
| S3-④-C | 估计值是否回写 L2 持久化 | 回写（重开复用）/ 仅内存 | 回写 L2，刷新元数据时作废重取 | 缓存失效边界 |
| S3-④-D | MySQL `TABLE_ROWS` 与 PG `reltuples` 误差是否在 tooltip 注明口径 | 注明 / 不注 | 注明一行弱色说明 | 文案与 token |
| S3-④-E | v2 是否补真 `COUNT`（手动触发、表数上限、独立后台任务） | 是 / 否 | 否，本期不预留入口，待有真机诉求再立项 | 不阻塞 v1 |

---

## 2. 议题 B-1：连接过滤器（include/exclude + 正则 mask + 全局过滤器）

### 2.1 现状取证（本地事实）

**现有过滤体系**：

| 层 | 机制 | 证据 |
| --- | --- | --- |
| 连接 facet | 归属域 / 类型 / 驱动 / 标签，四项 AND 叠加；`NavConnFilter::passes()` 纯函数 | `crates/database/src/nav_view.rs:448-507`；`crates/settings/src/model.rs:359-372` |
| 面板索引搜索 | 词 + 正则开关 + 大小写开关 + 仅已连接开关；防抖 250ms；签名判变；seq 过期丢弃 | `actions.rs:189-236,242-293`；`nav_jobs.rs:310-326,1238-1270` |
| 搜索命中精筛 | `hits.retain(...)` 在 Rust 侧做 contains / 正则过滤，是 per-connection mask 可叠加的同一插入点 | `crates/database/src/nav_jobs.rs:1260,1270` |
| 系统对象过滤 | 纯函数 `nav_keep_object` + `nav_is_system_catalog` / `nav_is_system_schema` | `navigator_service.rs:87-114` |
| 每连接持久化先例 | `visible_databases: BTreeMap<conn_id, Vec<String>>`（S3-① 白名单）；NavHost 端口读写方法；workbench 侧互转 | `settings/model.rs:403-409`；`database/src/nav_host.rs:144-152`；`workbench/src/components/nav_host.rs:129-149,201-216` |

**现状缺口**：`NavViewState` 全字段（`nav_view.rs:220-341`）中**不存在** per-connection 对象名 mask、staged 脏提交缓冲、typeahead 累积缓冲；没有 include/exclude 概念；过滤器只作用于连接级 facet，不作用于对象名。

### 2.2 DBeaver 竞品证据

- **树内 `TreeFilter extends PatternFilter`**：`setPattern` 补 `*` 前后缀构造 `TextMatcherExt`；`split("\\.")` 两段即 `schema.object` 匹配；`isMatchingNeeded` 四态（connection / container / 默认排除容器·文件夹·列 / file）；`isPatternMatched` 顺序 = 标签通配 → 两段沿父链找 `DBSStructContainer` → 回退 description；`filterShowConnected` 隐藏断连源；`CustomFilteredTree` 工具栏类型切换 + 已连接开关。证据：`nav-analysis-dbeaver.md:14,61`（源码 `TreeFilter` `DatabaseNavigatorTree.java:795-970`、`CustomFilteredTree:972-1080`，见 `:71`）。
- **include / exclude 双向标记**：`NavigatorUtils.filterSelection` 第 438–512 行。证据：`nav-analysis-dbeaver.md:67,219,228`（`NavigatorHandlerFilterInclude.java` 管 include/exclude；`NavigatorHandlerFilterConfig.java` 父节点回溯、`EditObjectFilterDialog`、全局过滤器、`SHOW_GLOBAL_FILTERS_ID` 分支，`:75`）。
- **过滤态持久化**：筛选功能清单含「过滤态持久化」一项（`nav-analysis-dbeaver.md:219`）；竞品笔记未单独定位其代码锚点，机制实现时回读 `NavigatorHandlerFilterConfig` / `NavigatorUtils` 源码再落码（注意 `:62` 是展开态 `NAVIGATOR_RESTORE_STATE_DEPTH`，与过滤器无关，勿混用）。
- **事件合并（对照项，结论见 2.3.5）**：`DBNModel.EventProcessingJob` system Job 每 100ms 批量合并事件快照后 `executeInMainThread` 分发；`LoadingUIJob` `DELAY=100ms`；`NodeLoadersPainter` `REPAINT_DELAY=100ms` / `WAIT_DELAY=500ms`。证据：`nav-analysis-dbeaver.md:16,102-115,146,174`（`DBNModel.java:550-557,610-645`，`:115`）。

### 2.3 架构决策建议

#### 2.3.1 作用域分层（建议 v1 两层，不混进 facet）

| 层 | 作用对象 | 生命周期 | 持久化 |
| --- | --- | --- | --- |
| **per-connection 对象名 mask** | 单条连接下的表 / 视图 / 过程 / 函数 / 序列 / 触发器名称 | 跟随连接配置，跨会话保留 | `settings::model::Navigator` 新增 `object_masks: BTreeMap<conn_id, ObjectMaskRule>`，仿 `visible_databases:403-409` |
| **面板全局 mask** | 面板内全部可见连接的对象名兜底过滤 | 面板偏好，跨会话保留（与 facet 同档） | `Navigator` 新增全局规则字段；不进 `navigator_state` 面板行（沿用 #8 决策：筛选是全局 UI 偏好，`architecture.md` §11 #8，`navigator_service` 侧既有口径） |
| 现有 facet | 只筛连接（归属域 / 类型 / 驱动 / 标签），**不叠加对象名** | 现状不变 | 现状链路不变 |

两层与 facet 之间为 **AND**：先过 facet 得到连接集合 → 连接 mask 过滤对象名 → 全局 mask 再过滤。

#### 2.3.2 匹配语义（建议）

- 默认 glob：`*`（任意多字符）/ `?`（单字符），大小写不敏感；空规则 = 不启用该层。
- 可选正则开关：复用现有搜索正则设施（`nav_jobs.rs:320-323,1270` 的 `regex` 路径与 `regex` crate 用法）；开关 per 规则层，不与面板搜索的 `.*` 开关共享状态。
- **include / exclude 优先级**：exclude 优先于 include；include 为空 = 全部候选进入候选集；exclude 为空 = 不排除；include 非空 = 只保留命中 include 且未命中 exclude 的对象。
- v1 **不做** `schema.object` 两段匹配（DBeaver `TreeFilter` 两段逻辑，`nav-analysis-dbeaver.md:61`）：v1 只匹配对象短名；两段匹配挂 v2，理由是现有树路径携带 `PropertyRef` / `NavPath`（`navigator_service.rs` 路径字段），两段匹配需要沿父链回退，工作量与边界另立。
- 过滤层级边界：mask 只作用于对象层；catalog / schema 层不进 mask（系统库 / 系统 schema 归 S3-① 白名单与待办 #14，`architecture.md:396`）。

#### 2.3.3 管线插入点

- **树路径**：`NavigatorService` 组装 `NavPage` 后、返回视图前，用纯函数 `nav_mask_passes(rule, name) -> bool` 过滤（与 `nav_keep_object:109-114` 同一纯函数范式，便于单测钉规则矩阵）。过滤后 `child_total` 口径 = **过滤后条数**（与 DBeaver「隐藏即不占计数」语义一致，`nav-analysis-dbeaver.md:61`）；v1 不提供「被隐藏 N 条」计数提示，留 v2。
- **搜索命中路径**：在 `hits.retain(...)`（`nav_jobs.rs:1260,1270`）同一精筛层追加连接 mask + 全局 mask retain；不改动 SQL / L2 索引本身。
- **存储读写**：`NavHost` 仿 `visible_databases` / `set_visible_databases`（`nav_host.rs:144-152`）新增 `object_mask(conn_id)` / `set_object_mask(...)` 与全局 mask 读写；workbench 侧仿 `nav_host.rs:129-149,201-216` 做两侧互转（`database` crate 不反向依赖 `settings`，`nav_host.rs:47-50` 既有边界）。

#### 2.3.4 UI 入口（草案，组件选型走 gpui-kit 标准件，不手搓）

- 连接右键新增「对象名过滤器…」项（对话框：include 多行输入、exclude 多行输入、正则开关、清空 / 重置按钮）；入口挂在连接菜单 `rows.rs:1643-1928` 段，与「数据库可见性…」（`:1702-1713`）相邻。
- 面板头「筛选 ▾」弹层内补全局过滤器入口与规则摘要 chip；有规则时在面板头显弱色标识；空规则不显示标识。
- 规则生效后树顶 / 空态给一句当前过滤口径（对照 §V12 筛空空态「当前筛选清单 + 清除筛选」先例，`architecture.md` §11 #25 第三轮）。

#### 2.3.5 事件合并结论：v1 **不引入** DBeaver 100ms Job

**建议**：本批不引入 `EventProcessingJob` 式 100ms 批量合并。理由：

- 现状高频路径已有防抖 + 签名 + 批次序号过期三件套（`actions.rs:203,227-235,277-281`；`nav_jobs.rs:314-316`）；mask 变更触发的是**纯 CPU 内存过滤**，不发后台查询，不存在 DBeaver 模型事件流高频打 UI 的问题（DBeaver 合并针对的是驱动层模型事件，`nav-analysis-dbeaver.md:146`）。
- 何时重估：未来若接入「驱动模型事件实时推送」或过滤触发大量回源内省，再按 `DBNModel.EventProcessingJob:550-557,610-645` 参照立项合并层；本批只在架构文档记录该触发条件。

### 2.4 分期建议

| 批次 | 范围 |
| --- | --- |
| B-1 v1 | per-connection include/exclude glob mask + 全局 glob mask + 搜索命中叠加过滤 + 对话框 / 面板头入口 + 规则持久化；纯函数 `nav_mask_passes` 规则矩阵单测 |
| B-1 v2 | `schema.object` 两段匹配；正则 per 规则独立；「被隐藏 N 条」提示；过滤态在搜索命中区的可见性标识；连接 mask 跨项目 / 共享域语义边界确认 |

### 2.5 未决拍板项（B-1）

| 编号 | 问题 | 候选选项 | 我方建议 |
| --- | --- | --- | --- |
| B-1-A | glob 与正则是否 v1 并存 | 并存（规则层各一开关）/ v1 仅 glob，正则挂 v2 | v1 仅 glob；正则开关随 v2，避免两套语义同期上线 |
| B-1-B | include 支持多模式行还是单条正则 | 多行 glob 列表 / 单条表达式 | 多行 glob 列表，每行一条，空行忽略 |
| B-1-C | 全局 mask 是否跨项目持久化 | 跨项目全局 / 跟随项目 | 跟随 `Navigator` 面板偏好（跨项目，与 facet 同档） |
| B-1-D | mask 是否作用于列层 | 作用 / 不作用 | 不作用：列由表层进入，列名过滤价值低且语义混杂，v2 再议 |
| B-1-E | 断连连接的 mask 是否随连接隐藏 | 是（复用 `search_connected_only` 口径）/ 否 | 树默认展示断连连接（现状），mask 照常过滤其缓存对象；不新增断连隐藏规则 |

---

## 3. 议题 B-2：按连接分片并发（每连接信号量 = 1，跨连接并行）

### 3.1 现状取证（本地事实）

**现有三套并发档**（`crates/database/src/nav_jobs.rs`）：

| 信号量 | 并发 | 任务族归属 | 证据 |
| --- | --- | --- | --- |
| `LOAD_SEM` | `LOAD_CONCURRENCY = 1` | `LoadChildren`（阻塞 `acquire`，`:855`）；`LoadProperties`（阻塞 `acquire`，`:959`，注释「详情卡并行 acquire 会把池占满」）；`GenerateDml`（阻塞 `acquire`，`:1061`）；`Warm`（`try_acquire` 让位，`:987`）；`WarmCompletion`（`try_acquire` 让位，`:1011`） | `:41-57` |
| `PREFETCH_SEM` | `PREFETCH_CONCURRENCY = 2` | `PrefetchColumns`（`try_acquire` 让位，`:1032`） | `:67-72,1028-1035` |
| 无信号量 | `SEARCH_PARALLEL = 8` 块 | `SearchIndex`：scoped 线程直开临时缓存连接，各自独立缓存文件，不占 `LOAD_SEM` / `PREFETCH_SEM` | `:170,1095-1128` |

**worker 模型**：单分发线程 + 优先级二叉堆（User / Background，`:526-571`）；每任务独立 `std::thread::spawn` + 独立 `tokio::Runtime::new()`（`:812-815`）；panic 隔离（`:819-830`）；在途建连短等 10ms × 50 拍、上限 500ms（`:861-876`）；任务超时 `JOB_TIMEOUT = 30s`（`:150`）。

**引擎连接模型**：

- `connection_manager.rs:114` `connections: RwLock<HashMap<ConnId, DynDatabase>>` 单实例；`cancel_tokens: RwLock<HashMap<ConnId, CancellationToken>>`（`:130`）；`transactions: Mutex<HashMap<ConnId, ActiveTransaction>>`（`:136`）；`scoped: RwLock<HashMap<(ConnId, String), DynDatabase>>`（`:144`）。
- 四个主路径驱动都是**单连接 + Mutex 串行**：`postgres.rs:53`、`mysql.rs:56`、`sqlite.rs:245`、`duckdb.rs:214`；PG `query()` 在 `lock_pg_conn` 持锁跨 await 执行 `fetch_all`（`postgres.rs:376-393`，查询级 15s 超时 `:380`）——同一实例并发查询被 Mutex 串行化，不同实例（不同连接）互不抢锁。
- **主路径已主动摒弃连接池**：`factory.rs:138-159` 记录 2026-10-08 用户拍板全参考 DBeaver Main + Metadata 单连接；旧池 min3/max6 的真机教训（app_run51~58：30s `acquire_timeout` 静默卡死、`test_before_acquire` 对 TCP 半开连接验证仍超时）写明在注释中。`postgres_pool.rs` / `standard_pool.rs` / `smart_pool.rs` / `duckdb_pool.rs` 存在但**非主路径**。
- L1 缓存是进程级全局 `Arc<Mutex<MetadataCache>>`（`crates/engine/src/cache/cache_manager.rs:66-73,83,104-118`），只做 CPU 内访问序列化，不构成 DB 瓶颈。

### 3.2 竞品证据

- **navop 会话复用池**：`sessions: HashMap<config_id, Vec<ConnectionSession>>`，`create_session` 复用空闲会话、`release` 归还；本地文件型驱动按 `physical_open_lock_key` 互斥串行物理打开。证据：`nav-analysis-navop.md:62`（`manager.rs:368-370,427-507,511-578,650-658`）。负向证据：navop 无并发上限、无优先级、无取消，大 schema 连续展开在会话池上无序竞争——朴素并发模型不宜整体照搬，`nav-analysis-navop.md:65,90`。
- **blanco 连接池化**：`DatabaseService` 按 `(config_id, database_name)` 池化活连接，`last_used` 触达，30s 周期清扫 + 空闲超时 / 断连驱逐；SSH 复用前先做隧道健康检查，不健康即丢弃重建。证据：`nav-analysis-fluxdb-blanco.md:139-140,210,213`。其调度层同样无队列、无优先级、无分片，需我方自己设计（`:213`）。
- **DBeaver family 分组取消**：后台 Job 按 `dataSource` family 归属，可按连接批量取消；模型事件 100ms 合并见 §2.2。证据：`nav-analysis-dbeaver.md:16`。

### 3.3 前置真机验证（立项闸门：先出验证报告，再动调度代码）

**硬约束（用户指定）**：先真机验证连接池 / 并发容量，再决定；方案不得回退到全局池扩容；驱动层单连接 Mutex 保持不变。

#### 3.3.1 验证形态

- 独立诊断探针（参照既有 `crates/engine/src/bin/pg_probe.rs:1-62` 探针范式：独立 Runtime、跨任务复用、timeout 实测）；过程产物落 `.rds/tmp/`，不入库；验证报告成文进 `docs/analysis/`（gitignore 过程研究）。
- 矩阵：PG（仓内真机端点 `192.168.3.138`，`pg_probe.rs:15` 先例）、MySQL 同网段真机、本地 SQLite 文件、本地 DuckDB；JDBC / wasm 路径本期排除，单列后续清单。

#### 3.3.2 验证场景与判据

| # | 场景 | 通过判据 | 失败处置 |
| --- | --- | --- | --- |
| V1 | 两条已连接各自 `LoadChildren` 并发执行 | 墙钟 ≈ 较慢者单连接水平，不存在全局锁导致的二次串行；服务端连接数 ≤ 实例数 | 维持现状全局 `LOAD_CONCURRENCY=1`，记录现象 |
| V2 | 同连接第二个 `LoadChildren` 排队 | 在 Mutex 上等待不超过 `JOB_TIMEOUT`、不死锁、不形成饿死；用户主动任务（User 优先级）能插队 | 记录等待分布，必要时同连接内部再加优先级 |
| V3 | 两条连接 + 一列预取 + 一批索引搜索同时在途 | 线程数 / Runtime 数有上界；不出现无限 `Runtime::new()` 堆积（现状每任务一个 Runtime，`nav_jobs.rs:812-815`） | 加全局软帽（§3.4.2）后复测 |
| V4 | 服务端长事务 / 锁等待下两条连接内省 | 各自超时可观测、互不拖垮；超时后连接懒重建路径可用 | 记录锁等待传播面，更新超时口径 |
| V5 | 断连 / TCP 半开连接上发起第二条任务 | 不挂死；错误如实返回节点红字 | 复用既有置 `None` 懒重建路径（`factory.rs:147-148`） |

#### 3.3.3 回退条件

任一判据失败 → **本议题搁置**，调度维持全局串行；在 `database-navigator-architecture.md` §11 登记现象、复测条件，不先改代码。

### 3.4 候选方案（仅在 V1–V5 通过后实施）

#### 3.4.1 调度分层

- **per-connection 信号量**：`HashMap<ConnId, Arc<Semaphore>>`，每连接信号量容量 = 1；`LoadChildren` / `LoadProperties` / `GenerateDml` 先取所属连接信号量，再进任务执行。
  - `LoadProperties` 与 `LoadChildren` **共用**所属连接信号量（现状二者共用全局 `LOAD_SEM`，`nav_jobs.rs:855,959`；分片后保持「同连接元数据通道唯一」语义，避免属性查询与内省抢同一条单连接）。
  - `Warm` / `WarmCompletion` 维持 `try_acquire` 让位语义，改为取所属连接信号量的 `try_acquire`，忙则跳过本批（现状 `:987,1011` 口径不变）。
  - `PrefetchColumns` 维持独立 `PREFETCH_SEM = 2`；`SearchIndex` 维持独立 scoped 线程池。
- **全局软帽**：在 per-conn 信号量之外套一层 `LOAD_GLOBAL_CAP = K`（建议起步 K=3，真机按 V3 数据定），防止连接数多时线程 / Runtime 数失控；软帽只挡排队，不改变 per-conn 互斥。
- **不做**：驱动层连接池扩容；同连接内并发查询（Mutex 串行保持）；跨连接查询合并。

#### 3.4.2 任务族归属对照（改造前后）

| 任务族 | 现状 | 目标（验证通过后） |
| --- | --- | --- |
| `LoadChildren` | 全局 `LOAD_SEM=1` | per-conn=1 → 全局软帽 K |
| `LoadProperties` | 全局 `LOAD_SEM=1` | per-conn=1 → 全局软帽 K |
| `GenerateDml` | 全局 `LOAD_SEM=1` | per-conn=1 → 全局软帽 K |
| `Warm` | `try_acquire` 全局让位 | `try_acquire` per-conn 让位 |
| `WarmCompletion` | `try_acquire` 全局让位 | `try_acquire` per-conn 让位 |
| `PrefetchColumns` | `PREFETCH_SEM=2` 独立 | 不变 |
| `SearchIndex` | 独立 8 块 scoped 线程 | 不变 |
| `TestConnection` | 独立探测会话 | 不变 |

#### 3.4.3 取消与分组（本期只登记，不实现）

- 按连接 family 批量取消参照 DBeaver `belongsTo(family)=dataSource`（`nav-analysis-dbeaver.md:16`）；落点候选为现有 `cancel_tokens: HashMap<ConnId, CancellationToken>`（`connection_manager.rs:130`）。
- 断开连接时取消该连接在途 `LoadChildren` / `LoadProperties` / `GenerateDml`；本期不实现，挂 v2。

### 3.5 未决拍板项（B-2）

| 编号 | 问题 | 候选选项 | 我方建议 |
| --- | --- | --- | --- |
| B-2-A | 全局软帽 K 取值 | 2 / 3 / 5，或不设软帽 | 起步 3，V3 真机线程 / Runtime 数据复测后定；不取消软帽 |
| B-2-B | `LoadProperties` 是否与 `LoadChildren` 共用 per-conn 信号量 | 共用 / 独立 | 共用，保持单连接元数据通道语义 |
| B-2-C | 预热让位阈值：per-conn 忙即跳，还是排队 N 毫秒 | 忙即跳 / 短等 | 忙即跳（现状 `try_acquire` 口径，不引入新等待） |
| B-2-D | JDBC / wasm 路径何时纳入分片矩阵 | v1 含 / v1 排除 | v1 排除，原生四驱动验证通过后另立探针批次 |
| B-2-E | 旧 `postgres_pool.rs` / `standard_pool.rs` / `smart_pool.rs` / `duckdb_pool.rs` 死代码是否本期清理 | 清理 / 保留观察 | 保留观察，与本议题解耦另立清理批 |

---

## 4. 议题 S4-N：树交互六项（新批次）

### 4.0 现状总览

| 项 | 现状 | 缺口 | 建议分期 |
| --- | --- | --- | --- |
| N1 右键先选中 | **已实现**（连接行 / 对象行） | 分组头不可选属设计；多选模型不存在 | 真机走查；多选不立项 |
| N2 Home / End / PageUp / PageDown | **已实现**（固定 10 行步长） | 视口真实行数未接入 | v1 维持固定步长；v2 待 vendor API |
| N3 前缀跳转 | 基础版已实现（单字符首字母循环） | 连续字符累积、超时重置、字符集、前缀匹配、编辑器焦点保护 | v1 增强 |
| N4 菜单能力门控 | 仅 kind / property / qualified 门控 + 连接菜单一项 connected 置灰 | 对象菜单无 connected 门控、无驱动能力位门控 | v1 补 connected 门控；v2 补能力位 |
| N5 行内重命名 | 分组行内已有；连接走对话框；对象重命名缺失 | 对象 DDL 重命名缺能力契约与 staged | v1 维持现状；随 DDL 批次立项 |
| N6 staged 提交模型 | 四个直接提交编辑器；`staged` / `undo` / `redo` 零命中（`dirty` / `pending` 为状态防抖与任务计数，非草稿缓冲） | 无草稿集合 / 着色 / 提交入口 / 撤销 | v1 不引入；边界预留给未来 DDL 草稿 |

### 4.1 S4-N1 右键先选中

**现状证据**：

- 连接行：`on_mouse_down(Right)` 在未选中时先 `set_nav_selected`，再由 `context_menu` 弹菜单（`rows.rs:1611-1624`，注释 `:1607-1610`；vendor `context_menu.rs:403-405` 右键只 `stop_propagation` + 开菜单、不选中，是该处理必须前置的原因）。
- 对象行：同款处理（`rows.rs:2218-2231`，注释 `:2215-2216`）。
- 分组头：`context_menu` 存在（`rows.rs:825-954`），但分组头本就不可选中（不进 `nav_order`，`rows.rs:134`），无右键先选中动作属设计如此；搜索命中行无右键菜单。
- 选中模型为单选：`selected_key: Option<String>`（`nav_view.rs:322`），无多选集合。

**竞品对照**：tusk 右键先选中行再弹菜单、菜单目标即高亮（`nav-analysis-zqlz-tusk.md:124,198`）；rustgrid 多选（单击 / Ctrl / Shift / 框选）属另一量级能力（`nav-analysis-cellar-rustgrid-sqlab.md:141,189`）。

**建议**：

- v1 不新增代码：N1 已实现；残留动作 = 旧 S4 §2.6 第 4 / 5 条真机走查（`database-nav-dev-plan.md:472-473`），把「右键连接行 / 对象行选中态跟随菜单」记进验收表。
- 多选（Ctrl / Shift / 框选）**本批不立项**：涉及选中模型、菜单目标集合、拖拽批量落点、能力门控矩阵多处联动，价值与成本另案评估。

### 4.2 S4-N2 Home / End / PageUp / PageDown

**现状证据**：

- 动作定义 `NavHome / NavEnd / NavPageUp / NavPageDown`（`commands.rs:29-35`）；按键绑定 `main.rs:286-289`；视图接线 `nav_move_edge(true/false)`（`nav_move_edge:657-668`，跳首/尾 + 居中滚动）、`nav_move_page(±1)`（`nav_move_page:674-691`）。
- 步长 `NAV_PAGE_STEP = 10` 固定值（`nav_view.rs:675`）；注释 `:672-673` 已记录有意取舍：虚拟列表不暴露可视行数（`list_scroll` 无对应查询 API），DBeaver 按视口翻页，我方取终端同款固定步长。

**建议**：

- v1 维持固定 10 行步长，不改。
- v2 触发条件：本地 vendor 0.8.0 暴露视口行数 / 可视区间查询 API 后，改为按视口行数翻页（上 `scroll_to_item` 同类可编程滚动入口，参照定位链路已接的 `reveal_scrolls_a_row_that_is_out_of_view` 先例，`architecture.md` §11 #23）。本批只在 §11 遗留项登记，不写接口猜测。

### 4.3 S4-N3 前缀跳转（连续字符累积 + 超时重置）

**现状证据**：

- 面板 `on_key_down` 取首字符，仅 `is_alphabetic()` 进 `nav_jump_prefix`（`chrome.rs:973-983`；注释 `:971-972`：搜索框 Input 有自己焦点，字符不落这里）。
- `nav_jump_prefix`（`nav_view.rs:696-726`）：大小写不敏感；只匹配带显示名的行（连接行 = 连接名，树行 = 节点名；Reference / More / Jump / SearchHit 置空不参与，`rows.rs:151,161,170`）；从选中行之后循环找第一个**首字母**匹配项；未命中原样返回。
- `NavOrderItem.name: Option<String>` 已存在（`nav_view.rs:383-385`），累积匹配的数据条件已具备。

**竞品对照**：rustgrid 裸字符前缀跳转带**超时重置**、**连续字符累积匹配前缀**；按键不抢占以免抑制行内编辑器字符输入（`nav-analysis-cellar-rustgrid-sqlab.md:140`，`keymap.rs:176-202`、`sidebar.rs:505-571`）。

**规格草案（v1 增强）**：

1. 状态：`NavViewState` 新增 `prefix_buf: String` + 后台计时句柄（空闲超时建议 800ms，参照 rustgrid 实现取数后再定值；超时清空缓冲并重置计时）。
2. 匹配语义：缓冲作为**名称前缀**（不再只比首字母）；大小写不敏感；从选中行之后循环；缓冲为空时按键回到现有「首字母下一个」行为作为兜底。
3. 字符集：v1 保留 `is_alphabetic()`；数字 / 符号字符是否纳入列入未决项（`nav_jump_prefix` 现状跳过它们，`chrome.rs:978`）。
4. 焦点保护：`Input`（搜索框 / 行内编辑器）持有焦点时直接 return，不抢占字符输入；行内编辑器聚焦路径在 v1 补事件测试（现状注释只声明了搜索框焦点，`chrome.rs:971-972`）。
5. 纯函数化：`nav_match_prefix(order: &[NavOrderItem], buf: &str, start: usize) -> Option<usize>` 抽纯函数，单测钉：累积匹配、循环、无名称行跳过、超时清空、大小写不敏感；菜单 / 键盘断言规范遵循 `gpui-kit-dev` skill「不读 PopupMenu / 内部状态」的既有要求。

**未决拍板 S4-N3**：数字 / 符号字符是否纳入前缀跳转（建议 v1 不纳入，保持与字母漫游一致）；超时阈值 800ms 是否采纳。

### 4.4 S4-N4 菜单能力门控

#### 4.4.1 现状审计（本地事实，逐项）

**连接菜单**（`rows.rs:1643-1928`）：

| 菜单项 | 现状门控 | 证据 |
| --- | --- | --- |
| 连接 / 断开 | 按 `is_connected` 切换文案 | `:1663-1674` |
| 测试连接 | 无条件（独立会话探测，不抢主连接） | `:1675-1694` |
| 编辑连接… | 无条件（本地配置对话框） | `:1695-1701` |
| 数据库可见性… | `disabled(!is_connected)`（唯一一项连接态置灰，注释 S4-③） | `:1702-1713` |
| 查看属性 | 无条件（缓存可渲染） | `:1715-1733` |
| 移动到分组… | 无条件（本地组织元数据动作） | `:1735-1743` |
| 设为主组 ▸ | `!menu_groups.is_empty()` 才摆（结构条件，非连接态） | `:1745-1802` |
| 复制连接（模板）… | `source != Shared` 才摆 | `:1806-1820` |
| 共享至项目 | `source == Global` 才摆 | `:1822-1834` |
| 取消共享 | `source == Shared` 才摆 | `:1836-1848` |
| 删除连接 | 无条件（本地记录删除，缓存保留） | `:1855` 段 |
| 复制名称 | 无条件（剪贴板本地动作） | `:1889` 段 |
| 刷新元数据 | 无条件 | `:1895` 段 |
| 在 SQL 编辑器中打开 | 无条件 | `:1906` 段 |
| 查看洞察 | 无条件（面板导航） | `:1920-1927` |

**对象菜单**（`rows.rs:2268-2455`）：

| 菜单项 | 现状门控 | 证据 |
| --- | --- | --- |
| 查看属性 | `menu_prop` 存在才摆 | `:2270-2293` |
| 查看数据（LIMIT 200） | `data_like && qualified`；**无连接态门控** | `:2294-2313` |
| 生成 SQL ▸ | `data_like && dml_target`；**无连接态门控** | `:2315-2354` |
| 复制名称 | 无条件（本地剪贴板） | `:2356-2365` |
| 复制限定名 | `qualified` 才摆 | `:2367-2377` |
| 刷新元数据 | `menu_path` 才摆；**无连接态门控** | `:2378-2391` |
| 在 SQL 编辑器中打开 | 无条件（开空查询壳） | `:2394-2409` |
| 生成 Mock 数据 | `data_like`；**无连接态门控** | `:2410-2424` |
| 查看统计 | `data_like && request`；**无连接态门控** | `:2427-2434` |
| 结构洞察 | `insight_schema` 才摆；**无连接态门控** | `:2438-2444` |
| 查看洞察 | 无条件（面板导航） | `:2445-2451` |

**动作路径现状**：`ensure_connected_for_browse`（`actions.rs:387-401`）只用于连接根展开（`:360`）、失败重试（`:557`）、定位链路（`:688`）；对象菜单上述六个在线动作**不经过**它，断连时点按会走到任务失败路径（而非菜单层提前置灰）。

**能力位现状**：`CAPABILITY_DICTIONARY` 共 15 键（`capability.rs:155-285`）：`tree / health_check / transactions / index_analysis(NotBuilt) / sql_autocomplete / schema_browser / table_editor(NotBuilt) / analytics / federation / export(App 归属) / mock(App 归属) / resource(App 归属) / ssh_tunnel / ssl_tls / proxy`——`NotBuilt` 是 `Stage`（阶段），`App` 是 `Scope`（应用级归属，与驱动级并列），两个维度不要混读；6 个 `MetaBit`（`Transaction / Streaming / Arrow / Federated / ConcurrentWrite / InMemory`，`:30-37`）；**没有** rename / drop / truncate / table_operations 键。`DriverMeta` 只带 `type_id / name / type_name / type_category`，**不含 capabilities**（`driver_catalog.rs:30-41`）；运行时能力键在引擎内存注册表，导航侧未直接读取。

#### 4.4.2 v1 建议：补连接态门控（纯函数菜单规格）

- 断开时置灰（不隐藏，保持菜单结构稳定，DBeaver `propertyTesters` 不可用项置灰同口径，`nav-analysis-dbeaver.md:57`；我方先例 `rows.rs:1703-1705`）：
  - 对象菜单：查看数据、生成 SQL ▸、刷新元数据、生成 Mock、查看统计、结构洞察。
  - 连接菜单：刷新元数据（断连无元数据可刷；其余无条件项按上表保留：测试连接 / 编辑连接 / 复制名称 / 面板导航类不置灰）。
- 保持可用（断连不禁用）：查看属性（L2 缓存可渲染）、复制名称 / 限定名、在 SQL 编辑器中打开（空查询壳）、查看洞察入口（纯面板导航，不回源库）、连接组织类动作（分组 / 共享 / 删除 / 模板复制，本地元数据动作）。
- 实现纪律：菜单条目抽纯函数（输入 = 节点 kind / qualified / menu_prop / connected / 能力键；输出 = 条目顺序 / 文案 / disabled 布尔），渲染只挂事件；单测钉状态矩阵（连接态 × 对象类型 × 门控条件），不读 `PopupMenu` 内部（`gpui-kit-dev` skill 窗口测试节既有规范；项目内 `project_menu_entries` + `menu_spec_*` 先例）。

#### 4.4.3 v2 建议：驱动能力位门控（挂对象 DDL 批次，不与 v1 同批）

- 先扩 `CAPABILITY_DICTIONARY`（新增 `table_operations` / `rename` / `drop` / `truncate` 等键，`Stage::NotBuilt` 起步，`capability.rs:129-135` 口径）；运行时读取路径二选一：扩 `DriverMeta` 能力字段，或经 `DriverRegistry::all_descriptors()`（`driver/registry/mod.rs:163`）读描述符现有 `capabilities: Vec<String>`（`driver/registry/descriptors.rs:185`）——两条路径与字典键的映射口径在 v2 对齐时定稿，本批不预判。现有派生先例：网络三键由描述符 `supports_ssh_tunnel` / `supports_ssl` / 代理布尔位经 `DriverDescriptor::capability_keys` 派生（`capability.rs:256-258` 注释），v2 可照此形态扩写操作类键。
- 竞品参照：rustgrid `DriverDescriptor.capabilities` 控制分类行 / 菜单 / 标签页显隐，不支持过程的引擎不渲染 Functions 分类（`nav-analysis-cellar-rustgrid-sqlab.md:159,164`，`descriptor.rs:1-90`、`driver.rs:39-58,81-101`、`tree.rs:1032-1037`）；blanco 按方言 `supports_table_operations` / `inspects_objects` / `supports_process_list` 门控（`nav-analysis-fluxdb-blanco.md:17` 总述；方法锚点 `:166` `supports_process_list`、`:168` `inspects_objects`、`:196` `supports_table_operations`）；DBeaver `propertyTesters` 矩阵（`canOpen / canRename / canDelete / canMoveUp / canFilter / hasFilter…`，`nav-analysis-dbeaver.md:57` 段）。
- v1 不引入能力位门控的理由：当前对象菜单动作全部是只读导航 / 面板入口，断连门控已覆盖真实风险；能力位门控的价值要等写操作（DDL）出现才成立，与 S4-N5 / N6 同批立项。

### 4.5 S4-N5 行内重命名

**现状证据**：

| 对象 | 现状 | 证据 |
| --- | --- | --- |
| 分组 | 行内重命名：`group_rename_for: Option<String>` 状态 + `commit_group_rename` 直接提交 | `nav_view.rs:317-318`；`editors.rs:381`；入口 `rows.rs:861-867` |
| 标签 | 行内编辑器，覆盖式逗号分隔直接保存 | `editors.rs:158,251` |
| 复制模板 | 行内编辑器 + `commit_copy_connection` / `cancel_copy_connection` | `editors.rs:191,350,375` |
| 连接 | `NavRename`（F2）仅匹配 `NavPath::Connection` → `host.edit_connection` 对话框；表 / 视图等数据库对象重命名**完全缺失** | `chrome.rs:1002-1008`；`nav_host.rs:206` |
| 驱动 DDL 通道 | 三个 trait 职责分离：`MetadataBrowser`（`:201-325`，内省方法 + 类别能力位 + `current_catalog`）；`Transaction`（`:425-434`，`query` :427 / `commit` :430 / `rollback` :433）；`Database`（`:438-706`，`query` :442 / `query_with_params` :453 / `query_with_cancel` :463 / `begin_transaction` :470 / `ping` :479）；**三者均无独立 rename / drop / truncate 方法**；写操作只能走 `query` 原始 SQL 路径 | `traits.rs:201-325,425-434,438-706` |

**竞品对照**：

- DBeaver `INLINE_RENAME_ENABLED = false`：行内编辑器代码存在但默认关闭，实际走模态 `EnterNameDialog` + `DBEObjectRenamer` 命令链 + 可选脚本预览 + `TasksJob` 后台保存，`supportsRename` 判定（`nav-analysis-dbeaver.md:71,225`，`DatabaseNavigatorTree.java` 开关）。
- blanco：表重命名走带 SQL 预览的弹窗；只读 / 生产环境 DDL 二次确认提示（`nav-analysis-fluxdb-blanco.md:51`，`connections.rs:796-891,821-826`）。
- tusk：行内重命名 Input 替换行，Enter / Blur 提交进 pending 队列（`nav-analysis-zqlz-tusk.md:124,198`）；rustgrid F2 行内重命名分组行与表行，连接档案走对话框（`nav-analysis-cellar-rustgrid-sqlab.md:139`，`sidebar.rs:573-579`、`objects.rs:2217-2225`、`dialogs.rs:853`）；zqlz 表 / 视图重命名走独立模态 `RenameWindow`（`nav-analysis-zqlz-tusk.md:42`，`rename_window.rs:6,107,195,275`）。

**建议**：

- v1 **维持现状**：不新增对象行内重命名；分组 / 标签 / 复制模板的行内直接提交模型不变；连接重命名保持对话框路径。
- 对象 DDL 重命名随未来「对象编辑批次」一并立项，前置条件：① 驱动能力契约（renames 支持判定，对齐 4.4.3 v2）；② staged 草稿模型（S4-N6）；③ 确认弹窗 + SQL 预览 + 只读 / 生产环境二次确认（blanco 参照，`nav-analysis-fluxdb-blanco.md:51`）；④ 不复制 DBeaver「默认关闭行内」结论到我方——我方分组 / 标签行内编辑已是既定交互，区别只在源库写操作需要确认与草稿。

### 4.6 S4-N6 staged 提交模型

**现状证据**：

- 四个直接提交模型：分组表单 `render_group_editor:13` / `save_group_form:288` / `delete_group:336`；标签 `render_tag_editor:158` / `commit_nav_tags:251`；复制模板 `render_copy_editor:191` / `commit_copy_connection:350` / `cancel_copy_connection:375`；分组重命名 `commit_group_rename:381`（`editors.rs`）。
- `staged` / `undo` / `redo` 在 `crates/database/src` 全量零命中：无草稿集合、无撤销栈、无行级着色状态。现存 `dirty`（`state_dirty` 防抖落库标记，承载选中 / 搜索词，`actions.rs:1199-1209,1292-1302`）与 `pending`（后台任务计数、`nav_pending_scroll` 待定位滚动键，`actions.rs:746,1252`）均不是对象编辑草稿缓冲，与 tusk staged 模型不是一回事。

**竞品对照**：tusk 待提交事务模型——重命名 / 删除 / 网格编辑先 staged，行级着色（绿 = 新增草稿、蓝 = 改名、红 = 删除线），`⌘S` 统一提交，带历史撤销；行内重命名 Enter / Blur 进 pending 队列（`nav-analysis-zqlz-tusk.md:124,196-200,210,221`，`app_objects.rs:1-516,690-728`、`app_edit.rs:817-869`、`app.rs:3075-3147`、`actions.rs:292-297`、`app.rs:274-275`）。

**建议（边界判断，不是现在开工）**：

- v1 **不引入全局 staged**。本地组织元数据（分组 / 标签 / 归组 / 排序 / 连接重命名 / 复制模板）保持直接提交：它们写本地 SQLite、失败可重试、无服务端不可逆语义；引入草稿缓冲只会增加状态复杂度。
- staged 边界只画给**未来源库对象 DDL 草稿**（rename / drop / truncate / 网格编辑回写）；届时随对象编辑批次定义：草稿集合结构、行级着色 token（新增 / 改名 / 删除三态，颜色走 rds-theme 三处同步：`product-tokens.json` → `product_tokens.rs` → 访问器，禁止裸 hex）、提交入口（`⌘S` 还是显式按钮）、撤销栈深度、部分失败回滚口径。
- 本期只在本稿预留边界与对照证据，不写状态结构、不铺提交入口、不铺着色。

### 4.7 与旧 S4 §2.6 真机清单的关系

- 旧清单 19 条（`database-nav-dev-plan.md:459-490`）继续作为鼠标 / 拖拽 / 行内编辑器走查依据，本稿 N1 / N5 的遗留项回挂该清单对应条目，不重复登记。
- 本稿 N3 / N4 v1 完成后，旧清单第 4 / 5 条（右键菜单可用性）按新门控矩阵复跑一遍。

---

## 5. 跨议题工程约束与批次切分建议

### 5.1 工程纪律（每批执行）

1. `render` 纯读：新状态在事件路径更新 + `cx.notify()`；后台任务走 `nav_jobs` 入队 + 结果队列回填。
2. 纯函数优先：规则匹配、菜单条目规格、前缀匹配、估计值格式化都抽无状态纯函数，单测钉矩阵；视图层只挂事件。
3. 颜色 / 尺寸：新增徽标颜色先确认主题 JSON 已定义，代码零裸 hex；结构尺寸登记 `workbench_shell/src/ui.rs`；契约测试 `cargo test -p rds-workbench --test ui_contract`。
4. 测试骨架：窗口测试不用 `use gpui_kit::*` 通配导入；菜单断言不读 `PopupMenu` 内部；键盘激活在 KeyUp 上断言业务状态真的变了（`gpui-kit-dev` skill 窗口测试节）。
5. 每批独立：`cargo check --workspace --all-targets -j 2` → 单测 → 简体中文提交信息；批次之间不跨改未授权模块。
6. 过程产物：探针 / 验证日志 / 一次性脚本落 `.rds/tmp/` 或 `tools/_archive/`；竞品研究报告进 `docs/analysis/`；架构变更回写 `database-navigator-architecture.md` §11 权威清单。

### 5.2 建议批次顺序（依赖关系自上而下）

| 批次 | 内容 | 前置闸门 | 交付物 |
| --- | --- | --- | --- |
| B-2-0 | 真机并发验证（V1–V5 探针 + 报告） | 本稿 §3.3 | 验证报告（`docs/analysis/`）；通过才进 B-2-1 |
| S3-④ v1 | 统计估计后台回填（PG / MySQL；SQLite / DuckDB 留空；L2 回写；占位 + 徽标） | 无 | 任务族 + 纯函数 + 驱动矩阵单测 + 真机四库走查 |
| B-1 v1 | per-connection + 全局 glob include/exclude mask；树路径与搜索命中叠加；对话框与面板头入口；持久化 | 无 | `nav_mask_passes` 规则矩阵单测；settings / NavHost / workbench 三层接线单测 |
| S4-N3 v1 | 前缀累积匹配 + 超时重置 + 焦点保护 + 纯函数单测 | 无 | 键盘漫游单测矩阵 |
| S4-N4 v1 | 对象 / 连接菜单连接态置灰 + 菜单规格纯函数 + 状态矩阵单测 | 无 | 菜单条目规格单测 |
| B-2-1 | per-conn 信号量 + 全局软帽（仅 B-2-0 报告通过后） | B-2-0 通过 | 调度改造 + 并发实测记录回写架构文档 |
| 后续 v2（另立） | 两段 mask 匹配；视口翻页；驱动能力位门控；对象 DDL 重命名 + staged；多选；按连接 family 批量取消 | 各自前置条件 | 另行立项书 |

### 5.3 明确不在本稿范围

- 连接池恢复 / 全局池扩容（`factory.rs:138-159` 决策维持不变）。
- 真 `COUNT(*)` 上树；文件夹子对象计数补填；行估计参与排序。
- 多选模型；拖拽批量落点；命令面板 / 全选等 rustgrid 重功能。
- 100ms 事件批量合并（触发条件见 2.3.5）。
- 旧 S4 §2.6 清单的自动化鼠标模拟（仓库既有结论：`simulate_click` 全套不可靠，走真机，`database-nav-dev-plan.md:461-463`）。

---

## 6. 待拍板决策汇总

| 编号 | 议题 | 待决问题 | 我方建议 | 阻塞 |
| --- | --- | --- | --- | --- |
| S3-④-A | 回填 | v1 默认开启 / 关闭 | 默认开、可关 | v1 落码 |
| S3-④-B | 回填 | 表数阈值才排队估计 | 阈值 20 张 | 任务粒度 |
| S3-④-C | 回填 | 估计值回写 L2 | 回写，刷新作废重取 | 缓存失效设计 |
| S3-④-D | 回填 | 误差口径 tooltip | 注明弱色说明 | 文案 / token |
| S3-④-E | 回填 | v2 真 COUNT 入口 | 否，本期不预留 | 无 |
| B-1-A | 过滤器 | glob / 正则 v1 是否并存 | v1 仅 glob，正则挂 v2 | 语义模型 |
| B-1-B | 过滤器 | include 多模式行 vs 单条表达式 | 多行 glob 列表 | 对话框规格 |
| B-1-C | 过滤器 | 全局 mask 持久化范围 | 跨项目面板偏好 | settings 结构 |
| B-1-D | 过滤器 | mask 是否作用列层 | 不作用 | 管线插入点 |
| B-1-E | 过滤器 | 断连连接 mask 行为 | 照常过滤缓存对象 | 边界说明 |
| B-2-A | 分片并发 | 全局软帽 K | 起步 3，V3 复测 | 调度改造 |
| B-2-B | 分片并发 | Props 与 Children 共用 per-conn 信号量 | 共用 | 调度模型 |
| B-2-C | 分片并发 | 预热让位：忙即跳 vs 短等 | 忙即跳 | 任务行为 |
| B-2-D | 分片并发 | JDBC / wasm 纳入矩阵时点 | v1 排除 | 探针范围 |
| B-2-E | 分片并发 | 旧池化死代码清理时点 | 另立批次 | 无 |
| S4-N3 | 前缀 | 数字 / 符号字符是否纳入 | v1 不纳入 | 匹配语义 |
| S4-N3 | 前缀 | 超时阈值 | 800ms，取 rustgrid 实测值后定稿 | 状态设计 |
| S4-N4 | 门控 | v1 置灰清单确认（4.4.2 表） | 按表执行 | 菜单规格 |
| S4-N5 | 重命名 | 对象重命名是否提前到 v1 | 否，随 DDL 批次 | 无 |
| S4-N6 | staged | v1 是否引入草稿缓冲 | 否，仅预留边界 | 无 |

---

## 7. 取证索引

### 7.1 本地源码（行号以本稿复核为准）

| 主题 | 文件:行号 |
| --- | --- |
| 视图状态全字段 | `crates/database/src/nav_view.rs:220-341` |
| 搜索开关 / 签名 / 序号 | `crates/database/src/nav_view.rs:272-284`；`crates/database/src/nav_view/chrome.rs:783-872` |
| `NavOrderItem.name` | `crates/database/src/nav_view.rs:376-385` |
| facet 过滤纯函数 | `crates/database/src/nav_view.rs:448-507` |
| 键盘动作实现 | `crates/database/src/nav_view.rs:636-726` |
| 键盘接线 | `crates/database/src/nav_view/chrome.rs:915-1008` |
| 命令定义 / 按键绑定 | `crates/database/src/commands.rs:27,29-35`；`crates/app/src/main.rs:284,286-289` |
| 行序同步 / 分组头菜单 / 渲染入口 | `crates/database/src/nav_view/rows.rs:128-181`；分组头 `:825-954`（重命名项 `:861-867`）；`render_connection_row:1076`、`render_nav_node:2001` |
| 连接 / 对象行右键先选中 | `crates/database/src/nav_view/rows.rs:1611-1624`、`:2218-2231` |
| vendor 右键菜单（只 stop_propagation + 开菜单，不选中） | `vendor/gpui-component/src/menu/context_menu.rs:403-405` |
| 连接菜单 / 对象菜单 | `crates/database/src/nav_view/rows.rs:1643-1928`、`:2268-2455` |
| 分区 / 继承徽标 | `crates/database/src/nav_view/rows.rs:2148-2166` |
| 行内编辑器与直接提交 | `crates/database/src/nav_view/editors.rs:13,158,191,251,288,336,350,375,381` |
| 搜索命中合成节点 / 测试夹具 `None` | `crates/database/src/nav_view/primitives.rs:521,1015`；`crates/database/src/nav_view/tests.rs:263,1439,1556,1632,1850` |
| 搜索防抖 / 签名 / seq | `crates/database/src/nav_view/actions.rs:189-236,242-293` |
| 连接补建连辅助 | `crates/database/src/nav_view/actions.rs:360,557,688,387-401` |
| 面板状态防抖落库 / 待定位滚动 | `crates/database/src/nav_view/actions.rs:746,1199-1209,1252,1292-1302` |
| 任务枚举 / 计数 / 优先级 | `crates/database/src/nav_jobs.rs:276-374,389-395,526-571` |
| 三信号量与任务归属 | `crates/database/src/nav_jobs.rs:41-72,133,139,150,161,164,170,855,959,987,1011,1032,1061,1095-1128` |
| worker / 线程模型 / 建连等待 | `crates/database/src/nav_jobs.rs:787-836,861-876` |
| 搜索命中精筛点 | `crates/database/src/nav_jobs.rs:1238-1270` |
| 内省服务与系统过滤 | `crates/database/src/navigator_service.rs:44,87-114,706,1027-1088` |
| 导航模型 | `crates/database/src/model.rs:129,143,167-195` |
| trait 内省 / 详情 / 事务 / 连接 | `crates/engine/src/driver/traits.rs:125-138,145-151,186-191,201-325,425-434,438-706` |
| 驱动单连接 Mutex | `crates/engine/src/driver/native/postgres.rs:53,376-393`；`mysql.rs:56`；`sqlite.rs:245`；`duckdb.rs:214` |
| 六驱动 `get_table_detail` 未回填行估计（全 `None`） | `native/postgres.rs:1520`、`native/postgres_native.rs:1178`、`native/mysql.rs:1563`、`native/mysql_native.rs:995`、`native/sqlite.rs:1212`、`native/duckdb.rs:1254` |
| 弃池决策 | `crates/engine/src/driver/factory.rs:138-159` |
| 连接管理器 | `crates/engine/src/connection_manager.rs:114,130,136,144` |
| L1 缓存全局锁 | `crates/engine/src/cache/cache_manager.rs:66-73,83,104-118` |
| L2 统计同步写值链路 | `crates/engine/src/persistence/metadata_cache.rs:1836,2321,3058,3073,3087` |
| L2 / 索引迁移列 | `migrations/connection_metadata/004_refactor_to_normalized.sql:43`；`006_add_metadata_index.sql:30,33` |
| 能力字典 / 位 / 阶段 | `crates/engine/src/driver/capability.rs:30-37,116-135,155-285` |
| 驱动目录元数据 | `crates/engine/src/persistence/driver_catalog.rs:30-41` |
| 驱动注册表 / 描述符能力串 | `crates/engine/src/driver/registry/mod.rs:90,163`；`crates/engine/src/driver/registry/descriptors.rs:149-194,185` |
| PG 属性面板 reltuples | `crates/database/src/property_providers/pg.rs:227,233-248,669` |
| 网格行数口径 | `crates/database/src/table_data.rs:11-12` |
| 设置模型 | `crates/settings/src/model.rs:359-409` |
| NavHost 端口 | `crates/database/src/nav_host.rs:51-61,79,121-122,144-152,157,160,163,206,247` |
| workbench 互转先例 | `crates/workbench/src/components/nav_host.rs:129-149,201-216` |
| 导航 UI 常量 | `crates/workbench_shell/src/ui.rs:118,123,125,130,140,151-155` |
| 真机并发探针参照形态 | `crates/engine/src/bin/pg_probe.rs:1-62`（端点 `:15`、独立 Runtime `:53`、入口 `main:50`） |

### 7.2 竞品分析文档（`docs/analysis/competitor-analysis/`，gitignore）

| 主题 | 文档:行号 |
| --- | --- |
| DBeaver 四套搜索机制 / TreeFilter / 事件合并 / 过滤器配置 / 行内关闭 | `nav-analysis-dbeaver.md:14,16,57,61,67,71,75,102-115,146,174,178,219,225,228` |
| rustgrid 两阶段 / 键盘 / 描述符能力门控 / 分级建议 | `nav-analysis-cellar-rustgrid-sqlab.md:139,140,141,151,159,164,174-181,189,320` |
| tusk staged / 右键先选中 / 行内重命名；zqlz 模态重命名 | `nav-analysis-zqlz-tusk.md:42,124,196-200,210,221` |
| navop 会话复用池 / 朴素并发负向证据 / manifest 动作 | `nav-analysis-navop.md:62,65,72,90` |
| blanco 池化 / 无队列负向证据 / 方向键 / 能力门控 / 重命名弹窗 | `nav-analysis-fluxdb-blanco.md:17,51,139-140,142,166,168,196,210,213` |

### 7.3 本仓架构文档与计划先例

| 主题 | 文件:行号 |
| --- | --- |
| 导航模块纪律（§4 第 1 条 render 零 I/O、第 5 条缓存只增不删） | `docs/architecture/database/README.md:94,98` |
| 权威待办 #8（facet 为全局 UI 偏好，不进 `navigator_state`） | `docs/architecture/database/database-navigator-architecture.md:390` |
| 权威待办 #14（系统库 / 系统 schema 过滤） | `docs/architecture/database/database-navigator-architecture.md:396` |
| 权威待办 #23（搜索结果在树中定位 + 可编程滚动入口先例） | `docs/architecture/database/database-navigator-architecture.md:405` |
| 权威待办 #25（V12 筛空空态先例） | `docs/architecture/database/database-navigator-architecture.md:407` |
| 旧 S4 真机清单 §2.6（19 条，与本稿 S4-N 编号显式区分） | `docs/architecture/database/database-nav-dev-plan.md:459-490`；`simulate_click` 不可靠记载 `:461-463`；右键连接行 `:472`、右键分组头 / 对象行 `:473` |
