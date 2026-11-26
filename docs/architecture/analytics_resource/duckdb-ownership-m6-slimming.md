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

- `AnalysisSession` 增 `role: User | Process | System`（`Control` 不是会话，控制连接直接执行，不经过闸门）；
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
| `engine/duckdb/temp_table/promote.rs::write_table_across_connections` | 项目连接 `COPY TO parquet` → 目标连接 `read_parquet` → `CREATE TABLE` | 已删除；改为 `write_shared_temp_to_project`（`CREATE TABLE analytics.<目标> AS SELECT * FROM rds_tmp.<源>`） |
| `analytics_resource/src/global/promote.rs` | 表型升级导出 parquet 再建全局表 | 已改 `promote_table_from_project`：`CREATE TABLE rds_global.<table> AS SELECT * FROM analytics.<source>` |
| `analytics_resource/src/global/distribute.rs` | 全局 `COPY TO parquet` → 项目 `read_parquet` | 已改 `CREATE TABLE analytics.<target> AS SELECT * FROM rds_global.<source>` |
| `analytics_resource/src/global/refs.rs` | 引用打开时短命 ATTACH / 取样 | 当前 refs 已无 ATTACH，待复核引用读取是否还有取样路径 |
| `analytics_resource/src/jobs/global.rs` | `ProjectDuckConn(duck_conn, sqlite_pool)` | `ProjectCtx(sqlite_pool, session)`（待做） |
| `workbench/services/persist_jobs.rs` | 调 `write_table_across_connections` | 已改调 `write_shared_temp_to_project`（同根 CTAS，无中转文件） |
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
| 2 | 会话角色 + 全局写守卫 | 守卫单测：`USE rds_global` / DDL / DML / ATTACH 全拒 | ✅ 已完成（2026-11-26） |
| 3 | M6 表型减法：`ProjectCtx`、CTAS、删 parquet 路径 | `cargo test -p rds-analytics-resource --lib` | 🟡 进行中：结果集 / 升级 / 分发已切同根 CTAS（286 passed）；`ProjectCtx` 与 system session 待做 |
| 4 | 全局表回收站 `trash_global` / `purge_global` | 表资产 trash/restore/purge 用例 | 🟡 表型已完成；文件回收站随第 5 步 |
| 5 | 全局文件资产 + 文件回收站 + 版本恢复 | 文件 hash、恢复、pin 依赖用例 | 🟡 本体/副本/恢复/回收站已落；UI 入口与裁剪待做 |
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

## 9.2 第二刀实施记录（2026-11-26）

代码落点：

- `crates/sql/src/guard.rs`：新增 `SqlWriteReport` / `write_report`，是 sqlglot AST 在
  RdataStation 里的**唯一写范围分类口**。按语句切分逐条判定：
  - `SELECT` / `WITH` / `EXPLAIN` / `SHOW` / `DESCRIBE` 等读语句放行；
  - DDL / DML 只看目标表（`TableRef` 的 `catalog` / `schema` / `name`），
    `rds_global` 目标才拒，读全局的子查询不误伤；
  - 目标不带 catalog / schema 时用会话默认 catalog 判（`USE rds_global` 之后的裸名就是全局）；
  - `CREATE TEMP` 落 temp schema，不算写全局；
  - `ATTACH` / `DETACH` / `USE rds_global` / 含 `rds_global` 的 `SET search_path` 直接拒；
  - `COPY ... TO` 只拦数据库文件目标；`COPY ... FROM` 目标含全局则拒；
  - `CALL` / `VACUUM` / `EXPORT` / 分类不了 `Command` 默认拒，宁可误伤再收集放宽。
- `crates/engine/src/duckdb/instance.rs`：
  - `SessionRole { User, Process, System }`；`SessionRole::can_write_global()` 只对 `System` 成立；
  - `AnalysisSession` 增 `role` / `default_catalog`；`open_session_with_role` /
    `ensure_session_with_role` / `ensure_system_session` 提供系统通道；
    `default_role_for_owner` 按 `process` / `system:` 前缀推默认角色；
  - `run_with_timeout` 在 `is_write` 之前调 `guard_global_write`，被拒语句不进写车道 / 不起看门狗；
  - 用户 SQL 的 `USE <catalog>` 执行成功后更新 `default_catalog`；
  - `use_catalog` 是引擎内部切库原语，**绕过闸门**：全局读取会话 `USE rds_global` 仍然可用，
    但用户 SQL 的 `USE rds_global` 仍被 `run` 拒绝。
- `analysis::global::open_global_session`：会话标签从 `process` 改为 `federated`，
  避免 `materialize_sample` 把全局取样误走“进程临时表”分支后 `move_sample_to_project` 失败；
  全局会话角色是 `User`，写入仍受闸门约束。
- `crates/engine/tests/global_write_guard.rs`：真机探针，验证用户 / 进程会话写全局被拒、
  `USE` / `ATTACH` 被拒、`CREATE TEMP` 可用、系统会话可写、全局读取会话内部 `USE` 不被误伤。

验证（串行）：

| 命令 | 结果 |
| --- | --- |
| `cargo test -p rds-sql --lib guard::` | 6 passed（读写目标 / 默认 catalog / USE / ATTACH / SET / COPY / 不可分类） |
| `cargo test -p rds-engine -j 1 --lib -- --test-threads=1` | 348 passed / 0 failed |
| `cargo test -p rds-engine --test analysis_db_exclusive_probe --test analysis_global_probe --test global_write_guard -- --test-threads=1` | 1 + 4 + 1 passed |
| `cargo check --workspace -j 1` | 通过（仅两条基线警告） |

已知边界：`with_connection` 仍绕过闸门（内部挂载 / 物化用；用户 SQL 一律走 `run`）；
Manager 句柄上的 DuckDB 迁移尚未挪到 system session，角色隔离目前只覆盖实例会话；
守卫首版对高级 `Command` 保守拒绝，后续按真实语句放宽。

## 9.3 第三段实施记录：表型链路切同根 CTAS（2026-11-26）

代码落点：

- `crates/engine/src/duckdb/temp_table/promote.rs`：
  - 删除 `write_table_across_connections`（跨连接 `COPY TO parquet` → `read_parquet`）；
  - 新增 `write_shared_temp_to_project(dest, source_table, target_table, mode)`：
    源解析为 `rds_tmp.<表>`，目标为 `"analytics"."<表>"`，执行
    `CREATE [OR REPLACE] TABLE ... AS SELECT * FROM ...`，再数目标表行数；
  - 测试 `writes_shared_temp_into_the_attached_project_without_parquet`：同一内存根
    建 `rds_tmp.tmp_q_orders`、`ATTACH` 临时项目库为 `analytics`，验证 DECIMAL 类型、
    引号、`Create` 撞名拒绝、`Replace` 覆盖。
- `crates/workbench/src/services/persist_jobs.rs`：
  - `persist_result_table` 改调 `write_shared_temp_to_project`；
  - 删除先锁进程单例再锁项目连接的顺序与 `DuckDbService` 依赖；
  - 错误提示去掉“另一个程序正在使用此文件”分支，保留撞名提示。
- `crates/analytics_resource/src/global/promote.rs`：
  - 新增 `promote_table_from_project(project, req, overwrite)`：项目会话执行
    `CREATE TABLE rds_global.<版本表> AS SELECT * FROM analytics.<源表>`；
  - 冲突 / 覆盖、版本号、表名、登记、`KeepVersions` 裁剪与文件型升级同口径；
  - 登记 `definition_sql` 支持调用方覆盖，表型写真实跨库 `SELECT`，不再留临时路径；
  - 测试 `g015` 增补同名冲突与覆盖：v2 接续、v1 登记与表都保留。
- `crates/analytics_resource/src/global/distribute.rs`：
  - 表型分发改为项目会话 `CREATE TABLE analytics.<目标> AS SELECT * FROM rds_global.<源表>`；
  - 删除 `COPY TO parquet` / `read_parquet` 中转与临时文件清理。
- `crates/analytics_resource/src/global/mod.rs` + `src/global/tests.rs`：
  - 新增测试专用 `open_at_handle`，登记库仍走临时 SQLite，DuckDB 用调用方会话；
  - `create_test_env` 改为 `shared::duckdb_root::isolated` 一条根连接，
    全局 / 项目各取 `handle(GLOBAL_ALIAS)` / `handle(PROJECT_ALIAS)`。

验证（串行）：

| 命令 | 结果 |
| --- | --- |
| `cargo test -p rds-engine --lib temp_table::promote` | 4 passed |
| `cargo test -p rds-engine -j 1 --lib -- --test-threads=1` | 348 passed / 0 failed（PowerShell 把 cargo 的 `Finished` 走 stderr，退出码显示 1，非断言失败） |
| `cargo test -p rds-analytics-resource --lib global::tests:: -- --test-threads=1` | 22 passed（含表型升级冲突 / 覆盖 / 版本保留、分发、引用、固化） |
| `cargo test -p rds-analytics-resource --lib -- --test-threads=1` | 286 passed / 0 failed |
| `cargo check -p rds-workbench -j 1` | 通过 |

未取得结果：`cargo test -p rds-workbench --test persist_result_table` 在编译 `gpui-base` 时
出现 `memory allocation ... failed` / `STATUS_STACK_BUFFER_OVERRUN`，属本机内存 / 页面文件限制，
不是代码断言失败；待条件允许补跑。

本段未动的 M6 路径：

- `analytics_resource/src/global/refs.rs` 已无短命 `ATTACH`；引用读取是否仍绕取样路径待复核；
- `jobs/global.rs::ProjectDuckConn` 仍是 `(duck_conn, sqlite_pool)`，未改名 `ProjectCtx`；
- `GlobalInstanceStore::production` 仍取 Manager 会话句柄，未收敛到实例 system session。

## 9.4 第四段实施记录：全局表资产回收站（2026-11-26）

代码落点：

- `crates/analytics_resource/src/global/delete.rs`：
  - 新增 `trash_global(name)`：活跃 pin 检查 → 只软删当前活跃行 → 写
    `<RDS_HOME>/data/trash/global/<asset_id>/<timestamp>/manifest.json`；表与版本快照保留；
  - 新增 `restore_global(name)`：读最新 manifest → 校验 `analytics_table` 仍在 →
    复活登记 → 清掉已消费的回收站条目；表被手工 `DROP` 时拒绝恢复；
  - 新增 `purge_global(name)`：`DROP` 同名链各版表 → 清版本快照 / 登记 / 分发回执 →
    清回收站条目；
  - `delete_global` 暂时保留旧的硬删除语义（内部转 `purge_global`），兼容现有调用。
- `crates/analytics_resource/src/jobs/global.rs`：删除动作改调 `trash_global`，
  回执新增 `OpOutcome::GlobalTrashed { name, version }`。
- UI：详情危险区 / 列表右键改「移入全局回收站…」，确认框说明
  “只软删登记、表与版本保留、可从回收站恢复”；回执改为“已移入回收站”。

验证（串行）：

| 命令 | 结果 |
| --- | --- |
| `cargo test -p rds-analytics-resource --lib global::tests::g023` | 1 passed（trash 后表仍在 / restore 复活 / 表被 DROP 后拒绝恢复 / purge 清表与条目） |
| `cargo test -p rds-analytics-resource --lib global::tests::g024` | 1 passed（活跃 pin 拒绝 trash，release 后可移入 / 恢复 / 永久删除） |
| `cargo test -p rds-analytics-resource --lib -- --test-threads=1` | 288 passed / 0 failed |

未取得结果：`cargo check -p rds-workbench` 仍被本机编译环境挡住——共享 target 会被主工作树
WIP 产物串包（`rds_sql::guard` 缺失 / `use_duckdb_fed` 类型不一致）；独立 target 在编译
`gpui-component` 时被系统终止。UI 改动已通过 rustfmt 解析，但类型检查待独立环境补跑。

本段未做：全局回收站列表 / 恢复入口 UI（当前只有服务层 `restore_global` 与测试）；
文件资产回收站随第 5 步（还没有全局文件资产本体可搬）。

## 9.5 第五段实施记录：全局文件本体与版本恢复（2026-11-26）

代码落点：

- `crates/store/src/persistence/global_paths.rs`：新增
  `get_global_resources_dir()` / `get_global_resource_versions_dir()` / `get_global_trash_dir()`。
- `crates/analytics_resource/src/global/payload.rs`：全局文件本体落盘（`resources/.staging` →
  原子替换）、sha256 / 体积取自实际字节、历史副本
  `resources/versions/<asset_key>/v<N>/<filename>`；`asset_key` 按资产名稳定生成，跨版本不换。
- `global/promote.rs`：
  - 文件型升级先保旧当前文件进旧版本目录，再替换当前副本，最后写 vN 副本；
  - `restore_global_file_version(name, N)`：校验版本副本存在 + hash 与快照一致 →
    以旧字节再走一次升级（`promote_overwrite`），生成新版本，不原地回滚。
- `global/register.rs`：登记 / 快照写真实 `content_hash` / `file_size` / `file_rel_path`；
  脚本档 `kind = file`（内容表降为兼容载体），数据文件档仍是 `kind = analysis` + 派生表。
- `global/distribute.rs` / `global/refs.rs`：形态判定按 `kind = file`，不再按
  `file_rel_path` 一刀切——数据文件新留下的本体只服务版本恢复，共享形态仍是表。
- `global/delete.rs`：回收站 manifest 增 `file_rel_path` / `payload_dir`；
  `trash_global` 把当前本体与历史副本移入 `trash/<id>/<time>/payload/`，
  `restore_global` 移回，`purge_global` 清本体与历史副本。旧 manifest 靠 `serde(default)` 兼容。

验证（串行）：

| 命令 | 结果 |
| --- | --- |
| `cargo test -p rds-analytics-resource --lib global::tests::g025` | 1 passed（v1 本体/副本 hash 一致；v2 后 v1 保留；恢复 v1 → v3 且当前字节回旧） |
| `cargo test -p rds-analytics-resource --lib global::tests::g026` | 1 passed（trash 移出本体与副本，restore 移回，purge 清掉） |
| `cargo test -p rds-analytics-resource --lib global::tests::g027` | 1 passed（`MetadataOnly` 裁掉 v1 副本、保当前 v2；恢复被裁版本明确拒绝） |
| `cargo test -p rds-analytics-resource --lib -- --test-threads=1` | 291 passed / 0 failed |

本段边界：现有全局表资产仍没有文件本体（旧数据不伪造）；表格 UI 还没有“恢复全局文件版本”
入口，服务层已可用；文件 `KeepVersions` 裁剪已实现（当前版与被 pin 版本不裁）；
派生表的归属标记仍待下一段。全局文件打开路径已按 `RowScope::Global` 改走
`data/system/resources/`，不再误拼到项目根。

## 9.6 第六段实施记录：全局文件“恢复上一版”入口（2026-11-26）

- `detail_view/actions.rs`：全局行且 `payload_rel_path` 非空、当前版本 > 1、状态不是 Missing 时，
  动作区摆「恢复上一版文件…」；分发按钮旁说明补一句“历史文件已留副本，可恢复”。
- `resource_host/detail_actions.rs`：确认框写清“旧副本生成新版本、当前版本保留、已分发副本不动、
  副本被裁会拒绝”；确认后 `enqueue_restore_global_version(name, version - 1)`。
- `jobs/global.rs`：新增 `RestoreGlobalVersionJob` / `run_restore_global_version`；
  回执 `OpOutcome::GlobalVersionRestored { name, from_version, version }`。
- `workbench/src/panels/resources.rs`：回执文案“已从全局「name」vN 恢复为 vM（旧版本保留）”。

验证：`cargo test -p rds-analytics-resource --lib --no-run` 通过；完整 UI 编译仍受本机环境限制。
本段只给“上一版”快捷入口，任意版本选择仍走后续版本历史 UI。

## 9.7 第七段实施记录：全局回收站列表与清空（2026-11-26）

- `global/delete.rs`：新增 `GlobalTrashEntry { name, version, analytics_table, file_rel_path,
  trashed_at, version_count, trash_dir, has_file_payload }`。
  `list_global_trash` 扫 `<RDS_HOME>/data/trash/global` 的两层目录，读 `manifest.json`，
  只收 `kind == "analysis"`；同名只保留 `trashed_at` 最新的一条，按时间倒序。
  `empty_global_trash` 逐条走 `purge_global`：`Referenced` 计入 skipped，`NotFound` 忽略，
  其余错误原样返回，返回 `(purged, skipped)`。
- 回收站根统一走 `store::persistence::global_paths::get_global_trash_dir()`，
  不再由 `system_dir.parent()` 反推——测试注入的 sqlite 路径不保证落在 `data/system` 下。
- `global/mod.rs` 导出 `GlobalTrashEntry`。
- 用例：`g028_global_trash_lists_restores_and_purges`（两条进站、列表齐全且带本体、
  恢复一条后只剩一条、永久删除后列表清空）、
  `g029_empty_global_trash_skips_pinned_entries`（先入站后被项目 pin，清空返回 `(0,1)`；
  释放 pin 后再次清空返回 `(1,0)`，列表清空）。

验证：`cargo test -p rds-analytics-resource --lib -- --test-threads=1`
293 passed / 0 failed。

本段边界：列表/清空只有服务层，资产库面板还没有“全局回收站…”入口（现有回收站对话框
接的是项目 `.RSmeta/trash`）；全局回收站 TTL 与清空遇 pin 的 UI 文案仍未定。

## 9.8 第八段实施记录：全局表历史版本恢复（2026-11-26）

- `global/promote.rs`：新增 `restore_global_table_version(name, N)`。校验活跃行是
  `kind = analysis` 且无文件本体（文件型走 `restore_global_file_version`）；从版本快照
  取 `analytics_table`，校验旧表仍在库；在全局会话里
  `CREATE TABLE rds_global.<新版本表> AS SELECT * FROM rds_global.<旧表>`；
  旧活跃行软删后登记新版本（版本号 +1，旧表 / 旧登记 / 版本快照全留）。
  旧表被 `KeepVersions` 裁掉或快照缺表名时明确失败。
- 用例：`g030_table_version_restore_copies_the_old_table`（v1 100 行、覆盖升级 v2 50 行，
  恢复 v1 → v3 是 100 行的新表，v1 / v2 / v3 三张表都还在）。

验证：`cargo test -p rds-analytics-resource --lib global::tests::g030` 1 passed。

- UI：`detail_view/actions.rs` 对全局表型行（`kind = AnalyticsTable`、表名非空、版本 > 1、
  非 Missing）摆「恢复上一版表…」；文件型仍摆「恢复上一版文件…」。两者都走
  `request_restore_global_previous`，按档型决定文案并给 `enqueue_restore_global_version`
  传 `table`；回执继续用 `GlobalVersionRestored`（workbench 文案不变）。

本段边界：只有“上一版”快捷入口，任意版本选择仍待版本历史 UI。

## 10. 风险与未决

1. **全局写守卫的绕过面**：`CALL` / `COPY` / `EXPORT` / prepared statement / 多语句；
   首版默认拒绝无法分类的语句，可能误伤高级 SQL，需要收集真实语句再放宽。
2. **切项目与活跃会话**：`DETACH analytics` 前必须停掉旧项目会话；否则报“database is locked”
   或让旧查询读到新库。需要写车道 + 会话关闭协议。
3. **迁移窗口**：全局 / 项目 DuckDB 迁移必须移进实例 system session；启动顺序错会回到
   “先开连接、实例挂不上”的老问题。
4. **测试夹具**：M6 测试已改为 `isolated` 同根夹具；后续改成实例后仍要统一测试实例工厂。
5. **资产文件与表双形态**：文件资产 + 派生表会产生两条全局行；UI / 删除 / pin 必须按
   `origin_kind='global_file'` 成组处理，不能只删一条。

## 11. 验收口径

- 真机：项目库 + 全局库同实例，双向 CTAS 成功（第一刀已达成）；
- 真机：全局表写入被 user session 拒绝，system session 成功（第 2 步已达成）；
- `cargo test -p rds-engine --lib` ≥ 344 passed（第一刀实测 348）；
- `cargo test -p rds-analytics-resource --lib -- --test-threads=1` ≥ 286 passed（第 3 步实测 286）；
- `cargo check -p rds-workbench -j 1` 通过（第一刀已过 workspace check）；
- 表型链路无 parquet 中转、无短命 ATTACH（结果集 / 升级 / 分发已达成；`ProjectCtx` / system session 待做）；
- 全局表资产删除只软删登记，表与版本保留；恢复校验表在、永久删除才 `DROP`（第 4 步表型已达成）；
- 资产引用 / 版本快照 / pin 的既有用例不改语义，只改测试夹具。
