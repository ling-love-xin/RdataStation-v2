# DuckDB 1.5.6 → 2.0.1 升级报告（duckdb crate 1.10506.1 → 1.20001.0）

> 升级日期 **2026-11-26**；隔离分支 `upgrade/duckdb-2.0-1122` 起、按刀提交，验证通过后合入 `main`。
> 本文只记录**当时实测到的事实**；任何"未复核"的项都明确标了未复核，不冒充结论。

## 1. 结论

- 分析引擎从 **DuckDB 1.5.6** 升到 **2.0.1**，crate 从 `duckdb 1.10506.1` 升到 **`=1.20001.0`**（`libduckdb-sys` 同版）。
- `arrow` **必须同批升**：`58.4.0` → **`60.2.0`**（`arrow-array` / `arrow-schema` 同为 60.2.0）；`comfy-table` 随之 `7.1` → **`8.0.1`**。
- 项目代码**不是零改动**：2.0.1 改变了"源表清单何时定型""`search_path` 能否用""appender 认不认 schema"三件行为，引擎侧的会话 / 挂载 / 临时表路径随之重写（§5、§6）。
- 存储格式**单向兼容**：2.0 能读写 1.5 建的文件（并保持其旧 storage version），但 2.0 **新建**的库是 `storage version 69`，1.5 **读不了**。这是本次升级唯一的不可逆点（§9）。
- 全仓版本口径已随之对齐（代码 / 文档 / 脚本 / 原型 / skill 面），清单见 §8。

## 2. 取证台账

| 项 | 值 | 来源 |
| --- | --- | --- |
| crate | `duckdb 1.20001.0`、`libduckdb-sys 1.20001.0` | `Cargo.lock`（`name = "duckdb"` / `"libduckdb-sys"`） |
| crate 依赖 | `arrow 60.2.0`（`arrow-array` / `arrow-schema` 同）、`comfy-table 8.0.1`、features `appender-arrow` 不变 | `Cargo.lock` |
| 库资产 | `duckdb-shared-libs-windows-amd64.tar.gz` **20,136,584 B**（19.2 MB） | GitHub release `v2.0.1`（本机走 `ghproxy.net` 镜像；本地留档 `D:\FluxDown\Archive\duckdb-shared-libs-windows-amd64.tar.gz`） |
| 解包内容 | `duckdb.dll` 61,957,416 · `duckdb.lib` 3,230,072 · `duckdb.h` 300,755 · `duckdb_extension.h` 136,943 · `duckdb_extension_v2.h` 215,356 · `duckdb_static_extension.h` 3,335 · `duckdb_v2.h` 789,337（合计 66,633,214 B） | `third_party/duckdb/2.0.1/`（`dir` 实测） |
| 版本映射 | 第二段十进制 `20001` → major `20001/10000=2` / minor `(20001/100)%100=0` / patch `20001%100=1` → `2.0.1` | `libduckdb-sys` `build.rs::duckdb_version_from_pkg_version`；本仓 `crates/workbench/src/version.rs::duckdb_engine_from` 同规则 |

**资产改名**：2.0 起官方发布资产从 `libduckdb-<平台>.zip` 改为 `duckdb-shared-libs-<平台>.tar.gz`（一个包里含 dll + lib + h 三类）；`tools/fetch-duckdb.sh` 已按主版本号分档，1.5.x 仍走旧的 zip 名。

## 3. 三处成对改点（+ 一处默认值）

| 位置 | 改成 |
| --- | --- |
| 根 `Cargo.toml` | `duckdb = { version = "=1.20001.0", features = ["appender-arrow"] }`；`arrow = "60.1.0"`（manifest 下界，锁到 60.2.0）；注释块改写成 2.0 口径，1.5 的 patch 沿革降为历史行 |
| `.cargo/config.toml` | `DUCKDB_LIB_DIR = { value = "third_party/duckdb/2.0.1", relative = true }` |
| `third_party/duckdb/2.0.1/` | 新增（**gitignore，不入库**）；`1.5.5/`、`1.5.6/` 已于 2026-11-26 删除（见 §10） |
| `tools/fetch-duckdb.sh` | 默认 `VERSION="${1:-2.0.1}"`；按主版本分档资产名；注释示例改成 `2.0.1` |

> `tools/package-release.sh` 从 `.cargo/config.toml` 的 `DUCKDB_LIB_DIR` 反查版本（单一来源），无需改；
> CI 的 DuckDB 缓存键是 `hashFiles('.cargo/config.toml')`，换版本自动换键，也无需改。

## 4. crate 侧连带

- `appender-arrow`（→ `vtab-arrow` → `vtab` + `num`）仍是**纯 crate 特性**，不动 C 内核、不动动态库；`Appender::append_record_batch` / `Statement::query_arrow` 签名未变，二次分析「结果集直灌 DuckDB」这条路不改代码。
- `arrow 58 → 60` 的核对（升级时做的）：116 处引用符号全在、`DataType` 41 个变体零增删、default features 未变；`comfy-table 7 → 8` 只是 `duckdb` 的 `prettyprint` 依赖，项目侧无直接用点。

## 5. 真机实测到的行为变化（2.0.1）

以下四条都是 2026-11-26 在本机真库（MySQL `192.168.3.138:3306` / PG `192.168.3.138:5432` / SQLite 文件）上跑出来的，不是推测：

1. **源表清单不再在 `ATTACH` 时定型**。1.5.x 时代"ATTACH 那一刻源库里有哪些表"就是可见集合，新表要重挂才看得见；2.0.1 下源库新建的表**不重挂也能直接查**。相应地，引擎里那条"新表必须报错"的断言已删（旧断言在 2.0.1 上是假话）。
2. **`SET search_path = 'main,rds_tmp'` 可用**。裸名与双引号名都能解析到 `rds_tmp`；两种顺序（`main,rds_tmp` / `rds_tmp,main`）都生效。这让"新临时表实际建进 `rds_tmp`、靠 search_path 让裸名照旧解析"成为可行做法（提交 `05cfa207`）。
3. **`SET search_path TO a, b` 语法报错**：`SET can only contain a single value`。只能写成单值字符串——写多值会在设置时就失败，不是"部分生效"。
4. **appender 不吃 `search_path`**：`SET schema = 'rds_tmp'` 对它同样无效，它只认 catalog 默认 schema（`main`）。跨 schema 追加必须 `Connection::appender_to_db(table, schema)`。

> 另一条来自历史文档、本次**未复核**：2.0 新增的 `VARIANT` / Triggers / Server 模式等能力，本项目当前没有使用点。

## 6. 与「分析实例 + 会话」重构的关系

2.0.1 让"多实例各建一条内存库"的旧结构失去意义（源清单动态化 + `search_path` 可用后，一条长期会话就能承担原先分实例的隔离），所以升级与重构是同一批：**`0df23fc2` 升版本打底，其后 17 个提交把引擎收成「一个分析实例 + 每编辑器一条会话」**。

| 提交 | 内容 |
| --- | --- |
| `0df23fc2` | 升 DuckDB 2.0.1（crate `1.20001.0` + arrow `60.2.0`） |
| `a9f0acc6` | 新增分析实例 / 会话层（一个实例 + 每编辑器一条会话） |
| `42d5dd99` | 2.0.1 口径 + 设计文档；fetch 脚本适配 2.0 资产 |
| `d971baa1` | 抽出 `mount_into`，让已有连接也能挂源（行为不变） |
| `11ae4fec` | 本地加速 / 联邦改走分析实例上的会话（不再各建内存库） |
| `009baedf` | 底座改挂在分析实例上，加换项目时 `refresh_bases` |
| `fa71c4f6` | 联邦会话改为「分析实例会话的门面」 |
| `99fa0975` | 加速档别名按源唯一，实例记录「连接 → 实际别名」 |
| `0ee81daf` | 移植 `editor_files::attach` 的 `default_dir` 参数 |
| `097cfb8e` | 加速档消费方全部改读分析实例，挂载口径收成一处 |
| `8a473169` | 退役 accel 的多实例会话机制，只留「挂实例」入口 |
| `9e8ac8fd` | 进程面并进分析实例，退役 `SessionHandle` 的 `ProcessDb` / `Federated` |
| `0222ac0e` | 临时结果建表即登进分析实例登记册 |
| `b1ed69e9` | 语句超时从设置读，被中断的语句说人话 |
| `c6d68ddc` | `analysis-instance-sessions.md` 状态改为「已落地并合入 main」 |
| `05cfa207` | 新临时表实际建进 `rds_tmp`，靠 `search_path` 让裸名照旧解析 |
| `3b57b302` | 联邦挂载前核对别名归属，被别的源占着就判失败并点名 |
| `46cdf8d8` | 迁移表 2g / 3b 收口，补两处做法与实证口径 |

设计口径与逐项取舍见 [`../database/analysis-instance-sessions.md`](../database/analysis-instance-sessions.md)。

## 7. 验证基线（2026-11-26 实跑）

| 项 | 结果 |
| --- | --- |
| `cargo test -p rds-engine --lib` | **348 passed** |
| `cargo test -p rds-workbench --lib` | **220 passed** |
| `cargo test --workspace --lib -j 2 -- --skip ctrl_alt_f2_to_f6_toggle_the_right_panels` | **0 failed** |
| `cargo check --workspace --all-targets` | 通过 |
| 真机 `duckdb_accel_probe` | **3 passed**（并行下三源分别拿到 `rds_src` / `rds_src_2` / `rds_src_3`） |
| 真机 `federation_credentials_probe` | **1 passed**（失败信息里口令被抹成 `******`，原因保留） |
| 一次性复核探针 `zz_dd2_recheck_probe`（2026-11-26） | **5 passed**（`memory_limit` 回读 / `duckdb_indexes()` 列集 / `duckdb_databases()` 列集 / 列元数据时机 / 扩展落点 / Secret / ODBC 清单，结论见 §11） |

已知失败且**与本次无关**：`ctrl_alt_f2_to_f6_toggle_the_right_panels` 在改动前的基线（`8a473169`）就失败。

## 8. 全仓口径清扫（本次一并改掉）

| 面 | 落点 |
| --- | --- |
| 代码 | `crates/workbench/src/version.rs`（字段注释、`duckdb_engine_from` 示例、夹具 `1.20001.0`、断言 `DuckDB 2.0.1` / `crate 1.20001.0`、解码用例同时保 2.0.1 与 1.5.6 两条编码）；`crates/workbench/src/components/version_dialog.rs:141` 的折行举例 |
| 构建 / 脚本 | 根 `Cargo.toml` 注释块；`tools/fetch-duckdb.sh` 示例；`tools/build-instance-project.py` 造库 docstring |
| 文档 | `README.md`（徽章、技术栈表、取库前置、`LD_LIBRARY_PATH`）、`README.en.md`（同四项）、`docs/project-progress.md`、`docs/architecture/core-design-current.md`、`docs/architecture/dependencies/dependency-strategy.md`（R4 两行 + 待办表）、`.../duckdb-linking.md`（实测数字 + 手取资产名）、`.../core-packages-2026-11-22-upgrade-assessment.md`（顶部加"后续已升"注，**正文结论不改**）、`docs/architecture/connection/connection-module-spec.md`、`docs/architecture/runtime/data-paths.md`、`docs/architecture/plugin/plugin-architecture.md`、`docs/architecture/mock/mock-view-ui-design.md`、`docs/architecture/ui/version-info-page-design.md`、`docs/architecture/ui/dbeaver-resultset-ui-baseline.md`、`docs/architecture/federation/federation-entry-handoff.md`、`docs/architecture/federation/prototypes/entry/07-analysis-sources-panel.html` |
| skill 面 | `.agents/skills/*` 与 `AGENTS.md` 全量复核：**没有任何 DuckDB 版本号**（只有结构描述），本轮无需改 |

**不改的部分（历史实证型）**：`DuckDB 1.5.5 的 mysql/pg 扫描器不认 Secret`（`connection/secret.rs`、`connection_dialog`、`editor_exec/conn.rs`、`accel.rs`、`data_objects_load.rs`）、`duckdb-rs 1.10505 列元数据须执行后可读`（`executor.rs` / `duckdb_service.rs`）、`DuckDB 1.5+ 设置值重格式化`（`manager.rs`）、各探针顶部实测记录、`duckdb-1.5.6-upgrade-report.md` / `gpui-kit-0.8.0/0.9.0-upgrade-report.md` / `docs/architecture/federation/*` 的 1.5.5 时代台账。把历史记录改成 2.0.1 才是造假。

## 9. 回退路径

- **代码 / 构建**：`third_party/duckdb/1.5.6/` 已随本次清理删除（§10）。要回退就先 `tools/fetch-duckdb.sh 1.5.6` 把库取回来，再改根 `Cargo.toml` 回 `=1.10506.1` + `DUCKDB_LIB_DIR=third_party/duckdb/1.5.6` + `arrow ^58`，即可重建。
- **数据（不可逆的部分）**：2.0 新建的 DuckDB 文件是 `storage version 69`，1.5 打不开。升级后用户已有分析库一旦被 2.0 写过（`ATTACH … READ_WRITE`）就可能被推高存储版本。**产品结论待定**：是否要在打开旧库前做一次"版本探测 + 只读挂载"，以及是否保留 1.5 客户端兼容承诺——这是升级带来的唯一需产品拍板的点。

## 10. 清理项（2026-11-26 已执行）

- **已删**：`third_party/duckdb/1.5.5/` 与 `third_party/duckdb/1.5.6/`（按用户决定直接删，不进回收站）。现在 `third_party/duckdb/` 下只剩 `2.0.1/` 与 `README.md`；回退要先把库取回来（见 §9）。
- 顺带释放的空间：另一废弃 worktree（`RdataStation-v2-duckdb2`，分支 `upgrade/duckdb-2.0-1122`，其 `.cargo/config.toml` 亦已指向 `2.0.1`）的 `target/` 已删，D 盘回收约 **99 GB**。
- `target/` 里的旧 `duckdb.dll` 由 `build.rs` 下次构建覆盖，无需手工清。

## 11. 1.5.x 时代旧假设的复核结果（2026-11-26 探针实跑）

升级当时只证了"能编、能跑"，没逐条重跑 1.5.x 的实测。这一节是用一次性探针
（`crates/engine/tests/zz_dd2_recheck_probe.rs`，**过程产物，跑完即删**；复现命令见 §12）
在 **2.0.1 真内核**上重问一遍的结果——**有一条把旧结论推翻了**。

| 旧假设 | 出处 | 2.0.1 复核结果 |
| --- | --- | --- |
| mysql / postgres 扫描器**不认** Secret | `connection/secret.rs:193`、`accel.rs:238`、`editor_exec/conn.rs:50`、`data_objects_load.rs:410` | ✅ **仍不认**：Secret 本身注册成功，但 `ATTACH 'mysql://host:port/db'`（URL 不带凭据）时扫描器用**操作系统账号**去连——`Access denied for user 'sling'@… (using password: NO)`。所以"凭据写进 URL + 出引擎脱敏"的取舍继续有效 |
| `duckdb_indexes()` 只有 `index_name` / `is_unique` / `is_primary` / `expressions` 等列，没有 `index_type` / `expression` | `driver/src/native/duckdb.rs:767,792` | ✅ **仍成立**，且列集已核对为 14 列：`database_name, database_oid, schema_name, schema_oid, index_name, index_oid, table_name, table_oid, comment, tags, is_unique, is_primary, expressions, sql`。现有 `list_indexes` 的 SQL 不需改 |
| duckdb-rs 的 `Statement::column_names()` / `schema()` 在**执行前**取会 panic | `engine/src/duckdb/executor.rs:202,273`、`duckdb_service.rs:249` | ✅ **仍会炸**：`raw_statement.rs:144` 的 `The statement was not executed yet` 原样触发。"列名一律执行后取"的纪律在 `duckdb 1.20001.0` 上一样成立 |
| `current_setting('memory_limit')` 回读被人类可读重格式化 | `engine/src/duckdb/manager.rs:25,531,675` | ✅ **仍重格式化**：`SET memory_limit='2GB'` 回读 **`1.8 GiB`**（不是 `2.0 GiB` 也不是原始串） |
| 扩展落在 `<extension_directory>/<内核版本>/<平台>/` | `tests/duckdb_export_probe.rs:24` | ✅ **布局不变**：2.0.1 装到 `v2.0.1/windows_amd64/mysql_scanner.duckdb_extension`（文件名仍带 `_scanner`） |
| 联邦"不用 ODBC 通道"——理由是 DuckDB 没有 ODBC 扫描器 | `federation/README.md:49`、`federation-architecture.md:30`、`federation-dev-plan.md` | ❌ **理由被推翻**：2.0.1 的 `duckdb_extensions()` 里**有 `odbc_scanner`**（未安装、连通性未实测）。结论（不采用 ODBC）可以先维持，但"内核没有"这条依据不能再写 |
| MySQL scanner 能动态解析源库新建的表 | `tests/duckdb_accel_probe.rs` | ✅ 已有真机用例覆盖（不重挂即可查） |

**顺带核清的三条（会影响文档/规格里的具体写法）**：

1. `SET dialect = 'mysql'` 仍**不可用**（`unrecognized configuration parameter "dialect"`），但 2.0.1 多了个
   `dialect_compatibility_mode` 设置项（本次未验证其取值与效果）。
2. **反引号现在能解析了**：`SELECT \`x\` FROM bt` 在 2.0.1 上返回 OK（1.5.5 会 `syntax error at or near "`"`）。
3. `duckdb_databases()` 的列名是 **`path` / `readonly`**（不是 `read_only`），且**没有 `storage_version` 列**
   —— `analysis-sources-spec.md` 那条待确认项可以按这个写死。

### 11.1 探针原始输出（关键行）

```text
[1] memory_limit 回读 = 1.8 GiB
[2] duckdb_indexes() 列 = ["database_name","database_oid","schema_name","schema_oid","index_name","index_oid","table_name","table_oid","comment","tags","is_unique","is_primary","expressions","sql"]
[3] duckdb_databases() 列 = ["database_name","database_oid","path","comment","tags","internal","type","readonly","encrypted","cipher","options"]
[4] duckdb_extensions() 里含 odbc 的扩展 = odbc_scanner
[5] SET dialect='mysql' → ERR Catalog Error: unrecognized configuration parameter "dialect"
[5b] SELECT `x` FROM bt → OK
[6a] prepare 后、执行前 column_names() → PANIC（未执行前取列名仍会炸）
[6b] 执行后 Rows::as_ref().column_names() → ["a", "b"]
[7a] INSTALL mysql → OK
[7c] 临时扩展目录内容 = ["v2.0.1/windows_amd64/mysql_scanner.duckdb_extension", "v2.0.1/windows_amd64/mysql_scanner.duckdb_extension.info"]
[8b] CREATE PERSISTENT SECRET → OK
[8c] ATTACH（URL 无凭据）→ ERR IO Error: Failed to connect to MySQL database …: Access denied for user 'sling'@'192.168.3.29' (using password: NO)
[9b] SET search_path TO a, b → ERR Parser Error: SET can only contain a single value
[9c] SET search_path = 'main,rds_tmp' → ERR Catalog Error: SET search_path: No catalog + schema named "rds_tmp" found.
```

> `[9c]` 的报错是"这个新内存库里没有 `rds_tmp`"，不是语法问题——语法本身在 2.0.1 可用（见 §5 第 2 条）。
> `[9a]` 查 `storage_version` 列报 Binder Error，即为上表第 3 条的来源。

## 12. 复现（隔离 worktree 一套）

```powershell
# 1) 新 worktree 两样 gitignore 资产必须补齐，否则链接阶段就崩
mkdir .rds\tmp
cmd /c mklink /J third_party\duckdb\2.0.1 D:\RdataStation\RDS\RdataStation-v2\third_party\duckdb\2.0.1

# 2) 独立 target：多 worktree 共用同一个 target 会撞 LNK1104（build script symbols.o）
#    与 LNK1318（PDB LIMIT），不是代码问题
$env:CARGO_TARGET_DIR='<本 worktree>\target'
$env:CARGO_BUILD_JOBS='2'
cargo test -p rds-engine --lib
cargo test -p rds-workbench --lib version

# 3) 真机探针（环境变量在命令内临时设，别落到用户环境）
$env:RDS_TEST_MYSQL_URL='mysql://root:root@192.168.3.138:3306/mysql'
$env:RDS_TEST_PG_URL='postgres://postgres:postgresql@192.168.3.138:5432/postgres'
$env:RDS_TEST_SQLITE_PATH='D:\FossilT\T.fossil'
cargo test -p rds-engine --test duckdb_accel_probe -- --include-ignored
```

### 12.1 复核 §11 那张表（一次性探针，跑完即删）

探针内容就是 §11.1 那段输出：内存库 + `extension_directory` 指临时区 + 关自动下载，然后依次问
`memory_limit` 回读 / `duckdb_indexes()` 与 `duckdb_databases()` 列集 / 执行前取列名 / `SET dialect`
与反引号 / `INSTALL mysql` 落点 / Secret 能否让不带凭据的 `ATTACH` 连通 / 匹配清单里有没有 `odbc`。

```powershell
$env:RDS_TEST_MYSQL_URL='mysql://root:root@192.168.3.138:3306/mysql'
cargo test -p rds-engine --test zz_dd2_recheck_probe -- --nocapture --test-threads=1
```

要点：`catch_unwind` **抓不住**"执行前取列名"的断言（它会先打 panic 日志再被捕获，看起来像失败，
但 `test result: ok` 才是判据）；`SET search_path = 'main,rds_tmp'` 在全新内存库里会报
`No catalog + schema named "rds_tmp"`，那是名字不存在，不是语法不行。
