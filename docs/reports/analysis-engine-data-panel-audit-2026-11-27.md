# 分析引擎 + 数据面板 完成度审计（2026-11-27）

> 基线：`main` @ `be71a476`（K4 系列合并后），审计只读，未改任何代码。
> 口径：**代码即事实**。凡结论都附文件行号或测试数字；凡「未取证」的推断显式标注，不写成结论。
> 范围：`crates/engine`（DuckDB 内核 / 三通道 / 联邦 / 临时表 / 扩展 / 安全面）+ `crates/workbench` 的数据对象面板（行模型 / 布局 / 视图 / 取数）。
> 不在范围：EDA/洞察（`crates/insight`）、结果集网格渲染、导出、连接域（对话框 / 隧道）。

---

## 0. 结论摘要

**架构主干已闭合，功能面已成型的；短板集中在四类：孤岛、不变量破口与文档滞后、类型保真、真机证据链空转。**

- 内核：三通道收口（`SessionHandle` 只剩「无会话 / 实例会话」两种），意图清单 replay、别名冲突即拒、引用计数拒绝 DETACH、发送前静态预检——设计、实现、测试三方对齐，K 系列 12 刀验收可逐条回查（`catalog-context-model.md` §9）。
- 面板：行模型（身份 / 通道 / 寿命 / 状态 / 挂载三态 / 三段分组）、窄宽丢列与走马灯（实测常量 + headless 真排版断言）、事件路径取数 + 后台 introspect + 分页、约 19 个动作位、两种空态、状态栏读数——都在。
- 短板：3 个**已实现未接线**的孤岛（legacy 联邦、`ExplainAnalyzer`、FTS，外加半途的 minicatalog）——其中 `ExplainAnalyzer` 已在刀36 接线；**Oracle 打破了「源只读」不变量**而文档仍按全覆盖写；跨源**类型降级警告**自述「未做」；跨源真机测试**全部环境门控、未设即跳过**。

### 成熟度评分（主观汇总，按证据完备度而非体验）

| 维度 | 分 | 依据 |
|---|---|---|
| 内核架构一致性 | 4.5 / 5 | 设计-实现-测试三方对齐；扣分项：文档 §11 滞后、2 个死面/孤岛 |
| 生命周期 / 持久化 | 4.5 / 5 | 意图 replay、引用计数、重开幂等；`analysis_restart_probe` 1 passed |
| 跨源能力 | 3 / 5 | L1 四类 + L2 Oracle；缺 MSSQL；类型保真弱；下推无旋钮 |
| 面板功能面 | 4 / 5 | 动作位与三态表达已超同级产品的「对象浏览」层 |
| 面板交互打磨 | 3 / 5 | 代码中未见拖拽 / 双击插入 / 列排序切换 / 键盘导航 |
| 真机证据链 | 2.5 / 5 | 跨源测试默认空转，无发布前必跑清单 |
| 代码卫生 | 3.5 / 5 | 技术债仅 11 处，但死面与孤岛未收 |

---

## 1. 证据基线（可复算）

| 项 | 数字 | 复算方式 |
|---|---|---|
| 引擎规模 | 75 个 `.rs`，≈ 3 万行 | 递归统计 `crates/engine/src` |
| 引擎技术债信号 | **11 处** | 全库 grep `TODO/FIXME/unimplemented/todo!/暂不/未实现/dead_code` |
| 引擎单测 | **357**（审计时 356；刀29 之后 explain 单测 11→19，刀27 删了 legacy 的 11 条） | `cargo test -p rds-engine --lib -- --list` |
| 面板规模 | `mod.rs` 3280 + `layout.rs` 192 + `data_objects_tab.rs` 3122 + `layout_probe.rs` 546 | 行数统计 |
| 面板单测 | workbench lib **227**（合并基线 225），其中 data_objects / layout_probe **55** | `cargo test -p rds-workbench --lib -- --list` |
| 面板真排版断言 | 3 个 headless 用例（行高、走马灯、toolbar/状态栏位置） | `crates/workbench/src/panels/data_objects_tab/layout_probe.rs` |
| 其余包（合并后实测） | store 207 / connection 104 / connection-dialog 88 / sql 245 / editor 889（审计时 886，刀36 加 3）/ `analysis_restart_probe` 1 | 各包 `cargo test` |

> 注：workbench 当前工作区为 227 而合并基线为 225，差异来自工作区内未提交的 `data_objects/layout.rs` 改动所带用例；本审计只读该文件，未修改。

### 证据分级（本报告内每条的强度）

| 级 | 含义 |
|---|---|
| **A** | 真机实测（当日台账）或 headless 真排版断言，或可确定性复现的单测 |
| **B** | 纯函数单测 + 源码级契约断言（未触真库 / 未触真窗口） |
| **C** | 环境门控「未设即跳过」的空转用例，或「已实现但无调用者」的孤岛 |

---

## 2. 分析引擎：分域判定

| 域 | 状态 | 证据（文件:行） | 缺口 |
|---|---|---|---|
| 会话收口 | 闭环 | `analysis/kernel.rs:77-87`：`SessionHandle` 只剩 `None` / `Instance`；老的 `ProcessDb` / `Federated` 已退役 | — |
| 三通道规划 | 闭环 | `analysis/mount.rs::plan_for`：Source 不挂源；Accelerated 恰一源（只能是连接或 DuckDB 文件）；Federated ≥1 源、`dedup_key` 去重、primary 必须指向已挂源；ProcessDuckDb 不挂外部源 | — |
| Catalog 快照 | 闭环 | `duckdb/catalog.rs`（299 行）+ K0a 三个纯函数单测；面板与 `source_picker` 读同一份 | — |
| 生命周期 / 引用计数 | 闭环 | `catalog_holders` / `holders_of` / `detach_refusal`；`unmount_conn(conn_id, acting)` 双语义（`None` 一律拒、`Some(owner)` 排除自己）；`file_view::detach_database_global` 过同一闸 | — |
| 意图持久 + replay | 闭环 | `analysis_sources`（034）+ `analysis_primary`（036）；`analysis_restart_probe` 1 passed（`clear_project` → `publish_project` → 按登记挂载，断言别名 / 主源 / 表清单三处回位） | — |
| 源能力矩阵 | 闭环（有例外） | `duckdb/accel.rs:57-207`：能力 / 扩展名 / `ATTACH TYPE` / scheme / 只读例外 / Secret 需求 / 限定名段数，全部收在引擎一处 | 例外见 P0-2 |
| 源覆盖 | 半闭环 | `AccelKind` = MySql / PostgreSql / Sqlite / DuckDb / Oracle；`mssql` 只出现在注释（`federation/mod.rs:9`） | MSSQL / DB2 / Mongo / ClickHouse 未接 |
| 类型保真 | 半闭环（直灌侧已闭环） | 刀30 后 `services/duckdb_service.rs:507` 的 `cast_decimal256_columns` 把 Decimal256 列 cast 成 `Float64` 并在 `map_arrow_type` 里带出降级说明（带列名 `tracing::warn!`，两条单测）；仍缺的是**预检侧**跨源同名列类型对照（30b） | 预检侧类型对照（需先补列类型事实 + 警告通道；见 §6 第 4 行） |
| 发送前预检 | 闭环 | `rds-sql::table_paths`（9 条抽取）+ `editor::exec::preflight`（13 条判定，含主源优先、内置库不参与歧义） | — |
| 临时表 / 结果集 | 闭环 | `duckdb/temp_table/temp_table.rs` 1170 + `temp_table/analysis.rs` 661；落点钉 `memory.rds_tmp`；`result_rows_reach_the_temp_table` | — |
| 物化 / 落库 / 存档 | 闭环 | `analysis/materialize.rs` 546、`temp_table/promote.rs` 368、`persist_result_table`、`duckdb/snapshot/snapshot.rs` 500 | 入口分散（面板 / 结果区 / 资产三处各一套入口） |
| 扩展治理 | 闭环 | `extensions/{inventory 474, reconcile 247, verify 174}` + `extension/plugin.rs` 502 + `duckdb_extensions_probe`；面板「装扩展」按钮 `data_objects_tab.rs:1489` | — |
| 安全面 | 闭环（有例外） | `services/policy_guard.rs` 721（editor 9 处引用）+ `refs.rs` 516 + `scrub_credentials`；`AccelSource` 手写 `Debug` 脱敏（`accel.rs:219-231`） | Oracle 例外见 P0-2 |
| 查询缓存 | 接线 | `services/sql_service.rs:634` 用 `get_query_cache()`；`connection_manager.rs:9` 用 `CacheManager`；`database/navigator_service.rs:2318` 用 L1 | 无 UI 观测：`cache_dialog` 清的是磁盘元数据缓存，不是这套 |
| EXPLAIN | 已闭环（刀36 / 刀37） | UI 入口 `editor/src/view/host/exec/tx.rs:19 explain_current`；本地通道改发 `EXPLAIN (FORMAT JSON)`，回执在 `exec/pump.rs` 处经 `exec/plan.rs` 展平成「层级 / 算子 / 对象 / 预估行数 / 下推 / 额外信息」六列表；源库方言的文本计划原样放行 | 计划树解析、「投影已推 / 过滤未推」与「整句下推 · 源端执行」已进结果区（真机形态见 §5）；全表扫描判定与性能建议仍只有引擎侧 API，未在 UI 呈现 |
| FTS | 孤岛 | `duckdb/search/fts.rs` 348 行，仅经 `duckdb/mod.rs:76-77` 的 `pub use search::*` 导出 | 零入口 |
| minicatalog | 未完成 | `cache/minicatalogs.rs:103` TODO（等 JSON 定义），全仓无引用 | 未接线 |
| legacy 联邦 | 死面 | `federation/legacy.rs:6` 自述「网络源拼 `ATTACH '<url>' AS 别名`，没有 `(READ_ONLY)`」；`FederationManager` 只有 `duckdb/mod.rs:64`、`lib.rs:87` 的 re-export + 自身测试 | 应删除（见 P0-1） |

### 2.1 引擎技术债全量（11 处，便于逐条销项）

> 2026-11-28 复核：Decimal256 那条已由刀30 销项（现为「降级 + 说明」而非「拒收」），其余 10 处仍成立；`duckdb_service.rs` 的 `allow(dead_code)` 因刀30 插入代码从 361 漂到 396。

| 位置 | 内容 | 归类 |
|---|---|---|
| `cache/lru_cache.rs:22` | `#[allow(dead_code)]` | 待用 / 清理 |
| `cache/query_cache.rs:34` | `#[allow(dead_code)]` | 待用 / 清理 |
| `cache/memory_guard.rs:70` | `#[allow(dead_code)]` | 待用 / 清理 |
| `services/duckdb_service.rs:396` | `#[allow(dead_code)]`（原记 361，刀30 后行号漂移） | 待用 / 清理 |
| `duckdb/manager.rs:524` | `#[allow(dead_code)]` | 待用 / 清理 |
| `duckdb/temp_table/temp_table.rs:733` | `#[allow(dead_code)]` | 待用 / 清理 |
| ~~`services/duckdb_service.rs:507`~~ | ~~「Decimal256 暂不支持（转成 DOUBLE 会丢精度）」~~ **已销项（刀30）**：现为 Arrow 层 cast 成 `Float64` + 带列名的降级说明 | P0-3 类型保真 |
| `cache/minicatalogs.rs:103` | TODO：JSON 定义就绪后换 `include_str!` | P1-6 |
| `duckdb/manager.rs:185` | TODO：未来可集成 `SqlEngine` 做更严格验证 | 观察，不阻塞 |
| `services/result_types.rs:2` | `TODO(migration)`：自 v1 抽取 | 记账，不阻塞 |
| `services/sql_parser_service.rs:172` | `dialect` 暂不参与切分（`split_sql` 的 `_dialect` 未用） | 设计取舍，文档已述 |

---

## 3. 数据面板：分域判定

| 域 | 状态 | 证据（文件:行） | 缺口 |
|---|---|---|---|
| 行模型 | 闭环 | `data_objects/mod.rs`：`Channel` 7 变体（31-46，各带 `label` / `hint`）、`Life` 4、`State` 3（含 `is_usable`）、`MountState` 3 且 `from_facts` 为纯函数（129-162）、`Group` 3 且 `ALL` 为唯一权威顺序（190） | — |
| 段内排序 | 闭环 | `Object::rank()`（326）：可解析 0/1/2 → 未生效 3；`sort_key()`（355）；用例 `rank_puts_the_objects_in_use_first_within_a_group` | 不支持用户切换排序（代码中未见 `sort_by` 切换） |
| 窄宽适配 | 闭环 | `layout.rs`：实测常量 `ROW_CHROME_PX=290`（15）、`CHANNEL_COL_PX=92`（21）、`PILL_COL_PX=56`（27）、`NAME_MIN_PX=120`（31）；三档 `RowTier`（37-62）；走马灯 4s / 两端停 0.18（88-92）；`should_marquee` / `marquee_offset`（98-122）+ 5 条单测 | — |
| 取数路径 | 闭环 | `collect()`（`mod.rs:1667`）在事件路径跑一次写 `Shared::data_objects`，渲染纯读（`data_objects_tab.rs:1-8` 模块注释）；源展开走 `services::source_kids_jobs` 后台 + 宿主泵回（`:85-86`）；叶子分页 `LEAF_PAGE_SIZE`（`:53-67`） | — |
| 交互面 | 闭环 | 搜索（`:735` `InputState`，按名/文件名过滤）、5 个 chips（`:926`）、段折叠、源→schema→表三级（`collapsed_schemas` `:115-120`）、约 19 个动作位（见下表）、两种空态（`:2381` 无对象 / `:2406` 条件无结果）、状态栏（`:2470`，内存 + 会话挂载读数） | 拖拽 / 双击插入 / 键盘导航：代码中未见 |
| 失败表达 | 闭环 | `PanelNote` 成功/失败分色（`data_objects_tab.rs:46-83`）；`MountState::from_facts` 的 `(false,false)` 明确按「未挂载 · 项目级」说（`mod.rs:147-151`），不冒充会话级 | 无「操作历史 / 错误清单」 |
| 面板真机 | 半闭环 | `data_source_lifecycle.rs` 1690 行：临时目录真跑（全局库 / 分析库 / Secret 目标库全隔离，模块注释 `:1-6`）；但 `federation_sources` / `oracle_federation` 需 `RDS_TEST_*`，未设即跳过 | 跨源面板链路默认不验证 |

### 3.1 面板动作位清单（约 19 项）

| 动作 | 位置 |
|---|---|
| 搜索（按名 / 文件名） | `:735` |
| 分组 chip 筛选（源 / 表 / 文件 + 全部） | `:926` |
| 加源 | `:862` |
| 存快照 | `:890` |
| 清空 | `:1114` |
| 清理临时对象 | `:1165` |
| 挂载 / 卸载 | `:1367` |
| 挂回（遮蔽底座） | `:1406` |
| 装扩展 | `:1489` |
| 在编辑器打开 | `:1805` |
| 洞察（看数据） | `:1818` |
| 作为 Mock 结构来源 | `:1832` |
| 登记为资产 | `:1876` |
| 升级为资产 | `:1890` |
| 清理这张表… | `:1905` |
| 登记为项目源 | `:2020` |
| 移除登记 | `:2034` / `:2077` |
| 属性…（身份键 / 通道 / 寿命 / 列结构） | `:2124` / `:2889` |
| 复制限定名（按 `two_segment_names` 分支） | `:2756` |

---

## 4. 缺口清单（按「先打磨成熟」排序）

### P0 —— 正确性与不变量

**P0-1 legacy 联邦死面应删除 —— 已执行（刀27，2026-11-27）**
- 现状：`duckdb/federation/legacy.rs` 529 行，`FederationManager` / `DataSourceConfig` 无任何应用调用点，但仍在 `duckdb/mod.rs:64` 与 `engine/src/lib.rs:87` 公开导出；其 `ATTACH` 生成**不带 `READ_ONLY`**（`:6` 自述）。
- 影响：任何按公开 API 使用它的新代码都会直接绕过不变量 #5（源只读），且没有测试能发现（该路径自身测试也只在拼 SQL 层）。
- 验收：`engine/src/lib.rs` 与 `duckdb/mod.rs` 中相关 `pub use` 消失；`grep -r "FederationManager" crates` 仅剩历史文档；`cargo test -p rds-engine -j 1` **345 = 356 − legacy 自带 11 条**，其余全过。

**P0-2 Oracle 与「源只读」不变量冲突，文档滞后 —— 已执行（刀28，2026-11-27）**
- 现状：`accel.rs:171-174` 明写 Oracle 扩展不接受 `read_only`；缓解三层——`federation/session.rs::l2_write_refusal`（`:141` / `:320` / 用例 `:1172`）会话层拒写、编辑器闸门、只读账号；凭据走会话级 Secret（`registry.rs:129-136`、`instance.rs:1212`）。
- 冲突：`catalog-context-model.md:183`（不变量 #5）与 `:224`（§11②「现阶段全部 `READ_ONLY`」）仍按全覆盖写；`§11③` 还把 Oracle 列为「未定」——实现已落地（`extensions/mod.rs:55-56`、`registry.rs:558-596` 用例），文档未更新。
- 影响：后续按文档做判断会得出「Oracle 也有引擎级只读」的错误结论；「必须用只读账号」这条产品约束目前只在代码注释里。
- 验收：`catalog-context-model.md` §8 #5 加 Oracle 例外与三层缓解；§11② 改为「L1 全强制、L2 靠会话层 + 账号」；§11③ 拆分为「MSSQL 驱动形态未定 / Oracle 已按 community scanner 落地」；同时确认连接对话框在 Oracle 类型上有「只读账号」提示（**未取证，需核 `connection_dialog`**）。

**P0-3 跨源类型保真 —— 部分执行（刀30，2026-11-27）**
- 原现状：`duckdb_service.rs` 的 `Decimal256` 直接报错；`DECIMAL→DOUBLE` 只警告「未做」（文档 `:102` 自述）。
- 影响：跨源同名列 join 静默降级或直接失败，用户拿不到「哪一列被降级」的解释。
- 刀30 落地（两条实测根因）：DuckDB 的 DECIMAL 宽度上限是 **38**（2.0.1 实测 `CREATE TABLE t(x DECIMAL(39,30))` → `Binder Error: DECIMAL type width must be between 1 and 38`）；且 DuckDB 的 **Arrow→DuckDB 转换不认 Decimal256**（原话 `Data type "Decimal256(65, 30)" not yet supported by Arrow-to-DuckDB conversion`）——所以「声明成 DOUBLE」也过不了 append。做法：直灌前在 Arrow 层 `cast` 成 `Float64` 再声明 `DOUBLE`，降级说明（列名 / 上限 38 / 丢小数位）走 `tracing::warn!`。
- 已达成验收：`Decimal256(65,30)` 直灌建表成功、列类型落 `DOUBLE`（`decimal256_column_survives_the_arrow_passthrough`）；`Decimal128(38,10)` 仍无损（`decimal256_is_taken_as_double_with_a_downgrade_note`）。
- 仍未做（记 **30b**）「预检侧同名列类型对照」：预检的 `Facts` 只有表名事实、没有列类型；要做需先补 `duckdb_columns()` 列类型快照 + join 键列抽取。原验收口径（MySQL `DECIMAL(65,30)` × PG `NUMERIC` 真机 join 给提示）在 30b 才能闭环。

### P1 —— 能力缺口（影响对标）

**P1-4 源覆盖：MSSQL 等未接**
- 现状：`AccelKind` 5 变体；`mssql` 只在 `federation/mod.rs:9` 注释里。
- 影响：源覆盖停在「MySQL / PG / SQLite / DuckDB 文件 + Oracle」，与「DataGrip 用表函数桥覆盖 JDBC 全名单」的差距主要在这里。
- 验收（照 Oracle 模板，不动内核）：新增 `AccelKind::MsSql` + `ExtensionAbility::FederationMsSql` + Secret/URL 分支 + 一条真机探针；验收看 `accel.rs` 的能力矩阵、`registry` 的 attach 断言、`extensions` 的仓库/来源断言三处同时通过。

**P1-5 `ExplainAnalyzer` 未接 UI（刀36 已修）**
- 原状：`explain.rs` 494 行（`PlanNode` / `PlanNodeType` / 全表扫描判定 / 建议）零调用者；UI 侧只有 `explain_current` 发 `EXPLAIN` 并把文本贴出来。
- 影响：用户看不到「谓词到底推没推下去、扫了多少行」——这正是与 DataGrip `#filter` / DBeaver `join.transitive.predicates` 对比时我们最缺的可见性。
- 做法（刀36）：本地通道（加速 / 联邦）改发 `EXPLAIN (FORMAT JSON)`；回执落到 store 之前，`exec/plan.rs` 用 `parse_json_plan` 建树、`collect_source_scans` 取外部源事实，展平成六列表（`层级 / 算子 / 对象 / 预估行数 / 下推 / 额外信息`），缩进按层级递进。命中条件三重（标题是「执行计划」+ 结果值确实以 `[`/`{` 开头且含 `"name"` + 解析成功），任一不满足就原样放行——源库方言的文本 `EXPLAIN` / `EXPLAIN QUERY PLAN` 不受影响。改写时 `column_types` / `column_annotations` 置空 vec、`key_columns` 置 `None`，守住「空 vec = 没报」与「非空时等长」两条不变式。
- 验收：3 条新用例（联邦扫描 + 扫描之上 `FILTER` → 「投影已推 · 过滤未推」落在扫描行；正文可从任一列找到、文本计划不被误判；整条回执在标题对/错两侧分别改写与原样保留）；`cargo test -p rds-editor --lib -j 1 -- --test-threads=1` **889 passed / 0 failed / 4 ignored**（基线 886 + 3）。

**P1-6 FTS 孤岛 + minicatalog 未完成**
- 现状：`search/fts.rs` 348 行仅 re-export；`minicatalogs.rs:103` TODO 且无引用。
- 影响：两块能力投了人力但没有入口，属于「账面能力」。
- 验收：二选一——接入口（面板搜索走 FTS / 元数据搜索复用）或删除并记账；不建议继续保留无入口实现。

**P1-9 测试隔离：`analysis_kernel_probe` 用例间抢 `rds_src`（刀31 首跑发现，刀33 已修）**
- 现象：`--tests` 全量跑时该 target 必红 3 条，报错都是 `别名 rds_src 已被连接 C_kernel_move 占用`；用 `--exact` 单跑每条都通过。
- 根因：进程级分析实例全进程共享，`rds_src` 是加速档**固定别名**、实例级唯一事实；先跑的用例挂上后不清，后跑的用例换个 conn_id 再挂就撞别名冲突。产品语义是对的，缺的是测试侧清理。
- 修法（刀33）：新增 `clean_accel_slate()`，每条加速档用例开头持进程级互斥锁，并把实例里残留的 `accel:*` 会话逐一 `unmount_conn`（`DETACH` + 放别名 + 关会话）。
- 验收：该 target 并行（默认线程数）**6 passed / 0 failed**，`--test-threads=1` 同样 6/0；engine 全量（`--tests --no-fail-fast -- --test-threads=1`）**21 targets、414 passed、0 failed**。

### P2 —— 面板打磨

**P2-7 交互顺滑度**
- 现状：代码中未见拖拽、双击插入、列排序切换、键盘导航（`data_objects_tab.rs` grep 无 `drag` / `on_key` / `sort_by`）。
- 影响：从「浏览」到「干活」之间还差一步——用户要双击/拖到编辑器，只能走右键「在编辑器打开」。
- 验收：双击行 → 在编辑器插入限定名（复用 `sql_ident` + `two_segment_names`）；方向键上下移动选中、Enter 触发默认动作；排序切换保留默认「在用优先」。

**P2-8 无操作历史 / 错误清单**
- 现状：失败只在底部一行 `PanelNote`，下一次动作即覆盖。
- 影响：连续挂 5 个源、失败 3 个时，用户只能逐个重试，没有「这次都发生了什么」。
- 验收：面板状态栏加「本次刷新内的事件列表」（挂载成败 / 卸载被拒话术 / 扩展安装结果），只存内存、随刷新清空。

---

## 5. 证据强度警告：跨源真机默认空转

`cargo test` 全绿**不等于**跨源行为已验证。下列用例全部 `RDS_TEST_*` 门控、未设即 `return`（部分还会打印「真机链路未验证（不算失败）」）：

| 包 | 用例 | 门控变量 |
|---|---|---|
| engine | `duckdb_accel_probe` | `RDS_TEST_MYSQL_URL` / `RDS_TEST_PG_URL` / `RDS_TEST_SQLITE_PATH` |
| engine | `federation_probe` | 同上（MySQL / SQLite） |
| engine | `federation_credentials_probe` | `RDS_TEST_MYSQL_URL` |
| engine | `read_only_flag_from_real_servers` | MySQL / PG / SQLite / DuckDB |
| engine | `type_decode_fidelity` | MySQL / PG |
| engine | `result_rows_reach_the_temp_table` | MySQL / PG / SQLite / DuckDB |
| engine | `zero_row_columns_reach_query_result` | 同上 |
| engine | `error_location_from_real_errors` | 同上 |
| engine | `oracle_probe` | `RDS_TEST_ORACLE_URL` + `RDS_TEST_SQLITE_PATH` |
| engine | `federation_explain_probe`（刀38 新增） | `RDS_TEST_MYSQL_URL` / `RDS_TEST_PG_URL` |
| workbench | `federation_sources` | `RDS_TEST_MYSQL_URL` / `RDS_TEST_SQLITE_PATH` |
| workbench | `oracle_federation` | `RDS_TEST_MYSQL_URL` / `RDS_TEST_ORACLE_URL` |
| workbench | `official_driver_real` | MySQL / PG |
| workbench | `editor_exec_real` | MySQL / PG / SQLite / DuckDB |
| workbench | `insight_source_real` / `insight_schema_real` / `insight_editor_entry` | 同 + `RDS_TEST_ORACLE_URL` |

**已执行（刀31）**：`tools/real-db-gate.ps1` —— 一次读 5 个门控变量（MySQL / PG / SQLite / DuckDB 文件 / Oracle），默认逐包（`rds-engine` / `rds-database` / `rds-workbench`）跑 `cargo test -p <包> -j 1 --no-fail-fast --tests -- --test-threads=1`（`--tests` 在 cargo 语义里含 lib 单测；`--no-fail-fast` 保证某 target 红了也跑完其余；串行必须，联邦/内核用例共用按 pid 命名的临时底座目录），每包日志与汇总落 `.rds/tmp/real-db-gate-<时间戳>*.log`。汇总行给 `targets / passed / failed / ignored / 跳过标记 / 已知噪音`。

结论分四态，关键是**「全绿」不等于「真机验收通过」**：门控变量没设齐时相关用例根本没跑（多数静默 return，日志里连跳过字样都没有），所以结论行会明确说「跨源真机语义未验证」。

`rds-database` 也有真机用例（`source_ddl_from_real_schema` / `postgres_nav_introspection` / `postgres_cross_database` / `ddl_from_real_schema`），此前的门控清单漏了这一包；脚本已覆盖。

**同一轮跑出的实证（都属「门控从不跑」的后果）**：
- `rds-database`：`5 targets / 332 passed / 0 failed / 1 ignored`。
- `rds-workbench`：本地 `--tests` 编译不过，根因是 `.gitignore` 掉的本地探针 `tests/zz_fixture_probe.rs:91` 仍给 `ConnectRequest` 写已删的 `use_duckdb_fed`（`error[E0560]`）。该文件**不入库**（`.gitignore:51 **/tests/zz_*.rs`），所以 CI 与别人的机器上看不到这条；刀34 改好后 `--no-run` → Finished 6m20s。随后本机该包又因并行改动（`analytics_resource/src/service/table_asset.rs` 未闭合分隔符）编译失败，与本审计无关。
- `rds-workbench` **实跑**（`real-db-gate-20261127-235830`，刀36 之后）：`44 targets / 405 passed / 3 failed / 0 ignored`。三条失败逐条归属如下（都不是本审计引入）：
  1. `data_source_lifecycle::save_without_federation_does_not_register_secret`（`tests/data_source_lifecycle.rs:390`，"未开启联邦加速不应注册 Secret"）——并行线（`analytics`）改动后的真实红；
  2. `ui_contract::shared_fields_are_whitelisted`（`tests/ui_contract.rs:1578`，"Shared 字段集变化：新增字段请先判定归属"）——同上，`Shared` 加了字段但白名单与 `panels-modules.md §3` 未同步；
  3. `zz_fixture_probe::probe_and_connect_all_fixtures`——**不入库的本地探针**，失败原话 `Unknown connection URL parameter 'ssl-mode'`（连接的 `ssl-mode` 参数与驱动 `parse_url` 不认），与本审计无关。

**本轮真机取证（刀37，MySQL + PG 已在 `192.168.3.138` 实连，Oracle 1521 未起）**

`EXPLAIN (FORMAT JSON)` 对外部源**能跑**，回执照旧是两列 `explain_key` / `explain_value`；但计划节点形态与刀29 的假设不一致，必须按实测改写：

| 源 | 查询形态 | 计划节点 | `collect_source_scans` | 结论 |
|---|---|---|---|---|
| MySQL | 任意（纯扫描 / 单列投影 / 带谓词 / 本地 join / 表达式） | **`MYSQL_QUERY`** 单节点 | 0 条 | MySQL 扩展把整句 SELECT 绑定成表函数，**永远**拿不到 `MYSQL_SCAN` |
| PG | 纯扫描 / 单列投影 / 带谓词 | `POSTGRES_SCAN`（带 `Table` / `Projections`，带谓词时带 `Filters`） | 1 条 | 与刀29 假设一致 |
| PG | 本地 join / 表达式 | **`POSTGRES_QUERY`** 单节点 | 0 条 | 整句下推时才退化成 `_QUERY` |

因此 `is_federated_operator` 只认 `*_SCAN` 是**错的**（MySQL 全部查询会被判成"没看到外部源"，「下推」列永远空白，`collect_source_scans` 永远是空）；刀37 已把 `*_QUERY` 纳入，并给 `SourceScanFacts` 增 `whole_statement`：整句下推时结论只说「整句下推 · 源端执行」，不再用「投影未推」替它说话。实测原文留在 `.rds/tmp/fed-explain-probe3.log`。

**已固化（刀38）**：`crates/engine/tests/federation_explain_probe.rs` 把上表钉成两条门控用例（`mysql_federated_plan_is_a_whole_statement_query` / `postgres_federated_plan_degrades_from_scan_to_query`）——断言 MySQL 的 `MYSQL_QUERY` 单节点、`whole_statement = true`、结论「整句下推 · 源端执行」；PG 简单查询的 `POSTGRES_SCAN` 带表名与 `projections_pushed`、加本地 join 后退化成 `POSTGRES_QUERY` 且拿不到表名。两个变量都设时实跑：**2 passed / 0 failed / 10.61s**。形态一变（DuckDB 升级、扩展换代）立刻红，不必等用户看到一张空白计划表才发现。

**首次带真机变量跑全量门控（engine + database）——7 条红，全部是本审计没预料到的**

`tools/real-db-gate.ps1 -Packages rds-engine,rds-database`，只设 `RDS_TEST_MYSQL_URL` / `RDS_TEST_PG_URL`（Oracle 1521 未起）：

| 包 | targets | passed | failed | 结论 |
|---|---|---|---|---|
| rds-engine（修前） | 22 | 416 | 1 | `duckdb_accel_probe::probe_postgres_source_attach` |
| rds-engine（刀40 后） | 22 | **417** | **0** | 全绿；`federation_explain_probe` 也在其中 |
| rds-database（修前） | 5 | 326 | 6 | 六条全是同一句 `驱动 postgres / mysql 应在注册表里` |
| rds-database（刀39 后） | 5 | **332** | **0** | 全绿（1 ignored） |

两条根因都是「门控用例从没在真机变量下跑过」，也正是 §1 说的「默认空转」的实证：

1. **驱动 id 过时（刀39）**。`mysql` / `postgres` 的实现早已剥离到侧车插件（`rds.driver.sqlx`），内核注册表只有 `mysql_native` / `postgres_native`（口径写在 `crates/driver/src/auto_register.rs` 自己的单测里）。四个套件的 `manager_with` 却还按老 id 取工厂，取不到就 `panic!("驱动 {driver} 应在注册表里")` —— 未安装插件时必然 6 条全红。改为返回 `Option`：取不到工厂即打印「未注册（本进程没有该实现）」并跳过。装插件后用例照常跑，语义没被削弱。
2. **secret manager 一旦被使用，`SET secret_directory` 就永久失效（刀40，P0）**。真机原话 `Invalid Input Error: Changing Secret Manager settings after the secret manager is used is not allowed!`，从 `AnalysisInstance::open_tagged` 的 `configure_connection` 抛出。成因不是测试专属：所有分析实例共用进程内的根库（`shared::duckdb_root::ensure` 返回同一个 `DuckDbRoot`），前一个实例挂过网络源（`CREATE SECRET`）之后 secret manager 即为「used」，后一个实例再配置就炸。**产品语义上等于「打开第二个文档」或「同文档挂第二个网络源」直接失败**，而错误信息只说配置失败，看不出是这条规矩。
   已实证的最小复现（`crates/store/src/duckdb_config.rs` 新单测）：同一内存库上 `configure` 两次都 OK；做一次 secret 操作（**哪怕 `CREATE SECRET` 本身失败**，manager 也被标成 used）后再 `configure` 就必错。修法：把 `secret_directory` 从批量设置里单拆出来，**只对这一句话对应的错误降级为 warn**（目录上一次已设成同一处，语义无变化），其余错误照旧致命——目录真写错仍会在挂源认证时以「找不到凭据」暴露。验收：`duckdb_config` 4 passed；`cargo test -p rds-engine --test duckdb_accel_probe`（MySQL + PG）**3 passed / 0 failed / 11.70s**（修前 1 failed）。

**首次带真机变量跑 workbench —— 8 红，其中 1 条是本审计完全没看到的 P0**

`tools/real-db-gate.ps1 -Packages rds-workbench`（同样只设 MySQL / PG）：

| 包 | targets | passed | failed | 结论 |
|---|---|---|---|---|
| rds-workbench（修前） | 44 | 400 | 8 | 见下表归因 |
| rds-workbench（刀42 后） | 44 | **404** | **4** | 剩 2 条并行 analytics 线、1 条本地探针、1 条真问题（见末行） |
| rds-workbench（刀44 后） | 44 | **408** | **0** | 4 红全部结清（`1d2befab`） |

| 红 target | 现象 | 归因 |
|---|---|---|
| `data_source_lifecycle` | `save_without_federation_does_not_register_secret` | 并行 analytics 线改动，不属本审计 |
| `ui_contract` | `shared_fields_are_whitelisted`「Shared 字段集变化」 | 同上 |
| `zz_fixture_probe` | `Unknown connection URL parameter 'ssl-mode'` | **本地不入库探针**（`.gitignore:51`），CI 看不到 |
| `insight_schema_real` / `insight_editor_entry` | `Driver 'postgres'/'mysql' not found` | 与刀39 同性质的老驱动 id；刀42 改为「同名 → 同名 `*_native`」候选 |
| `insight_source_real` | MySQL `amount`（INT）被判 **Text**，PG 同列判 Numeric | **驱动层 P0**，见下 |
| `editor_exec_real` | `CREATE TEMP TABLE … AS SELECT 42` 成功，紧接着 `SELECT answer FROM …` 报「表「rds_accel_vars」不存在：已挂载的源里都没有这张表」 | **真问题未修**：本地通道的 SQL 预检不认「本会话刚建的 TEMP 表」（错误文案出自 `editor/src/exec/preflight.rs`，不是 DuckDB 的 Catalog Error）。预检当前只有 `Result<(), String>`，没有「警告不阻断」出口（与 30b 同一障碍），修法需要产品口径 |

**P0：`mysql_native` 把结果集全字符串化（刀42）**

真机取证（临时探针走 `SqlService` + 原生驱动，`.gitignore` 挡住入库）：`SELECT CAST(1 AS SIGNED) AS amount_i, CAST(12.34 AS DECIMAL(12,2)) AS amount_d, 'x' AS note` 与 `SELECT id, status FROM mall_business.order` 在两个 MySQL 驱动（插件 `mysql` 与内核 `mysql_native`）下**每一列都是 `Utf8`**。

根因：`mysql_native_rows_to_arrow` 按**行值**猜类型，而 `mysql_async` 文本协议把数值也回成 `Value::Bytes`，于是「最宽类型」恒为 Utf8。连带后果不止洞察判型：刀30 的 Decimal 降级判定、结果区列类型徽标、取样临时表保型全部失真（临时表成了全 VARCHAR）。

修法：以列的**声明类型**为准 —— `Column::column_type()`（`mysql_async::consts::ColumnType`）映射成 Arrow：整型族（`TINY/SHORT/LONG/INT24/LONGLONG/YEAR`）→ `Int64`，`FLOAT/DOUBLE` → `Float64`，`DECIMAL/NEWDECIMAL` → `Float64`（保「数值列」判断，`Decimal128` 的精度/标度换算留待专用映射），其余 → `Utf8`；取值放宽（`Bytes` 可解析成 `i64`/`f64`）。**不做「只有 0/1 就判布尔」的启发**——实测会 `SELECT count(*)` 得 `"true"`（`count_rows` 直接红），声明类型说话即可。元数据缺失（如无行）时保留原「按值取最宽」兜底。

验收：`rds-driver --lib` **95 passed / 0 failed / 8 ignored**；`insight_source_real` MySQL 由 `Text` → **`Numeric`**，`2 passed`；`insight_editor_entry` 2 passed；`insight_schema_real` 1 passed；`editor_exec_real` 的 `SELECT 1 AS n` 由 `"true"` 回到 `"1"`，MySQL / PG 的 native 段（执行 / 批量 / 中断 / 事务 / 错误定位 / 计划 / 分段 / 影响行数 / 下发源库 / 历史 / 超时 / 切 schema）**全部 ✅**。

**刀43：预检看不见连接级临时表 → 合法语句被拦（`editor_exec_real` 的另一条红）**

真机链路（测试里临时插探针，跑完即撤）：

1. `CREATE TEMP TABLE rds_accel_vars AS SELECT 42` 成功（`error` 为空）。
2. 紧接着同一通道执行 `SELECT database_name, table_name FROM duckdb_tables() …` → 回 `[["rds_src","rds_accel_e2e"], ["temp","rds_accel_vars"]]` —— **表真实存在，执行连接看得见**。
3. 但 `SELECT answer FROM rds_accel_vars` 被拦：`表「rds_accel_vars」不存在：已挂载的源里都没有这张表（现有别名：rds_src）`。

根因：预检的事实来自 `Facts::capture → catalog::table_names → AnalysisInstance::catalog_table_names`，用的是实例的 **control 连接**；临时表是**连接级**的，只存在于执行连接。于是「看不见」被当成「不存在」。这也违反预检自己写在文件头的 §4 口径（「事实不足就不猜」）。

修法（`crates/editor/src/exec/preflight.rs`）：`Facts::local_tables_visible()` —— 本机临时区（`temp` / `memory`）**列不列得出来**；裸名一个源都没命中时，若临时区不可见则**放行**（交给 DuckDB 执行时报原话），可见时才报「不存在」。新单测 `missing_bare_name_is_let_through_when_local_area_is_invisible`，并把 `missing_bare_name_names_the_aliases` 的世界补上可见的临时区（否则它也会走进放行分支）。`exec::preflight` **14 passed**；`rds-editor --lib` **891 passed / 0 failed / 4 ignored**。

**连带发现（推翻了原用例的一条假设）**：预检放行后，`⑤ CREATE TABLE mysql.<t>` 之后立刻 `count(*)` **执行成功** —— 即「表清单在 ATTACH 时定型、必须重新挂载才看得见」这条假设在 DuckDB 2.0.1 + mysql 扩展下**不成立**（源端新建对象实时可见）。原断言 `error.is_some()` 一直是靠预检拦出来的假象。已按事实改写为「应当可见」，并给该段补 `DROP TABLE IF EXISTS` 前置（上一次跑会把表留在源库，导致重跑时 `CREATE` 报「已存在」——门控用例必须幂等）。`editor_exec_real` **3 passed / 0 failed / 19.22s**。

---

## 6. 打磨顺序（五刀，均不动架构）

> **编号对照**：下表左列「1–5」是**审计排序号**；实际提交按「刀27…刀38」编号，落地刀号见每行「已执行」括注与 §8 执行记录。两套编号一一对应，不再互换。

| 序 | 内容 | 改动面 | 验收 |
|---|---|---|---|
| 1 | 删 `legacy` 死面 + 收 `pub use`（P0-1）**——已执行（刀27）** | `duckdb/federation/{legacy,mod}.rs`、`engine/src/lib.rs`、`duckdb/mod.rs` | `cargo check -p rds-engine` 通过；`cargo test -p rds-engine --lib` **345 passed / 0 failed**；`grep FederationManager` 在 `crates` 下清零 |
| 2 | 文档按 Oracle 现实改写（P0-2）**——已执行（刀28）** | `docs/architecture/federation/catalog-context-model.md` §8 #5 / §11；`driver-capability-matrix.md` §1 / §4 | 逐条对照 `accel.rs:171-174`、`session.rs::l2_write_refusal`、`registry.rs:558-596`；文档内「一律只读」表述不再与实现冲突 |
| 3 | 接 `ExplainAnalyzer` 进结果区（P1-5）**——已执行（刀36 + 刀37）** | `crates/editor/src/view/host/exec/{tx,pump,plan}.rs`；引擎侧 `duckdb/explain.rs` 复用并修正 | 新用例：联邦源计划里「过滤在扫描节点之上」可被识别并渲染；MySQL 的整句下推形态渲染为「整句下推 · 源端执行」（真机取证见 §5） |
| 4 | 跨源类型降级提示（P0-3）**——直灌路径执行（刀30）；预检侧列类型对照未做（30b）** | `engine/services/duckdb_service.rs`；预检侧列类型对照记 30b | Decimal256 直灌从「报错」改为「Arrow 层 cast 成 DOUBLE + 带列名的降级说明」（两条单测）。30b 现状：`crates/sql` 已有 `sqlglot_rust` AST（`SelectStatement.joins[i].on` 可取等值列对），但预检 `Facts` 只有「别名 + 表名」，缺列类型；且两个预检调用点（`ports/editor_exec/{federation,accel_exec}.rs`）的 `check()` 只返回 `Result<(), String>`，**没有「警告不阻断」的出口**——要么加 notice 通道（端口层改动），要么只在执行后提示，属需要产品口径决定的一刀，未强推 |
| 5 | 真机门控脚本（§5）**——已执行（刀31）** | `tools/real-db-gate.ps1` + `.rds/tmp/` 日志 | 五变量、三包逐包跑、日志按包落盘并汇总；engine 实测 `21 targets / 414 passed / 0 failed`（5 个门控变量未设 → 结论行明示「跨源未验证」）。顺带修掉两条阻塞级测试债（刀32 断言错、刀33 用例抢 `rds_src`） |

顺序理由：1、2 是「把账做平」（不改行为，风险最低）；3、4 是「把可见性补齐」（直接对着竞品差距）；5 是「把证据补齐」，为后续任何扩展（含 MSSQL、双轨）提供可比较的基线。

---

## 7. 附录：复算命令（Windows PowerShell 7）

```powershell
$env:CARGO_HOME="D:\Dev\Rust\.cargo"
Set-Location "D:\RdataStation\RDS\RdataStation-v2"

# 用例总数（两条命令的 stdout 里看 `test result:`，不要看 stderr 的 Compiling）
cargo test -p rds-engine -j 1
cargo test -p rds-workbench --lib -j 1
cargo test -p rds-engine --test analysis_restart_probe -j 1

# 真机门控：5 个变量都不设时相关用例自行跳过（跳过不算失败）；设了才真连
pwsh -NoProfile -File tools/real-db-gate.ps1
pwsh -NoProfile -File tools/real-db-gate.ps1 -Packages rds-engine   # 只跑 engine

# 清单（按分区计数）
cargo test -p rds-engine --lib -- --list 2>$null | Out-File -Encoding utf8 .rds\tmp\eng-list.txt
cargo test -p rds-workbench --lib -- --list 2>$null | Out-File -Encoding utf8 .rds\tmp\wb-list.txt
```

注意：PowerShell 会把 cargo 的 `Compiling` stderr 判成 `NativeCommandError` 导致 exit≠0，一律以 stdout 的 `test result:` / `Finished` 为准；`--lib` 首次会编译 `gpui-pre`，内存紧张时可能崩 `0xc0000409`，重试即过。

---

## 8. 执行记录

| 刀 | 日期 | 改动 | 验收 | 状态 |
|---|---|---|---|---|
| 刀27 | 2026-11-27 | 删 `duckdb/federation/legacy.rs`（529 行）；收 `duckdb/mod.rs` 与 `engine/src/lib.rs` 的 `pub use`（`DataSourceConfig` / `DataSourceType` / `FederationManager`）；`federation/mod.rs` 的「一律只读」硬约束补 Oracle 例外（对齐 `accel.rs:171-174`、`session.rs::l2_write_refusal`） | `cargo fmt -p rds-engine -- --check` 对本刀触及的行无差异（包内既有差异见下）；`cargo check -p rds-engine -j 1` Finished 1m16s；`cargo test -p rds-engine --lib -j 1` **345 passed / 0 failed**（41.08s） | 完成 |
| 刀28 | 2026-11-27 | 文档对账：`catalog-context-model.md` §8 #5（源只读加 L2 Oracle 例外）、§11②③（L1 强制 / Oracle 已落地 / 只剩 MSSQL 驱动形态未定）、§12 变更记录；`driver-capability-matrix.md` §1 联邦行写操作列、§4 联邦硬约束段；顺手删 §4 表里重复的一条 SQLite 行 | 文档内「一律只读」表述与 `accel.rs:171-174`、`session.rs::l2_write_refusal`、`registry.rs:558-596` 三处对上；纯文档，无编译面 | 完成 |

| 刀29 | 2026-11-27 | 重写 `duckdb/explain.rs`：计划解析改走 `EXPLAIN (FORMAT JSON)`（DuckDB 默认 `EXPLAIN` 是框线图，原实现按行缩进建树且只在栈空时入栈、栈长恒 ≤1，除根外全部平铺；还误把第 0 列 `explain_key` 当计划正文）；`PlanNode` 增 `operator` / `extra_info`；新增 `SourceScanFacts` + `collect_source_scans` 判「投影已推 / 过滤未推」；外部源算子不再计入 `has_full_table_scan`；`duckdb/mod.rs`、`engine/src/lib.rs` 导出新类型 | `cargo check -p rds-engine` 通过；`cargo test -p rds-engine --lib` **353 passed**（explain 单测 11 → 18，含真实 DuckDB 链路与 L1/Oracle 台账等价形状） | 完成（引擎侧）；结果区接入记 29b（`explain_current` 走异步执行通道，需先摸清结果集注入） |

| 刀30 | 2026-11-27 | `duckdb_service.rs`：`duckdb_type_of` 拆成 `map_arrow_type`（返回类型 + 降级说明），新增 `cast_decimal256_columns` 在直灌前把 Decimal256 列 cast 成 `Float64`；`create_temp_table_from_batches` 收集降级并带列名 `tracing::warn!` | 两条实测：DuckDB DECIMAL 上限 38（`DECIMAL(39,30)` 被拒）；Arrow→DuckDB 不认 Decimal256（`not yet supported by Arrow-to-DuckDB conversion`）。新增 2 条单测；`cargo test -p rds-engine --lib` **355 passed** | 完成（直灌路径）；预检侧同名列类型对照记 30b |

| 刀31 | 2026-11-27 | 新增 `tools/real-db-gate.ps1`：读 5 个 `RDS_TEST_*`；逐包（engine / database / workbench）跑 `cargo test -p <包> -j 1 --no-fail-fast --tests -- --test-threads=1`；每包独立日志 + 汇总日志；解析 `test result:` 出 targets / passed / failed / ignored，另计「跳过标记」「已知噪音」；结论四态（含「全绿但门控未设 → 跨源未验证」） | 首跑暴露两条既有红（刀32、刀33 分别修掉）。最终 `-Packages rds-engine`：**21 targets / 414 passed / 0 failed**；5 个门控变量未设 → 结论行明示「跨源真机语义未验证」 | 完成（脚本 + 报告） |

| 刀32 | 2026-11-27 | `federation/session.rs` 测试：`assert!(without_bases.is_empty())` → `assert_eq!(without_bases, "-")`；`dcdf13b9` 把原本正确的 `assert!(!without_bases.is_empty())` 改错，注释写「清空后指纹就是空串」，与 `bases_fingerprint()` 空底座返回 `"-"` 直接矛盾 | 该条 `--exact` 复跑 1 passed；`cargo test -p rds-engine --lib -j 1 -- --test-threads=1` **356 passed / 0 failed** | 完成 |

| 刀33 | 2026-11-27 | `tests/analysis_kernel_probe.rs`：新增 `clean_accel_slate()`——进程级互斥锁 + 逐条 `unmount_conn` 清 `accel:*` 残留；加速档 4 条用例开头持锁 | 该 target 并行（默认线程数）**6 passed / 0 failed**，串行同；engine 全量 `--tests --no-fail-fast -- --test-threads=1` **21 targets / 414 passed / 0 failed** | 完成（P1-9 关闭） |

| 刀34 | 2026-11-27 | 本地探针 `crates/workbench/tests/zz_fixture_probe.rs`：删掉 `ConnectRequest { use_duckdb_fed: Some(false) }` 一行（K4c 删字段时没跟着改，`error[E0560]`）。注意该文件被 `.gitignore:51 **/tests/zz_*.rs` 挡住、**不入库**，所以这一改只落在工作区，不产生代码提交 | `cargo test -p rds-workbench --tests -j 1 --no-run` → **Finished 6m20s**。随后该包因并行改动 `analytics_resource/service/table_asset.rs` 未闭合分隔符再次编译失败，与本审计无关 | 完成（本地环境） |

| 刀36 | 2026-11-27 | 29b：`exec/tx.rs` 的本地通道（加速 / 联邦）改发 `EXPLAIN (FORMAT JSON)`（源库档仍按方言）；新增 `exec/plan.rs`——`explain_json_payload` 从两列回执抓计划正文，`plan_table` 用 `parse_json_plan` + `collect_source_scans` 先序展平成六列表（`层级 / 算子 / 对象 / 预估行数 / 下推 / 额外信息`），`flatten_explain_result` 在 `pump.rs` 的 `store.push` 之前改写，并把 `column_types` / `column_annotations` 置空、`key_columns` 清 `None`；标题常量 `EXPLAIN_TITLE` 与 `tx.rs` 同源 | `cargo check -p rds-editor -j 1` Finished 40.32s；新增 3 条用例全绿；`cargo test -p rds-editor --lib -j 1 -- --test-threads=1` **889 passed / 0 failed / 4 ignored** | 完成（P1-5 关闭） |

| 刀37 | 2026-11-27 | 真机取证（MySQL 3306 + PG 5432 实连，Oracle 1521 未起）推翻刀29 的一条假设：MySQL 的外部源查询**只出** `MYSQL_QUERY`（整句下推），PG 简单查询出 `POSTGRES_SCAN`、本地 join / 表达式退化成 `POSTGRES_QUERY`。改 `duckdb/explain.rs`：`is_federated_operator` 纳入 `*_QUERY`；`SourceScanFacts` 增 `whole_statement`，整句下推时 `summary()` 给「整句下推 · 源端执行」而不是「投影未推」；`walk_source_scans` 对 `_QUERY` 形态不再按 `Projections` / `Filters` 硬判。editor 侧补一条 `MYSQL_QUERY` 展平用例 | `explain` 单测 19 passed；`cargo test -p rds-engine --lib -j 1 -- --test-threads=1` **357 passed / 0 failed**；editor `exec::plan::` **4 passed**；两文件 `rustfmt --check` 干净 | 完成（纠正刀29 假设） |

| 刀38 | 2026-11-27 | 新增 `crates/engine/tests/federation_explain_probe.rs`（入库，门控）：把刀37 的真机形态钉成两条用例——MySQL 单节点 `MYSQL_QUERY` + `whole_statement` + 「整句下推 · 源端执行」；PG 简单查询 `POSTGRES_SCAN`（表名 + `projections_pushed`）、加本地 join 退化成 `POSTGRES_QUERY`（无表名）。同时断言 `EXPLAIN (FORMAT JSON)` 回执恒为 `explain_key` / `explain_value` 两列、正文是 JSON 数组 | 两变量都设时实跑 **2 passed / 0 failed / 10.61s**；`rustfmt --check` 干净 | 完成（真机结论可复算） |

| 刀39 | 2026-11-28 | 四个真机套件（`ddl_from_real_schema` / `source_ddl_from_real_schema` / `postgres_cross_database` / `postgres_nav_introspection`）的 `manager_with` 在取不到驱动工厂时不再 panic，改为跳过：`mysql` / `postgres` 已剥离到侧车插件，内核只有 `*_native` | 带 MySQL+PG 变量实跑 `rds-database` **5 targets / 332 passed / 0 failed / 1 ignored**（修前 326 passed / 6 failed） | 完成（门控从"必红"变"未装插件即跳过"） |

| 刀40 | 2026-11-28 | 修 P0：`crates/store/src/duckdb_config.rs` 的 `secret_directory` 改单条执行，仅对 `secret manager is used` 这一种错误降级为 warn —— 进程内根库共享，前一个实例挂过网络源后此设置永久不可改（真机原话 `Changing Secret Manager settings after the secret manager is used is not allowed!`），会让第二个文档 / 第二个网络源直接「配置分析实例失败」。新增两条单测（二次配置 OK / secret manager 用过后再配置不得失败） | `cargo test -p rds-store --lib duckdb_config` **4 passed**；`cargo test -p rds-engine --test duckdb_accel_probe`（MySQL+PG）**3 passed / 0 failed / 11.70s**（修前 1 failed） | 完成（P0 已修，真机验证） |

| 刀41 | 2026-11-28 | 纯文档：§2.1 技术债逐条复核（Decimal256 已由刀30 销项、`duckdb_service.rs` `allow(dead_code)` 行号 361→396、`sql_parser_service.rs:172` 的 `dialect` 仍未参与切分），§2「类型保真」改为「半闭环（直灌侧已闭环）」，§1/§5 基线随刀39/40 更新 | 文档与代码逐条对照；无代码改动 | 完成（账做平） |

| 刀42 | 2026-11-28 | 修 P0（本审计没看到的）：`mysql_native` 按**声明类型**出 Arrow，不再按行值猜。真机实证 `CAST(1 AS SIGNED)` / `DECIMAL` / `TINYINT` 全出 `Utf8`（`mysql_async` 文本协议把数值回成 `Value::Bytes`），导致洞察把 INT 判 Text、Decimal 降级判定与结果区列类型失真、取样临时表全 VARCHAR。改：`declared_arrow_type`（`ColumnType` → Arrow）、取值放宽到 `Bytes` 解析、**不做 0/1 判布尔启发**（会把 `count(*)` 得 `true`）。同时三个洞察真机套件在老驱动 id 上退到同族 `*_native`；`insight_editor_entry` 断言改读宿主待打开目标（源目标按设计不落右栏） | `rds-driver --lib` **95 passed**；gate engine **23/418/0**、database **5/332/0**、workbench **44/404/4**（8 红→4 红）；`insight_source_real` MySQL 由 Text→**Numeric** | 完成（P0 已修，真机验证） |

| 刀43 | 2026-11-28 | `crates/editor/src/exec/preflight.rs` 加 `Facts::local_tables_visible()`：裸名一个源都没命中时，若本机临时区（`temp`/`memory`）**列不出来**就放行（看不见 ≠ 不存在，符合预检头部 §4 口径）——临时表是连接级的，预检的 control 连接看不到执行连接建的 `TEMP TABLE`（真机：`duckdb_tables()` 里确有 `temp.rds_accel_vars`）。同时按真机事实改写 `editor_exec_real` ⑤：源端新建表在旧 ATTACH 会话**实时可见**（原「不该可见」是预检拦出来的假象），并补 `DROP TABLE IF EXISTS` 让该段幂等 | `exec::preflight` **14 passed**（含新用例）；`rds-editor --lib` **891 / 0 / 4 ignored**；`editor_exec_real` **3 passed / 0 failed / 19.22s**；gate workbench **44 / 404 / 4**（4 红 = 2 条并行 analytics 线 + `zz_fixture_probe` 本地探针 + `mock_generator` 文件占用偶发） | 完成（真机验证） |

| 刀44 | 2026-11-28 | workbench 剩余 4 红结清。①`data_source_lifecycle`：旧用例假设「未开联邦加速就不注册 Secret」被 2026-11-27 的新口径推翻（`data_source_service.rs:820-827` 注释 + `secret_integration::has_federable_extension` 静态名单：有 DuckDB 扩展就注册），用例改名 `save_registers_secret_by_capability_not_by_switch`，断言 `mgr.list().len() == 1`。②`ui_contract`：`shared.rs:440` 新增的 `stale_publish_dialog` 补进白名单，`panels-modules.md` M6 段补该流转字段。③**真缺陷（产品面）**：`mysql_native::new_with_tls` 直接 `mysql_async::Opts::from_url`，而该库遇未知键**直接报错**；连接对话框按 `mysql`（sqlx）词汇把 `ssl-mode` / `ssl-ca` / `ssl-cert` / `ssl-key` 拼进 URL，内核把 `mysql` 解析到本驱动后 `parse_url` 必失败 —— 真机复现「选 MySQL + SSL 连不上」。改为 `strip_sqlx_ssl_params` 剥掉这四个键后重解析一次（证书与校验仍走 `TlsRequest`），新增单测。④本地探针 `zz_fixture_probe` 夹具改用内核真实注册的 `mysql_native` / `postgres_native`（`mysql` / `postgres` 已是侧车插件，未装即 `Driver not found`） | `rds-driver --lib` **96 passed / 8 ignored**；`data_source_lifecycle` **30**、`ui_contract` **12**、`mock_generator` **10**、`zz_fixture_probe` MySQL/PG/SQLite/DuckDB 四夹具全连上；gate **workbench 44 targets / 408 passed / 0 failed**、**driver 1 / 96 / 0 / 8 ignored** | 完成（`1d2befab`） |

> **环境故障留痕（2026-11-28，非代码）**：刀43 之后跑全量门控先报 `targets=0`（编译期 `os error 1455 页面文件太小`），改逐靶跑后 workbench/store 大面积红，报错统一为 `no such table: schema_version` / `no such table: editor_contexts`。最小化取证：①Rust 探针 `Connection::open` → `CREATE TABLE` → `sqlite_master`，`CREATE` 返回 Ok 但表为空、db **0 字节**（WAL / DELETE 两种模式一样）；②Python 在**同一目录**建库同样 0 字节，**同一份 Python 写 C 盘 → 8192 字节、表正常**；③普通文件写 D 盘 1MB 内容读回完全一致，`fsutil file createnew` 成功；④删除 `target\zz_*` 后 `Test-Path` 仍为 True。结论：当次 D 盘「普通文件写正常、SQLite 落盘不生效」，属文件系统层异常，与代码无关；磁盘为单块 477GB SSD、`Healthy/OK`、NTFS、剩 83GB，System 日志近 300 条无 Ntfs/disk 错误。用户处理后已恢复（同探针 D/C 均 8192 字节），刀44 的全量门控在恢复后跑出。**教训**：这类「大面积 sqlite 建表失败」不要当回归查，先做跨实现（Rust/Python）+ 跨盘（D/C）对照。

**刀42 留下的红**（全部已销项：刀43 修第 1 条，刀44 修其余 3 条）
- `editor_exec_real::the_editor_execution_port_runs_a_query_on_every_configured_database`：本地通道建 `TEMP TABLE` 成功，随后 `SELECT` 被**预检**拦成「表「rds_accel_vars」不存在：已挂载的源里都没有这张表」——预检不认本会话刚建的临时表；`check()` 只有 `Result<(), String>`，没有「警告不阻断」出口（与 30b 同一障碍）。**已由刀43 修掉**（见 §5 刀43：`local_tables_visible` 放行 + ⑤ 假设按真机改写）。
- `data_source_lifecycle` / `ui_contract`：并行 analytics 线，旧断言与新口径冲突（Secret 改为「按能力注册」）。**已由刀44 按新口径结清**（改名 `save_registers_secret_by_capability_not_by_switch` + 白名单补 `stale_publish_dialog`）。
- `zz_fixture_probe`：本地不入库探针（`ssl-mode` 参数不被识别），CI 无此文件。**已由刀44 修驱动侧真缺陷**：`mysql_native` 剥掉 sqlx 专属 SSL 参数后重解析，夹具改用 `*_native`。


**刀27 备注**

- 删除 `legacy` 自带的 11 条用例，故验收基线是 `345 = 356 − 11`，不是「356 不掉」。
- `cargo fmt -p rds-engine -- --check` 在整包上**本来就不干净**（示例：`duckdb/mod.rs:31` 的 `mod extension` / `pub mod extensions` 排序、`analysis/global.rs` 的 use 排序与闭包格式），属合并后既存债，与刀27 无关；本刀未做整包格式化，以免把几百行无关 diff 混进同一个 commit，另立一刀处理。
- 同名 `DataSourceType` 在 `store::persistence::driver_store` 另有独立定义（驱动类型表），workbench 走的是 `engine::persistence::driver_store::DataSourceType`，不受本次收 export 影响。

