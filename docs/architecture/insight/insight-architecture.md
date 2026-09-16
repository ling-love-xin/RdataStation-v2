# 洞察模块（M8）· 设计理念与架构

> 状态：**设计定稿 + Phase 0 已落地**（2026-09-15） · 关联文件：`README.md`（模块入口）、`insight-prototype-design.md`（原型）、`insight-dev-plan.md`（开发方案与进度）、`insight-user-guide.md`（使用手册）
> 本文回答**为什么这样设计 / 怎么运转**：概念模型 → 不变式 → 分层与归属 → 状态所有权 → 数据流 → 决策表 → 并发 → 降级 → 测试 → 实现映射 → 已知问题（权威）。
> 现状口径：**后端算法与规则体系已完整落地并有测试覆盖；视图层尚未开始**（Phase 1 起，见开发方案 §5）。

## 1. 定位与边界

M1~M5 解决「连得上、看得见、查得动」；**M8 解决「这份数据能不能用、哪里有问题」**——把数据变成结论。

能力分四层，由内向外：

| 层 | 能力 | 输入 | 输出 |
| --- | --- | --- | --- |
| L1 描述统计 | 列画像：类型识别 → 统计量 → 分布 → 样本 | DuckDB 临时表 | `ColumnInsightFull` |
| L2 质量判断 | 四维加权评分（完整性 .35 / 唯一性 .25 / 类型一致 .20 / 分布 .20）→ 单列分 → 表聚合分 | L1 结果 | `QualityScore` / `TableQuality` |
| L3 关系与结构 | 多列规则（相关性 / 交叉表 / 分组）、Schema 洞察（外键推断 / 类型不一致 / 孤立表 / 冗余列 / 健康分） | 源库 + 规则 | `SchemaInsightReport` |
| L4 规则引擎（横切） | TOML 声明式：SQL 模板 + 输出映射 + 质量门控 + 渲染提示 | 规则文件 | `ExecutionResult` |

**L4 是本模块的架构特色**：把「新增一种洞察」从「改 Rust + 重编译」降级为「加一个 `.toml`」，且用户可覆盖内置规则。这也是它区别于「一堆统计函数」的地方。

**边界**：做画像 / 评分 / 规则 / 报告 / 快照历史；**不做** SQL 执行与结果集（M5）、对象树与内省（M4）、连接与只读策略（M3）、Mock 生成（M7）、资源目录（M6）、图表可视化（洞察只出 `RenderHint`，渲染归 M5/M6）。

## 2. 概念模型与不变式

### 2.1 五个概念

| 概念 | 含义 | 载体 |
| --- | --- | --- |
| **分析目标** | 一次分析的对象：列 / 表 / 多列集合 / Schema。**没有「全局洞察」** | 视图层选定 |
| **规则** | 声明式分析单元：SQL 模板 + 参数 + 输出映射（+ 质量门控 + 渲染提示） | `.rule.toml` 文件 |
| **规则作用域** | 规则的三层归属，决定可见范围与覆盖优先级 | `RuleScope` |
| **画像** | 对目标的统计结论（`ColumnInsightFull` / `TableQuality` / `SchemaInsightReport`） | 内存值 |
| **快照** | 画像的持久化副本 + 版本链 | 项目 `analytics.duckdb` + `project.db` |

### 2.2 八条不变式

1. **一次分析 = 一个目标 + 一份结论**——面板头部永显当前目标，杜绝「这数是哪来的」。
2. **结论可追溯**——每个数字来自统计量、评分维度或规则之一；UI 不发明结论。
3. **规则正文以文件为唯一真相源**——库表只存索引与状态（`checksum` / `enabled` / 校验结果），正文永不入库。
4. **同名整体覆盖**——`meta.id` 相同即视为同一条规则，后加载层整体替换前者；**不做字段级合并、不加命名空间前缀**。
5. **单条规则失败不连坐**——解析失败只标记该条并记录错误原文，其余规则照常可用（含「坏规则必须能被看见」，见 D6）。
6. **启停在覆盖之后应用**——禁用针对「最终生效的那一条」，与它来自哪一层无关。
7. **采样必须明示**——任何基于采样得出的结论，界面必须标出采样行数（表探查与批量评估后端为 `LIMIT 500`）。
8. **快照正文与元数据成对写入**——不允许「只写一半也算成功」的静默降级。

## 3. 分层与 crate 归属

```
workbench ──► insight ──► engine ──► shared
   │              │           ▲
   └──────────────┴───────────┘
```

| 层 | 职责 | 不负责 |
| --- | --- | --- |
| `insight` | 规则引擎、分析算法、规则索引、洞察服务的装配与编排 | 连接管理、SQL 执行、用户 SQL |
| `engine` | DuckDB 单例与临时表、SQL 执行、项目库/全局库连接池与迁移 | 洞察语义 |
| `workbench` | 结果集服务（`re_execute_with_filter` / `execute_duckdb_analysis` / `export_result`）、Dock 装配 | 洞察算法与规则 |

**已知的归属偏差（Phase 0 / 0.2 待归位）**：

> ✅ **已于 2026-09-15 完成归位**（Phase 0 / 0.2）。下表保留为「当时为何要搬」的记述：

| 原位置 | 现位置 | 理由 |
| --- | --- | --- |
| `engine::persistence::insight_types`（16 个领域类型） | `insight::model::types` | 领域词汇不属数据层 |
| `engine::persistence::insight_{store,meta_store}`（正文 + 元数据仓库） | `insight::store::{body, meta}` | 洞察快照的领域持久化 |
| `engine::services::duckdb_service::detect_extremes` | `insight::insight_engine::detect_extremes` | 唯一调用方是洞察且返回洞察类型（曾是**方向倒置**） |
| `workbench::services::{result,persistence}_service` 的洞察部分 | `insight::service::{InsightService, persistence}` | 业务服务不复住在壳层 |

engine 侧只剩**连接与迁移**（`ProjectDatabaseManager` 及其 SQLite/DuckDB 句柄），不再认识「洞察」。

> **视图归属尚待拍板**（方案 A 入 `crates/insight`，方案 B 留 `workbench`），见开发方案 §3.1。

**当前 workspace 内 Cargo 依赖（Phase 0 / 0.2 后）**：

| crate | 依赖 |
| --- | --- |
| `rds-insight` | `engine`, `shared`, `tokio`, `serde`, `serde_json`, `specta`, `uuid`, `include_dir`, `duckdb`, `tracing`, `toml`, `rusqlite`, `sha2` |
| `rds-engine` | 不再依赖洞察；`sha2` / `uuid` 仍自用于元数据身份与缓存 |
| `rds-workbench` | 输出 `insight`（只用了监听与门面）；已移除仅 `persistence_service` 用过的 `sha2` |

视图落地后按拍板结果决定是否给 insight 加 `gpui-kit`。

## 4. 状态所有权（单一写入者）

| 状态 | 唯一所有者 | 可写者 | 说明 |
| --- | --- | --- | --- |
| 规则**正文** | 文件系统 | 用户 / 外部编辑器 | 三层目录 |
| 规则**索引列**（`checksum` / `load_status` / `source_path` / 分类与展示名） | `insight_rule_index` | **仅**索引同步器 | 界面不得直接改 |
| 规则**启停**（`enabled`） | 同上 | **仅**用户动作（`RuleIndexStore::set_enabled`） | 唯一的用户写入口 |
| **规则集**（已应用覆盖与启停） | 进程内 `registry_for` 缓存 | `registry_for` 首次构建 / `reload_insight_rules` / `apply_disabled_rules` 失效 | 按项目根键控 |
| **禁用集合**（同步可读） | `DISABLED_RULES` 静态缓存 | `apply_disabled_rules` | 见 D9/D10 的异步-同步桥 |
| **快照正文** | 项目 `analytics.duckdb` | `ProjectInsightStores::save_column_snapshot` | — |
| **快照元数据 + 版本链** | 项目 `project.db` | 同上（**成对写入**） | 不允许单独写 |
| **面板状态** | Phase 1 视图 | 视图自身 | — |

两条红线：**界面不改索引表除 `enabled` 外的列**；**代码不硬编码规则**。

## 5. 数据流

### 5.1 规则装配（读路径）

```
registry_for(project_root)
  ├─ 缓存命中 → 直接返回
  └─ 未命中 → build_registry：
       ① builtin_registry()            内层：include_dir 编译期嵌入（18 条，只读）
       ② get_system_dir() → {system}/insight-rules/    全局层（跨项目）
       ③ get_project_rules_dir(root) → {项目}/.RSmeta/insight-rules/   项目层
          · 每层 load_from_dir(dir, scope)：同名 insert 覆盖 + 记录 RuleSource + 记录失败
       ④ 日志逐条播报 failures
       ⑤ apply 禁用：disabled_snapshot(key) → registry.remove_rules(&set)
       ⑥ 返回（缓存）
```

`with_rules(project_root, f)` 是服务层统一入口：查缓存 → 加读锁 → 执行闭包，避免各处重复「取表 → 加锁 → 解引用」样板。

### 5.2 规则索引同步（写路径）

```
sync_project_rules(root, global_store, project_store, builtin_ids)
  对每一层：
    scan_scope_dir(dir, scope)         读磁盘：SHA256 + 试解析 → 索引行
                                       （★ 解析失败的文件也进索引，用文件名兜底 id）
    plan_index(scanned, store.load(), builtin_ids)   合并：
                                       · 磁盘为准（来源/校验和/状态）
                                       · enabled 以库中为准（用户决定）
                                       · 库中多出的行：id 属内置 → 抑制记录，原样保留
                                                        否则 → 转 missing 保留
    store.replace(planned)             单事务整体替换（清空 + 写入）
  → 汇总 SyncOutcome
  → apply_disabled_rules(root, 禁用 id 集)
       → 更新 DISABLED_RULES + 使该项目注册表缓存失效
       → 下次 registry_for 按新启停集合重建
```

**为什么整体替换而非逐行 upsert**：索引是磁盘现状的投影，逐行合并会留下「已删除文件」的陈旧行；事务保证界面看不到中间态。

### 5.3 列画像计算

```
get_column_insight_full(registry, temp_table, column)
  ① 并发令牌（上限 4，失败即返回可读错误）
  ② 取 DuckDB 单例连接并持锁
  ③ get_column_insight_full_on(registry, conn, ...)
       ├─ get_column_stats_internal  → 行数 / 空值率 / 唯一值
       │     └─ 按类型分派 compute_{numeric,text,datetime,boolean}_stats
       │           └─ ★ 每个都是「按 id 取 TOML 规则」执行（numeric-stats / text-* / …）
       ├─ get_column_sample_internal → 样本（DEFAULT_SAMPLE_SIZE = 5）
       └─ get_column_histogram_internal → 数值列直方图（行数 ≥ HISTOGRAM_MIN_ROWS = 10）
```

> **关键含义**：**基础统计也是规则驱动的**。所以「规则分层随项目变化」这件事会一直影响到列画像的最内层——这正是规则集必须**显式传入**、不能取进程级全局的原因（D10）。

**面板侧的编排**（Phase 1 第二批落地）：宿主把目标交给
`InsightService::profile_column_view(project_root, temp_table, column)`，它一次完成
「取当前项目的规则集 → 上面的计算链 → 映射为视图模型 `ColumnProfileView`」；
失败时用 `InsightService::describe_error(&err)` 得到「给人看的文案 + 是否可重试」——
**不把 `CoreError` 的 `Display` 直接展示给用户**（它带 `[code]` 内部错误码前缀）。

> 阻塞语义：整条链要抢 DuckDB 全局锁与并发配额（D12 / D13），因此**宿主负责把它放到
> 后台线程**，算完再用 `InsightView::set_profile` / `set_error` 回填（渲染路径零 I/O）。
> 识别失败原因是按**消息内容**匹配的权宜——引擎侧还没有 typed error。

### 5.4 快照保存与版本链

```
ProjectInsightStores::save_column_snapshot(insight, entity_source, row_count, elapsed_ms)
  ① meta.get_latest_meta("column", column_name) → parent_version_id（首次为 None）
  ② storage.columns.save_snapshot(insight, parent) → (snapshot_id, version_id)
        · 版本数超 MAX_VERSIONS_PER_COLUMN 时先淘汰最旧
        · 正文序列化 → 存 insight_column_snapshots
  ③ snapshot_checksum(insight) → 写元数据 insight_snapshots（含 entity_source / 版本链）
```

读取侧排序统一为 `ORDER BY created_at DESC, rowid DESC`（D18）。

### 5.5 表级评估

```
batch_evaluate_columns(root, conn_id, db, schema, table)
  ① 采样 LIMIT 500 → 建 DuckDB 临时表
  ② with_rules(root, |reg| { 逐列 get_column_insight_full(reg, ...) })  ← 规则集只取一次
  ③ compute_table_quality(table, &stats_list) → 最差列排在前的加权聚合分
```

**串行逐列是刻意的**：并发上限为 4 且快速失败，并行会把「部分列静默缺失」变成常态；串行虽慢，但「哪些列没评上」是确定的（单列失败记日志并继续）。

### 5.6 规则热加载（目录监听）

```
RulesWatcher（后台线程，drop 即停）：
  起线程前：baseline = fingerprint(watch_dirs(watched_project_root()))
  每轮（默认 2s）：
    root = watched_project_root()            ← 宿主在项目打开/切换时更新，故自动跟随
    fp   = fingerprint(watch_dirs(root))
    fp == last_fp && root == last_root  → 继续
    否则：reload_insight_rules(root) + index_is_stale = true
```

两条设计要点：
- **内容哈希而非事件**：变更判定不看事件、只看内容哈希，因此「编辑器保存 / 工具重写 / 连写多次」都只产生一个结论，无需防抖窗口（D22）。
- **监听不碰数据库**：它只重载规则集（立即影响分析），索引表由 `sync_project_rules` 刷新；两者靠 `index_is_stale()` 解耦——**分析正确性不依赖数据库**，索引的刷新时机与「谁在看它」对齐。

## 6. 决策表

| # | 决策 | 理由 | 代价 / 备注 |
| --- | --- | --- | --- |
| D1 | 分析粒度**随目标分派**，一个面板渲染四路内容 | 四套面板会各自演化出不一致的交互；目标决定展示是天然正交的 | 视图分支较多 |
| D2 | 规则**三层作用域**（`Builtin` / `Global` / `Project`） | v1 只有两层，跨项目复用只能手工复制文件；三层与连接/环境/认证的既有作用域范式对齐 | 需维护三处目录 |
| D3 | 同名**整体覆盖**，无字段级合并、无命名空间前缀 | 字段级合并会让「这条规则到底怎么跑」需要拼脑；前缀命名空间让用户无法真正「改掉」内置规则 | 覆盖需整份照抄 |
| D4 | 规则**正文不入库**，库只存索引与状态 | 正文留在文件才能 diff / 进 git / 手工编辑；入库会丢失这些能力 | 需同步器维持两者一致 |
| D5 | **内置规则可禁用**（项目库写抑制记录） | v1 只能伪造同名覆盖文件，而 `deny_unknown_fields` 要求逐字段照抄，极易写坏 | 需区分「抑制记录」与「已删文件」（D6 的 `builtin_ids`） |
| D6 | 索引**必须覆盖解析失败的文件**（用文件名兜底 id） | 索引的价值恰在把「静默跳过的坏规则」变可见；只扫注册表则坏规则依旧不可见 | 兜底 id 不是真 `meta.id`，界面需提示 |
| D7 | 文件删失**转 `missing` 保留**，不删索引行 | 无声消失比显式提示差；`missing` 让界面能说「规则文件不见了」 | 索引行会随删除累积（可由清理动作兜底） |
| D8 | 启停在**覆盖之后**应用 | 禁用针对「最终生效的那一条」；在覆盖前应用会被上层覆盖「复活」 | — |
| D9 | 注册表**按项目根缓存**，非进程单例 | v1/v2 一期的进程单例把「切项目要重载」变成调用方义务，v2 一期直接漏掉 → 用户规则静默不生效 | 需在项目关闭时清缓存 |
| D10 | 规则集**显式传入**分析函数（`&RuleRegistry`） | 基础统计本身由规则驱动，取全局会在切项目时用错规则 | API 面变大（调用方都要拿注册表） |
| D11 | 禁用集合走**同步可读静态缓存**（异步侧推、同步侧读） | 分析链路是同步的（`with_rules` → `get_column_insight_full`），为一份很小且变动很少的集合把全链路改异步不值 | 首次访问可能先于同步，属可接受时序 |
| D12 | 并发上限 **4 且快速失败**（`try_acquire`） | 同步函数内无法 await，阻塞等待会把 UI 线程卡住 | 批量场景须由调用方串行化 |
| D13 | DuckDB 用**单例连接**（非连接池） | 临时表在同一进程内全局可见，跨调用链可直接读到 | 所有查询全局串行（`Mutex`） |
| D14 | 内部接缝 `*_on` / `*_internal` **只吃显式连接** | `std::sync::Mutex` 非重入：已持锁的场景再调公开入口会自锁 | 调用方需选择入口 |
| D15 | 采样行数**必须在 UI 明示** | 采样结论被当成全量结论是最常见的误读 | 需视图配合 |
| D16 | 快照正文与元数据**成对写入**，失败即抛 | 只写一半会造成「历史列表有记录但读不出正文」 | 需补偿或重试策略（Phase 5 决定） |
| D17 | 校验和算法**单一来源**（`store::body::snapshot_checksum`） | 多处各写一份，一旦有一处改算法，版本比对静默失效（已收敛：原先 engine / workbench 各一份） | — |
| D18 | 排序一律带**确定性兜底**（`, rowid DESC`） | `CURRENT_TIMESTAMP` 只有秒级精度，同一秒内两次写入时「最新」返回任意一条 → 版本链挂错父版本 | 依赖 `rowid` 伪列（DuckDB / SQLite 均支持） |
| D19 | 行级读取错误**向上抛**，不用 `filter_map(r.ok())` 丢弃 | 静默少行比报错难查得多（曾表现为「历史列表恒为空」） | — |
| D20 | 洞察**不自己取数** | 数据入口只有 `temp_table` 与 `conn_id`；自建连接会绕开 M3 的池化与只读策略 | — |
| D21 | 视图归属 = **方案 A（视图入 `crates/insight`）**（2026-09-16 定案） | 硬依据是架构约束「Feature 可以直接依赖 gpui-kit，把同一业务能力的 model / service / view 放在同一 crate」；`project` 已跑通该范式（含宿主桥），而 `panels/` 已 8600+ 行不宜再增 | 开发方案 §3.1 |
| D22 | 规则热加载用**内容哈希轮询**，不用 `notify` / `watch_dir` | `notify` 在本仓只是传递依赖（未声明）；`watch_dir` 需要 gpui `Context`，而洞察在视图归属拍板前不依赖 gpui；内容哈希天然去抖 | 默认 2s 间隔；变更检测有最多一个间隔的延迟 |
| D23 | 监听线程**不访问数据库**，只重载规则集并置 `index_is_stale` | 把项目库连接拉进后台轮询不划算；解耦后分析正确性不依赖数据库 | 索引可能短期落后于磁盘，由规则管理视图打开时消费标记后同步 |
| D24 | 规则管理是个**独立实体 + 模态对话框**（不进面板 Tab） | 三层分组 + 每行状态 + 错误原文需要宽度（280px 放不下）；「配置分析器」与「看分析结果」是两类活动 | 对话框宽度 40rem；列表区固定高 + 内部滚动（高度不随条数跳） |
| D25 | 内置规则的启停写成**项目层的抑制记录** | 内置层不可写，而「本项目不用这条内置规则」本就是项目级决策；`plan_index` 已按「id ∈ 内置层且无来源文件」原样保留这类行 | 无项目时才落全局层；界面把抑制记录从项目规则列表里摘掉（它不是规则，`source_path` 为空） |
| D26 | 启停写入后**立即重跑一次同步**（而不是只改库） | 索引只有被消费才有意义：同步才会 `apply_disabled_rules` + 失效注册表缓存，否则用户点了开关而分析照旧用旧规则 | 代价是每次开关多一轮磁盘扫描（几十个小 TOML，可忽略）|
| D27 | 规则文件用**系统默认应用**打开，不用应用内编辑器 | 正文是 TOML，应用内编辑器没有该语言的语法支持；且规则随项目走，本就在版本库里编辑 | 只拉起进程不等待；`OpenFileRequested` 是公开事件，宿主将来想改成内开也不改面板 |
| D28 | 作用域 → 目录的派发**只留一处**（`service::indexer::rule_dir`） | 同步器 / 新建 / 对话框三处都要这个路径，各拼一次字符串迟早在 `.RSmeta` 的拼写上分叉 | 视图层的 `RuleDataInput` 只收已算好的路径 |
| D29 | 面板的表目标按 **DuckDB 临时表**内省（`DESCRIBE`），不走源库 | 面板拿到的目标只有 `temp_table`（无 conn/db/schema，见 D20）；`DESCRIBE` 对临时表 / 视图一视同仁，不必按 catalog 过滤（`ATTACH` 进来的文件库表不能混进来） | 代价：临时表由查询结果建出，**没有主键约束**，`PK` 角标基本不会出现（如实不显示） |
| D30 | 「评估全表」**串行逐列**，每列都单独回填面板 | 并发会撞上引擎并发上限（D12）而失败重试比慢更让人困惑；逐列回填才能给出**真进度**（测试靠观察者断言进度序列确实有中间态） | 大表列多时耗时与列数成正比，UI 上进度可见 |
| D31 | 界面写的是**实际**采样口径（全量统计 + 样本前 5 行），不照搬原型的「500 行采样」 | 引擎的规则统计是聚合查询，根本不存在 500 行采样；写一个自己不执行的口径比不写更坏 | 原型 §3.2 已加实现期修正说明 |
| D32 | 多列分析的列清单与表探查**同源**（同一个临时表内省），不另存一份 | v1 的 `availableColumns` 恒空正是「多列分析从未跑通」的根因；两份列清单迟早在临时表重建后不一致 | 列清单的实时性靠「切到该 Tab 时才取数」（事件路径） |
| D33 | 多列规则的“吃不吃这几列”只做**展示层判定**（列数 + 类型族逐位比对），不在服务层拦 | 用户选错类型时 SQL 自己会给可读错误；服务层再拦一道只会把「为什么不行」变成两处口径 | `MultiRuleView::accepts` / `arity`，界面据此置灰执行入口 |
| D34 | 多列结果按**数据形态**渲染，不看声明的 `result_type` | 声明与事实不一致时按事实渲染，不会出现「声明 list 却只拿到一个数」的空白表 | 表头取各行键的并集，缺键补「—」（不错位） |
| D35 | 数据态**按 Tab 分开存载荷**（`PanelData { column, table, multi, schema }`） | Tab 条是「同一目标的多个视角」而数据态只有一个格子；合在一起就要求“切 Tab 重新取数”，切回去就把已取到的内容丢了（实测：切「表」再切回「多列」丢表单与结果） | 渲染以载荷为准、状态只管错误/骨架；切 Tab 时载荷缺失才取数（事件路径）。**换目标必须清载荷**：载荷只对旧目标成立，留着会比空白更坏 |
| D36 | Schema 报告的等级与导出都从**视图模型**出发 | 分档阀值只有 `quality_scorer` 一份（顺手把 `schema_analyzer` 自带的「需改进」换成同一份）；导出的是「用户看到的这份结论」，与界面同源，不会出现界面说 3 个孤立表而 JSON 里 4 个 | JSON 分组键取稳定英文，不拿中文展示名当键 |
| D37 | 下钻只报「看哪张表」，不自己拼临时表名 | 把**源表**变成面板能分析的临时表是宿主的活（它才知道连接与临时表约定）；洞察 crate 自拼会有第二套命名约定 | 事件 `TableDrilldownRequested` 带 conn / db / schema / table |

## 7. 并发与资源

| 资源 | 上限 / 策略 | 位置 |
| --- | --- | --- |
| 洞察并发操作 | 4，**超出即返回可读错误**（不排队） | `INSIGHT_MAX_CONCURRENT` |
| DuckDB 内存连接 | **进程级单例**，`Mutex` 全局串行 | `DuckDBManager::get_or_create_in_memory` |
| 洞察中间表 | 前缀 `tmp_i_`，TTL 30 分钟，上限 100，惰性清理 | `engine::duckdb::temp_table` |
| 查询结果表 | 前缀 `tmp_q_`，无 TTL，项目关闭清理 | 同上 |
| 样本行数 | `DEFAULT_SAMPLE_SIZE = 5` | `insight_engine` |
| 直方图最小行数 | `HISTOGRAM_MIN_ROWS = 10` | 同上 |
| 每列保留版本数 | `MAX_VERSIONS_PER_COLUMN`（超出淘汰最旧） | `store::body` |
| 表级评估 | **串行逐列**（不依赖并发上限兜底） | `service::persistence` |
| 规则目录监听 | 后台线程，默认轮询间隔 2s；**drop 即停**；每轮仅哈希几十个小文件 | `insight::service::watcher` |

**为什么快速失败而非排队**：分析入口是同步函数（`get_column_insight_full`），语义上无法 await；若改为阻塞等待 `Mutex`/信号量，会把调用线程（可能是 UI 主线程）卡住。代价是超限时用户要重试，故错误文案面向用户（「洞察分析任务过多，请稍候重试」），由 UI 呈现为加载态。

## 8. 降级矩阵

| 场景 | 行为 | 用户可见结果 |
| --- | --- | --- |
| 系统目录不可用（`get_system_dir` 失败） | 跳过全局层，内置层照常 | 全局规则不生效，洞察功能正常 |
| 全局 / 项目规则目录不存在 | 该层返回 0 条，不报错 | 视为「该层无规则」 |
| 单条规则解析失败 | 标记 `invalid` + 记错误原文，其余照常 | 规则列表红行 + 错误原文 |
| 规则文件被删 | 索引转 `missing`，规则不再生效 | 「规则文件不见了」提示 |
| 内置规则被删（如 `numeric-stats` 被覆盖成坏规则） | 该列统计计算失败并向上抛 | 该列画像报错（**基础统计依赖规则，属已知耦合**） |
| 并发超限 | 返回 `洞察分析任务过多，请稍候重试` | 行内提示 + 可重试 |
| 临时表 TTL 过期（30 分钟） | 查询失败 | 「结果已过期，请重新执行查询」 |
| 源库连接断开（表探查 / Schema） | 错误向上抛 | 错误 + 重试按钮 |
| 未打开项目 | 快照与规则管理不可用；临时表画像仍可算 | 面板提示「打开项目后可保存快照」 |
| 全 NULL / BLOB / ARRAY 列 | `Unknown` 变体，只出计数 | 「类型未识别」，**不生成分布** |
| 空表 | `TableQuality` 返回「表为空或无数据」 | 不产假分数 |
| 规则目录为空（首次使用） | 只有 18 条内置规则 | 全局层需用户自行创建目录 |

## 9. 测试策略

按**可测性分层**，越靠内越纯：

| 层 | 对象 | 手段 | 现状 |
| --- | --- | --- | --- |
| 纯函数 | 规则解析 / 覆盖 / 分类派生 / `plan_index` / 评分算法 / Schema 检测 | 进程内直接断言；不依赖文件系统与全局状态 | 高覆盖 |
| 文件系统 | `scan_scope_dir` / `load_from_dir` / 索引合并 | **每测试独占临时目录**（目录名含 pid + tag），避开并发下的同目录竞争 | 已覆盖 |
| 存储 | 索引表读写、快照双写与版本链 | 临时项目目录 + `ProjectDatabaseManager::open`（迁移会建表） | 已覆盖 |
| 装配 | `registry_for` 缓存、启用禁用生效 | 用**不存在的项目根**避免碰真实项目；注意缓存是进程级静态量 | 已覆盖 |
| 契约 | 零裸色 / 零裸 `px(` | `cargo test -p rds-workbench --test ui_contract` | 视图落地后纳入 |

**基线**：`cargo test -p rds-insight` 当前 **184 项**（迁移基线 53 + Phase 0–4 新增），另有集成测试 9 项；新增功能不得减少。

三条测试纪律（来自实际踩坑）：
1. **进程级静态量（缓存）是测试隔离的敌人**：规则注册表缓存与禁用集合都是进程级静态量，「写入 → 断言」之间被另一个测试的清理动作插入就会间歇失败（实测约 1/5 概率）。对策两条：
   - **拆开清理粒度**：`clear_registry_cache`（只清注册表）与 `clear_disabled_rules_cache`（只清禁用集合）分开，需要整体重置时用 `clear_rule_caches`。生命周期不同的状态不应共用一个「全清」开关。
   - **写入类测试持锁串行**：`crate::tests::rule_state_guard()`（进程内 `MutexGuard`，容忍中毒）由所有写缓存的测试先取；只读测试不取。
2. **临时目录必须逐测试唯一**：`ProjectDatabaseManager::open` 会独占 DuckDB 文件锁；目录名带 pid + tag。
3. **临时表名与临时目录都要可回收**：测试结束显式 `remove_dir_all`；写入类测试结束时把本测试的缓存键清掉（避免污染后续用例）。

## 10. 实现位置映射（设计决策 → 代码文件）

| 设计决策 | 代码文件 |
| --- | --- |
| D2/D3 作用域与覆盖 | `crates/insight/src/rule.rs`（`RuleScope` / `RuleSource` / `RuleLoadFailure`）、`rule_registry.rs`（`insert_rule` / `list_by_scope` / `remove_rules`） |
| D4/D5/D7 索引与启停 | `crates/insight/src/service/indexer.rs`、`crates/engine/migrations/{global/024, project_meta/019}_insight_rule_index.sql` |
| D6 覆盖解析失败的文件 | `indexer.rs`（`scan_one_file` 的 `file_stem` 兜底） |
| D8 启停后置 | `crates/insight/src/lib.rs`（`build_registry` 尾段） |
| D9/D11 注册表缓存与禁用桥 | `lib.rs`（`registry_for` / `with_rules` / `apply_disabled_rules` / `disabled_snapshot` / `clear_registry_cache` / `clear_disabled_rules_cache` / `clear_rule_caches`） |
| D10 规则集显式传入 | `insight_engine.rs`（`*_internal` / `*_on` 的 `&RuleRegistry` 形参） |
| D12 并发上限 | `insight_engine.rs`（`INSIGHT_MAX_CONCURRENT` / `ERR_TOO_MANY_CONCURRENT`） |
| D13 DuckDB 单例 | `engine/src/duckdb/manager.rs`、`engine/src/services/duckdb_service.rs` |
| D14 内部接缝 | `insight_engine.rs`（`get_column_insight_full_on` / `get_column_stats_internal` / `get_column_sample_internal` / `get_column_histogram_internal`） |
| D16/D17/D18/D19 快照链路 | `crates/insight/src/store/{mod.rs, body.rs, meta.rs}`（正文与元数据仓库 + 装配点 `ProjectInsightStores`） |
| D1 目标分派 / D15 采样明示 | `crates/insight/src/insight_view.rs`（面板头 + 五 Tab + 四态 + 列画像四区）、`model.rs`（视图模型 `PanelTab` / `InsightTarget` / `InsightPanelState` / `ColumnProfileView`） |
| D21 视图归属（方案 A） | `crates/insight/Cargo.toml`（`gpui-kit`）、`insight_view.rs`（视图）、`ui.rs`（M8 尺寸常量）、`commands.rs`（`OpenInsight` / `InsightRefresh` / `ReloadInsightRules`） |
| D22/D23 目录监听与索引解耦 | `crates/insight/src/service/watcher.rs`（`RulesWatcher` / `rules_fingerprint` / `watch_dirs` / `set_watched_project_root` / `index_is_stale`）；接线在 `workbench/src/view.rs`（构造期启动）与 `workbench/src/components/project_host.rs`（项目切换告知） |
| D20 不自己取数 | `engine/src/services/{duckdb_service,sql_service}.rs` 为唯一数据入口 |
| 规则执行（SQL 模板与输出映射） | `crates/insight/src/rule_executor.rs` |
| D24～D27 规则管理对话框 | `crates/insight/src/rule_view.rs`（视图模型 + 实体 + 渲染）、`jobs.rs`（`attach_rules` / `handle_rules_event` / 打开系统编辑器）、`service/mod.rs`（`rules_data` / `toggle_rule` / `create_rule_file` + 两层索引库装配）、`service/indexer.rs`（`rule_dir` / `create_rule_file` / `new_rule_template`）；宿主一行：`workbench/src/panels/right.rs` |
| D29～D31 表探查与评估全表 | `insight_engine.rs`（`get_temp_table_profile` / `*_on`）、`model.rs`（`TableProfileView` / `TableColumnView` / `TableQualityView` / `TableEvalProgress` / `PanelData`）、`insight_view.rs`（`render_table_profile` + 列名下钻）、`jobs.rs`（`ProfileRequest::Table` / `request_table_evaluation` 的串行进度）、`service/mod.rs`（`profile_table_view`）、`ui.rs`（`INSIGHT_TABLE_*`） |
| D32～D34 多列分析 | `service/mod.rs`（`multi_column_view` / `list_multi_rules` / `run_multi_rule` / `rule_params`）、`model.rs`（`MultiColumnView` / `MultiRuleView` / `MultiResultView` / `KeyValueRow` / `quality_notes`）、`insight_view.rs`（`render_multi_view` + 列多选/规则单选）、`jobs.rs`（`MultiColumnRequested` / `MultiRunRequested`） |
| D35 数据态按 Tab 分栏 | `model.rs`（`PanelData` + `InsightPanelState::Data` 无载荷）、`insight_view.rs`（`render_body` 以载荷为准 / `emit_request_for_tab` / `ensure_data_for_tab`）、`test_support.rs`（记录型宿主） |
| D36/D37 Schema 健康报告 | `schema_view.rs`（新：视图模型 + `to_json` / `to_markdown`）、`schema_analyzer.rs`（等级共用 `Grade`）、`service/mod.rs`（`schema_report_view`）、`insight_view.rs`（`render_schema_report` + 下钻热点）、`jobs.rs`（`SchemaReportRequested` / `TableDrilldownRequested`） |
| 类型族判定（唯一来源） | `crates/insight/src/model.rs`（`type_base` / `is_numeric_type` / `is_datetime_type` / `is_binary_type` / `is_array_type` + `ColumnKind::of_type_name`）；列画像与表探查共用 |
| 面板头 ⚙ 入口 | `insight_view.rs`（`render_header`，无项目时禁用）、`InsightView::rules_view`（宿主接缝用） |
| 质量评分四维与**等级**（`Grade`；列级与表级共用阈值/文案） | `crates/insight/src/quality_scorer.rs` |
| 评分卡视图模型（`ScoreView` / `DimensionView`；全空列不产分） | `crates/insight/src/model.rs`（`ColumnProfileView::score`） |
| 评分卡渲染（钉在滚动区之外；四维细条） | `crates/insight/src/insight_view.rs`（`render_score_card` / `dimension_row` / `ratio_bar`） |
| 表探查 | `crates/insight/src/table_profile_service.rs` |
| Schema 洞察 | `crates/insight/src/schema_analyzer.rs` |
| 规则资产（18 条） | `crates/insight/insight-rules/` |
| 服务层统一入口 | `crates/insight/src/service/mod.rs`（`with_rules` 契约） |

## 11. 已知问题与后续项（**权威清单**）

| # | 问题 | 影响 | 状态 |
| --- | --- | --- | --- |
| K1 | **无 SQL 沙箱**：v1 文档声称有 `ATTACH`/`INSTALL` 等黑名单，v2 代码里**不存在**；唯一防线是 `validate_identifiers`——只校验**参数值**字符（字母数字 / `_` / `-` / `.`），不校验规则 SQL 本身 | 安全：用户规则文件的 SQL 直接在该进程的 DuckDB 上执行，可 `ATTACH`/`COPY` 到任意路径 | 待决策（见 §12） |
| K2 | `crates/engine/insight-rules/` 是 `crates/insight/insight-rules/` 的**逐字节重复副本**，全仓零代码引用 | 后来者可能改错副本 | 待删除确认 |
| K3 | `table-quality-overview` 规则的 SQL 引用表 `insight_column_stats`，该表**不存在**（只有 `insight_column_snapshots`） | 该规则解析通过但执行必失败 | 待决策：建表 / 改 SQL / 下线 |
| K4 | ~~归属偏差未归位~~ | — | ✅ 已归位（Phase 0 / 0.2）：类型 → `model::types`、仓库 → `store::{body,meta}`、`detect_extremes` → `insight_engine`、门面 → `service::{InsightService,persistence}` |
| K5 | ~~目录监听热加载未做~~ | — | ✅ 已实现（Phase 0 / 0.6，D22/D23） |
| K6 | `insight_table_reports` / `insight_schema_reports` 两张表为**预留**，无写入者 | 完成度易被高估 | Phase 4 |
| K7 | ~~用户全局规则目录（`{system}/insight-rules/`）**不自动创建**~~ | 已消除：规则管理对话框在目录缺失时给「创建目录并新建规则」入口，**首次写入时建**（不在启动时预设空目录） | ✅ 已修（Phase 2 / 2.4） |
| K8 | ~~视图归属待拍板（D21）~~ **已定案**（D21 = 方案 A，2026-09-16） | 已消除：`insight` 依赖 gpui-kit，视图落 `insight/src/insight_view.rs` + `ui.rs`；`panels/` 只负责装配与发命令 | ✅ 已定案 |
| K9 | `get_column_insight_full` 并发超限时的**用户重试**由 UI 承担 | 批量场景体验 | ✅ 已修（Phase 2 / 2.2）：「评估全表」串行逐列，不会超限；进度逐列回填（D30） |
| K10 | 内置规则的**基础统计耦合**（覆盖 `numeric-stats` 会连带影响列画像） | 用户误以为只影响「那条规则」 | 文档说明（本文件 §5.3 + 使用手册） |
| K11 | `null-check` 规则的 `[[quality]] field = "null_rate"` 指向**不存在的输出字段**（其 `[[output]]` 只有 `total_count` / `non_null_count` / `unique_count`）→ `actual == None`，而 `evaluate_quality` 在**只设 `max` 且 actual 为 None** 时不判定失败 → 该质量门控**永不触发**（静默通过） | 中：用户以为有门控，实际没有 | 待决策：补 `null_rate` 输出字段 / 改 `field` / 让「字段不存在」报错而不是静默通过（倾向后者） |
| K12 | `quality-score` 规则用 `value_type = "str"`，该值**不在支持列表**中，靠 `match` 的兜底分支当成 `String` 处理而侥幸工作 | 中：`value_type` 写错时不会报「未知类型」，而是在读取时报出难以归因的错误（如把 DOUBLE 列当 String 读） | 待决策：解析期校验 `value_type` 白名单（与 `deny_unknown_fields` 同一立场：早失败优于静默错） |
| K13 | ~~`workbench` 依赖 `mock` 而 `mock` 编译不过~~ | — | ✅ 已解除（2026-09-15）；`engine/tests/transaction_affinity.rs` 的 `as_i64` 编译错误也已修（`Value` 只有 `as_int`，随 `b838ea0` 提交） |

## 12. 待确认

| # | 问题 | 选项 |
| --- | --- | --- |
| Q1 | **规则 SQL 的安全边界**（K1） | (a) 启动期静态黑名单（`ATTACH`/`INSTALL`/`COPY`/`EXPORT`…）；(b) DuckDB 侧限制（只读连接 / 禁用扩展）；(c) 明确「用户规则文件 = 可信本地文件」并在文档声明（当前事实）；(d) 引入沙箱执行（成本最高） |
| Q2 | 视图归属（K8） | 方案 A / B（开发方案 §3.1） |
| Q3 | `table-quality-overview` 的处置（K3） | 建表 / 改 SQL / 下线 |
| Q4 | 快照保留上限与清理默认值 | 每列 `MAX_VERSIONS_PER_COLUMN`；清理默认 30 天（v1 硬编码） |
| Q5 | 快照双写失败的补偿策略（D16） | 重试 / 标记待清理 / 放任（Phase 5 决定） |
| Q6 | 质量门控的「字段不存在」应报错还是静默通过（K11） | 倾向报错（与 `deny_unknown_fields` 同一立场） |
| Q7 | `value_type` 是否在解析期做白名单校验（K12） | 倾向做（否则写错表现为难以归因的读值错误） |
