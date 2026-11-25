# 分析引擎：单实例 + 每编辑器一条会话

> 状态：**已落地并合入 `main`**（2026-11-26；代码 `crates/engine/src/duckdb/instance.rs`）。
> 迁移表 `2a`–`2g`、`3`、`3b`、`4`、`5` **全部收口**，见 §迁移表。
> 口径来源：2026-11-26 用户裁决（"打开项目的分析引擎，本质就是打开了项目的分析库，全局库也挂在
> 这里，一个 SQL 编辑器就是这个实例的一个会话"）。本文只写确定的事实与边界，不写规划语。

## 1. 形态

```
进程
└── AnalysisInstance（单例，一次 open_in_memory）
    ├── 控制连接：挂底座 / 维护 rds_tmp / 建会话
    ├── catalog（实例级共享）
    │   ├── analytics      ← 项目分析库（读写）
    │   ├── rds_global     ← 全局分析库（只读）
    │   └── <源别名>       ← ATTACH 的源（只读；L2 源由会话层拦写）
    ├── rds_tmp            ← 实例级共享临时结果（手动清理）
    └── Session s1 … sN    ← 每个 SQL 编辑器一条（Connection::try_clone）
        └── TEMP 表         ← 会话级，别的会话看不到
```

| 事实 | 结论 | 证据 |
| --- | --- | --- |
| 第二条会话怎么开 | `Connection::try_clone()` | 实测（1.5.6 与 2.0.1 均可用，无文件锁问题） |
| 普通表 / ATTACH catalog 跨会话 | **可见**（实例级） | 用例 `persistent_tables_are_shared_across_sessions` |
| `TEMP` 表跨会话 | **不可见**（DuckDB 语义） | 用例 `temp_tables_stay_inside_one_session` |
| 关会话 | `TEMP` 随之消失，别人不受影响 | 用例 `closing_a_session_drops_its_temp_tables_and_spares_others` |

### 为什么不是"按项目 / 按 owner 各一条库"

旧形态是 `federation::session` 每条 owner 一条 `open_in_memory`、`accel` 每源一条连接。
后果之一是**第 ⑪ 条遗留**：`tmp_q_`（结果集临时表）在进程单例里，联邦源在另一条实例里，
`tmp_q_ JOIN mysql_src` 必然报 Catalog Error —— 不是 SQL 写错，是两份实例之间没有 JOIN 这回事。
单实例把这件事从"能力缺失"变成"会话边界问题"，而会话边界是用户能理解的（刷新可见 / TEMP 不跨会话）。

## 2. 会话边界（用户可见语义）

- 一个 SQL 编辑器 = 一条会话；关掉编辑器 = 会话关，`TEMP` 回收；
- **会话之间不通信**：A 里写的持久表，B 里刷新后才出现（B 不自动感知）；
- 要跨会话共享的临时结果 → 写 `rds_tmp.<名字>`，两边都能读；
- `rds_tmp` 的清理**只由用户手动触发**（面板入口 → `drop_temp` / `drop_all_temps`），
  引擎不静默删（用户明确要求）。

## 3. 共享一个实例的代价与保护

| 风险 | 保护 | 位置 |
| --- | --- | --- |
| 写并发互相踩 | 单写者车道（读不排队） | `AnalysisInstance::write_lane` / `AnalysisSession::run_with_timeout` |
| 一条慢查询卡住整实例 | 每句超时 → 中断**本会话**的 `InterruptHandle` | `spawn_watchdog` + `AnalysisSession::interrupt` |
| 内存被吃光 | `DuckDBManager::configure_connection` 统一 `memory_limit`（默认 2GB，`RDS_DUCKDB_MEMORY_LIMIT` 可覆盖） | `rds_store::duckdb_config` |
| 扩展重复加载 | 实例只加载一次（`ATTACH` / `INSTALL` 复用） | 已有 `accel::ensure_extension` 幂等 |
| 一句话崩掉实例 | 错误不跨会话传播；会话连接锁污染时提示重开会话 | `run_with_timeout` 错误分支 |

## 4. 别名（MountRegistry）

- 别名由**实例单点**分配，冲突后到者让位（`mysql_src` → `mysql_src_2`）；
- 底座 / 系统 catalog 名（`analytics` / `rds_global` / `rds_tmp` / `memory` / `system` …）**拒绝占用**；
  同一源重复挂载回原名（幂等）；
- 用户写下的名字与实际挂名不一致时，以映射提示呈现，**不静默改写 SQL**。

## 5. 迁移计划（本分支）

| 步 | 内容 | 状态 |
| --- | --- | --- |
| 1 | `AnalysisInstance` / `AnalysisSession` / `MountRegistry` / `rds_tmp` 登记册 / 写车道 / 超时 | ✅ `a9f0acc6`（7 用例通过） |
| 2a | kernel 改道：`SessionHandle::Instance`，加速 / 联邦通道都走实例会话 + `mount_sources` | ✅ `11ae4fec` |
| 2b | 底座挂实例 + 换项目 `refresh_bases` | ✅ `009baedf` |
| 2c | 联邦会话门面化：`FederatedSession` 只持有 `Arc<AnalysisSession>`，不再自建内存库 | ✅ `fa71c4f6` |
| 2d | 加速档别名按源唯一（`reserve_alias` + 实例 `conn_alias` 映射 + `USE` 实际别名） | ✅ 本刀 |
| 2e | 消费方迁移：工作台三处 `accel::session_for` / 编辑器直跑加速档 / 卸载 / 会话源树根全部改读实例 | ✅ 本刀（`accel::ensure_on_instance` 单一入口） |
| 2e-1 | `accel` 模块退役（删 `AccelSession` / `ensure_session` / `SESSIONS` 缓存 / `owners` / `drop_session` / `drop_all` / `cancel` / `list_tables_for`） | ✅ 本刀（用例与两条真机探针同批改到实例 API） |
| 2f | 进程面并进实例：`DuckDBManager::global()` 返回的"进程单例"就是实例 `owner = process` 那条会话的连接；`SessionHandle::{ProcessDb, Federated}` 两个旧变体删除 | ✅ |
| 2g | 联邦侧别名归属核对：挂前按实例别名登记判归属，被别的源占着的判 `Failed` 并点名占位者（不替用户改名） | ✅ 本刀 |
| 3 | `rds_tmp` 自动登记：结果集 / 洞察样本建表即登进实例登记册（`drop_temp` 手动清理，两处落点都认） | ✅ |
| 3b | 新临时表**实际建进** `rds_tmp`，靠 `SET search_path = 'main,rds_tmp'` 让裸名/双引号名照旧解析（引用模板零改动） | ✅ 本刀 |
| 4 | 保护接线：语句超时从设置读（`insight.statement_timeout_ms`，默认 30s、0 = 不限时），被中断的语句在错误里说人话 | ✅ |
| 5 | 取库脚本与文档（2.0.1 资产改名、存储格式单向 → 2026-11-26 已定**不兼容 1.5 客户端**，见升级报告 §9.1） | ✅ |

**2d 的口径变化（要知道的一处契约变动）**：加速档的别名从"永远是 `rds_src`"变成"按源唯一"。第一条源仍是
`rds_src`，第二条起退为 `rds_src_2`、`rds_src_3`；未限定名由会话 `USE` 实际别名保证照旧可用，
**但手写 `rds_src.表名` 在第二条源上会失效**——面板按 `alias_for_conn(conn_id)` 显示真实别名。
改这一刀的直接原因：单实例下所有加速源共用 `rds_src` 时，第二个 `ATTACH` 会命中"已挂载"判定，
静默指到第一个源（多源查询结果串库的真实 bug）。

**2e 的收口方式（挂载口径只剩一处）**：`accel::ensure_on_instance(source)` 同时做「申请别名 → 取源会话 →
挂载（幂等）→ `USE`」，内核加速档、编辑器直跑加速档、面板右键「挂载」三处都调它。实例侧配套：

| 入口 | 用途 |
| --- | --- |
| `AnalysisInstance::mounted_conns()` | 面板「会话源」树根：`(conn_id, 实际别名, kind)`，与 `duckdb_databases()` 一致 |
| `AnalysisInstance::list_mounted_tables_for_conn(conn_id)` | 面板叶子：控制连接查元数据，限定名按登记的 `kind` 拼 |
| `AnalysisInstance::session_for_conn(conn_id)` | 载入 / 归档「在这个源上再跑一条 SQL」；**不负责挂载** |
| `AnalysisInstance::interrupt_conn(conn_id)` | 界面「取消」按源，不按会话 id |
| `AnalysisInstance::unmount_conn(conn_id)` | 右键「卸载」：`DETACH` + 放别名 + 关该源会话，只断挂载不动连接 |

判定"这个源挂没挂"一律读 `alias_for_conn`，**不再**由 `accel::session_for(conn_id).is_some()` 回答——
那是旧的多实例口径，单实例下它必然为 `None`（加速档已不在自建会话）。

**2e-1 之后 `accel` 只剩三样东西**：`AccelSource`（组装源）、`AccelKind`（类型/命名口径）、
以及三个挂实例上的入口（`ensure_on_instance` / `remount_on_instance` / `extension_state`）。
`AccelSession`、会话表、`owner` 枚举、逐个源的 `drop_*` 全部删除——单实例下"会话"就是实例的会话。

**2f 的做法与理由（进程面不再另开一条内存库）**：77 处调用方拿的都是
`DuckDbService::get_or_create_duckdb()`（导出 / 结果集临时表 / 洞察取样）。与其逐点改签名，不如让
**那个"单例"本身就是实例里的 `owner = process` 会话的连接**：

| 位置 | 现在 |
| --- | --- |
| `AnalysisSession.conn` | `Arc<Mutex<Connection>>`（多一个 `connection_handle()` 给历史入口复用同一连接） |
| `AnalysisInstance::process_session()` | `ensure_session("process", "process")` |
| `DuckDBManager::global()` | 首次调用时取实例 → `process_session()` → 连接句柄；`OnceCell` 里存的就是它 |
| `SessionHandle` | 只剩 `None` 与 `Instance { session, label }`，`label = process / accelerated / federated` |
| `AnalysisInstance::current()` 的槽 | **强引用**（原来存 `Weak`：调用方一放手实例就没了，所有会话跟着失效） |

后果（正是第 ⑪ 条遗留要的）：`tmp_q_` / `tmp_i_` / `rds_tmp` 与源、与底座同处一个 catalog，
`tmp_i_ JOIN 源` 成立；编辑器侧 `FederatedSession::adopt(session, primary)` 只接联邦语义
（快照 / L2 写拒绝 / 错误脱敏），不再二次 `ATTACH`。离线验收用
`crates/engine/tests/process_face_catalog_probe.rs`（DuckDB 文件源，无需网络）。

**2g 的收口（别名归属先核对再挂）**：`ATTACH` 出来的 catalog 是**实例级**的，别名不再只属于
某条会话，所以挂在哪一层判归属就成了要害。现在两档共用一份实例别名登记
（`MountRegistry`：`别名 → 源连接 id`）：

- `AnalysisSession::mount_sources` 在挂之前逐个源核对 `AnalysisInstance::alias_owner(别名)`；
  **被别的连接占着的源直接判 `Failed`**，原话是「别名 X 已被另一个源（连接 Y）占用，换个别名再挂；
  这个源本批不参与查询（别名是实例级的，不会替你改名）」——宁可报失败，也不替用户改名后
  让 SQL 悄悄指到别的库；
- 名额归自己（同 `conn_id` 重复挂）→ 幂等，照旧走 `attach_source` 的"已挂上就算 Ready"；
- 核对通过的源会 `reserve_alias` 登记进实例，之后面板查挂载 / 冲突判定都以它为准。

用例：`instance.rs::an_alias_held_by_another_source_is_refused_not_hijacked`
（两个 DuckDB 文件源抢 `shared`：后到者 `Failed` 并点名 `conn-a`，且 `shared.items` 仍读到
先挂那份的 3 行，不会被 5 行那份静默顶掉）。

**3 的落点（登记先接上，搬家留下一刀）**：`DuckDBManager::register_temp_table` 在建表那一步
把名字**同时**登进实例登记册（`AnalysisInstance::register_temp`），于是
`instance.temps()` / `drop_temp` / `drop_all_temps` 这套"用户看得见、点得掉"的入口对结果集
（`tmp_q_`）与洞察样本（`tmp_i_` / `tmp_l_`）都生效；`drop_temp` 认两种落点
（`rds_tmp.<名>` 与历史口径 `memory.main.<名>`），并顺手销掉管理器侧的登记，避免计数漂移。
**3b 的落点（临时表真的搬进 `rds_tmp`，引用侧零改动）**：卡点原是引用侧——洞察的规则 /
直方图 / 列统计模板是 `"SELECT … FROM \"{table}\""`（裸标识符 + 双引号），表名一带
`rds_tmp.` 就被当成"名字里带点的表"。解法是 **`search_path`**（2.0.1 实测可用
`SET search_path = 'main,rds_tmp'`，裸名与 `"双引号名"` 都会落到 `rds_tmp`）：

- 建表侧一律落 `rds_tmp."名"`：`duckdb_service::create_temp_table_internal` /
  `create_temp_table_from_batches`、`temp_table::analysis` 的
  `create_analysis_temp_table{,_arrow,_as,_as_for}`；**返回给调用方的仍是裸名**，
  既有前缀判定（`starts_with("tmp_q_")`）与洞察模板一行没改；
- 落点规则收在 `duckdb/temp_table/temp_schema.rs`（常量 `SHARED_TEMP_SCHEMA`、
  幂等的 `ensure_shared_schema` 建 schema + 设 `search_path`、`qualified` 拼限定名）；
  模块**不叫 `shared`**——engine 已有一个顶层 `shared` crate 模块，同目录再挂会二义（E0659）；
- 设 `search_path` 的时机：实例控制连接（`open_tagged`）、每条会话（`open_session`，克隆虽
  继承设置但显式设一次更稳）、以及**单测里裸 `open_in_memory` 的连接**（由
  `ensure_shared_schema` 兜住，它们没有实例）；
- `appender` 不吃 `search_path`（它认默认 schema），Arrow 直灌两处改用
  `Connection::appender_to_db(table, "rds_tmp")`；
- 删表两处都认：`drop_temp_table` 先 `DROP … rds_tmp."名"` 再补一刀 `memory.main."名"`
  （历史表），`instance::drop_temp` 保持双落点口径；
- 枚举口径跟着放宽：`TempTableManager::list_by_source` 从只看 `main` 改成 `main` / `rds_tmp`，
  否则按来源清理与"表段列出来"都看不见新表。

用例：`analysis.rs::create_uses_the_insight_prefix_and_registers` 增加落点断言（`schema_of` = `rds_tmp`），
跨会话按裸名可见由 `tests/process_face_catalog_probe.rs` 第 5 步覆盖
（进程面建洞察表 → 编辑器会话 `SELECT … FROM "裸名"` 读到 1 行）。

**4 的接线（超时不再是写死的 30s）**：`AnalysisInstance` 的 `timeout_ms` 是原子量，
`AnalysisSession::run` 每句现读；设置页新项 `insight.statement_timeout_ms`
（预设 不限时 / 5s / 10s / 30s 默认 / 60s / 2min，越界值拒绝）落盘后经
`install_insight_statement_timeout_sink` → `DuckDbService::apply_statement_timeout_ms`
即时生效；装配层（`crates/app/src/main.rs`）注册出口，洞察面板重建时
（`apply_insight_prefs`）再兜底对齐一次（同值幂等）。面板文案：
超时中断回「语句超过 N ms 被中断（本会话已恢复，可继续执行）」，用户点取消回
「查询已被中断（…）」（原来是 DuckDB 的 `INTERRUPT` 原文，看不出是"被中断"还是"写错了"）。

**真机实证（2026-11-26，`192.168.3.138` + `D:\FossilT\T.fossil`）**：

| 探针 | 结果 |
| --- | --- |
| `duckdb_accel_probe`（MySQL / PostgreSQL / SQLite 三源） | **3 passed**：未限定名查询、源库插入实时可见、写源库被"read-only attached"拒、`CREATE TEMP TABLE` 允许、`rds_src.orders` 限定名可用 |
| `federation_credentials_probe` | **1 passed**：错误文本里的口令被抹成 `root:******@…`，原因照留 |
| `cargo test -p rds-engine --lib` | **344 passed / 0 failed** |

**2.0.1 的一处行为变化（旧断言已过期）**：MySQL 源的**表清单不再在 `ATTACH` 时定型**——
源库新建的表不重挂也能查（1.x 时代要先 `refresh`）。探针里的对应断言已改成实测口径，
`remount_on_instance` 仍保留（界面「重新挂载源库」还按它走，`TEMP` 不会因此丢）。

## 6. 验收口径

- `cargo test -p rds-engine --lib` 全绿（2.0.1 基线 336 + 本层 8 → 当前 **344 passed / 0 failed**）；
- 真机：两个编辑器会话，A 建 `CREATE TABLE` → B 刷新可见；A 建 `CREATE TEMP TABLE` → B 报
  `Catalog Error`；两会话都能读 `rds_tmp`；面板能列 `rds_tmp` 并手动清理；
- 一句 `range(1e5) × range(1e5)` 的查询在 30ms 超时下被中断，会话仍可继续用。
