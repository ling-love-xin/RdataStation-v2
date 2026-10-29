# engine crate 瘦身方案（crate 层拆分 + duckdb 目录化）

> 基线：2026-10-29，`main` 分支（领先 origin/main 22 提交）。
> 前置实证：`crate-slimming-plan.md`（文件级目录化，P3+）与本方案是**两条独立路线**：
> 前者在 crate 内部拆文件，本方案在 **crate 边界**上拆分。先做本方案，再做文件级拆分，
> 避免「搬完又拆」的双重返工。
>
> 分支策略：本方案全部工作落在独立分支 `refactor/engine-slimming`，逐刀验证全绿后合回。
> 工作区中其他会话的未提交改动（`crates/editor/src/support/bookmark.rs` 等 3 个文件）
> 一律不碰、不提交。

## 1. 目标与硬约束（用户确认）

| # | 约束 | 落点 |
|---|---|---|
| 1 | 不影响 engine 定位 | 定位原文「DuckDB 分析引擎 + 驱动层 + DBI + 缓存 + 连接管理」的对应代码留在 engine（或经 re-export 保真） |
| 2 | 不违反 gpui-kit 代码规范 | 本次全是后端 crate 移动，无视图代码；duckdb 目录化也不产生视图代码 |
| 3 | 接受插件数据库驱动、热插拔 | `Database` trait / `DriverRegistry` / `SidecarDriverFactory` 契约进入 rds-driver，插件驱动只依赖薄契约 |
| 4 | 支持动态渲染数据新增模块 | engine 保持数据提供者（descriptor/capability/property_spec/introspection），渲染在 database（导航）/ property（属性）两个 crate，均零改动 |
| 5 | 数据库导航栏、属性零影响 | database / property crate 不动 |

## 2. 现状实证（2026-10-29 全仓实测）

engine（`crates/engine`，rds-engine）**61,489 行 src**，12 个 crate 依赖，258 处 `use engine::…`。

| 子模块 | 行数 | 性质 | 与 engine 内部耦合（`use crate::` 实测） |
|---|---|---|---|
| persistence | 20,032 | 纯逻辑存储 | → driver::utils(2 函数)、migration、logging::record |
| driver | 17,417 | 驱动层 | → duckdb::row_to_arrow、cache::MemoryPressure、persistence(declaration.rs) |
| duckdb | 8,448 | 分析引擎 | → sql（temp_table 用 SqlEngine/ColumnDefInfo） |
| sql | 5,250 | 编辑器语言服务 | **零内部依赖**（仅 sqlglot+serde+tracing） |
| services | 2,948 | 统一数据访问层 | 保留 |
| cache | 2,004 | 缓存 | 保留（MemoryPressure 下沉后无 driver 边） |
| script | 1,554 | 脚本词法器 | → sql::TokenClass/HighlightSpan |
| migration | 1,106 | 应用自身迁移 | ↔ persistence（强耦合环，必须同仓） |
| logging | 1,102 | 应用日志 | → persistence::log_store（必须同仓） |
| 顶层(connection_manager/csv/refs/lib) | 1,577 | 连接管理/引用 | 保留 |

**关键结论（决定分组的边，均已逐文件核实）**：
1. `persistence ↔ migration ↔ logging` 是强耦合环，**必须一仓搬出**；
2. `sql` 是唯一零内部依赖块，可独立成仓且最干净；
3. `driver` 与 engine 内部的 duckdb::row_to_arrow、cache::MemoryPressure 有边，抽仓前必须先下沉这两个符号；
4. `persistence → driver::utils(quote_identifier/escape_sql_string)`：这两个函数必须先下沉 shared，否则 rds-store → rds-driver → rds-store 成环；
5. `driver → persistence`（declaration.rs → GlobalDatabaseManager）是合法向下边（driver 层在 store 之上）。

## 3. 目标架构（全向下、无环）

```
rds-shared ──┬── rds-paths      （底层，已有）
             ├── rds-connection （已抽，保持）
             ├── rds-sql        = engine::sql + engine::script            （新抽，−6.8k）
             ├── rds-store      = persistence + migration + logging       （新抽，−22.2k）
             └── rds-driver     = driver 全层 + duckdb::row_to_arrow      （新抽，−17.6k）
                   └── rds-engine = duckdb + cache + services + connection_manager + refs + csv
                         └── 12 个 feature crate（database/property/plugin/editor/…，零改动）
```

engine 预估从 61,489 行 → **约 14,800 行（−76%）**，剩余恰好是定位原文。

## 4. 三刀明细

### 第一刀：`rds-sql`（crates/sql）
- 迁出：`engine/src/sql/`（18 文件 5,250 行）+ `engine/src/script/`（7 文件 1,554 行）+ `engine/tests/sqlglot_capabilities.rs`（探针随仓，保住「sqlglot 唯一接入点」约束）
- 依赖：sqlglot-rust、serde、tracing（实测无 shared/chrono/regex）
- 前置手术：**模块名冲突**——`sql/script.rs`（ScriptRewrite，内部供 formatter/transpiler 用）与 `script/`（词法器，editor 在用 `engine::script::python_spans` 等）同名。处理：`sql/script.rs → rewrite.rs`（模块 `rewrite`，根仍 `pub use rewrite::ScriptRewrite`），词法器模块保名 `script`。
- engine 留壳：`sql/mod.rs = pub use rds_sql::*;`、`script/mod.rs = pub use rds_sql::script::*;`
- 内部引用改写：`crate::sql::X → crate::X`（生产代码零处，仅 2 处测试 + 注释）
- engine Cargo.toml：`sqlglot-rust` 移除（唯一接入点随仓走），加 `sql.workspace = true`

### 第二刀：`rds-store`（crates/store）
- 迁出：`persistence/`（29 文件 20,032 行）+ `migration/`（6 文件 1,106 行）+ `logging/`（5 文件 1,102 行）
- 依赖：shared、paths、duckdb(crate)、rusqlite 等（随迁）
- 前置手术（第一刀后做）：`quote_identifier`/`escape_sql_string` 下沉 shared（`driver::utils` 改为重导出，`persistence` 两处改引用 shared）
- engine 留壳：`persistence/`、`migration/`、`logging/` 三个命名空间 `pub use rds_store::*;`

### 第三刀：`rds-driver`（crates/driver）
- 迁出：`driver/` 全层（40 文件 17,417 行）+ `duckdb::row_to_arrow`（238 行，随 DuckDB 驱动走）
- 依赖：shared、connection、arrow、duckdb(crate)、sqlx/rusqlite/mysql_async/tokio-postgres/native-tls/russh/tokio-socks 等（随迁，engine 的驱动依赖整体搬走）
- 前置手术：`MemoryPressure`（cache/lru_cache.rs 的枚举 + detect，~100 行）下沉 shared（cache 与 driver::smart_pool 共用）
- 引用改写：`engine::duckdb::executor`/`accel` 与 `crates/mock` 的 `row_to_arrow` 改引用 rds-driver
- driver→store 边（declaration.rs → GlobalDatabaseManager）保留，合法
- engine 留壳：`driver/mod.rs = pub use rds_driver::*;`

### 第四刀（可选）：rds-plugin 直连
- `crates/plugin` 的驱动桥与持久化引用（22 处 `engine::driver|persistence|migration`）改直连 rds-driver / rds-store；`plugin_bridge` 的 `engine::get_connection_manager` 保留 engine 依赖
- 收益：插件 crate 不再背 engine 的 duckdb/cache/sql 编译面；非必须，可后置

## 5. duckdb 模块目录化（责任边界清晰）

> 前提：`row_to_arrow` 已随第三刀迁出。目录化只移动代码不重写控件/逻辑，
> `duckdb/mod.rs` 的 `pub use` 全量保真，外部路径 `engine::duckdb::X` 零改动。
>
> **已实施（2026-10-29，提交 `9e8a45d8`）**：按下表建目录 + git mv + 各目录 `mod.rs`
> 域内重导出 + 顶层通配传导。实测外部路径全部保真（jobs.rs 的 `engine::duckdb::probe_file`、
> resource_host.rs 的 `engine::duckdb::file_reader_function`、persistence.rs 的
> `engine::duckdb::analysis::*` 零改动）；`duckdb/mod.rs` 顶层 `pub use` 改通配
> （`probe::*` / `transfer::*` / `temp_table::*` / `snapshot::*` / `extension::*` /
> `search::*`），`FTSManager` 经 search 通配保真。
> 踩坑两处：① `temp_table/analysis.rs` 与 `temp_table.rs` 的 `super::manager` 在移动后
> 层级断链，改 `crate::duckdb::manager` 绝对路径；② `import_export.rs` 测试硬编码
> `src/duckdb/import_export.rs` 自引用路径，随移动更新为 `src/duckdb/transfer/…`。
> 另：`row_to_arrow.rs` / `value_text.rs` 两个死文件（内容已下沉 shared，仅剩薄壳）git rm。

| 目录 | 责任（一句话） | 迁入文件 | 行数 |
|---|---|---|---|
| `federation/` | 联邦查询（已有，保持） | bridge/legacy/registry/session | 1,910 |
| `probe/` | 文件探查与读取函数 | file_probe.rs、file_reader.rs | 170 |
| `transfer/` | 数据导入导出 | import_export.rs | 541 |
| `temp_table/` | 临时表域（建/登记/清理 + 分析用生命周期） | temp_table.rs、analysis.rs | 1,284 |
| `search/` | 全文检索 | fts.rs | 308 |
| `snapshot/` | 快照与备份 | snapshot.rs | 411 |
| `extension/` | DuckDB 扩展插件接口 | plugin.rs | 408 |
| `value/` | 值 → 文本 | value_text.rs | 152 |
| 顶层保留 | 实例管理 / SQL 执行 / 加速档 / 计划分析 / 指标 | manager.rs、executor.rs、accel.rs、explain.rs、metrics.rs | 2,878 |

选型依据：目录名 = 责任名，一句话说清；为后续域内拆分（如 transfer 分 import/export）预留位置；
单文件目录通过 `pub mod X` + `pub use X::*` 保持路径不变，外部零感知。

## 6. 动工顺序与验收口径（每步全绿再进下一步）

| 步 | 内容 | 验收 |
|---|---|---|
| 0 | 开分支 `refactor/engine-slimming` | 分支存在，工作区他人改动未动 |
| 1 | 第一刀 rds-sql | `cargo check -p rds-app -j 2` 全绿；`cargo test -p rds-engine --lib` 全绿 |
| 2 | 小手术：工具函数下沉 shared | 同上 + `cargo test -p rds-driver`（或对应 crate） |
| 3 | 第二刀 rds-store | 同上 |
| 4 | 第三刀 rds-driver | 同上 + `cargo test -p rds-plugin`（驱动桥） |
| 5 | duckdb 目录化 | 同上 + `cargo test -p rds-mock`（row_to_arrow 引用方） |
| 6 | 文档收尾（本文件更新、lib.rs 注释、module-status） | 全仓 `cargo check-all` |

## 7. 诚实标注

- 拆 crate **不省编译时间**（P2 实证：Rust 编译单位是 crate）；收益是增量编译、职责边界、插件解耦、单 crate 行数。
- 最大风险在第三刀 Cargo.toml 依赖搬迁（17 个驱动依赖整体移动），故放最后、每步独立验证。
- 本方案与 `crate-slimming-plan.md`（P3+ 文件级）并行不冲突；完成后该计划中 engine 相关条目（P3/P9）自然缩减。
