# 模块状态与测试基线（实测版）

> **本文件的定位**：全仓**唯一一份「刚跑过」的状态与基线台账**。
> - `core-design-current.md` 管**架构现状与偏差**；本文件管**接通状态与可复现的数字**。
> - 各模块 `README.md` / `*-dev-plan.md` §0 仍是该模块的**语义权威**；本文件的数字是**某一时刻的实测快照**，两者冲突时：语义看模块文档，**数字看本文件**（或照 §1 重跑一遍）。
> - 约定：本文件的每个数字都必须能在**本机一条命令重跑出来**；跑不出来的一律不写。
>
> 基线：**2026-09-19**（Windows · stable · `-j 2`）· 当日**两次复跑**，下表是**第二次**（死代码清理三批之后）的数字 · 维护方式见 §7。

---

## 0. 摘要

| 项 | 实测值 |
| --- | --- |
| 工作区测试目标 | **84**（含 16 个 Doc-tests 目标） |
| 通过 | **1970** |
| 失败 | **0**（本轮全绿） |
| 忽略 | **51**（真机探针 24 + Doc-tests 27） |
| 编译告警 | `cargo check --workspace --all-targets` 零告警 |
| 代码规模 | 221,971 行 Rust / 440 个 `.rs` 文件（`crates/`，不含 `v1/`） |
| 口径 | 上表是**原始命令的输出**；其中 `rds-workbench --test zz_fixture_probe` 是**本机诊断脚本**（未跟踪、`.gitignore` 已挡，见 §6.1）——**项目自身套件 = 83 个目标 / 1969 项**，见 §2 的表下注 |

---

## 1. 如何复现

```bash
# 0) 前置：DuckDB 预编译内核（每台机器 / 每个版本一次；Windows 用 Git-Bash 或 MSYS）
tools/fetch-duckdb.sh

# 1) 全量（必须 -j 2：并发链接重型 crate 会耗尽内存）
cargo test-all            # = test --workspace -j 2
cargo check-all           # = check --workspace --all-targets

# 2) 只要一份「目标 ↔ 结果」对照表
cargo test --workspace -j 2 --no-fail-fast 2>&1 \
  | awk '/^ +Running|^ +Doc-tests/{n=$0} /^test result:/{gsub(/^[ \t]+/,"",n); sub(/ \(target.*/,"",n); sub(/test result: /,""); print n " :: " $0}'

# 3) 单个 crate
cargo test -p rds-engine --lib -j 2
```

**环境异常：rustup shim 报 `Permission denied`**（本机遇到过，`~/.cargo/bin/*.exe` 是指向 `rustup.exe` 的符号链接，可能整体不可用）——绕过 shim 直接用工具链二进制：

```bash
RUSTC=<toolchain>/bin/rustc.exe RUSTDOC=<toolchain>/bin/rustdoc.exe <toolchain>/bin/cargo.exe \
  test --workspace -j 2 --no-fail-fast
# 本机实证：<toolchain> = D:/Dev/Rust/.rustup/toolchains/stable-x86_64-pc-windows-msvc
```

> `RUST_MIN_STACK` 与 `DUCKDB_LIB_DIR` 由 `.cargo/config.toml` 提供，**不要**在命令行覆盖掉；本机跑测试不需要手工设环境变量。

---

## 2. 逐包基线（2026-09-19 第二次复跑）

「本包合计」= lib 单测 + 本包 `tests/` 下各集成目标。

| 包 | lib 单测 | 集成目标（`tests/`） | 本包合计 | 忽略 |
| --- | --- | --- | --- | --- |
| `rds-engine` | **440** | 32（8 个目标） | **472** | 24 |
| `rds-editor` | **377** | — | **377** | — |
| `rds-workbench` | **110** | 130（32 个目标） | **240** | — |
| `rds-insight` | **227** | 14（`column_profile_e2e`） | **241** | — |
| `rds-mock` | **190** | 48（4 个目标） | **238** | — |
| `rds-analytics-resource` | **125** | 27（`panel_window` 18 · `dialog_window` 9） | **152** | — |
| `rds-connection` | **44** | 4（`tunnel_roundtrip`） | **48** | — |
| `rds-project` | **43** | 5（`project_registry` 1 · `project_store` 4） | **48** | — |
| `rds-database` | **51** | — | **51** | — |
| `rds-scratchpad` | **37** | — | **37** | — |
| `rds-shared` | **22** | — | **22** | 9（doctest） |
| `rds-settings` | **21** | — | **21** | — |
| `rds-plugin` | **11** | — | **11** | — |
| `rds-paths` | **10** | 1（`test_support_is_wired`） | **11** | — |
| `rds-workbench-shell` | **1** | — | **1** | — |
| `rds-app` | 0 | — | 0 | — |
| **合计** | **1709** | **261** | **1970** | **51** |

> **口径注（重要）**：合计里的 `rds-workbench` 集成 130 项中，**1 项来自本机诊断 `zz_fixture_probe`**（未跟踪、不入库，见 §6.1）。
> 去掉它：**项目自身套件 = 83 个目标 / 1969 项通过**。两个数都是真的，**引用时必须写明用的是哪个口径**。
>
> `rds-plugin` 有 11 项单测且全绿，但这**不代表它接通了**——整包仍**无任何 crate 依赖**（见 §5）。

### 2.1 `rds-engine` 的 8 个探针目标

| 目标 | 数量 | 性质 |
| --- | --- | --- |
| `sqlglot_capabilities` | 10 | 报告式：核验 sqlglot 能力后再封装 |
| `transaction_affinity` | 12 | 无端点时逐项跳过 |
| `duckdb_accel_probe` | 3 | 本地加速通道 |
| `duckdb_export_probe` | 3 | Parquet / XLSX 往返（含真机台账） |
| `duckdb_extensions_probe` | 1 | 扩展可装可载（联网，约 18s） |
| `federation_probe` | 1 | 跨源挂载 |
| `federation_credentials_probe` | 1 | 凭据脱敏出口 |
| `oracle_probe` | 1 | Oracle 扩展腿 |

### 2.2 `rds-workbench` 的 32 个集成目标（130 项）

| 目标 | 数量 | | 目标 | 数量 |
| --- | --- | --- | --- | --- |
| `data_source_lifecycle` | 28 | | `insight_entry` | 2 |
| `mock_generator` | 12 | | `insight_editor_entry` | 2 |
| `mock_jobs` | 11 | | `query_export` | 3 |
| `connection_project_picker` | 7 | | `query_history` | 3 |
| `connection_type_driver` | 7 | | `connection_dialog_ui` | 4 |
| `ui_contract` | 7 | | `dialog_host_layer` | 4 |
| `connection_staging` | 6 | | `editor_session_real` | 4 |
| `real_connections` | 5 | | `connection_multi_save` | 3 |
| `connection_tunnel_cleanup` | 1 | | `connection_render_matrix` | 2 |
| `connection_drafts_persist` | 1 | | `connection_scope_and_state` | 2 |
| `connection_edit_backfill` | 1 | | `connection_template` | 2 |
| `db_navigator` | 2 | | `global_service_singleton` | 2 |
| `editor_exec_real` | 1 | | `insight_source_real` | 2 |
| `federation_sources` | 1 | | `insight_schema_real` | 1 |
| `log_dialog_layer` | 1 | | `mock_job_cancel` | 1 |
| `oracle_federation` | 1 | | `zz_fixture_probe` | 1（本机诊断，未跟踪） |

> 真机目标（`*_real` / `*_probe` / `oracle_*` / `federation_*`）在无环境变量或无服务时按设计跳过或空跑；
> `zz_fixture_probe` 是**本机专用**脚本（未跟踪、`.gitignore` 已挡），**不属于项目测试套件**——它出现在上表只是因为本机命令输出里确实有它。

---

## 3. 模块接通状态

状态口径：**✅ 主线可用** · **🟡 部分可用**（有具名缺口） · **⛔ 未接通**。

| 模块 | crate | 状态 | 已知缺口（权威清单在各模块文档） |
| --- | --- | --- | --- |
| M1 项目管理 | `project` | ✅ | 窗口退出草稿兜底 · 项目目录移动 · 提升 / 快照 |
| M2 双引擎底座 | `engine` | ✅ | 缓存层与持久化层**仍有零调用项**（权威清单见 [`data-layer-wiring-matrix.md`](data-layer-wiring-matrix.md) §2.2 / §7；本轮已处置约 4.3k 行，见其 §6）· 增量同步未接 |
| M3 数据源连接 | `connection` | ✅ | SSH 主机密钥默认放行 · Secret 无门控不清理 |
| M4 数据源管理 / 导航 | `database` | ✅ | 虚拟列表 · 命中后树内定位 · PG 跨库浏览（**内容档不在本模块**：导航面板搜索框只有名称档，全文走 Quick Open 的 `#`） |
| M5 草稿箱 | `scratchpad` | ✅ | Phase D（归档 / 取回 / 版本）· 系统拖入导入 · 命中跳转到行 |
| M6 资产库 / 分析存档 | `analytics_resource` | ✅ | Phase 4 分析表档 · Phase 5 引用档 · 内容预览 |
| M7 Mock 造数 | `mock` | ✅ | 出口不可取消 · 大导出非流式 · 并发生成 |
| M8 洞察 | `insight` | ✅ | 分析表型存档入口 · 解析级策略门 · 表 / 库级快照 |
| M9 插件宿主 | `plugin` | ⛔ | **整包无调用方**，等 beta3 立项 |
| SQL 编辑器 | `editor` | ✅ | Phase 1c 分析单元（搁置）· 值预览 / 编辑 · 血缘持久化 |
| 联邦查询 | `engine/duckdb/federation` | 🟡 | L3 桥接 · SQL Server 真机 · 扫描量可见 |
| Quick Open | `workbench/quick_open` | ✅ | 源码（视图 / 例程定义）未进 FTS · 命中后树内定位 · `@` 当前连接限定 · 最近使用 / 空态建议（Phase 2） |
| 设置 | `settings` | ✅ | `effect` 字段无人消费 · 无跨进程写锁 |

---

## 4. 横切事实（实测）

| 项 | 值 | 出处 |
| --- | --- | --- |
| 工作区 crate 数 | 16 | 根 `Cargo.toml` |
| `default-members` | `crates/app`（裸 `cargo test` **不含** `plugin`） | 根 `Cargo.toml` |
| 驱动 | 6 个，覆盖 4 类引擎 | `engine/src/driver/loader.rs` |
| 迁移目录 | 4 套（global / project_meta / connection_metadata / project_analysis） | `engine/migrations/` |
| 界面契约 | `ui_contract` 7 项：零裸尺寸 / 零裸色值 / 面板登记 / 共享字段白名单 | `workbench/tests/ui_contract.rs` |

---

## 5. 未接通与零调用（如实口径）

- **M9 `plugin` 整包未接通**：无任何 crate 依赖它；`plugin/src/sidecar/{health_checker,hot_reload_manager}.rs` 为 0 字节。
- **`engine` 仍有零调用项**：`CacheLevel` 枚举、`l2_enabled` / `l3_enabled`、`CacheVersionManager`（L2 版本链）；查询缓存的 `use_cache` 默认 `false`，生产路径从不触发；L2 还有 5 张 v1 遗留表（`compressed_metadata` 等）无消费者。**旧口径「约 150 个公开项」本轮未重数，不要引用具体数字**——逐项权威清单见 [`data-layer-wiring-matrix.md`](data-layer-wiring-matrix.md) §2.2 / §7，已处置的那批见其 §6.1 / §6.2（`dbi` 死层 2124 行 · 重复的扩展管理 570 行 · 持久化 v1/V7 1636 行 · L1 四组零调用）。
- **元数据 FTS 读 / 写侧都已接线**（本条曾写作「写入侧未接」，2026-09-19 复核纠正）：写侧 `rebuild_fts_schema` 挂在 `rebuild_schema_index` 同批（由 `NavigatorService::collect_objects` 的 Tables 分支 → `NavCache::rebuild_index` 触发），读侧 `search_fts` 供 Quick Open 的 `#` 档（`Mode::FullText` → `SearchKind::FullText`）。**真实边界**：语料只收名称 / 注释 / 数据类型（`view_definition` / `routine_definition` **未进 FTS**）；trigram ⇒ **≥ 3 字**，不足必然无命中。详账见 `data-layer-wiring-matrix.md` §2.2 与 `database/metadata-cache-vs-dbeaver-datagrip.md` §4.6。
- **缓存身份指纹（`meta_{fp}.sqlite`）**：`metadata_identity` 已写并有 13 项单测，但**未接线**，L2 仍按 `conn_{id}` 命名。
- **增量同步 / 快照表**：有意不补，等真实消费者。

---

## 6. 本轮实测发现（2026-09-19）

### 6.1 🔴 本机诊断脚本曾入库且已推到公开仓库

`crates/workbench/tests/zz_fixture_probe.rs` 的头部写着「**临时诊断（未跟踪文件）**……⚠️ 含**内网明文凭据**……**不要提交**」，但该文件**实际已被 git 跟踪**（commit `7814b9b6`，且已是 `origin/main` 的祖先，即**已公开**）。文件内含内网地址 `192.168.3.138` 与明文口令（MySQL `root:root`、PostgreSQL `postgres:postgresql`）。

处置（本轮已完成）：

1. `git rm --cached crates/workbench/tests/zz_fixture_probe.rs` —— **取消跟踪，保留本机文件**；
2. `.gitignore` 增加规则 `**/tests/zz_*.rs` —— 本机诊断按 `zz_` 前缀命名即自动不入库；
3. 该目标此前是全仓**唯一失败**的测试目标（见 §6.2）；**本次复跑它通过了**（同机同代码，只是 DBeaver 不再占用目标文件）——所以它是**环境相关**的，不能当作代码健康度指标。这也正是它被取消跟踪的理由：项目套件不该依赖本机内网与某个文件的占用状态。

**仍待用户处理（超出文档与代码修复范围）**：

- **轮换上述凭据**（口令已在公开仓库历史中）；
- 若需从历史中清除，需 `git filter-repo` / BFG 改写历史 + 强推，并请求 GitHub 清理缓存视图；
- 该文件在无 `D:\data\123` 与内网服务的机器上必然失败，本就不应进入任何「全绿」口径。

### 6.2 上一轮的唯一失败目标：环境原因，非代码缺陷（本轮已复跑通过）

上一轮的输出（保留作为连接链路的真机正面证据）：

```
[测试连接] MySQL      success=true  版本=9.7.2
[真实连接] MySQL      OK   耗时=150ms
[测试连接] PostgreSQL success=true  耗时=112ms
[真实连接] PostgreSQL OK   耗时=187ms
[测试连接] SQLite     success=true  版本=3.53.2
[真实连接] SQLite     OK   耗时=18ms
[测试连接] DuckDB     success=false
   IO Error: Cannot open file "D:\data\123": 另一个程序正在使用此文件
   File is already open in D:\Program Files\DBeaver\dbeaver.exe (PID 22980)
```

结论：**DuckDB 腿失败的原因是目标库文件被 DBeaver 占用**，与代码无关。四条链路里 MySQL / PostgreSQL / SQLite 的「测试连接」与「真实连接」**全部通过**——这同时是本仓连接链路的真机正面证据。

**当日第二次复跑：该目标 `1 passed`，全仓失败数归 0。** 同机、同代码、同命令，唯一差别是 DBeaver 不再占用 `D:\data\123`。
因此 §0 的「失败 0」与上一轮的「失败 1」不矛盾：**这一项的结果取决于本机环境**。

### 6.3 文档数字漂移（本轮已修）

各模块文档里的测试基线是**不同日期**的快照，本轮实测后逐项核对，漂移如下（「文档值 → 实测值」）：

| 文档 | 文档值 | 实测值 | 状态 |
| --- | --- | --- | --- |
| `crates/engine/README.md` | 全量 53 目标 / 846 通过（09-16） | **84 目标 / 1963 通过** | ✅ 已更新 |
| `docs/architecture/database/README.md` | `rds-database` 20 · `rds-workbench` 70 | **42 · 110** | ✅ 已更新 |
| `docs/architecture/database/database-navigator-showcase.md` | engine 382 · database 38 | **443 · 42** | ✅ 已更新 |
| `docs/architecture/mock/README.md` | 165 单元 + 34 引擎集成 | **190 + 37** | ✅ 已更新 |
| `docs/architecture/scratchpad/README.md` | 32 项 / 36 项（两处不一致） | **37** | ✅ 已更新 |
| `crates/scratchpad/README.md` | 36 项 | **37** | ✅ 已更新 |
| `docs/architecture/editor/editor-showcase.md` | workbench 106 · engine 440 | **110 · 443** | ✅ 已更新 |
| `docs/architecture/insight/README.md` | engine 428 | **443** | ✅ 已更新 |
| `docs/architecture/connection/README.md` | lib 71 | **44**（该模块口径跨 crate，总数另见 §2） | ✅ 已更新 |
| `docs/architecture/quick_open/quick-open-dev-plan.md` | engine 443 | **443** | ✅ 未漂移 |
| `crates/analytics_resource/README.md` | 125 + 18 + 9 | **125 + 18 + 9** | ✅ 未漂移 |
| `docs/architecture/insight/README.md` | 227 + 14 | **227 + 14** | ✅ 未漂移 |
| `docs/architecture/project/project-showcase.md` | workbench 110 · project 43 | **110 · 43** | ✅ 未漂移 |

> 规律与 `core-design-current.md` §12 一致：**模块 `README.md` 与 `dev-plan.md` §0 较新；`*-showcase.*`（宣传页）与 `crates/*/README.md` 的数字最易过期**。
> 本表更新后，若再发现冲突，请以 §1 重跑为准并顺手更新本表。

#### 6.3.1 第二次复跑新校正的数字（同日，死代码清理之后）

`9edd66a8`（本文件初版）之后落了 6 个改 `engine/src` 的提交，其中三批是死代码清理（合计约 −4.7k 行，见 `data-layer-wiring-matrix.md` §6）。数字随之变化，**§0 / §2 已换成第二次的值**，差异归因如下：

| 项 | 第一次实测 | 第二次实测 | 归因 |
| --- | --- | --- | --- |
| 工作区通过 | 1963 | **1970** | = 下列三项之和 |
| 失败 | 1（`zz_fixture_probe`） | **0** | 本机环境（§6.2） |
| `rds-engine` lib | 443 | **440** | 清理批次删掉的测试（`duckdb/extensions.rs` **12 项** + 持久化层 **1 项**，`dbi/` 本就 **0 项测试**）多于同期新增（L2 接线、PG 内省）；净值 **−3** |
| `rds-database` lib | 42 | **51** | `68bd7610` 新增 6 项（L2 命中与触发器往返）+ `68853de` 新增 3 项（写侧留痕） |
| `rds-workbench` 集成 | 129 | **130** | `zz_fixture_probe` 由失败转通过（§6.2） |
| 其余包 | —— | 不变 | —— |

**三个已核实的漂移点**（本轮已顺手改）：`crates/engine/README.md`（库单测写的 **301**，实际 440）·
`docs/architecture/database/database-navigator-showcase.{md,html}`（engine 443 → **440**、database 42 → **51**，KPI「测试 485 项」→ **491**）·
`docs/architecture/{editor,insight,quick_open}/*` 里引用 engine **443** 的位置（→ 440）。

**有意不动的**：`*-dev-plan.md` / `*-architecture.md` 里**带日期的进度记录**（如「engine 428 → 435」）——它们是当日快照，改掉等于篡改历史；
`docs/architecture/connection/*` 的 5 个文件本轮带他人的未提交改动，**为避免冲突未动**（它们写的 443 / 1963 属于上一轮快照）。

---

## 7. 维护约定

1. **数字变了就重跑 §1 的命令**，并更新 §0 / §2；不要手改数字。
2. **新增测试目标**（`tests/*.rs`）请同时更新 §2 的「集成目标」列。
3. **本机专用诊断一律用 `zz_` 前缀**（`.gitignore` 已挡住）；它们会出现在**原始命令输出**里，所以 §2 的合计按原样记录，但**必须同时给出「项目自身套件」口径**（去掉那一行）——两个数都写，别只写一个。
4. **接通状态变化时**（某能力从「零调用」变「活」）：更新 §3 / §5，并在模块文档留痕。
5. 本文件**只放能复现的数字**；口径不清的（例如跨 crate 组合出来的「N 个目标 / M 用例」）要么删掉，要么在 §2 里拆成可归属的逐目标数。
