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
| 2e | `accel` 模块退役（删 `AccelSession` / `ensure_session` 缓存）与工作台消费方迁移 | ⬜ 阻塞：本分支基线缺他人未提交的 `editor_files::attach` 改动，`rds-workbench` 在此分支编不过 |
| 2f | `SessionHandle::ProcessDb` / `Accelerated` / `Federated` 三个旧变体删除；`ProcessDuckDb` 取样落 `rds_tmp` | ⬜ 与 3 同一刀（取样表与下游读端必须同处一个实例） |
| 3 | `rds_tmp` 自动登记（结果集保存 / 洞察取样落 `rds_tmp` 时登记） | ⬜ |
| 4 | 保护接线（超时值从设置读、面板显示"被中断"） | ⬜ |
| 5 | 取库脚本与文档（2.0.1 资产改名、存储格式单向） | ✅ |

**2d 的口径变化（要知道的一处契约变动）**：加速档的别名从"永远是 `rds_src`"变成"按源唯一"。第一条源仍是
`rds_src`，第二条起退为 `rds_src_2`、`rds_src_3`；未限定名由会话 `USE` 实际别名保证照旧可用，
**但手写 `rds_src.表名` 在第二条源上会失效**——面板按 `alias_for_conn(conn_id)` 显示真实别名。
改这一刀的直接原因：单实例下所有加速源共用 `rds_src` 时，第二个 `ATTACH` 会命中"已挂载"判定，
静默指到第一个源（多源查询结果串库的真实 bug）。

## 6. 验收口径

- `cargo test -p rds-engine --lib` 全绿（2.0.1 基线 336 + 本层 8 → 当前 **344 passed / 0 failed**）；
- 真机：两个编辑器会话，A 建 `CREATE TABLE` → B 刷新可见；A 建 `CREATE TEMP TABLE` → B 报
  `Catalog Error`；两会话都能读 `rds_tmp`；面板能列 `rds_tmp` 并手动清理；
- 一句 `range(1e5) × range(1e5)` 的查询在 30ms 超时下被中断，会话仍可继续用。
