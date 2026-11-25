# 分析引擎：单实例 + 每编辑器一条会话

> 状态：**落地中**（代码 `crates/engine/src/duckdb/instance.rs`，分支 `feat/analysis-instance-sessions`）
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
| 2g | 联邦侧别名归属核对（同实例下两个源抢同一别名会静默指到先挂的那个，加速档已用 `reserve_alias` 收口） | ⬜ |
| 3 | `rds_tmp` 自动登记：结果集 / 洞察样本建表即登进实例登记册（`drop_temp` 手动清理，两处落点都认） | ✅ |
| 3b | 把新建临时表**实际搬进** `rds_tmp`（洞察的引用模板按裸标识符拼 SQL，要整批改模板才动得干净） | ⬜ |
| 4 | 保护接线（超时值从设置读、面板显示"被中断"） | ⬜ |
| 5 | 取库脚本与文档（2.0.1 资产改名、存储格式单向） | ✅ |

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

**2g 的已知边界（同实例带来的新风险）**：`ATTACH` 出来的 catalog 是**实例级**的，别名不再只属于
某条会话。加速档已用 `reserve_alias` 按源唯一分配；联邦侧 `attach_source` 目前仍是
"别名已存在就算 Ready"，若另一个源抢先占了同名别名，查询会静默指到那个库上。收口方向：
联邦挂载也走实例别名登记（后到者让位 `别名_2`，或直接判 Failed 并说清"该别名已被别的源占用"）。

**3 的落点（登记先接上，搬家留下一刀）**：`DuckDBManager::register_temp_table` 在建表那一步
把名字**同时**登进实例登记册（`AnalysisInstance::register_temp`），于是
`instance.temps()` / `drop_temp` / `drop_all_temps` 这套"用户看得见、点得掉"的入口对结果集
（`tmp_q_`）与洞察样本（`tmp_i_` / `tmp_l_`）都生效；`drop_temp` 认两种落点
（`rds_tmp.<名>` 与历史口径 `memory.main.<名>`），并顺手销掉管理器侧的登记，避免计数漂移。
**3b（未做）**：把新建的临时表真的搬进 `rds_tmp` schema——卡在引用侧：洞察的规则 / 直方图 /
列统计模板都是 `"SELECT … FROM \"{table}\""`（裸标识符 + 双引号），表名一带 `rds_tmp.`
就会当成一张名字里带点的表而报"不存在"，所以搬家必须整批改模板，不能单改建表侧。

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
