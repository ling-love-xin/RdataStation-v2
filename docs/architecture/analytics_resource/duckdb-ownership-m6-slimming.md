# DuckDB 所有权收口 + M6 减法设计

> 状态：草案，2026-11-26。分支 `feat/duckdb-ownership-m6-slim`，工作树
> `D:\RdataStation\RDS\rds-v2-ownership`（从 `main` 49df3338 拉出）。
>
> 前置事实：DuckDB 2.0.1 已在 main（`0df23fc2`、`a9f0acc6`、`42d5dd99`），
> `docs/architecture/database/analysis-instance-sessions.md` 已把“单实例 + 每编辑器一条会话”落地。
> 本文处理它**没覆盖完的所有权问题**，以及 M6 为绕开多连接而堆出来的搬运代码。

## 1. 结论先行

1. **一个 DuckDB 文件在一个进程里只能有一条根连接**；同文件第二条连接即使是
   `READ_ONLY` 也会失败（`File is already open ... same PID`）。这不是偏好，是硬约束。
2. 因此 `ProjectDatabaseManager` / `GlobalDatabaseManager` 不能再各自持 DuckDB 写连接；
   `AnalysisInstance` 必须**独占**项目分析库与全局分析库。
3. 全局库必须挂 **RW**，否则同实例内 `CREATE TABLE analytics.x AS SELECT * FROM rds_global.y`
   不成立；全局写权限改用**会话角色守卫**收住，不用“只读挂载”收。
4. M6 的 `ProjectDuckConn`、parquet 中转、跨连接搬表、引用打开时短命 ATTACH
   全部退化成同一实例内的 `CREATE TABLE ... AS SELECT`；引用行 / 版本快照 / pin 不动。
5. 资产归档的全局文件、全局表回收站、全局版本恢复是本分支的第二条主线，设计见
   `global-file-asset-design.md`。

## 2. 探针证据（2026-11-26，DuckDB 2.0.1 / Windows）

日志：`.rds/tmp/duckdb-handle-probe-2.log`（临时 example 已删除，不留源码树）。

| # | 场景 | 结果 |
| --- | --- | --- |
| 1 | 项目库已有 RW 连接，另一条内存连接 `ATTACH` 同一文件 `READ_ONLY` | **失败**：`File is already open ... same PID` |
| 3 | 同一文件两条 RW 连接 | **失败**：同上 |
| 5 | 实例同时只读挂项目 + 全局，再 CTAS 到项目 | **失败**：项目库只读 |
| 8 | 单实例独占项目 RW + 全局 RO | 成功 |
| 9 | 单实例内 `CREATE TABLE analytics.x AS SELECT * FROM rds_global.t` | **成功（3 行）** |
| 10/11/12 | 项目 RW + 全局 RW，双向 CTAS | **成功** |

结论：**“多连接 + 实例再挂载”在生产上不可能同时成立**；`attach_bases` 的 READ_ONLY
与 `ProjectDatabaseManager` / `GlobalDatabaseManager` 的写连接只能留一侧。

## 3. 改造前所有权矩阵（2026-11-26 第一刀之前）

| 资源 | 改造前谁打开 | 形态 | 问题 |
| --- | --- | --- | --- |
| 项目 SQLite `project.db` | `ProjectDatabaseManager` | 池 | 无 |
| 项目分析库 `analytics.duckdb` | `ProjectDatabaseManager::open` | `ProjectDuckdbConnection` RW 长连接 | 与实例 `ATTACH analytics` 冲突 |
| 项目分析库只读口 | `open_read_only` | 只读 DuckDB 连接 | 同文件第二连接，仍会撞单句柄 |
| 全局 SQLite `global.db` | `GlobalDatabaseManager` | 池 | 无 |
| 全局分析库 `analytics.duckdb` | `GlobalDatabaseManager::new` | `GlobalDuckdbConnection` RW 长连接 | 与实例 `ATTACH rds_global` 冲突 |
| 全局 DuckDB 读写 | `GlobalInstanceStore::production()` | 取 `GlobalDatabaseManager` 的写连接 | 连接不在实例里，M6 无法 CTAS |
| 分析实例 | `AnalysisInstance` | 内存库 + `analytics`/`rds_global` ATTACH | 两个底座都 `READ_ONLY`，且常挂不上 |

## 4. 目标所有权

```text
进程
└── shared::duckdb_root（唯一 DuckDB 根连接，memory）
    ├── analytics      ← 当前项目分析库（RW）
    ├── rds_global     ← 全局分析库（RW）
    ├── rds_tmp        ← 实例级临时 schema（由 AnalysisInstance 建）
    └── 源别名          ← ATTACH（只读，实例挂）
        │
        ├── AnalysisInstance control session（复制根连接）
        │   ├── system session（M6 作业 / 迁移 / 全局维护；第 2 步落地）
        │   └── user sessions（每个 SQL 编辑器一条；受写守卫）
        ├── ProjectDatabaseManager 会话（`USE analytics`）
        └── GlobalDatabaseManager 会话（`USE rds_global`）
```

| 组件 | 改造后 |
| --- | --- |
| `shared::duckdb_root` | **唯一根连接**：ATTACH / DETACH `analytics` / `rds_global`，给会话克隆 |
| `ProjectDatabaseManager` | SQLite 池 + **`USE analytics` 会话句柄**；不再打开 DuckDB 文件；`analysis_db_path` 保留为路径定义 |
| `GlobalDatabaseManager` | SQLite 池 + **`USE rds_global` 会话句柄**；不再打开 DuckDB 文件；`init_duckdb_tables` 复用句柄跑迁移 |
| `AnalysisInstance` | 从根连接复制控制会话，负责 `rds_tmp` / 源挂载；**不再自己 open 文件** |
| `GlobalInstanceStore` | SQLite 走 `GlobalDatabaseManager`；DuckDB 暂取 Manager 句柄（第 3 步改实例 system session） |
| `ProjectDuckConn` | 仍存在，第 3 步改名 `ProjectCtx` 并收敛到实例会话 |
| `GlobalDuckdbConnection` / `ProjectDuckdbConnection` | 生产只持会话句柄；`new` 仅保留给测试 / 一次性迁移窗口 |

## 5. 启动与切项目顺序

### 5.1 启动（第一刀已落地）

1. `GlobalDatabaseManager::new`：`ensure_write(None, Some(global))` 挂全局库，拿 `USE rds_global`
   句柄，跑全局 SQLite + DuckDB 迁移；
2. `ProjectDatabaseManager::open`：`ensure_write(Some(project), Some(global))` 挂项目库
   （全局已在根上则幂等），拿 `USE analytics` 句柄，跑项目 SQLite + DuckDB 迁移；
3. 首次要分析实例时：`AnalysisInstance::open` 从同一根连接复制控制连接，建 `rds_tmp`，
   对已挂底座跳过、只补挂缺席的；
4. `process session` / user session 都由实例 `try_clone()` 出来。

第 2 步（会话角色 + 写守卫）再把项目 / 全局 DuckDB 迁移挪到 `system session`；
当前仍在 Manager 的句柄上跑，语义等价（同一根连接），只是还没有角色隔离。

关键：任何 DuckDB 文件在根连接之外不能再有长连接。全局 / 项目管理器如果还要做
“打开即迁移”，迁移必须复用根连接句柄，或整体挪到实例 `system session`。

### 5.2 切项目（第一刀已落地）

1. 等写车道空闲、停掉该项目所有 user session 的正在执行语句；
2. `shared::duckdb_root::ensure(Some(new_project), Some(global))`：由根连接摘旧 `analytics`、挂新；
3. 关旧项目 SQLite 池；
4. 开新项目 SQLite 池；
5. `AnalysisInstance::refresh_bases()` 再补挂缺席底座并刷新面板。

不允许“旧项目还挂着、新项目已打开”的中间态；否则用户会对着上一个项目的表查数。
根连接摘旧时会因旧会话仍存活而报错——这正是需要先停旧会话的原因。

## 6. 全局写权限：RW + 会话守卫

全局必须挂 RW，但**只有 system session 能写**。user / process session 在 `run` 前过闸：

| SQL | user / process | system |
| --- | --- | --- |
| `SELECT` / `WITH` / `EXPLAIN` / `DESCRIBE` | 允许 | 允许 |
| `CREATE TEMP TABLE` | 允许（会话级） | 允许 |
| 写 `analytics`（项目库） | 允许（现有编辑器能力） | 允许 |
| 写 `rds_global` 的 DDL / DML | **拒绝** | 允许 |
| `ATTACH` / `DETACH` | **拒绝**（实例控制连接专属） | 允许（仅控制路径） |
| `USE rds_global` / `SET search_path` 含 `rds_global` | **拒绝** | 允许 |
| `COPY ... TO` 指向全局库文件 | 拒绝 | 允许 |

实现口径：

- `AnalysisSession` 增 `role: User | Process | System | Control`；
- 守卫在 `run` / `run_with_timeout` 入口，**不是**在 UI 层；
- 语句分类复用 workspace 已有的 `sqlglot-rust` AST，不写正则；
- 解析不了 / 多语句里有一条可疑 → **默认拒绝**，错误文案说清“全局库只允许系统写入，请把它分发/固化到项目后再改”；
- 错误不跨会话传播，和现有超时 / 中断语义一致。

> 为什么不用 DuckDB 的 READ_ONLY：同文件只能一条根连接，全局 RO 时 M6 的跨库 CTAS 不成立。
> 为什么不用“第二条实例”：同文件第二连接直接失败，探针已证。

## 7. M6 减法清单

### 7.1 可以直接删的搬运层

| 位置 | 现在 | 改造后 |
| --- | --- | --- |
| `engine/duckdb/temp_table/promote.rs::write_table_across_connections` | 项目连接 `COPY TO parquet` → 目标连接 `read_parquet` → `CREATE TABLE` | 删除；调用方改 `CREATE TABLE <目标> AS SELECT * FROM <源>` |
| `analytics_resource/src/global/promote.rs` | 表型升级导出 parquet 再建全局表 | `CREATE TABLE rds_global.<table> AS SELECT * FROM analytics.<source>` |
| `analytics_resource/src/global/distribute.rs` | 全局 `COPY TO parquet` → 项目 `read_parquet` | `CREATE TABLE analytics.<target> AS SELECT * FROM rds_global.<source>` |
| `analytics_resource/src/global/refs.rs` | 引用打开时短命 ATTACH / 取样 | 直接 `SELECT ... FROM rds_global.<source_table>` |
| `analytics_resource/src/jobs/global.rs` | `ProjectDuckConn(duck_conn, sqlite_pool)` | `ProjectCtx(sqlite_pool, session)` |
| `workbench/services/persist_jobs.rs` | 调 `write_table_across_connections` | 调实例 CTAS |
| `ProjectDatabaseManager::open_read_only` | 只读 DuckDB 连接 | 删除；读路径从实例取 |
| `GlobalInstanceStore::production` | 取全局管理器写连接 | 取实例 system session |

### 7.2 不能删的语义层

- `analytics_resources` 登记行；
- `analytics_resource_versions.snapshot`；
- `analytics_global_refs` 按项目版本 pin / release；
- 表资产 `analytics_table` 一版一张表；
- 全局文件资产 / 全局表回收站（见 `global-file-asset-design.md`）；
- `KeepVersions` 裁剪守卫。

减法只减“数据怎么搬”，不减“资产是什么、版本怎么留”。

### 7.3 改造后的表型链路

```text
项目临时表 / 查询
  → CREATE TABLE analytics.<持久名> AS SELECT ...        （同实例）
  → 登记项目表资产 v1
  → 再保存新表 + v(n+1)
  → 回退只改登记指向（表留库）

升级全局：
  CREATE TABLE rds_global.<版本表> AS SELECT * FROM analytics.<源表>
  → 登记全局表资产 + 版本快照
  → 按版本 pin / 裁剪

分发 / 引用 / 固化：
  CREATE TABLE analytics.<目标> AS SELECT * FROM rds_global.<版本表>
```

文件型链路仍走文件复制 / 文件资产，不套表 CTAS。

## 8. 资产归档遗留（本分支第二主线）

### 8.1 P0

1. **全局文件资产**：`data/system/resources/` + `versions/<asset_id>/<v>/`；文件本体 + 可选派生表；
   文件级版本恢复。设计见 `global-file-asset-design.md`。
2. **全局表回收站**：`delete_global` 拆 `trash_global` / `purge_global`；删除只软删当前活跃行，
   表留库；恢复预检表；永久删除才 `DROP`。
3. **全局文件回收站**：`data/trash/global/`，RDS 自管，不走 Windows 回收站；
   文件 payload + manifest；恢复 / 永久删除。
4. **全局版本浏览 / 回退 UI**：文件资产“恢复此版本”；表资产“恢复此版本”走 CTAS。
5. **`global/list.rs:70`** 的“全局 resources”陈旧注释修正。

### 8.2 P1

6. 派生表回指文件版本加显式列 `derived_file_id` / `derived_file_version`（不靠 JSON 查询）。
7. 固化标记 `solidified_resource_id` 持久化，UI 区分已固化 / 未固化引用版本。
8. 文件引用（`file_ref`）后置；先把“分发副本 + 派生表引用”跑稳。
9. 项目表型历史表的 `KeepVersions` 裁剪（现在只裁全局表与文件历史副本）。
10. reference 历史版本还原仍按现有降级口径，不在本分支扩。

## 9. 实施顺序

| 步 | 内容 | 验证 | 状态 |
| --- | --- | --- | --- |
| 0 | 本设计 + 资产遗留清单（本文件） | 文档评审 | ✅ 已完成 |
| 1 | 所有权移交：两个 Manager 去文件连接；根连接统一挂两库；实例复用 | 串行单测 + 真机双库 CTAS 探针 | ✅ 已完成（2026-11-26） |
| 2 | 会话角色 + 全局写守卫 | 守卫单测：`USE rds_global` / DDL / DML / ATTACH 全拒 | ⬜ 待做 |
| 3 | M6 表型减法：`ProjectCtx`、CTAS、删 parquet 路径 | `cargo test -p rds-analytics-resource --lib` | ⬜ 待做 |
| 4 | 全局表回收站 `trash_global` / `purge_global` | 表资产 trash/restore/purge 用例 | ⬜ 待做 |
| 5 | 全局文件资产 + 文件回收站 + 版本恢复 | 文件 hash、恢复、pin 依赖用例 | ⬜ 待做 |
| 6 | UI：回收站、版本恢复、分发文件 | `dialog_window` / `panel_window` | ⬜ 待做 |

每步单独 commit；不把 1–3 和 4–6 混在一个提交里。

## 9.1 第一刀实施记录（2026-11-26）

代码落点：

- `crates/shared/src/duckdb_root.rs`：进程级 `OnceLock<Mutex<Option<Arc<DuckDbRoot>>>>`；
  `ensure` / `ensure_write` 挂载 `analytics` / `rds_global`（`READ_WRITE`），
  `handle(alias)` 复制 `USE alias` 会话，`control()` 给实例控制连接，`reset()` 释放；
  `isolated(project, global)` 给测试一条未登记的独立 `memory` 根。
- `ProjectDatabaseManager::open`：写路径不再 `ProjectDuckdbConnection::new`，
  改 `ensure_write(Some(project), Some(global))` + `handle(PROJECT_ALIAS)`；
  `open_read_only` 仍只走 SQLite，不碰 DuckDB。
- `GlobalDatabaseManager::new`：改 `ensure_write(None, Some(global))` + `handle(GLOBAL_ALIAS)`；
  `init_duckdb_tables` 复用该句柄执行迁移，删掉二次 `open_duckdb_for_migration`。
- `AnalysisInstance::open_tagged`：`analysis` 标签走进程根连接；测试标签走 `isolated`
  （否则多个用例共享 `main` 持久表，engine 库会稳定挂 4 个计数用例）。
- `AnalysisInstance::refresh_bases`：先 `ensure` 摘旧挂新，再 `attach_bases` 补缺；
  不再自己 `DETACH` 绕过根注册表。
- `attach_bases`：`READ_WRITE`；已挂别名跳过，避免 “already attached”。
- `analysis::global::open_global_session`：不再 `open_mounted` 另开文件；改为根连接实例会话
  `USE rds_global`，owner 加 `global:` 前缀避免撞 `process` 键。
- `ProjectDuckdbConnection::close`：只放会话克隆，不再删 `-wal` / `-shm`（文件归根连接）。

验证（全部串行）：

| 命令 | 结果 |
| --- | --- |
| `cargo test -p rds-shared duckdb_root` | 1 passed |
| `cargo test -p rds-store -j 1 -- --test-threads=1` | 203 passed / 2 ignored |
| `cargo test -p rds-engine -j 1 --lib -- --test-threads=1` | 348 passed / 0 failed |
| `cargo test -p rds-engine --test analysis_db_exclusive_probe` | 1 passed（项目 + 全局 + 实例共用一根连接，双向可读、跨库 CTAS） |
| `cargo test -p rds-engine --test analysis_global_probe` | 4 passed（根连接挂全局库、`global_tables` 生产路径、路径口径） |
| `cargo check --workspace -j 1` | 通过 |

未在本机跑完的：`cargo test -p rds-analytics-resource` 全量测试在链接
`panel_window` 时因系统页面文件不足失败（`os error 1455`，非代码错误）；
第 3 步会在内存条件允许时补跑。

## 10. 风险与未决

1. **全局写守卫的绕过面**：`CALL` / `COPY` / `EXPORT` / prepared statement / 多语句；
   首版默认拒绝无法分类的语句，可能误伤高级 SQL，需要收集真实语句再放宽。
2. **切项目与活跃会话**：`DETACH analytics` 前必须停掉旧项目会话；否则报“database is locked”
   或让旧查询读到新库。需要写车道 + 会话关闭协议。
3. **迁移窗口**：全局 / 项目 DuckDB 迁移必须移进实例 system session；启动顺序错会回到
   “先开连接、实例挂不上”的老问题。
4. **测试夹具**：M6 测试目前直开 `ProjectDuckConn`；改成实例后需要统一测试实例工厂，
   否则每个测试都要自己造双库。
5. **资产文件与表双形态**：文件资产 + 派生表会产生两条全局行；UI / 删除 / pin 必须按
   `origin_kind='global_file'` 成组处理，不能只删一条。

## 11. 验收口径

- 真机：项目库 + 全局库同实例，双向 CTAS 成功（第一刀已达成）；
- 真机：全局表写入被 user session 拒绝，system session 成功（第 2 步）；
- `cargo test -p rds-engine --lib` ≥ 344 passed（第一刀实测 348）；
- `cargo test -p rds-analytics-resource --lib -- --test-threads=1` ≥ 286 passed（第 3 步）；
- `cargo check -p rds-workbench -j 1` 通过（第一刀已过 workspace check）；
- 表型链路无 parquet 中转、无短命 ATTACH（第 3 步）；
- 资产引用 / 版本快照 / pin 的既有用例不改语义，只改测试夹具。
