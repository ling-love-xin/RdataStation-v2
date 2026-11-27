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
- 短板：3 个**已实现未接线**的孤岛（legacy 联邦、`ExplainAnalyzer`、FTS，外加半途的 minicatalog）；**Oracle 打破了「源只读」不变量**而文档仍按全覆盖写；跨源**类型降级警告**自述「未做」；跨源真机测试**全部环境门控、未设即跳过**。

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
| 引擎单测 | **356**（其中 analysis/catalog/federation/accel/file_view/temp_table 分区 **158**） | `cargo test -p rds-engine --lib -- --list` |
| 面板规模 | `mod.rs` 3280 + `layout.rs` 192 + `data_objects_tab.rs` 3122 + `layout_probe.rs` 546 | 行数统计 |
| 面板单测 | workbench lib **227**（合并基线 225），其中 data_objects / layout_probe **55** | `cargo test -p rds-workbench --lib -- --list` |
| 面板真排版断言 | 3 个 headless 用例（行高、走马灯、toolbar/状态栏位置） | `crates/workbench/src/panels/data_objects_tab/layout_probe.rs` |
| 其余包（合并后实测） | store 207 / connection 104 / connection-dialog 88 / sql 245 / editor 886 / `analysis_restart_probe` 1 | 各包 `cargo test` |

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
| 类型保真 | 半闭环 | `services/duckdb_service.rs:507`：Decimal256 直接拒；`catalog-context-model.md:102` 自述「DECIMAL→DOUBLE 只警告不阻断（**未做**）」 | 降级警告未实现 |
| 发送前预检 | 闭环 | `rds-sql::table_paths`（9 条抽取）+ `editor::exec::preflight`（13 条判定，含主源优先、内置库不参与歧义） | — |
| 临时表 / 结果集 | 闭环 | `duckdb/temp_table/temp_table.rs` 1170 + `temp_table/analysis.rs` 661；落点钉 `memory.rds_tmp`；`result_rows_reach_the_temp_table` | — |
| 物化 / 落库 / 存档 | 闭环 | `analysis/materialize.rs` 546、`temp_table/promote.rs` 368、`persist_result_table`、`duckdb/snapshot/snapshot.rs` 500 | 入口分散（面板 / 结果区 / 资产三处各一套入口） |
| 扩展治理 | 闭环 | `extensions/{inventory 474, reconcile 247, verify 174}` + `extension/plugin.rs` 502 + `duckdb_extensions_probe`；面板「装扩展」按钮 `data_objects_tab.rs:1489` | — |
| 安全面 | 闭环（有例外） | `services/policy_guard.rs` 721（editor 9 处引用）+ `refs.rs` 516 + `scrub_credentials`；`AccelSource` 手写 `Debug` 脱敏（`accel.rs:219-231`） | Oracle 例外见 P0-2 |
| 查询缓存 | 接线 | `services/sql_service.rs:634` 用 `get_query_cache()`；`connection_manager.rs:9` 用 `CacheManager`；`database/navigator_service.rs:2318` 用 L1 | 无 UI 观测：`cache_dialog` 清的是磁盘元数据缓存，不是这套 |
| EXPLAIN | 半闭环 | UI 有入口：`editor/src/view/host/exec/tx.rs:19 explain_current`，两条拒例测试（`editor/src/view/tests/format.rs:432/451`）；但 `duckdb/explain.rs:122 ExplainAnalyzer` 全仓无调用者 | 计划树解析 / 全表扫描判定 / 性能建议未接 UI |
| FTS | 孤岛 | `duckdb/search/fts.rs` 348 行，仅经 `duckdb/mod.rs:76-77` 的 `pub use search::*` 导出 | 零入口 |
| minicatalog | 未完成 | `cache/minicatalogs.rs:103` TODO（等 JSON 定义），全仓无引用 | 未接线 |
| legacy 联邦 | 死面 | `federation/legacy.rs:6` 自述「网络源拼 `ATTACH '<url>' AS 别名`，没有 `(READ_ONLY)`」；`FederationManager` 只有 `duckdb/mod.rs:64`、`lib.rs:87` 的 re-export + 自身测试 | 应删除（见 P0-1） |

### 2.1 引擎技术债全量（11 处，便于逐条销项）

| 位置 | 内容 | 归类 |
|---|---|---|
| `cache/lru_cache.rs:22` | `#[allow(dead_code)]` | 待用 / 清理 |
| `cache/query_cache.rs:34` | `#[allow(dead_code)]` | 待用 / 清理 |
| `cache/memory_guard.rs:70` | `#[allow(dead_code)]` | 待用 / 清理 |
| `services/duckdb_service.rs:361` | `#[allow(dead_code)]` | 待用 / 清理 |
| `duckdb/manager.rs:524` | `#[allow(dead_code)]` | 待用 / 清理 |
| `duckdb/temp_table/temp_table.rs:733` | `#[allow(dead_code)]` | 待用 / 清理 |
| `services/duckdb_service.rs:507` | 「Decimal256 暂不支持（转成 DOUBLE 会丢精度）」 | P0-3 类型保真 |
| `cache/minicatalogs.rs:103` | TODO：JSON 定义就绪后换 `include_str!` | P1-6 |
| `duckdb/manager.rs:185` | TODO：未来可集成 `SqlEngine` 做更严格验证 | 观察，不阻塞 |
| `services/result_types.rs:2` | `TODO(migration)`：自 v1 抽取 | 记账，不阻塞 |
| `services/sql_parser_service.rs:172` | `dialect` 暂不参与切分 | 设计取舍，文档已述 |

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

**P0-3 跨源类型保真**
- 现状：`duckdb_service.rs:507` Decimal256 直接报错；`DECIMAL→DOUBLE` 只警告「未做」（文档 `:102` 自述）。
- 影响：跨源同名列 join 静默降级或直接失败，用户拿不到「哪一列被降级」的解释。
- 验收：新增「跨源列类型对照 + 降级提示」的数据结构，预检/执行各报一次（不阻断）；补一条用例：MySQL `DECIMAL(65,30)` × PG `NUMERIC` 同名列 join 时给出提示而非报错。

### P1 —— 能力缺口（影响对标）

**P1-4 源覆盖：MSSQL 等未接**
- 现状：`AccelKind` 5 变体；`mssql` 只在 `federation/mod.rs:9` 注释里。
- 影响：源覆盖停在「MySQL / PG / SQLite / DuckDB 文件 + Oracle」，与「DataGrip 用表函数桥覆盖 JDBC 全名单」的差距主要在这里。
- 验收（照 Oracle 模板，不动内核）：新增 `AccelKind::MsSql` + `ExtensionAbility::FederationMsSql` + Secret/URL 分支 + 一条真机探针；验收看 `accel.rs` 的能力矩阵、`registry` 的 attach 断言、`extensions` 的仓库/来源断言三处同时通过。

**P1-5 `ExplainAnalyzer` 未接 UI**
- 现状：`explain.rs` 494 行（`PlanNode` / `PlanNodeType` / 全表扫描判定 / 建议）零调用者；UI 侧只有 `explain_current` 发 `EXPLAIN` 并把文本贴出来。
- 影响：用户看不到「谓词到底推没推下去、扫了多少行」——这正是与 DataGrip `#filter` / DBeaver `join.transitive.predicates` 对比时我们最缺的可见性。
- 验收：结果区「执行计划」页签接入 `ExplainAnalyzer` 解析，对每条联邦源标注「投影已推 / 过滤未推」；用例覆盖 `has_full_table_scan` 与「Oracle 过滤在扫描节点之上」两类计划。

**P1-6 FTS 孤岛 + minicatalog 未完成**
- 现状：`search/fts.rs` 348 行仅 re-export；`minicatalogs.rs:103` TODO 且无引用。
- 影响：两块能力投了人力但没有入口，属于「账面能力」。
- 验收：二选一——接入口（面板搜索走 FTS / 元数据搜索复用）或删除并记账；不建议继续保留无入口实现。

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
| workbench | `federation_sources` | `RDS_TEST_MYSQL_URL` / `RDS_TEST_SQLITE_PATH` |
| workbench | `oracle_federation` | `RDS_TEST_MYSQL_URL` / `RDS_TEST_ORACLE_URL` |
| workbench | `official_driver_real` | MySQL / PG |
| workbench | `editor_exec_real` | MySQL / PG / SQLite / DuckDB |
| workbench | `insight_source_real` / `insight_schema_real` / `insight_editor_entry` | 同 + `RDS_TEST_ORACLE_URL` |

**建议**：把这批门控用例固化成 `tools/` 下一个可复跑的「真机门控脚本」（一次给 5 个变量，逐包跑并落日志到 `.rds/tmp/`），发布前必跑；否则每次「356 绿 / 227 绿」都不含跨源真实语义。

---

## 6. 打磨顺序（五刀，均不动架构）

| 刀 | 内容 | 改动面 | 验收 |
|---|---|---|---|
| 1 | 删 `legacy` 死面 + 收 `pub use`（P0-1）**——已执行（刀27）** | `duckdb/federation/{legacy,mod}.rs`、`engine/src/lib.rs`、`duckdb/mod.rs` | `cargo check -p rds-engine` 通过；`cargo test -p rds-engine --lib` **345 passed / 0 failed**；`grep FederationManager` 在 `crates` 下清零 |
| 2 | 文档按 Oracle 现实改写（P0-2）**——已执行（刀28）** | `docs/architecture/federation/catalog-context-model.md` §8 #5 / §11；`driver-capability-matrix.md` §1 / §4 | 逐条对照 `accel.rs:171-174`、`session.rs::l2_write_refusal`、`registry.rs:558-596`；文档内「一律只读」表述不再与实现冲突 |
| 3 | 接 `ExplainAnalyzer` 进结果区（P1-5） | `crates/editor`（结果区）、`engine/duckdb/explain.rs` | 新用例：联邦源计划里「过滤在扫描节点之上」可被识别并渲染 |
| 4 | 跨源类型降级提示（P0-3） | `engine/services`、预检 | 新用例：DECIMAL 同名列 join 给提示不报错 |
| 5 | 真机门控脚本（§5） | `tools/` + `.rds/tmp/` 日志 | 一次跑通 MySQL / PG / SQLite / Oracle 四类，日志可追溯 |

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

**刀27 备注**

- 删除 `legacy` 自带的 11 条用例，故验收基线是 `345 = 356 − 11`，不是「356 不掉」。
- `cargo fmt -p rds-engine -- --check` 在整包上**本来就不干净**（示例：`duckdb/mod.rs:31` 的 `mod extension` / `pub mod extensions` 排序、`analysis/global.rs` 的 use 排序与闭包格式），属合并后既存债，与刀27 无关；本刀未做整包格式化，以免把几百行无关 diff 混进同一个 commit，另立一刀处理。
- 同名 `DataSourceType` 在 `store::persistence::driver_store` 另有独立定义（驱动类型表），workbench 走的是 `engine::persistence::driver_store::DataSourceType`，不受本次收 export 影响。

