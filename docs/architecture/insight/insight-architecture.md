# 洞察模块（M8）· 设计理念与架构

> 状态：**Phase 0–5 完成 + 规则安全边界收口**（2026-09-17） · 关联文件：`README.md`（模块入口）、`insight-prototype-design.md`（原型）、`insight-dev-plan.md`（开发方案与进度）、`insight-user-guide.md`（使用手册）
> 本文回答**为什么这样设计 / 怎么运转**：概念模型 → 不变式 → 分层与归属 → 状态所有权 → 数据流 → 决策表 → 并发 → 降级 → 测试 → 实现映射 → 已知问题（权威）。
> 现状口径：**画像 / 评分 / 规则 / 报告 / 快照历史全部落地**（含右 Dock 面板与规则管理对话框）；规则安全边界 = 解析期静态门（D52）+ 项目规则信任门（D53）；临时表走 `duckdb::analysis`（D50/D51）；入口统一为源取样（D58），导航树 / 分析存档 / 草稿箱三个入口与**文件类数据源**（CSV / Parquet / Excel / JSON）已接（D59），Schema 报告的**导出与下钻**已接（D60）。宿主侧仍欠：编辑器结果集入口（见开发方案 §0）。

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

### 1.1 可分析范围（口径，2026-09-18 修正）

| 来源 | 数据画像 | 结构画像 | 判据 |
| --- | --- | --- | --- |
| 数据库对象（表 / 视图 / schema） | ✅ 源取样 | ✅ 驱动元数据 | **能出现在数据库导航树上**——有真正驱动 + 活连接（MySQL / PostgreSQL / SQLite / DuckDB） |
| 草稿箱 / 分析存档里的文件 | ✅（`SampleSource::duckdb_file`） | ✖（文件没有可报告的 schema 结构） | 直接可以：CSV / Parquet / Excel / JSON（含需 DuckDB 扩展的 Excel） |
| 只能靠 `ATTACH` / 三方扩展到达的远程库 | 机制上能（`SampleSource::on_duckdb`） | ✖ | **不在边界内**：它不经过导航树，也没有驱动层元数据 |

一句话：**洞察的边界 = 导航树所见 + 草稿箱/分析存档两块文件**。扩展是「文件读取器」的实现手段（Excel 就靠它），不是把没有驱动的远程库拉进来的通道（D62）。

## 2. 概念模型与不变式

### 2.1 五个概念

| 概念 | 含义 | 载体 |
| --- | --- | --- |
| **分析目标** | 一次分析的对象：列 / 表 / 多列集合 / Schema。**没有「全局洞察」** | 视图层选定 |
| **规则** | 声明式分析单元：SQL 模板 + 参数 + 输出映射（+ 质量门控 + 渲染提示） | `.rule.toml` 文件 |
| **规则作用域** | 规则的三层归属，决定可见范围与覆盖优先级 | `RuleScope` |
| **画像** | 对目标的统计结论（`ColumnInsightFull` / `TableQuality` / `SchemaInsightReport`） | 内存值 |
| **快照** | 画像的持久化副本 + 版本链 | 项目 `analytics.duckdb` + `project.db` |

### 2.2 九条不变式

1. **一次分析 = 一个目标 + 一份结论**——面板头部永显当前目标，杜绝「这数是哪来的」。
2. **结论可追溯**——每个数字来自统计量、评分维度或规则之一；UI 不发明结论。
3. **规则正文以文件为唯一真相源**——库表只存索引与状态（`checksum` / `enabled` / 校验结果），正文永不入库。
4. **同名整体覆盖**——`meta.id` 相同即视为同一条规则，后加载层整体替换前者；**不做字段级合并、不加命名空间前缀**。
5. **单条规则失败不连坐**——解析失败只标记该条并记录错误原文，其余规则照常可用（含「坏规则必须能被看见」，见 D6）。
6. **启停在覆盖之后应用**——禁用针对「最终生效的那一条」，与它来自哪一层无关。
7. **采样必须明示**——任何基于采样得出的结论，界面必须标出采样行数（表探查与批量评估后端为 `LIMIT 500`）。
8. **快照正文与元数据成对写入**——不允许「只写一半也算成功」的静默降级。
9. **项目层规则先得信任才装配**——项目规则跟着仓库走，未信任时**不装配也不执行**，但必须把「带了什么」摊在用户面前（D53）。

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
       ① builtin_registry()            内层：include_dir 编译期嵌入（16 条，只读）
       ② get_system_dir() → {system}/insight-rules/    全局层（跨项目）
       ③ get_project_rules_dir(root) → {项目}/.RSmeta/insight-rules/   项目层
          · **信任门**（D53）：project_rule_trust(root) 不是 Trusted 就**不装配**，
            只扫描磁盘把「带了什么」记进 registry.pending_project（不执行任何 SQL）；
            Trusted 才走 load_from_dir(dir, scope)
          · 每层 load_from_dir(dir, scope)：同名 insert 覆盖 + 记录 RuleSource + 记录失败
       ④ 日志逐条播报 failures
       ⑤ apply 禁用：disabled_snapshot(key) → registry.remove_rules(&set)
       ⑥ 返回（缓存）
```

信任状态的读取走 `project_rule_trust`：进程级缓存 → 未命中则**自开一条只读连接**查全局库的
`insight_rule_trust`（不用连接池的 `acquire_sync`——它在 tokio 运行时内会直接报错，
而装配路径确实可能从异步侧进来）；查不到 / 出错一律按「未决定」（fail closed）。
用户做出决定后 `apply_project_rule_trust` 更新缓存并使注册表缓存失效，下次装配就是新结果。

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

**面板侧两个动作**（Phase 5.1）：

```text
「保存」 ──► 事件 SnapshotSaveRequested { temp_table, column }
             └─► InsightService::save_column_snapshot(root, temp_table, column)
                   ① get_column_insight_full(root, …)   ← 重取领域画像（D42）
                   ② ProjectInsightStores::open(root)    ← 本次操作只开一次（D41）
                   ③ stores.save_column_snapshot(…)      ← 正文 + 元数据双写
                   ④ read_history(&stores, column)       ← 复用同一句柄读回
                 ──► set_history（失败则 set_history_notice：只挂行内提示，D40）

切到「历史」──► 事件 HistoryRequested { column }
             └─► InsightService::column_history_view(root, column) → 同 ④
                 ──► set_history（失败则整页错误态）
```

无项目时**不发事件**也不摆骨架：看不了就说看不了（D39）。

清理（Phase 5.3）走同一条链，多一道确认框：

```text
「清理」 ──► 确认框（本 crate 内开，D45；天数取 SNAPSHOT_RETENTION_DAYS，D46）
          └─► 事件 SnapshotCleanupRequested { column }（不带天数）
                 └─► InsightService::cleanup_old_snapshots(root, column, days)
                       ① 开一次项目库（D41）
                       ② 正文删（DuckDB）+ 元数据删（SQLite）——两侧条数都带回（D47）
                       ③ 读回列表 + 贴回执（复用同一个句柄）
                     ──► set_history（失败则 set_history_notice）
```

> 踩过的坑：`created_at < (CURRENT_TIMESTAMP - INTERVAL ? DAY)` 在 DuckDB 里**不合法**
> （解析器不接受 `INTERVAL` 里的绑定参数）——那段代码自迁入后从未真被调用过。
> 天数只能拼字串（类型是 `i64`，没有注入面）。

对比（Phase 5.2）走的是同一条链，只是多读一版正文：

```text
点更早的一版 ──► 事件 VersionCompareRequested { column, version_id }
                 └─► InsightService::compare_column_snapshots(root, column, version_id)
                       ① 读列表（最新在前）  ← 基准与「当前」都从这份列表里取
                       ② 两侧正文 parse_insight()  ← 比的是**存下来的结论**，不是现在重算的
                       ③ VersionDiffView::between(old, old_label, new, new_label)
                       ⋮ 护栏：基准 = 最新 → 「最新一版没有更新的版本可比」
                       ⋮      版本不在列表 → 「这一版已不在历史里」
                    ──► set_history（列表 + 对比一起回来；失败则 set_compare_notice）
```

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
| D37 | 下钻只报「看哪张表」，不自己拼临时表名 | 把**源表**变成面板能分析的临时表是宿主的活（它才知道连接与临时表约定）；洞察 crate 自拼会有第二套命名约定 | 事件 `TableDrilldownRequested` 带 conn / db / schema / table。**D60 收尾时改为源取样**：宿主不再建临时表，只给「在哪条连接上查哪张表」（D58 之后建临时表是多余的一步） |
| D38 | 历史列表的**顺序与条数口径单一来源**：顺序由存储层 `ORDER BY` 决定（视图不重排），条数由 `HISTORY_PAGE_SIZE` 决定（查询与界面提示共用） | 「谁是最新」两处各写一份，迟早在撞秒（D18）时给出互相矛盾的答案；被截断的列表必须明示（与 D15 同一立场：别让人把截断当成全部） | 满一页时列表下方写「只列出最近 N 条」；列表**不另设内层滚动**（面板主体已是滚动区） |
| D39 | 取数请求**发不出去也要落一个状态**：`emit_request_for_tab` 发得出去进加载态，发不出去落空态 | 调用方为了「点了有反应」已先摆上骨架；沉默返回会让骨架**一直转下去**（无项目时切「历史」即此：那是「没得看」，不是「在加载」） | 空态文案按 Tab + 项目状态给（同一 Tab 两句话） |
| D40 | 失败语义分两档：**保存失败只挂行内提示，读失败推整页错误态** | 判据是「失败会不会让人怀疑已有数据没了」：保存失败时已有历史还在，整页错误态反而像快照丢了；而列表读不出来时无从部分展示，「为什么读不到」才是答案 | 与多列执行的失败语义（Phase 3）同形 |
| D41 | 项目库**现开现用，一次操作只开一个句柄** | DuckDB 在同一进程里对同一份文件只允许一个实例，重叠 `open` 必失败（实测表现为「快照真落库了，却提示保存失败」） | 保存后的读回复用同一句柄（`service::read_history`）；宿主现有做法（资源目录 / Mock 历史 / 连接列表）本就是开→用→放 |
| D42 | 快照的**正文重取不拼装**：保存时重跑一次领域画像，不把视图模型反拼回领域结构 | 视图模型是**渲染用的投影**（缺字段、带展示文案），反拼回去只能靠猜；而视图模型正是为了「改界面不像改契约」才与领域类型分开的 | 代价：保存多跑一次统计（与打开面板同量级的查询） |
| D43 | 对比方向固定为**「选中版本 → 最新版本」**，且**对比结果也在载荷里**（`HistoryView.diff`） | 方向可选只是多一个状态（还要多一套「谁是基准」的文案），而这个 Tab 的问题永远是「和上次比变了什么」；对比结果与列表同属一个载荷，才能让「有没有对比」只认载荷——否则列表一刷新（保存 / ⟳），手里就留下一份指向旧「当前」的差值 | 选择位在出数时从载荷反推；刷新列表不自动续取对比；最新一版不给点击入口 |
| D44 | 对比的**行集合与「列」Tab 同源**（同一个 `ColumnProfileView`），差值一律**按展示精度算** | 另立一套对比口径，迟早在两处对不上（如「空值」在 Tab 里是 `12（14.3%）` 一行、而对比必须拆成两个可对齐的数）；按展示精度算差值则避免了「显示 62 → 62 却 +0.4」这种自相矛盾 | 解不出数字的展示串（区间 / 带方向后缀 / 类型名）只说「已变」，不编差值 |
| D45 | 删除类动作**过确认框**，且确认框在**本 crate 内**开（`window.open_dialog`） | 快照删除不可撤销，误点是真丢数据；而弹框是纯视图行为，没有理由把它推给宿主（规则对话框已跑通这条路径，宿主零配合） | 危险按钮（danger 变体）+ 写清删什么、留多少、当前多少 |
| D46 | 清理的**天数只允许有一个来源**（`model::SNAPSHOT_RETENTION_DAYS`），事件不带天数 | 界面写「30 天」而实际按别的天数删，是这类功能最坏的不一致（且无法从界面看出来）；把数字放在事件里，多一个入口就多一个可能写错的数 | 接缝持有策略（取常量），服务收参数（可测），视图只负责把同一常量写在话里 |
| D47 | 清理回执**两侧条数分开报**，对不上就报警 | 正文与元数据成对写入（D16），删的时候也必须成对；只报一个「清理成功」会把半写残留（一边删多了一边没删）盖住 | 两侧不等时行内提示转 danger，并把两个数都写出来 |
| D48 | `value_type` 走**白名单校验**（解析期，2026-09-17 定案 = Q7） | 以前白名单外的值靠 `match` 的兜底分支当 `String` 读：把 DOUBLE 列当字符串读，报出来的是一句与「类型名写错了」毫无关系的读值错误（K12） | 白名单常量 `rule_registry::VALUE_TYPES` 与执行器分支一一对应；报错直接列出可用取值 |
| D49 | 质量门控的 `field` 必须在 `[[output]]` 里真实存在（解析期，2026-09-17 定案 = Q6） | 字段不存在时取值为 `None`，而「只设 `max`」的判定对 `None` 是**通过**——门控形同虚设而界面看不出来（K11：内置的 `null-check` 就指着不存在的 `null_rate`） | 与 `deny_unknown_fields` 同一立场（早失败优于静默错）；**值合法地为 `null`**（如空表算不出空值率）仍算通过——那是刻意的 |
| D50 | 分析用临时表**统一走 `engine::duckdb::analysis`**：名字带 `tmp_i_`（与 `TempTableSource::Insight` 对齐）、建表即登记、**用完即删**，惰性清理负责真正 DROP（2026-09-17） | 临时表的一切回收机制都靠**前缀**识别（TTL / 上限 / 按来源清理）；建表时另起一个名字（历史上的 `rs_<uuid>`）等于把所有回收机制关掉（K16）。而「只腾登记表不执行 DDL」会让表变成再也找不到的孤儿 | 中间产物用 `with_analysis_temp_table`（建 → 用 → 无论成败都收）；需要跨函数持有时用 create / drop 对；`drop` 只接受 `tmp_i_` 开头（防误删）。分析侧的 engine 支撑落在 `crates/engine/src/duckdb/`（新模块 `analysis.rs`） |
| D51 | 连接建立时给 DuckDB 上**内存闸与溢写口**：`memory_limit` 默认 **2GB**、`temp_directory` 钉 `<RDS_HOME>/tmp`、`max_temp_directory_size` 默认 **10GB**；两个大小值可由 `RDS_DUCKDB_MEMORY_LIMIT` / `RDS_DUCKDB_MAX_TEMP_SIZE` 覆盖（非法值回退默认并告警）；登记概览用 `DuckDBManager::temp_table_stats()` 暴露，洞察建表时接近上限打 warn（2026-09-17，K16 ③④） | 内存库是**进程级单例**，DuckDB 默认吃物理内存的 80%——桌面应用把机器吃光不是可接受的失败方式；设了上限后到顶是**溢写到 `tmp/`**（慢一点）而不是报错，而溢写目录必须是我们自己会清理的地方（系统临时目录是用户清理不到的角落）。可观测是配套：登记表只增不减时没有任何外部表现，计数是唯一的提前信号 | 值拼进 SQL 前过 `parse_size_setting` 白名单（DuckDB 的 `SET` 不接受绑定参数）；临时目录建不出来时跳过溢写设置、不挡启动 |
| D52 | 规则 SQL 走**解析期静态门**：只放行单条 `SELECT` / `WITH`，拒分号多语句、拒黑名单关键字（`ATTACH`/`COPY`/`INSTALL`/`SET`/`CREATE`/`DROP`…）与表函数（`read_*` / `*_scan` / `*_attach` / `*_query` / `glob` / `query_table`…）；**校验前先剥字符串字面量、注释与双引号标识符**（2026-09-17，Q1 ① 落地） | 规则文件随项目走（能来自别人仓库），而模板 SQL 原样交给 DuckDB 执行——最便宜的防线是在解析期就把「不是只读查询」的写法拒掉。**定位是防呆，不是安全边界**：DuckDB 没有官方解析沙箱，黑名单天然有漏（`SELECT` 里仍可能藏着未列入的函数），它做的是把风险从「随手就撞上」压到「得刻意构造」 | 报错文案点出规则 id + 撞上的词 + 改法（列名 / 表名加双引号即放行——剥掉双引号标识符是有意的，防误报）；真正的边界是「项目规则信任门」（Q1 ③） |
| D53 | **项目规则信任门**：项目层规则在用户做出决定前**不装配、不执行**；决定（`trusted` / `declined`）记在**全局库** `insight_rule_trust`（键 = 规范化项目路径），默认无记录 = 未决定；首次在规则管理里遇到时**弹一次确认框**，之后由列表顶部的横幅改主意（2026-09-17，Q1 ③ 落地） | 静态门（D52）只挡「不是只读查询」的写法，挡不住一条**合法但恶意**的查询；而项目规则跟着仓库走——「克隆不信任的仓库 + 打开项目」是一个真实的攻击面。决定必须由人做，且必须记在**项目碰不到的地方**：存项目库等于让项目自己给自己发信任（自我授权） | `declined` 也落库：把「不加载」记成一次决定，才不会每次开项目都追问。**fail closed**：无记录 / 查库失败 / 值不认识，一律按未决定。已知取舍：信任绑定**路径**而非内容——用户自己改规则不会被反复打扰，代价是 `git pull` 带进来的新规则不会重新确认 |
| D54 | **查询结果临时表**：命名统一走 `duckdb::generate_unique_name(Query, …)`（前缀 `tmp_q_` + 时间戳 + 8 位随机）、建完即登记；回收两个口——**定向** `drop_temp_table(conn, Query, name)`（结果集被丢弃 / 替换 / 关文档）与**清场** `DuckDBManager::drop_in_memory_temp_tables(Query)`（项目切换 / 关闭）（2026-09-17，K16 结果集侧） | 结果集表历史上叫 `rs_<uuid>`，不属于任何来源前缀 → TTL / 上限 / 按来源清理 / 关项目清场对它**全都看不见**（K16）。命名与回收口必须成对设计：只改名字不提供定向口，就只能「全清」而不能「丢一个」——那会把用户还在看的结果一起删掉 | 命名与删除都收在 `duckdb::temp_table`（与洞察中间表 / mock 共用一份实现）；**建表方负责回收**（宿主路径目前未接线，见 K16） |
| D55 | 快照**双写失败时回滚已写入的那一半**（正文成了、元数据没成 → 删正文），错误文案里写清「已回滚」；回滚自身失败时同时报两件事（2026-09-17，Q5 定案） | D16 说「成对写入」，就得说得出成对失败怎么办。原实现只把错误往上抛，留下的是**界面上看不见、清理也配不上对**的孤儿正文——存储用量与真实历史从此对不上 | 回滚失败不掩盖原错误（原错 + 回滚失败 + 出路）；孤儿靠存储清理（按时间删正文）兜底 |
| D56 | 快照**保留期固定 30 天**（`model::SNAPSHOT_RETENTION_DAYS`），**不做配置项**（2026-09-17，Q4 定案） | v1 就是 30 天；快照是轻量 JSON（几 KB 级），30 天足够回答「上周和现在有什么不同」。而每多一个配置项就多一处能写错的数——清理对话与执行取同一个常量，就没有「写的和删的不一致」的可能 | 要改就改常量（单一来源）；上限另有 `MAX_VERSIONS_PER_COLUMN` 兜底 |
| D57 | 版本链被清理剪过的**头部**在界面上标「更早的版本已清理」（数据层**不改**）——只在列表未分页截断时判（2026-09-17，K14 定案） | 剪链是「删 30 天前」的必然副产物：存活版本的 `parent_version_id` 仍指着已删的父版，那它就不再满足「首版」条件（`parent = None`），界面会既不标首版也不解释。改数据（把头部 `parent` 置空）会丢掉「它前面还有历史」这个事实；改界面则只说真话 | 判据要三合一（最旧一版 + 父版不在列表 + 列表完整），否则分页会造成假阳性；对比不沿链走，所以不影响差值 |
| D58 | **入口统一为「源取样」**：导航树 / 分析存档 / 草稿箱 / 编辑器结果集都只给 `SampleSource { conn_id, sql, label }`（一段**只读取样查询** + 来源标签），洞察侧统一包一层 `SELECT * FROM (…) LIMIT 500` → 落 `tmp_i_` 分析临时表 → 之后与「临时表目标」**完全同路**（D53 后的新目标形态：`SourceColumn` / `SourceTable`）（2026-09-17） | 数据入口五花八门（表 / 查询 / 存档记录），但**分析能力只有一套**（列画像 / 表探查 / 多列 / 下钻都要临时表）。每个入口各写一套取数 + 一套回收，必然分叉；而「把数据拼成临时表」又绝不能反拼装——结果集手里只有字符串化的行，拼出来的表会把 DOUBLE 当字符串，质量评分给出**错**结论（D42 同一立场） | 抽样口径（行数）只在洞察侧（不写进入口）；样本表由现成机制回收（TTL 30 分钟 / 上限 100）；SQL 由入口给是因为**只有它知道该源的方言与引号规则**；下钻 / 多列 / 保存快照接着用**同一份样本表**（`PanelData.source_sample`），不重新抽样 |
| D59 | **文件类数据源与三个入口接线**：`SampleSource.conn_id` 改 `Option`——`None` = 跑在 **DuckDB 内存库**（CSV / Parquet / Excel / JSON 这类**靠扩展直接读的文件**，以及已 `ATTACH` 的表）；扩展名 → 读取函数的映射与 `load_file_source` **共用一处**；取样落表分两条（源库连接走引擎 JSON 打型，DuckDB 侧走 `CREATE TABLE … AS` **不过 Rust**）。入口：导航树 / 分析存档 / 草稿箱右键「查看统计」（2026-09-18） | 边界以**导航树**为准（有真正驱动 + 活连接），草稿箱 / 分析存档两块文件**直接可以**（2026-09-18 修正口径，见 §1.1）；对文件而言「DuckDB 能读」是必要条件（Excel 要靠扩展）；库表与文件在能力上没有区别，区别只在**数据怎么进 DuckDB**：库表过引擎（JSON 打型），文件让 DuckDB 自己读（还省一次序列化往返，类型也更准）。入口侧的「能不能分析」判定必须与取样共用同一处口径，否则会出现「菜单亮着但点了报不支持」 | 分析表型存档（本体是 `analytics.duckdb` **库文件**，要 ATTACH + 用重建定义取数）与编辑器结果集入口**未接**（后者用户明确说不急）；格式判定在**宿主**侧——草稿箱不依赖 `engine`，走端口问宿主，否则读取器口径会被抄第二份 |
| D60 | **Phase 4 收尾：Schema 报告的导出与下钻**。导出：`content` 在**面板侧**编码（`to_json` / `to_markdown`），事件只带 `format` / `file_stem` / `content`，宿主弹保存对话框（`rfd`）、写文件、状态栏回执；下钻：**不建临时表**——宿主按 `{conn_id, database, schema, table}` 拼源取样 SQL（与导航树同口径）交 `SampleSource`，洞察侧自己取样（D58/D59）（2026-09-18） | 「导出与界面同源」是老口径（D36），**编码放在哪**是新的：让宿主再懂一遍 JSON 分组键 / Markdown 转义，就会出现「界面说 3 项、文件里 4 项」。下钻的旧设计（宿主登记临时表）在 D58 之后是多余的一步：源取样通道本来就能从「连接 + 表名」取到数据，建临时表只是把同一件事做两遍 | 导出回执走**状态栏**（不在 280px 面板里再塞一行）；下钻的面板标题用表名（不带 schema 前缀）；导出内容随事件传（报告几 KB 级，不让宿主回读面板——避开渲染期租借冲突） |
| D61 | **实现手段的三层边界与扩展准入**：编排层（我们，必有：类型分派 / NULL 语义 / 采样口径 / 质量模型 / 治理 / 呈现）→ 内置计算层（DuckDB 内置，默认）→ 扩展层（可选）；两条硬约束——① **扩展可换实现不可换语义**（依赖扩展的结论，在「只靠前两层」时必须能给出**同一种**结论）；② **扩展产物不许成为长期格式**（快照只存「值 + 方法标识」，除非我们先有自持的序列化契约与版本）；准入四件套 = 白名单 + 版本锚定 + 静态门登记（D52）+ 降级与退出；能力准入顺序 = **规则 → 内置 SQL → 扩展**（2026-09-18，调研见 `insight-extension-notes.md`） | 动因：「能不能直接用 DuckDB 三方扩展（`dq` / `stats_duck` / `datasketches` / `stochastic` / 内置 `SUMMARIZE`）实现洞察」。调研结论：扩展能替代的是**统计函数层**（最不该自研、也不必替换），替代不了治理与呈现（三层作用域 / 信任门 / 静态门 / 采样明示 / 质量模型 / 快照链）——后者才是本模块的差异化；且我们用的是**外部编译库**（1.5.5），每个扩展都是一格分发矩阵 + 一次升级核对 | 规则引擎是**第一个扩展面**（多数「想要一个新统计量」的诉求应在规则里消化）；真正值得评估的只有「内置与规则都表达不出」的能力（当前候选：语义类型识别 / sketch 可合并 / 统计推断）；**可采取之处**（不引扩展也能拿的 10 条）见 `insight-extension-notes.md` §6 |
| D62 | **结构洞察的元数据只走驱动接口**（`MetadataBrowser` / `Database::list_*`），边界与导航树一致（2026-09-18） | 原实现自己拼 `information_schema` 方言 SQL（MySQL 用 `table_schema`、PG 用 catalog + schema、SQLite 直接回绝）——而同一件事各驱动早就做了（导航树 / 属性面板在用）。代价是三样：两套实现在同一边界上走偏（报告与树可能不一致）、驱动特有的坑每个消费方各踩一遍（MySQL 元数据列名全大写、`column_key` 是私有列）、新驱动要改两处 | 收益：SQLite 结构洞察**自动可用**（原来被「没有 information_schema」挡掉）；主键角标改用驱动算好的 `is_primary_key`。代价：逐表一次 `get_table_detail`（**N+1**，真机实测：MySQL 47 表 ≈0.45s / PG 25 表 ≈0.4s / SQLite 27 表 ≈3ms / DuckDB 10 表 ≈50ms，可接受；将来大 schema 可在驱动接口加批量方法）。另：`SqlService` 的逐条超时在这里用不上（驱动接口没有超时形参），改为整段 30s 上界 |

## 7. 并发与资源

| 资源 | 上限 / 策略 | 位置 |
| --- | --- | --- |
| 洞察并发操作 | 4，**超出即返回可读错误**（不排队） | `INSIGHT_MAX_CONCURRENT` |
| DuckDB 内存连接 | **进程级单例**，`Mutex` 全局串行 | `DuckDBManager::get_or_create_in_memory` |
| DuckDB 内存上限 | `memory_limit` 默认 **2GB**（`RDS_DUCKDB_MEMORY_LIMIT` 覆盖；非法值回退默认并告警） | `DuckDBManager::configure_connection` |
| DuckDB 溢写 | `temp_directory` = `<RDS_HOME>/tmp`（`paths::temp_dir()`）；`max_temp_directory_size` 默认 **10GB**（`RDS_DUCKDB_MAX_TEMP_SIZE` 覆盖） | 同上 |
| 临时表登记概览 | `DuckDBManager::temp_table_stats()`（按来源计数）；洞察建表时 ≥ 上限 80% 打 warn | `duckdb::{manager, temp_table, analysis}` |
| 洞察中间表 | 前缀 `tmp_i_`（**由 `duckdb::analysis` 统一产出，建表即登记**）；TTL 30 分钟 / 上限 100 由管理器判定，`cleanup_analysis_temp_tables` 真正 DROP | `engine::duckdb::{analysis, temp_table}` |
| 查询结果表 | 前缀 `tmp_q_`（**由 `duckdb::generate_unique_name` 统一产出，建表即登记**）；无 TTL / 上限；结果集被丢弃 / 替换 / 关文档时定向 `drop_temp_table`，项目切换 / 关闭时 `drop_in_memory_temp_tables(Query)` 清场（D54） | `engine::duckdb::temp_table` |
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
| 全局库不可用（信任记录写不进） | 项目层保持**未装配**，信任动作报错并挂行内提示 | 「信任状态未保存」；规则管理里仍能看到未信任横幅 |
| 全局 / 项目规则目录不存在 | 该层返回 0 条，不报错 | 视为「该层无规则」 |
| 单条规则解析失败 | 标记 `invalid` + 记错误原文，其余照常 | 规则列表红行 + 错误原文 |
| 规则文件被删 | 索引转 `missing`，规则不再生效 | 「规则文件不见了」提示 |
| 内置规则被删（如 `numeric-stats` 被覆盖成坏规则） | 该列统计计算失败并向上抛 | 该列画像报错（**基础统计依赖规则，属已知耦合**） |
| 并发超限 | 返回 `洞察分析任务过多，请稍候重试` | 行内提示 + 可重试 |
| 源表临时表被回收（宿主关结果集 / 会话结束） | 源表读不到 → 查询失败 | 「结果集已失效或已过期，请重新执行查询」（不给重试；洞察自身的样本表用完即删，无过期面；宿主侧回收时机见 K16 结果集侧） |
| 源库连接断开（表探查 / Schema） | 错误向上抛 | 错误 + 重试按钮 |
| 未打开项目 | 快照与规则管理不可用；临时表画像仍可算 | 面板提示「打开项目后可保存快照」 |
| 快照**双写失败**（正文成了、元数据没成） | **回滚正文**（要么都成、要么都不留），错误向上抛且写清已回滚（D55） | 「快照未保存：…（已回滚刚写入的正文，未留下半写快照）」 |
| 全 NULL / BLOB / ARRAY 列 | `Unknown` 变体，只出计数 | 「类型未识别」，**不生成分布** |
| 空表 | `TableQuality` 返回「表为空或无数据」 | 不产假分数 |
| 规则目录为空（首次使用） | 只有 16 条内置规则 | 全局层需用户自行创建目录 |

## 9. 测试策略

按**可测性分层**，越靠内越纯：

| 层 | 对象 | 手段 | 现状 |
| --- | --- | --- | --- |
| 纯函数 | 规则解析 / 覆盖 / 分类派生 / `plan_index` / 评分算法 / Schema 检测 | 进程内直接断言；不依赖文件系统与全局状态 | 高覆盖 |
| 文件系统 | `scan_scope_dir` / `load_from_dir` / 索引合并 | **每测试独占临时目录**（目录名含 pid + tag），避开并发下的同目录竞争 | 已覆盖 |
| 存储 | 索引表读写、快照双写与版本链 | 临时项目目录 + `ProjectDatabaseManager::open`（迁移会建表） | 已覆盖 |
| 装配 | `registry_for` 缓存、启用禁用生效 | 用**不存在的项目根**避免碰真实项目；注意缓存是进程级静态量 | 已覆盖 |
| 契约 | 零裸色 / 零裸 `px(` | `cargo test -p rds-workbench --test ui_contract` | 视图落地后纳入 |

**基线**：`cargo test -p rds-insight` 当前 **218 项**（迁移基线 53 + Phase 0–5 新增），另有集成测试 13 项；新增功能不得减少。

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
| D55 双写失败回滚 | `crates/insight/src/store/mod.rs`（`rollback_snapshot_body`）、`service/persistence.rs`（`save_column_insight_snapshot` 的接入点） |
| D56 保留期常量 | `crates/insight/src/model.rs`（`SNAPSHOT_RETENTION_DAYS`）、`jobs.rs` / `insight_view.rs`（确认框与执行取同一个常量） |
| D57 剪链头部标注 | `crates/insight/src/model.rs`（`HistoryEntryView.chain_truncated` + `HISTORY_PAGE_SIZE` 判据）、`insight_view.rs`（`history_entry_row` 的三档标记） |
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
| 6.1 交叉校验（内置 `SUMMARIZE`） | `crates/insight/tests/column_profile_e2e.rs`（`summarize` / `exact_distinct` / `stats_agree_with_duckdb_summarize`）——**只做测试级**，不把 `SUMMARIZE` 接进产品路径（它是校验参照，不是实现，D61） |
| D38～D42 快照历史 | `model.rs`（`HistoryView` / `HistoryEntryView` / `StorageStatsView` / `HISTORY_PAGE_SIZE`）、`insight_view.rs`（`render_history` + `history_entry_row` / `history_chip` / `set_history_notice` / `history_saving` / `emit_request_for_tab` 的状态落点）、`jobs.rs`（`SnapshotSaveRequested` / `HistoryRequested` → `request_snapshot_save` / `request_history`）、`service/mod.rs`（`save_column_snapshot` / `column_history_view` / `read_history`） |
| D43/D44 版本对比 | `model.rs`（`VersionDiffView` / `DiffRowView` / `DeltaView` / `VersionDiffView::between` + `display_number` / `CANONICAL_LABELS`）、`insight_view.rs`（`render_version_diff` / `diff_row_value` / `toggle_compare_version` / `dismiss_diff` / `set_compare_notice` / `is_latest_version`）、`jobs.rs`（`VersionCompareRequested` → `request_version_compare`）、`service/mod.rs`（`compare_column_snapshots` / `history_entries`）、`ui.rs`（`INSIGHT_DIFF_LABEL_WIDTH`） |
| D45～D47 存储清理 | `model.rs`（`SNAPSHOT_RETENTION_DAYS` / `CleanupOutcome` / `HistoryView.cleanup`）、`insight_view.rs`（`begin_cleanup` 的确认框 / `request_cleanup` / `history_busy_ready` / 回执行）、`jobs.rs`（`SnapshotCleanupRequested` → `request_cleanup`）、`service/mod.rs`（`cleanup_old_snapshots`）、`store/body.rs` + `store/meta.rs`（`cleanup_older_than`） |
| D48/D49 规则语义校验（早失败） | `rule_registry.rs`（`VALUE_TYPES` / `validate_rule` / `parse_rule_toml` 的校验挂点；索引器与注册表共用这一个入口）、`insight-rules/{column/null-check,table/table-column-overview,table/table-null-overview}.rule.toml`（内置规则内容改正）、`rule_executor.rs`（K11 回归：拿二进制里的规则跑真表） |
| D50 分析临时表生命周期 | `crates/engine/src/duckdb/analysis.rs`（新：`ANALYSIS_TABLE_PREFIX` / `create_analysis_temp_table` / `drop_analysis_temp_table` / `with_analysis_temp_table` / `cleanup_analysis_temp_tables` / `analysis_temp_tables`）、`duckdb/manager.rs`（`temp_table_manager()` 访问器）、`duckdb/mod.rs`（挂模块）、`services/duckdb_service.rs`（`infer_type` / `json_to_duckdb_value` 改 `pub(crate)` 共用）、`crates/insight/src/service/persistence.rs`（两处样本表改用作用域封装） |
| D51 内存闸与可观测 | `duckdb/manager.rs`（`configure_connection`：`memory_limit` / `temp_directory` / `max_temp_directory_size` + `size_setting` / `parse_size_setting` 白名单 + `temp_table_stats()` 访问器）、`duckdb/temp_table.rs`（`TempTableStats` + `TempTableManager::stats()`）、`duckdb/analysis.rs`（`warn_if_near_capacity`） |
| D52 规则 SQL 静态门 | `crates/insight/src/rule_registry.rs`（`SQL_FORBIDDEN_KEYWORDS` / `validate_rule_sql` / `is_forbidden_function` / `strip_literals_and_comments` / `sql_tokens`；挂点在 `validate_rule` 的第一条） |
| D53 项目规则信任门 | `crates/engine/migrations/global/025_insight_rule_trust.sql`（新表）、`crates/insight/src/service/rule_trust.rs`（`RuleTrust` / `trust_key` / `read_at` / `write`）、`lib.rs`（`normalized_project_key` / `project_rule_trust` / `apply_project_rule_trust` / `scan_pending_project_rules`）、`rule_registry.rs`（`PendingProjectRules` + 注册表字段）、`rule_view.rs`（`PendingRulesView` + 横幅 + 首次确认框 + `RulesEvent::TrustDecided`）、`service/mod.rs`（`decide_project_rules_trust`）、`jobs.rs`（`request_rules_trust`） |
| D54 查询结果临时表命名与回收 | `crates/engine/src/duckdb/temp_table.rs`（`generate_unique_name` / `drop_temp_table` / `quote_ident` + 三条用例）、`duckdb/mod.rs`（导出）、`services/duckdb_service.rs`（`create_temp_table_internal` 改用它 + 用例） |
| D58 源取样统一入口 | `crates/insight/src/model.rs`（`SampleSource` / `InsightTarget::{SourceColumn, SourceTable}` / `PanelData::source_sample` + `sample_table()`）、`service/persistence.rs`（`SOURCE_SAMPLE_LIMIT` / `sample_source_to_analysis_table` / `profile_source_column` / `profile_source_table`）、`service/mod.rs`（两个同步门面）、`jobs.rs`（`ProfileRequest::Source*` + 取样后回填样本表）、`insight_view.rs`（`set_source_sample` / 保存·多列·下钻改用 `sample_table`） |
| D59 文件类数据源 | `engine/src/dbi/engine/duckdb_engine.rs`（`file_reader_function` + `load_file_source` 改用它）、`engine/src/duckdb/analysis.rs`（`create_analysis_temp_table_as`）、`insight/src/model.rs`（`SampleSource::{on_duckdb, duckdb_file}`）、`insight/src/service/persistence.rs`（按 `conn_id` 分流：`sample_from_connection` / 内存库 CTAS） |
| D59 机制回归（扩展源） | `workbench/tests/insight_source_real.rs`（`extension_provided_source_is_analyzable`：Oracle 经 community 扩展 `oracle_scanner` 读入 → 取样 → 列画像 / 表探查）。**定位是机制回归，不是边界判据**：`SampleSource::on_duckdb` 这条通道（文件类用的就是它）确实能读扩展提供的源；但产品边界是「导航树所见 + 草稿箱/分析存档两块文件」（§1.1） |
| D59 三个入口接线 | `analytics_resource/src/resource_view.rs`（`can_view_stats` + `ResourcesHost::request_view_stats` + 行菜单）、`scratchpad/src/{host,scratchpad_view}.rs`（`ScratchpadHost::{can_view_stats, view_stats}` + 行菜单）、`workbench/src/components/{nav_host,resource_host,scratchpad_host}.rs`、`workbench/src/panels/shared.rs`（`insight_sample_sql` 口径一处 + `open_insight_source_table`） |
| D60 导出与下钻落地 | `insight/src/schema_view.rs`（`SchemaExportFormat` / `schema_export_file_stem`）、`insight_view.rs`（`InsightEvent::SchemaExportRequested` / `request_schema_export` / 健康条的「导出 ▾」）、`workbench/src/components/insight_actions.rs`（新：订阅事件 → 下钻与导出）、`workbench/src/panels/right.rs`（装配 + 持有订阅）、`panels/shared.rs`（`Shared::say`） |
| **取数口径（进程内直读）** | `crates/insight/src/service/mod.rs`（`result_columns_and_rows` / `value_to_json`）——**唯一**把引擎结果变成「列 + 行」的地方；四个调用点：`schema_analyzer.rs`（表 / 列）、`service/persistence.rs`（源取样 / 表列画像 / 批量评估）。**禁止**改回 JSON 契约序列化（见 K17） |
| 结构洞察入口（§10 #5） | `database/src/model.rs`（`SchemaRef`）、`database/src/nav_host.rs`（`open_insight_schema`）、`database/src/nav_view.rs`（`insight_schema_target` + 菜单项）、`workbench/src/components/nav_host.rs`、`workbench/src/panels/shared.rs`（`Shared::open_insight_schema`） |
| 结构洞察取数（D62） | `insight/src/schema_analyzer.rs`（`analyze` → `ensure_target_exists` + `fetch_metadata` → 驱动接口；`TableColumnInfo::from_detail` 把 `ColumnDetail` 映射成报告行；`report_title`）。**没有方言分支、没有任何手写 SQL**——方言差异全在驱动层 |
| 类型族判定（唯一来源） | `crates/insight/src/model.rs`（`type_base` / `is_numeric_type` / `is_datetime_type` / `is_binary_type` / `is_array_type` + `ColumnKind::of_type_name`）；列画像与表探查共用 |
| 面板头 ⚙ 入口 | `insight_view.rs`（`render_header`，无项目时禁用）、`InsightView::rules_view`（宿主接缝用） |
| 质量评分四维与**等级**（`Grade`；列级与表级共用阈值/文案） | `crates/insight/src/quality_scorer.rs` |
| 评分卡视图模型（`ScoreView` / `DimensionView`；全空列不产分） | `crates/insight/src/model.rs`（`ColumnProfileView::score`） |
| 评分卡渲染（钉在滚动区之外；四维细条） | `crates/insight/src/insight_view.rs`（`render_score_card` / `dimension_row` / `ratio_bar`） |
| 表探查 | `crates/insight/src/insight_engine.rs`（`get_temp_table_profile`；源库内省路径已于 2026-09-18 **删除**——零调用且未跑通） |
| Schema 洞察 | `crates/insight/src/schema_analyzer.rs` |
| 规则资产（16 条） | `crates/insight/insight-rules/` |
| 服务层统一入口 | `crates/insight/src/service/mod.rs`（`with_rules` 契约） |

## 11. 已知问题与后续项（**权威清单**）

> 分工：本表是**问题视角**（缺陷与取舍：什么坏了 / 为什么这样取舍）；「接口状态与收口计划」（哪些写好了但没入口、接还是删、量级）见 `insight-dev-plan.md` §10——那是**施工视角的唯一权威**，两表互相引用、不重复。

| # | 问题 | 影响 | 状态 |
| --- | --- | --- | --- |
| K1 | ~~**无 SQL 沙箱**~~ | — | **已收口**（2026-09-17）：① 原来的「禁用扩展 / 只读连接」选项（Q1-b）**在现有共享连接上不可行**——`register_external_database`（`ATTACH`）、`load_file_source`（`read_csv_auto`/`read_parquet`/`read_excel_auto`）与 `INSTALL`/`LOAD` 都跑在洞察所在的**同一个内存单例**上；② **解析期静态门已落地**（D52）：多语句 / DDL / 文件与远端表函数在解析期被拒；③ **项目规则信任门已落地**（D53）：未信任的项目规则不装配、不执行，决定记在全局库。剩下的是**已知取舍**而非缺口：静态门是防呆（黑名单有漏）、信任绑路径而非内容（`git pull` 带进的新规则不重新确认） |
| K2 | `crates/engine/insight-rules/` 是 `crates/insight/insight-rules/` 的**逐字节重复副本**，全仓零代码引用 | 后来者可能改错副本 | 待删除确认 |
| K3 | ~~`table-quality-overview` 的 SQL 引用表 `insight_column_stats`，该表不存在~~ | — | ✅ 已处置（2026-09-17）：**下线**。那条 SQL 要从一张「逐列统计物化表」里读，而静态 SQL 不可能对任意表的每一列算统计（需动态 SQL）；能力本身已在 Rust 侧（「评估全表」+ 表质量摘要）。规则文件已删，需要时从 git 历史取 |
| K4 | ~~归属偏差未归位~~ | — | ✅ 已归位（Phase 0 / 0.2）：类型 → `model::types`、仓库 → `store::{body,meta}`、`detect_extremes` → `insight_engine`、门面 → `service::{InsightService,persistence}` |
| K5 | ~~目录监听热加载未做~~ | — | ✅ 已实现（Phase 0 / 0.6，D22/D23） |
| K6 | `insight_table_reports` / `insight_schema_reports` 两张表仍**无写入者**（`save_table_quality` / `save_schema_insight` 零调用者）：**表级与 Schema 级报告存不了快照**（只有列级能存；`k8` 的历史 / 对比 / 清理都是列级那一套） | 完成度易被高估：表探查与结构 Tab 都有「保存」预期的空位 | **未接**：要接「表 / Schema 报告」的保存入口与历史视图，可照列级快照那套（D16/D55/D56 已有范本）；另需确认产品上要不要（表级快照的比对语义与列级不同——行数 / 列清单都在变） |
| K7 | ~~用户全局规则目录（`{system}/insight-rules/`）**不自动创建**~~ | 已消除：规则管理对话框在目录缺失时给「创建目录并新建规则」入口，**首次写入时建**（不在启动时预设空目录） | ✅ 已修（Phase 2 / 2.4） |
| K8 | ~~视图归属待拍板（D21）~~ **已定案**（D21 = 方案 A，2026-09-16） | 已消除：`insight` 依赖 gpui-kit，视图落 `insight/src/insight_view.rs` + `ui.rs`；`panels/` 只负责装配与发命令 | ✅ 已定案 |
| K9 | `get_column_insight_full` 并发超限时的**用户重试**由 UI 承担 | 批量场景体验 | ✅ 已修（Phase 2 / 2.2）：「评估全表」串行逐列，不会超限；进度逐列回填（D30） |
| K10 | 内置规则的**基础统计耦合**（覆盖 `numeric-stats` 会连带影响列画像） | 用户误以为只影响「那条规则」 | 文档说明（本文件 §5.3 + 使用手册） |
| K11 | ~~`null-check` 规则的 `[[quality]] field = "null_rate"` 指向**不存在的输出字段**~~ | — | ✅ 已修（2026-09-17）：① 解析期新增「门控 `field` 必须存在于 `[[output]]`」（D49），② `null-check` 的 SQL 真算出 `null_rate`（`f64?`，空表给 NULL 不误报），③ 回归用例直接拿**二进制里那一条**跑真表，钉住「50% 空值必失败 / 空表不误报」 |
| K12 | ~~`quality-score` 用 `value_type = "str"`，不在支持列表里，靠兜底当 `String` 读~~ | — | ✅ 已修（2026-09-17）：① 解析期新增 `value_type` 白名单（D48），② 内置规则里**共 8 处** `"str"` 全部改正（`quality-score` 3 × `String?`、`table-column-overview` 4 × `String`、`table-null-overview` 1 × `String`） |
| K13 | ~~`workbench` 依赖 `mock` 而 `mock` 编译不过~~ | — | ✅ 已解除（2026-09-15）；`engine/tests/transaction_affinity.rs` 的 `as_i64` 编译错误也已修（`Value` 只有 `as_int`，随 `b838ea0` 提交） |
| K14 | ~~清理旧快照后，**存活版本的 `parent_version_id` 可能指向已被删的父版**（链的起段被剪掉）~~ | — | ✅ 已处置（2026-09-17，D57）：界面上把被剪过的头部标「更早的版本已清理」（数据层不改——置空 `parent` 会丢掉「它前面还有历史」这个事实）；判据要求「列表完整」以免分页造成假阳性 |
| K15 | ~~`quality-score` 是残留规则（无代码按 id 执行；SQL 对文本列跑不通）~~ | — | ✅ 已处置（2026-09-17）：**下线**。它的自述就写着真实评分在 `quality_scorer.rs`——留着就是「同一能力两份口径」。规则文件已删，需要时从 git 历史取 |
| K16 | **临时表的回收机制与建表命名对不上**：`create_duckdb_temp_table` / `create_temp_table_internal` 建的表叫 `rs_<uuid>`，而回收全靠前缀识别（`tmp_q_` / `tmp_i_` / `temp_mock_` / `tmp_p_`）——`list_by_source`、`drop_by_source`、`lazy_cleanup_insight_tables` 对这些表**全都看不见**，`register` 触发的惰性清理因此对它们无效；且 `drop_in_memory_temp_tables` 只有 mock 调过 | 中：结果集表与洞察样本表在进程内只增不减（内存库，吃 RSS）；而文档写的「TTL 30 分钟 / 项目关闭清理」对它们不成立——文档与运行行为不一致比单纯泄漏更难查 | **已收口（2026-09-17）**：① 洞察侧——`duckdb::analysis` 统一按 `tmp_i_` 建表、用完即删（D50）；② 内存闸与可观测——`memory_limit` / `temp_directory` / 登记概览（D51）；③ 结果集侧——命名改为 `tmp_q_` + 建表即登记，并备好定向 `drop_temp_table` 与清场 `drop_in_memory_temp_tables(Query)`（D54）。**唯一还没落的是宿主接线（2026-09-18 更新）**：**清场口已接**——项目切换 / 关闭时清 `TempTableSource::Query`（`workbench/components/project_host.rs::clear_result_temp_tables`，非阻塞）；**定向口** `drop_temp_table(Query)` 仍无生产调用者（建表侧 `create_duckdb_temp_table` / `ResultService` / `execute_duckdb_analysis` 也零调用，整条链路未接 UI），随「编辑器结果集入口」一起接（施工单见 `insight-dev-plan.md` §10 #3 / #6） |
| K18 | `RenderHint`（规则的 `[render]` 段）**解析后无人消费**：它只入类型（`RuleFile.render`）与校验，视图按**数据形态**渲染，不用 `component` / `display_order` | 中（诚实性）：字段名与取值看着像契约，写规则的人会以为它生效；`display_order` 还容易被当成排序依据 | **已定案（2026-09-18）：保留 + 登记为「解析后无人消费」**（手册 §4.2 已写）。**不删的理由是兼容性**：`RuleFile` 是 `deny_unknown_fields`，删字段会让已有带 `[render]` 的用户规则**整体加载失败**（静默消失）。触发条件：将来做图表可视化时消费它；若明确不做，则删字段 + **同时**加迁移（把 `[render]` 当可忽略段） |
| K17 | **洞察侧取数照 v1 的 JSON 形态读引擎结果**：`json["batches"][0]["columns"\|"rows"]`——而 v2 `QueryResult` 的 `Serialize` 是**契约层扁平序列**（`columns` / `column_types` / `rows` / `affected_rows` / `is_read_only` / `total_rows`），**Arrow `batches` 不参与** | 高（**静默失败**）：native 驱动只填 `batches`、`rows` 字段恒空 → 结构洞察恒报「0 张表」、源取样恒报「取样没有拿到列」（导航树 / 存档 / 草稿箱「查看统计」与结构报告下钻**全走这条**）；不报错，只是拿不到数据 | **已修（2026-09-18）**：新增 `service::result_columns_and_rows`（进程内直读 `columns` + `to_rows()`），四个调用点全部改走它；回归用例用手造 Arrow 批钉住（`service::tests::rows_come_from_arrow_batches_without_a_json_round_trip`）+ 两个真机用例（`insight_schema_real` / `insight_source_real`）。**教训**：`QueryResult` 有两套形态（内存态靠 `batches`、契约态靠 `rows`）——要序列化就先把行物化（`to_rows()`），不要指望 `to_value` 带出 `batches` |
| K19 | **PostgreSQL 偶现「报告后第一条语句卡 ~30s」**：`InsightService::schema_report_view` 返回后，紧接的 `SELECT 1`（或紧跟的 DROP）有时要 30 秒才回来。实测：**仅 PG**；MySQL / SQLite / DuckDB 无此现象；**偶发**（5 轮里 3 轮出现，其中一轮全程不缺）；不在查询超时内（语句**最终成功**）；当时池状态 `size=1 idle=0 active=1` | 中：切换项目后那一次清理/下一条查询可能莫名慢 30 秒（不报错、不丢数据，很难归因） | **未修（2026-09-18 发现）**。已排除：不是慢查询（元数据那一段真机实测 0.4s 级）、不是服务端锁（语句最终成功）。**下一步方向**：洞察门面 `service/mod.rs::block_on` **每次调用新建一个 runtime**，而它驱动的是**宿主建的 sqlx 池**——怀疑是连接在“临时 runtime 被 drop”后未干净归还，池到期（~30s）才重建。验证：把同一段元数据调用放在调用方 runtime 上跑，对比是否消失；若成立，修法是把宿主的 runtime 传进来（或门面改异步） |

## 12. 待确认

| # | 问题 | 选项 |
| --- | --- | --- |
| Q1 | **规则 SQL 的安全边界**（K1） | **已定案（2026-09-17）**：① **解析期静态门**——已落地（D52）；② **项目规则信任门**——已落地（D53，首次弹确认 + 决定入全局库）；③ **诚实声明**——规则文件仍是「可信本地文件」量级，但「不可信项目」这条路已经堵上（未信任不装配）。✖ 不采用 (b) 「禁用扩展 / 只读连接」：已核查会连带砸掉产品自身的 `ATTACH` / `read_csv` / `INSTALL`（同一内存单例）；✖ 不采用 (d) 沙箱执行（成本与收益不成比例）。**若将来要再严一档**：在 `insight_rule_trust` 上加一列规则集内容指纹，指纹变了重新确认 |
| Q2 | 视图归属（K8） | 方案 A / B（开发方案 §3.1） |
| Q3 | `table-quality-overview` 的处置（K3） | **已定（2026-09-17）：下线**（K3 已处置：静态 SQL 无法对每列算统计，能力已在「评估全表」） |
| Q4 | 快照保留上限与清理默认值 | **已定（2026-09-17）：保留期固定 30 天（D56，单一常量、不做配置项）；每列上限另有 `MAX_VERSIONS_PER_COLUMN` 兜底** |
| Q5 | 快照双写失败的补偿策略（D16） | **已定（2026-09-17）：回滚已写入的那一半**（D55）——重试会让用户看到「一次点击、两条快照」，标记待清理又多一个需要清理的状态 |
| Q6 | 质量门控的「字段不存在」应报错还是静默通过（K11） | **已定（2026-09-17）：解析期报错**（D49，产物 = K11 已修）——与 `deny_unknown_fields` 同一立场 |
| Q7 | `value_type` 是否在解析期做白名单校验（K12） | **已定（2026-09-17）：做**（D48，产物 = K12 已修）——写错时应在解析期就指明「哪个值不合法 + 可用取值」 |
