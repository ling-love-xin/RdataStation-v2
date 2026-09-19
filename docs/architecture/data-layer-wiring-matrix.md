# 数据层接线矩阵（有什么、谁在用、缺在哪）

> **定位**：与 [`driver-capability-matrix.md`](driver-capability-matrix.md) 同一类工具——把**隐性状态摊成表**。
> 它回答的问题是：「这个方法 / 这张表 / 这套抽象，**真的有人用吗**？」
>
> 本仓反复踩同一个坑：实现了 trait、建了表、写了方法，**但没有消费者**。下面每一行都是
> 2026-09-19 实测（`grep` 全仓调用点 + 测试反证），标 ❌ 的都有据可查。
>
> **判据四级**：
> **✅ 活**（有生产消费者，非测试）· **⚠️ 半接线**（读或写缺一半）· **❌ 零调用** · **🔒 有意不做**（附理由）

---

## 1. 驱动接口面（本次已统一）

| 类型 / 方法 | 生产者 | 消费者 | 状态 |
| --- | --- | --- | --- |
| `NodeInfo`（对象列表项） | 6 个驱动的 `get_*` / `list_*` | `MetadataService` → `NavigatorService` → `NavCache` | ✅ |
| `ColumnDetail` | 驱动 `get_table_detail` | 导航树列节点、属性面板、SQL 生成、mock 导入 | ✅ |
| `IndexDetail` / `ConstraintDetail` | 驱动 `get_indexes` / `get_constraints` | 属性面板 | ✅ |
| `NodeDetail` | 驱动 `get_table_detail` | `MetadataService::list_columns`（用 `columns` 字段） | ✅ |
| `Database::list_*` | 各驱动 | `MetadataService` 回退路径（browser 无 / 空时） | ✅ |
| `MetadataBrowser::get_*` | 6 个驱动 | `MetadataService` 主路径 | ✅ |
| `ObjectRef`（引用） | 搜索结果 / 导航节点 | Quick Open、属性面板定位、`property_ref_of` | ✅ |

> 2026-09-19 之前 `Database::list_*` 返回另一套 `SchemaObject`（与 `NodeInfo` 重叠），
> 5 个驱动各写一遍「降级映射」——那是本仓最大的一处「同形类型重复」。详见
> [`core-design-current.md`](core-design-current.md) §3.1。

---

## 2. 缓存层

### 2.1 L1（进程内 `MetadataCache`，`engine/src/cache/metadata_cache.rs`）

| 类别（key） | getter / setter | 消费者 | 状态 |
| --- | --- | --- | --- |
| Catalogs | `get_catalogs` / `set_catalogs` | `NavigatorService::load_catalogs` | ✅ |
| Schemas | `get_schemas` / `set_schemas` | `NavigatorService::load_schemas` | ✅ |
| Tables | `get_tables` / `set_tables` | `NavigatorService::collect_objects` | ✅ |
| Views | `get_views` / `set_views` | 同上（分键存储） | ✅ |
| Columns（详情） | `get_columns_detail` / `set_columns_detail` | `NavigatorService::load_columns` | ✅ |
| Procedures | `get_procedures` / `set_procedures` | `NavigatorService::l1_read_routines` / `l1_write_routines` | ✅ **本轮接线** |
| Functions | `get_functions` / `set_functions` | 同上（与 Procedures 成对） | ✅ **本轮接线** |
| Sequences | `get_sequences` / `set_sequences` | `collect_objects` 的 Sequences 分支 | ✅ **本轮接线** |
| Triggers | `get_triggers` / `set_triggers` | `collect_objects` 的 Triggers 分支 | ✅ **本轮接线** |
| Indexes | `get_indexes` / `set_indexes` | —— | ❌ 零调用 |
| Constraints | `get_constraints` / `set_constraints` | —— | ❌ 零调用 |
| DataSourceMeta | `get_data_source_meta` / `set_*` | —— | ❌ 零调用 |
| RoutineSource | `get_routine_source` / `set_*` | —— | ❌ 零调用 |

> **Indexes / Constraints 为什么不接**：它们的消费方是属性面板，而属性面板按设计走
> **实时内省**（`property_panel.rs` 模块文档：「数据来自 `MetadataService`（实时内省）」）。
> 给「要看到此刻事实」的界面套一层缓存是错的。这两组（连同 key）建议**删掉**而不是接线。
> **RoutineSource 同理**：例程源码走驱动 `get_routine_source` 实时查询，缓存它会让「查看源码」看到旧定义。

### 2.2 L2（每连接 `conn_{id}.sqlite`，`engine/src/persistence/metadata_cache.rs`）

| 表 | 写入方 | 读取方 | 状态 |
| --- | --- | --- | --- |
| `schemata` | `NavCache::put_schemas` | `NavCache::{schemas, all_schemas, schema_id}` | ✅ |
| `tables` | `NavCache::put_objects`、`save_trigger_for_table`、`save_node_detail` | `NavCache::{objects, objects_chunk, object_counts}` | ✅ |
| `columns` | `NavCache::put_columns` | `NavCache::columns` | ✅ |
| `view_definitions` | `NavCache::put_objects`（`save_view`） | `NavCache::objects`（按 `table_type` 过滤） | ✅ |
| `routines` | `NavCache::put_routines` | `NavCache::routines` | ✅ **本轮接线** |
| `sequences` | `NavCache::put_sequences` | `NavCache::list_sequences` | ✅ **本轮接线** |
| `triggers` | `NavCache::put_triggers` | `NavCache::list_triggers` | ✅ **本轮接线** |
| `metadata_index` | `rebuild_schema_index`（冷启动那趟） | `search_index`、`objects_chunk`、`object_counts` | ✅ |
| `metadata_fts` | `rebuild_fts_schema`（与索引同批、schema 级幂等） | Quick Open `#` 档（`search_fts`） | ✅ |
| `indexes` / `index_columns` | ——（`save_table_indexes` 有实现，无人调） | ——（`load_table_indexes` 有实现，无人调） | 🔒 **有意不做**（属性面板实时语义） |
| `foreign_keys` / `foreign_key_columns` / `check_constraints` | ——（`save_table_constraints`） | ——（`load_table_foreign_keys`） | 🔒 **有意不做**（同上；且 `ConstraintDetail` 的四种约束散在三张表，往返语义不完整——接了会出「主键消失」这类 bug） |
| `routine_parameters` | ——（`save_routine_parameter`） | ——（`list_routine_parameters`，被 `list_routines` 内部调用） | ⚠️ 半接线（参数只在写侧缺，读侧已接进 `RoutineDetailInfo`） |
| `sync_snapshot` / `sync_operations` / `sync_marker` | —— | —— | 🔒 有意不做（增量同步未立项） |
| `compressed_metadata` / `cache_version` / `cache_migration_history` / `sync_log` / `search_history` | —— | —— | ❌ 零调用（v1 遗留，见 §5） |

---

## 3. 持久化层的方法面（规模）

`persistence/metadata_cache.rs` 约 5.6k 行、**200+ 公开方法**。按本轮实测：

| 分组 | 代表方法 | 状态 |
| --- | --- | --- |
| 规范化读写（表 / 列 / 视图 / 例程 / 序列 / 触发器 / schema） | `save_table` / `list_tables_normalized` / … | ✅ 活（本轮补齐例程 / 序列 / 触发器的读侧） |
| 索引与约束 | `save_table_indexes` / `load_table_foreign_keys` / … | 🔒 有实现、无消费者（见 §2.2） |
| 索引与搜索（V6） | `rebuild_schema_index` / `search_index` / `search_fts` / `get_objects_chunk` | ✅ 活 |
| 旧接口（v1 `metadata` 单表） | `save_table_metadata` / `save_column_metadata` / `list_tables` / `list_columns` | ❌ 零调用（被 004 的规范化表取代） |
| 同步与变更检测（V7） | `create_snapshot` / `apply_sync_operations` / `detect_changes` / `update_sync_status` | ❌ 零调用 |
| 压缩（V2） | `compress_data` / `decompress_data` | ⚠️ 内部使用（`compression_threshold`），无外部消费者 |

> **纪律推论**：本文件的每一行 ❌ 都是「先写实现、后找消费者」的产物。新增持久化表 / 方法时，
> **同一批里给出消费者**——否则它大概率会加入这张表。这不是洁癖：本轮接线就是被这些
> 「有读无写 / 有写无读」的半成品挡住了（见 §6）。

---

## 4. 缓存的「事实源」纪律（复述，但值得重复）

| 层 | 是什么 | 谁写 | 谁读 |
| --- | --- | --- | --- |
| **L1** | 进程内内存，**不是事实源** | 导航命中 L2 / 实时内省后回填 | 导航（`fresh` 模式一律跳过） |
| **L2** | 每连接 SQLite，**可管理资产** | 冷启动内省 / L2 未命中后的实时内省 | 导航、SQL 补全预载、Quick Open 搜索 |
| **L3** | 实时内省，**不是缓存** | —— | L2 未命中时的事实源；属性面板与例程源码**总是**走它 |

---

## 5. 「缓做三项」的调查结论（2026-09-19）

上一轮把它们列为「等触发条件」，这里给出调查结果。

### 5.1 稳定 ID（`TableId` / `SchemaId`）——**不做，且理由已经从「待观察」变成「设计已回避」**

调查对象：M6 资产库的「远端引用」档（最需要稳定 ID 的场景）。

结论：`ArchiveKind::TableRef` 在本仓被**有意设计为「不承诺」**——`analytics_resource/src/model.rs`
写明「本体不在本机，失效风险最高（不承诺，见架构 §2.2）」，复现强度 `Weak`，
`present.rs` 对它的展示是「无指纹」，`detail_view.rs` 的常态提示是「源表可能已变更或被删除，使用前建议立即校验」。

**所以「引用必须跨重命名存活」这个需求在当前产品语义里是被显式放弃的**，而不是「暂未实现」。
既然连「本体还在不在」都不承诺，为它引入 `TableId` 没有意义。`ObjectRef`（按名字）够用。

> **触发条件更新**：只有当产品决定把 `TableRef` 从「提示用户校验」升级为「自动定位」时，才需要稳定 ID。
> 那时要改的是 M6 的语义，不是数据层的类型。

### 5.2 L2 行类型 ↔ 驱动类型合并——**不做，转换点仍是 1 处（干净）**

调查：统计「行类型 → 驱动类型」的手工映射点。

结论：**只有 `NavCache`（`database/src/cache.rs`）一处**——`columns()` / `put_columns()` 一对，
外加 `put_objects` 的 `save_table`。`persistence` 层内部返回行类型（`ColumnDetailInfo` 等），
**不向上暴露**给导航；`save_node_detail` / `load_node_detail` 是唯一的例外（它直接吃 `NodeDetail`），
但它服务的是同一层（引擎内的 `Database::list_columns` 回退），不构成第二处映射。

→ **保持不动**。触发条件仍是「出现第三处转换点」。

### 5.3 `dbi` 与 `services` 两条执行入口——**调查完成：不是两条活入口，是「设计的那条已经死了」**

这是本轮调查最重要的发现。

**设计的执行路径**（`dbi/mod.rs` 与 `driver/mod.rs` 的模块文档都写着）：

```
commands → services → dbi → driver → native
```

**实际的执行路径**：

```
editor_exec / insight / result_service → SqlService（services）→ ConnectionManager → Database trait
```

| `dbi` 成员 | 行数 | 消费者 | 状态 |
| --- | --- | --- | --- |
| `DBI`（门面：router + session） | 71 | **无人构造**（`DBI::new` 零调用） | ❌ 死门面 |
| `QueryRouter`（多引擎路由） | 287 | 只有 `DBI` | ❌ 随 DBI 死 |
| `DriverEngine` | 90 | 只有 `QueryRouter` | ❌ 死 |
| `StreamEngine` | 363 | 只有 `QueryRouter` | ❌ 死 |
| `QueryContext` / `ExecutionContext` | 107 | 上述三者 | ❌ 死 |
| `Session` | 191 | 只有 `DBI` | ❌ 死 |
| `PerformanceCollector` | 270 | 只有 `QueryRouter` | ❌ 死 |
| `DuckDBEngine` | 704 | ① `duckdb_service::accelerate_query`（**该方法本身零调用**）；② 静态 `file_reader_function` ← **3 个 crate 在用** | ⚠️ **只有那一个静态方法活** |

**dbi 层合计 2124 行，唯一的生产消费者是 `DuckDBEngine::file_reader_function`**（把文件扩展名映射到
`read_csv_auto` / `read_parquet` / `read_excel_auto` / `read_json_auto` 的小工具）。

连带发现：**扩展清单有三套，两套死**：

| 清单 | 位置 | 状态 |
| --- | --- | --- |
| `AccelKind::extension()` + `install_sql` | `duckdb/accel.rs` | ✅ 活（加速档 / 联邦用） |
| `EXTENSION_MANIFEST`（P0/P1 优先级） | `dbi/engine/duckdb_engine.rs` | ❌ 零调用（唯一入口 `init_extensions` 只被零调用的 `accelerate_query` 调） |
| `ExtensionManager`（install / load / discover / validate） | `duckdb/extensions.rs`（约 570 行） | ❌ 零调用（**只有自己的测试在用**） |

**建议**（不在本轮实施）：

1. **不要为了「让 dbi 活起来」而把 `SqlService` 迁过去**——那是拿一个死的设计去覆盖一个活的事实，
   且 `SqlService` 已经承载了历史 / 事务 / 超时 / 通道（B13）等真实语义。
2. `dbi` 层按「**已废弃的 v1 设计**」处置：保留 `DuckDBEngine::file_reader_function`（把那个静态方法
   挪进 `duckdb/` 或 `driver/utils`），其余 2100 行**标注为待退役**或直接删除；
   同步修正 `dbi/mod.rs` 与 `driver/mod.rs` 顶部那张**与实际不符的架构图**。
3. 扩展清单收敛到 `accel.rs` 一处（或把它提为 `duckdb/extensions.rs` 的唯一实现）。

> ⚠️ 处置属破坏性改动（涉及删 2000+ 行、动模块文档、可能影响未来「多引擎路由」的预留），
> 建议**单独一批**做，并先在 `core-design-current.md` 记录一次。

---

## 6. 本轮接线做了什么（2026-09-19）

| 项 | 内容 |
| --- | --- |
| **补读侧** | `list_sequences(schema_id)` / `list_triggers(schema_id)`（持久化层此前**只有写没有读**） |
| **补写侧** | `save_sequence_name`（只存名字，避免为凑字段造数据）、`save_trigger_for_table`（自动补齐最小表行——`triggers.table_id` 是 `NOT NULL`） |
| **`NavCache` 包装** | `routines` / `put_routines`、`sequences` / `put_sequences`、`triggers` / `put_triggers` |
| **导航接线** | `collect_objects` 的 Routines / Sequences / Triggers 三个分支改为 cache-aside（L1 → L2 → L3，与表 / 视图同构） |
| **L1 接线** | 上述三类 + 例程的「过程 / 函数」双键合并读写（`l1_read_routines` / `l1_write_routines`） |
| **测试** | 4 条 L2 命中测试（「空连接管理器」反证法）+ 2 条触发器所属表往返测试 |
| **顺带修的两个真 bug** | `save_trigger` / `save_sequence` 的 SQL 引用了**不存在的 `last_accessed` 列**（`sequences` / `triggers` 建表时就没这列）——语句一直报 `no such column`，因零调用而被掩盖；接线时才暴露 |
| **有意不做** | 索引 / 约束的 L2 缓存（属性面板实时语义，见 §2.2）；`routine_parameters` 写侧 |

**效果**：展开「例程 / 序列 / 触发器」三类文件夹，从「每次回源库」变为「命中 L2（<5ms）」；
大库上这三类的首次展开仍走实时内省（内容照旧写进缓存供下次用）。

---

## 7. 已知缺口（权威清单）

| # | 级别 | 缺口 | 影响 | 建议 |
| --- | --- | --- | --- | --- |
| 1 | 🟡 | `dbi` 层 2100 行死代码 + 模块文档里的架构图与实际不符 | 新人按图理解会走错路 | 见 §5.3 的三条建议，单独一批处置 |
| 2 | 🟡 | 扩展清单三套（accel 活、manifest 死、`ExtensionManager` 死） | 同一件事三处定义，改一处漏两处 | 收敛到 `accel.rs` 一处 |
| 3 | 🟡 | L1 的 `indexes` / `constraints` / `data_source_meta` / `routine_source` 四组零调用 | 表面积虚高 | 连同 key 一并删除（属性面板走实时，见 §2.2） |
| 4 | ⚪ | `persistence` 的 v1 旧接口（`metadata` 单表那批）零调用 | 表面积虚高 | 随 §5.3 一并清理 |
| 5 | ⚪ | `routine_parameters` 只有读侧接进 `list_routines`，写侧无人调 | 例程参数永不落盘（读时为空 vec） | 属性面板若要显示参数签名，接线时补上写侧 |
| 6 | ⚪ | 缓存写侧用 `let _ =` 吞错（与 `put_objects` / `put_columns` 同一口径） | 写失败只表现为「下次仍回源」，无日志 | 批量加 `tracing::warn!`（本仓已有日志模块） |

---

## 8. 实现位置映射

| 内容 | 落点 |
| --- | --- |
| L1 缓存 | `crates/engine/src/cache/metadata_cache.rs` |
| L2 读写（规范化表） | `crates/engine/src/persistence/metadata_cache.rs` |
| L2 包装（导航域） | `crates/database/src/cache.rs`（`NavCache`） |
| 导航 cache-aside | `crates/database/src/navigator_service.rs`（`collect_objects` / `load_columns` / `index_page`） |
| 元数据唯一闸门 | `crates/database/src/metadata_service.rs` |
| 属性面板（**有意实时**） | `crates/database/src/property_panel.rs` |
| 连接池（每缓存文件一次固定开销） | `crates/engine/src/persistence/metadata_cache_pool.rs` |
| dbi 死层（见 §5.3） | `crates/engine/src/dbi/` |
