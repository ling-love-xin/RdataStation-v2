# 模块状态与测试基线（实测版）

> **本文件的定位**
>
> ：全仓
>
> **唯一一份「刚跑过」的状态与基线台账**
>
> 。
> `core-design-current.md`
>
>  管
>
> **架构现状与偏差**
>
> ；本文件管
>
> **接通状态与可复现的数字**
>
> 。
> 各模块 
>
> `README.md`
>
>  / 
>
> `*-dev-plan.md`
>
>  §0 仍是该模块的
>
> **语义权威**
>
> ；本文件的数字是
>
> **某一时刻的实测快照**
>
> ，两者冲突时：语义看模块文档，
>
> **数字看本文件**
>
> （或照 §1 重跑一遍）。
> 约定：本文件的每个数字都必须能在
>
> **本机一条命令重跑出来**
>
> ；跑不出来的一律不写。
> 基线：
>
> **2026-10-23**
>
> （Windows · stable 1.98.0 · 
>
> `-j 2`
>
> ）・全仓
>
> **第十一次（r9）**
>
> 复跑（§6.10 A.3，**3162 通过 · 1 失败 · 48 忽略**；唯一失败为现象 1 命令面板鼠标点击暂停用例，0.2.4 不修，非编译错误）・下表即其数字・此前基线为 2026-10-22（第十次，§6.9）与 2026-10-21（第八次，§6.7，plugin 首次纳入全量）与 2026-10-16（第七次，§6.6，排除 plugin）与 2026-10-15（第六次，§6.5）与 2026-10-14（第五次，§6.4）与 2026-09-19（当日四次复跑，§6.3）・维护方式见 §7。



***

## 0. 摘要



| 项       | 实测值                                                                                                                                        |
| ------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| 工作区测试目标 | **108** 个 Running 目标 + **21** 个 crate 的 Doc-tests 段（含 `rds-plugin` 全量；上一轮 cargo 将 Doc-tests 合并为 1 行输出，本轮逐 crate 展开为 21 段，计数口径不变）                          |
| 通过      | **3162**                                                                                                                                   |
| 失败      | **1**：`quick_open::tests::clicking_a_row_executes_the_selected_action`（现象 1 命令面板鼠标点击进右 dock，任务 #21/#25，用户明确暂停、0.2.4 不修；非编译错误，日志无 `error[E`）                                                                                                                        |
| 忽略      | **48**（lib 28：engine 真机探针 24 · editor 1 · kernel 3；Doc-tests 20：engine 16 · connection/insight/plugin/workbench_shell 各 1）                          |
| 编译告警    | `cargo check --workspace --all-targets` **3 行既有 warning 头**（plugin `supervisor.rs` mut 不必要 ×2 · workbench `history_editor_entry` unused import · kernel `registry.rs` unused var；见 §6.10 A.3 第 4 点）；`cargo clippy --workspace --all-targets` **0 error**，存量风格 lint（CI 不设硬门禁）；本轮改动未引入新 warning |
| 代码规模    | **333,742 物理行**（非空 307,851）/ **591** 个 `.rs` 文件（`crates/`，不含 `target/`、`v1/` 历史区与 `vendor/`；2026-10-22 第十次复跑时重测，脚本 `.rds/tmp/count_code.py`，口径 = `crates/**/*.rs` 物理行含空行与注释） |
| 口径      | 上表是**原始命令的输出**；本次为 2026-10-23 第十一次复跑 r9（§6.10 A.3，`--no-fail-fast`）；其中 `rds-workbench --test zz_fixture_probe` 是**本机诊断脚本**（未跟踪、`.gitignore` 已挡，见 §6.1）——**项目自身套件（去掉 zz）= 107 个 Running 目标 / 3161 项通过** |



***

## 1. 如何复现



```
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

**环境异常：rustup shim 报&#x20;**`Permission denied`（本机遇到过，`~/.cargo/bin/*.exe` 是指向 `rustup.exe` 的符号链接，可能整体不可用）—— 绕过 shim 直接用工具链二进制：



```
RUSTC=<toolchain>/bin/rustc.exe RUSTDOC=<toolchain>/bin/rustdoc.exe <toolchain>/bin/cargo.exe \
  test --workspace -j 2 --no-fail-fast
# 本机实证：<toolchain> = D:/Dev/Rust/.rustup/toolchains/stable-x86_64-pc-windows-msvc
```

> `RUST_MIN_STACK`
>
>  与 
>
> `DUCKDB_LIB_DIR`
>
>  由 
>
> `.cargo/config.toml`
>
>  提供，
>
> **不要**
>
> 在命令行覆盖掉；本机跑测试不需要手工设环境变量。



***

## 2. 逐包基线（2026-10-15 第六次复跑）

「本包合计」= lib 单测 + 本包 `tests/` 下各集成目标。



| 包                        | lib 单测                        | 集成目标（`tests/`）                                                                                                                | 本包合计     | 忽略     |
| ------------------------ | ----------------------------- | ----------------------------------------------------------------------------------------------------------------------------- | -------- | ------ |
| `rds-engine`             | **571**                       | 55（15 个目标）                                                                                                                    | **626**  | 24     |
| `rds-editor`             | **622**                       | 5（`offline_baseline`）                                                                                                         | **627**  | —      |
| `rds-workbench`          | **81**                        | 155（36 个目标）                                                                                                                   | **236**  | —      |
| `rds-insight`            | **228**                       | 18（`column_profile_e2e` 14 · `ui_contract` 4）                                                                                 | **246**  | —      |
| `rds-mock`               | **213**                       | 49（4 个目标：`mock_engine_tests` 38 · `temp_table_cleanup` 2 · `persistence_roundtrip` 5 · `history_roundtrip` 4）                 | **262**  | —      |
| `rds-analytics-resource` | **162**                       | 35（`panel_window` 25 · `dialog_window` 10）                                                                                    | **197**  | —      |
| `rds-database`           | **151**                       | 15（`ddl_from_real_schema` 6 · `source_ddl_from_real_schema` 5 · `postgres_cross_database` 2 · `postgres_nav_introspection` 2） | **166**  | —      |
| `rds-connection`         | **102**                       | 6（`tunnel_roundtrip`）                                                                                                         | **108**  | —      |
| `rds-connection-dialog`  | **69**（新 crate，2026-10 起入工作区） | —                                                                                                                             | **69**   | —      |
| `rds-project`            | **45**                        | 9（`project_registry` 1 · `project_store` 3 · `project_service_lifecycle` 5）                                                   | **54**   | —      |
| `rds-scratchpad`         | **42**                        | —                                                                                                                             | **42**   | —      |
| `rds-shared`             | **19**                        | —                                                                                                                             | **19**   | —      |
| `rds-settings`           | **29**                        | —                                                                                                                             | **29**   | —      |
| `rds-plugin`             | **124**                       | 61（9 个 sidecar 真机目标）                                                                                                          | **185**  | —      |
| `rds-paths`              | **15**                        | 1（`test_support_is_wired`）                                                                                                    | **16**   | —      |
| `rds-workbench-shell`    | **9**                         | —                                                                                                                             | **9**    | —      |
| `rds-app`                | **6**（`src/main.rs`，非 lib）    | —                                                                                                                             | **6**    | —      |
| `rds-property`           | **6**（新 crate）                | —                                                                                                                             | **6**    | —      |
| `rds-search`             | **5**（新 crate）                | —                                                                                                                             | **5**    | —      |
| **合计**                   | **2499**                      | **409**                                                                                                                       | **2908** | **44** |

> **口径注（重要）**
>
> ：合计里的 
>
> `rds-workbench`
>
>  集成 155 项中，
>
> **1 项来自本机诊断&#x20;**
>
> `zz_fixture_probe`
>
> （未跟踪、不入库，见 §6.1）。
> 去掉它：
>
> **项目自身套件 = 118 个目标 / 2907 项通过**
>
> 。两个数都是真的，
>
> **引用时必须写明用的是哪个口径**
>
> 。
> 忽略合计 44 = engine lib 单测 24 + Doc-tests 20（connection 1・engine 16・insight 1・plugin 1・workbench_shell 1）。
> `rds-plugin`
>
>  有 185 项测试且全绿（含 61 项 sidecar 真机集成），但这
>
> **不代表它接通了**
>
> —— 整包仍
>
> **无任何 crate 依赖**
>
> （见 §5）。

### 2.1 `rds-engine` 的 8 个探针目标



| 目标                             | 数量 | 性质                       |
| ------------------------------ | -- | ------------------------ |
| `sqlglot_capabilities`         | 10 | 报告式：核验 sqlglot 能力后再封装    |
| `transaction_affinity`         | 12 | 无端点时逐项跳过                 |
| `duckdb_accel_probe`           | 3  | 本地加速通道                   |
| `duckdb_export_probe`          | 3  | Parquet / XLSX 往返（含真机台账） |
| `duckdb_extensions_probe`      | 1  | 扩展可装可载（联网，约 18s）         |
| `federation_probe`             | 1  | 跨源挂载                     |
| `federation_credentials_probe` | 1  | 凭据脱敏出口                   |
| `oracle_probe`                 | 1  | Oracle 扩展腿               |

### 2.2 `rds-workbench` 的 39 个集成目标（165 项，含本机诊断 `zz_fixture_probe`；项目自身 38 目标 / 164 项）



| 目标                          | 数量                |   | 目标                           | 数量               |
| --------------------------- | ----------------- | - | ---------------------------- | ---------------- |
| `data_source_lifecycle`     | 30                |   | `insight_entry`              | 2                |
| `mock_generator`            | 12（本次修复目标，见 §6.4） |   | `insight_editor_entry`       | 2                |
| `mock_jobs`                 | 11                |   | `query_export`               | 3                |
| `connection_project_picker` | 7                 |   | `query_history`              | 3                |
| `connection_type_driver`    | 9                 |   | `connection_dialog_ui`       | 4                |
| `ui_contract`               | 9                 |   | `dialog_host_layer`          | 5                |
| `connection_staging`        | 7                 |   | `editor_session_real`        | 5                |
| `real_connections`          | 5                 |   | `connection_multi_save`      | 4                |
| `connection_tunnel_cleanup` | 1                 |   | `connection_render_matrix`   | 6                |
| `connection_drafts_persist` | 1                 |   | `connection_scope_and_state` | 2                |
| `connection_edit_backfill`  | 1                 |   | `connection_template`        | 2                |
| `db_navigator`              | 2                 |   | `global_service_singleton`   | 2                |
| `editor_exec_real`          | 2                 |   | `insight_source_real`        | 2                |
| `official_driver_real`      | 5                 |   | `insight_schema_real`        | 1                |
| `federation_sources`        | 1                 |   | `log_dialog_layer`           | 1                |
| `oracle_federation`         | 1                 |   | `mock_job_cancel`            | 1                |
| `connection_rule12`         | 2（新增，`27e854ac`）  |   | `nav_property_real`          | 2（新增，`b12c65a5`） |
| `scratchpad_editor_entry`   | 1（新增，`8e4a7a55`）  |   | `zz_fixture_probe`           | 1（本机诊断，未跟踪）      |
| `a11y_roles`                | 4（新增，2026-10-16 a11y 断言，见 §6.6） |   | `history_editor_entry`      | 1                |
| `mock_panel_probe`          | 5（§6.7 修复后 5/5 全绿，本轮补登台账） |   |                              |                  |

> 真机目标（
>
> `*_real`
>
>  / 
>
> `*_probe`
>
>  / 
>
> `oracle_*`
>
>  / 
>
> `federation_*`
>
> ）在无环境变量或无服务时按设计跳过或空跑；
> `zz_fixture_probe`
>
>  是
>
> **本机专用**
>
> 脚本（未跟踪、
>
> `.gitignore`
>
>  已挡），
>
> **不属于项目测试套件**
>
> —— 它出现在上表只是因为本机命令输出里确实有它。



***

## 3. 模块接通状态

状态口径：**✅ 主线可用** · **🟡 部分可用**（有具名缺口）・**⛔ 未接通**。



| 模块            | crate                      | 状态 | 已知缺口（权威清单在各模块文档）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| ------------- | -------------------------- | -- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| M1 项目管理       | `project`                  | ✅  | 项目目录移动・提升 / 快照（窗口退出草稿兜底已于 2026-09-19 补：`on_window_should_close` → 会话批量落库）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| M2 双引擎底座      | `engine`                   | ✅  | 缓存层与持久化层**仍有零调用项**（权威清单见 [data-layer-wiring-matrix.md](data-layer-wiring-matrix.md) §2.2 / §7；本轮已处置约 4.3k 行，见其 §6）・增量同步未接                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| M3 数据源连接      | `connection`               | ✅  | SSH 主机密钥默认放行・Secret 无门控不清理                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| M4 数据源管理 / 导航 | `database`                 | ✅  | 虚拟列表（**实施规格已就绪**：`database/database-nav-dev-plan.md` §2.5）・PG 跨库浏览（**内容档不在本模块**：导航面板搜索框只有名称档，全文走 Quick Open 的 `#`）· **属性面板已抽取为独立 crate&#x20;**`rds-property`**（2026-10-03，tab 化 + 描述驱动 + 权限灰置，见&#x20;**[property/property-module-spec.md](property/property-module-spec.md)**）**· **表数据预览已上线（同日：属性面板「数据」页签，分页 / 排序 / NULL 置灰，只读；SQLite/DuckDB 全链路验证通过，见&#x20;**[property/property-test-report-2026-10-03.md](property/property-test-report-2026-10-03.md)**）**· **导航栏文档族按 M3 体例补齐（同日：新增规格书 / 测试报告，见&#x20;**[database/database-navigator-module-spec.md](database/database-navigator-module-spec.md)**&#x20;与&#x20;**[database/database-navigator-test-report-2026-10-03.md](database/database-navigator-test-report-2026-10-03.md)**）** |
| M5 草稿箱        | `scratchpad`               | ✅  | Phase D（归档 / 取回 / 版本）・系统拖入导入・命中跳转到行                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| M6 资产库 / 分析存档 | `analytics_resource`       | ✅  | Phase 4 分析表档・Phase 5 引用档・内容预览                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| M7 Mock 造数    | `mock`                     | ✅  | 出口不可取消・大导出非流式・并发生成                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| M8 洞察         | `insight`                  | ✅  | 分析表型存档入口・解析级策略门・表 / 库级快照                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| M9 插件宿主       | `plugin`                   | ⛔  | **整包无调用方**，等 beta3 立项                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| SQL 编辑器       | `editor`                   | ✅  | Phase 1c 分析单元（搁置）・值预览 / 编辑・血缘持久化                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| 联邦查询          | `engine/duckdb/federation` | 🟡 | L3 桥接・SQL Server 真机・扫描量可见                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| Quick Open    | `workbench/quick_open`     | ✅  | 源码（视图 / 例程定义）未进 FTS・`@` 当前连接限定・最近使用 / 空态建议（Phase 2）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| 设置            | `settings`                 | ✅  | `effect` 字段无人消费・无跨进程写锁                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |



***

## 4. 横切事实（实测）



| 项                 | 值                                                                  | 出处                               |
| ----------------- | ------------------------------------------------------------------ | -------------------------------- |
| 工作区 crate 数       | **21**（`rds-property`、`rds-search`、`rds-connection-dialog`、`rds-kernel`、`rds-tag` 为后补入独立 crate） | 根 `Cargo.toml`                   |
| `default-members` | `crates/app`（裸 `cargo test` **不含** `plugin`）                       | 根 `Cargo.toml`                   |
| 驱动                | 6 个，覆盖 4 类引擎                                                       | `engine/src/driver/loader.rs`    |
| 迁移目录              | 4 套（global /project\_meta/connection\_metadata /project\_analysis） | `engine/migrations/`             |
| 界面契约              | `ui_contract` 9 项：零裸尺寸 / 零裸色值 / 面板登记 / 共享字段白名单                     | `workbench/tests/ui_contract.rs` |



***

## 5. 未接通与零调用（如实口径）



* **M9&#x20;**`plugin`**&#x20;整包未接通**：无任何 crate 依赖它；`plugin/src/sidecar/{health_checker,hot_reload_manager}.rs` 为 0 字节。

* `engine`**&#x20;仍有零调用项**：`CacheLevel` 枚举、`l2_enabled` / `l3_enabled`、`CacheVersionManager`（L2 版本链）；查询缓存的 `use_cache` 默认 `false`，生产路径从不触发；L2 还有 5 张 v1 遗留表（`compressed_metadata` 等）无消费者。**旧口径「约 150 个公开项」本轮未重数，不要引用具体数字**—— 逐项权威清单见 [data-layer-wiring-matrix.md](data-layer-wiring-matrix.md) §2.2 / §7，已处置的那批见其 §6.1 / §6.2（`dbi` 死层 2124 行・重复的扩展管理 570 行・持久化 v1/V7 1636 行・L1 四组零调用）。

* **元数据 FTS 读 / 写侧都已接线**（本条曾写作「写入侧未接」，2026-09-19 复核纠正）：写侧 `rebuild_fts_schema` 挂在 `rebuild_schema_index` 同批（由 `NavigatorService::collect_objects` 的 Tables 分支 → `NavCache::rebuild_index` 触发），读侧 `search_fts` 供 Quick Open 的 `#` 档（`Mode::FullText` → `SearchKind::FullText`）。**真实边界**：语料只收名称 / 注释 / 数据类型（`view_definition` / `routine_definition` **未进 FTS**）；trigram ⇒ **≥ 3 字**，不足必然无命中。详账见 `data-layer-wiring-matrix.md` §2.2 与 `database/metadata-cache-vs-dbeaver-datagrip.md` §4.6。

* **缓存身份指纹（**`meta_{fp}.sqlite`**）**：`metadata_identity` 已写并有 13 项单测，但**未接线**，L2 仍按 `conn_{id}` 命名。

* **增量同步 / 快照表**：有意不补，等真实消费者。



***

## 6. 本轮实测发现（2026-09-19）

### 6.1 🔴 本机诊断脚本曾入库且已推到公开仓库

`crates/workbench/tests/zz_fixture_probe.rs` 的头部写着「**临时诊断（未跟踪文件）**……⚠️ 含**内网明文凭据**……**不要提交**」，但该文件**实际已被 git 跟踪**（commit `7814b9b6`，且已是 `origin/main` 的祖先，即**已公开**）。文件内含内网地址 `192.168.3.138` 与明文口令（MySQL `root:root`、PostgreSQL `postgres:postgresql`）。

处置（本轮已完成）：



1. `git rm --cached crates/workbench/tests/zz_fixture_probe.rs` —— **取消跟踪，保留本机文件**；

2. `.gitignore` 增加规则 `**/tests/zz_*.rs` —— 本机诊断按 `zz_` 前缀命名即自动不入库；

3. 该目标此前是全仓**唯一失败**的测试目标（见 §6.2）；**本次复跑它通过了**（同机同代码，只是 DBeaver 不再占用目标文件）—— 所以它是**环境相关**的，不能当作代码健康度指标。这也正是它被取消跟踪的理由：项目套件不该依赖本机内网与某个文件的占用状态。

**仍待用户处理（超出文档与代码修复范围）**：



* **轮换上述凭据**（口令已在公开仓库历史中）；

* 若需从历史中清除，需 `git filter-repo` / BFG 改写历史 + 强推，并请求 GitHub 清理缓存视图；

* 该文件在无 `D:\data\123` 与内网服务的机器上必然失败，本就不应进入任何「全绿」口径。

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

结论：**DuckDB 腿失败的原因是目标库文件被 DBeaver 占用**，与代码无关。四条链路里 MySQL / PostgreSQL / SQLite 的「测试连接」与「真实连接」**全部通过**—— 这同时是本仓连接链路的真机正面证据。

**当日第二次复跑：该目标&#x20;**`1 passed`**，全仓失败数归 0。** 同机、同代码、同命令，唯一差别是 DBeaver 不再占用 `D:\data\123`。

因此 §0 的「失败 0」与上一轮的「失败 1」不矛盾：**这一项的结果取决于本机环境**。

### 6.3 文档数字漂移（本轮已修）

各模块文档里的测试基线是**不同日期**的快照，本轮实测后逐项核对，漂移如下（「文档值 → 实测值」）：



| 文档                                                          | 文档值                                    | 实测值                          | 状态    |
| ----------------------------------------------------------- | -------------------------------------- | ---------------------------- | ----- |
| `crates/engine/README.md`                                   | 全量 53 目标 / 846 通过（09-16）               | **84 目标 / 1963 通过**          | ✅ 已更新 |
| `docs/architecture/database/README.md`                      | `rds-database` 20 · `rds-workbench` 70 | **42 · 110**                 | ✅ 已更新 |
| `docs/architecture/database/database-navigator-showcase.md` | engine 382 · database 38               | **443 · 42**                 | ✅ 已更新 |
| `docs/architecture/mock/README.md`                          | 165 单元 + 34 引擎集成                       | **190 + 37**                 | ✅ 已更新 |
| `docs/architecture/scratchpad/README.md`                    | 32 项 / 36 项（两处不一致）                     | **37**                       | ✅ 已更新 |
| `crates/scratchpad/README.md`                               | 36 项                                   | **37**                       | ✅ 已更新 |
| `docs/architecture/editor/editor-showcase.md`               | workbench 106 · engine 440             | **110 · 443**                | ✅ 已更新 |
| `docs/architecture/insight/README.md`                       | engine 428                             | **443**                      | ✅ 已更新 |
| `docs/architecture/connection/README.md`                    | lib 71                                 | **44**（该模块口径跨 crate，总数另见 §2） | ✅ 已更新 |
| `docs/architecture/quick_open/quick-open-dev-plan.md`       | engine 443                             | **443**                      | ✅ 未漂移 |
| `crates/analytics_resource/README.md`                       | 125 + 18 + 9                           | **125 + 18 + 9**             | ✅ 未漂移 |
| `docs/architecture/insight/README.md`                       | 227 + 14                               | **227 + 14**                 | ✅ 未漂移 |
| `docs/architecture/project/project-showcase.md`             | workbench 110 · project 43             | **110 · 43**                 | ✅ 未漂移 |

> 规律与 
>
> `core-design-current.md`
>
>  §12 一致：
>
> **模块&#x20;**
>
> `README.md`
>
> **&#x20;与&#x20;**
>
> `dev-plan.md`
>
> **&#x20;§0 较新；**
>
> `*-showcase.*`
>
> **（宣传页）与&#x20;**
>
> `crates/*/README.md`
>
> **&#x20;的数字最易过期**
>
> 。
> 本表更新后，若再发现冲突，请以 §1 重跑为准并顺手更新本表。

#### 6.3.1 第二次复跑新校正的数字（同日，死代码清理之后）

`9edd66a8`（本文件初版）之后落了 6 个改 `engine/src` 的提交，其中三批是死代码清理（合计约 −4.7k 行，见 `data-layer-wiring-matrix.md` §6）。数字随之变化，**§0 / §2 已换成第二次的值**，差异归因如下：



| 项                  | 第一次实测                 | 第二次实测    | 归因                                                                                                         |
| ------------------ | --------------------- | -------- | ---------------------------------------------------------------------------------------------------------- |
| 工作区通过              | 1963                  | **1970** | = 下列三项之和                                                                                                   |
| 失败                 | 1（`zz_fixture_probe`） | **0**    | 本机环境（§6.2）                                                                                                 |
| `rds-engine` lib   | 443                   | **440**  | 清理批次删掉的测试（`duckdb/extensions.rs` **12 项** + 持久化层 **1 项**，`dbi/` 本就 **0 项测试**）多于同期新增（L2 接线、PG 内省）；净值 **−3** |
| `rds-database` lib | 42                    | **51**   | `68bd7610` 新增 6 项（L2 命中与触发器往返）+ `68853de` 新增 3 项（写侧留痕）                                                     |
| `rds-workbench` 集成 | 129                   | **130**  | `zz_fixture_probe` 由失败转通过（§6.2）                                                                            |
| 其余包                | ——                    | 不变       | ——                                                                                                         |

**三个已核实的漂移点**（本轮已顺手改）：`crates/engine/README.md`（库单测写的 **301**，实际 443）・

`docs/architecture/database/database-navigator-showcase.{md,html}`（database 51 → **54**，KPI「测试 491 项」→ **497**）·

`docs/architecture/{editor,insight,quick_open}/*` 里引用 engine 单测数的地方（→ 443）。

**有意不动的**：`*-dev-plan.md` / `*-architecture.md` 里**带日期的进度记录**（如「engine 428 → 435」）—— 它们是当日快照，改掉等于篡改历史；

`docs/architecture/connection/*` 的 5 个文件本轮带他人的未提交改动，**为避免冲突未动**（它们写的 443 / 1963 属于上一轮快照）。

#### 6.3.2 第三次复跑（同日）：含他人未提交改动的工作树

第三次复跑（**84 目标 / 1983 通过 / 0 失败 / 51 忽略**）是在一个**并发工作树**上测的：

相对第二次的 **+13 项**里，本次改动只占 **4 项**（engine **+1**：对象位次查询；database **+3**：定位目标映射 2 + 位次→页 1），

其余 **+9 项**来自并行会话的驱动 / 连接重构（connection **44→48** · engine **440→443** · workbench **110→113**）。

**引用本表时须知**：这些数字描述的是**那一刻的工作树**，不是某一个提交。

**本轮（**`641c33b5`**&#x20;+ 定位入口接入）**：84 目标 / **1990** 通过 / 0 失败 / 51 忽略。

相对第三次的 **+7** 里，本批改动占 **+3**（database **+1**：两条入口映射一致性；workbench **+2**：`⌥↵` 定位的判别性窗口测试两条），

其余 **+4** 来自并行会话（engine **443→446** · workbench **113→116** 中的另一条）。

#### 6.3.3 定位的窗口级验收（本批）：`rds-database` lib **55 → 59**

本批给「搜索结果在树中定位」补了 **3 条窗口级测试**（`nav_view.rs` 内 `#[gpui_kit::test]`：stub 宿主 + 播种连接 /catalog/schema / 文件夹四层状态）：



* 展开链路 + 选中 + **渲染窗口覆盖目标**（否则选中了也在屏外）；

* 目标在已全部加载的文件夹里也不存在 → **如实说一句并收尾**（不留悬念）；

* 大 schema（分页）下目标不在窗口里 → **发「跳页」请求并等回执**。

为支持它，`crates/database/Cargo.toml` 的 dev-deps 开了 gpui-kit 的 `test-support`

特性（与 workbench /editor 同一写法）。`rds-database` lib **55 → 59**（本批 +3，另一条来自并行会话）。

> **本批已重跑全量**
>
> （并行会话的驱动批次已提交，工作树自洽）：
>
> **85 目标 / 2005 通过 / 0 失败 / 51 忽略**
>
> （§0 / §2 已按它更新）。
> 相对第三次复跑的 
>
> **+15**
>
>  里：本批占 
>
> **+4**
>
> （database 
>
> **+3**
>
>  窗口级定位验收 + workbench 
>
> **+1**
>
>  
>
> `editor_session_real`
>
>  的窗口退出兜底），
> 其余 
>
> **+11**
>
>  来自并行会话（engine 
>
> **446→452**
>
> ・新增 workbench 集成目标 
>
> `official_driver_real`
>
>  
>
> **4 项**
>
> 等）。

#### 6.3.4 虚拟列表切片（S1–S3）：`rds-database` lib **59 → 62**



* `843b5e1` 扁平静态 + 8 阶段行序契约（`collected_rows_match_render_order`，**+1**）；

* `2f51176` 改装 `v_virtual_list` 后 **+1**：窗口级「行被真的画出来」（尺寸 / 次序 / 行高分级）；

* 余 **+1** 来自并行会话（本批两条改动均在 `nav_view.rs` 内，可直接归属）。

**本次全量重跑本机记录**（`cargo test-all`，未含并行会话未提交改动）：



```
37 个测试目标 / 1901 通过 / 1 失败 / 24 忽略
失败：workbench quick_open::tests::opening_the_palette_loads_scratchpad_files
```

该失败是**并行满载下的超时抖动**（该用例自述「文件清单应在 20 s 内回填」，全量满载时后台任务没在窗口内跑完）；

**单跑即过**（`cargo test -p rds-workbench quick_open::tests` 绿），且与本次改动的模块无关（quick\_open / 草稿盘，不经导航树）。

> **口径差异（重要）**
>
> ：本节数字是本机 
>
> `cargo test-all`
>
>  的
>
> **目标计数**
>
> （37）；§0 / §2 的「85 目标 / 2005 通过」
> 是含集成目标与并行会话已提交内容的旧口径，两者
>
> **不可直接相减**
>
> 。下次系统复跑时以 §1 命令、逐目标落数。

#### 6.4 全仓第五次全量复跑（2026-10-14）：116 目标 / 2775 通过

**基线已更新为本次**（§0 / §2 即其数字）。命令与 §1 相同（`cargo test --workspace -j 2 --no-fail-fast`，绕 rustup shim）；

测试日志 `.rds/tmp/rds-integration-workspace-2026-10-14.log`（4158 行），check 日志 `.rds/tmp/rds-check-workspace-2026-10-14.log`

（`Finished dev profile in 5m 08s`，无 error）。

**回归修复（本次唯一代码改动）**：`crates/workbench/tests/mock_generator.rs::parse_data_type_maps_common_types` 的 6 处

`parse_data_type("...")` → `.unwrap()`。根因：`4d5051db`（Mock S1–S5 收口）把 `crates/mock/src/schema_map.rs::parse_data_type`

签名改为 `MockResult<ColumnDataType>`，workbench 集成测试没跟上 → 6 个 E0308，曾使全量批在**测试目标构建阶段**整体中止

（`--no-fail-fast` 只管运行阶段）。已核对 6 个用例全部返回 `Ok(...)`（未知类型回退 `Ok(Text)`），`.unwrap()` 安全；

修复后该目标 12 项全绿。

**逐包变化（lib + 集成，通过数）**：



| 包                        | 基线（09-19） | 本次（10-14） | 说明                                     |
| ------------------------ | --------- | --------- | -------------------------------------- |
| `rds-engine`             | 484       | **591**   | 集成 32→55（8→15 目标）                      |
| `rds-editor`             | 377       | **590**   | 新增 `offline_baseline` 5                |
| `rds-workbench`          | 251       | **296**   | 集成 135→154（33→36 目标）                   |
| `rds-insight`            | 241       | **241**   | =                                      |
| `rds-mock`               | 238       | **265**   | 集成 48→52（4→5 目标）                       |
| `rds-analytics-resource` | 152       | **197**   | `panel_window` 25 · `dialog_window` 10 |
| `rds-database`           | 59        | **128**   | 新增 4 个真机集成目标                           |
| `rds-connection`         | 52        | **108**   | lib 102 + `tunnel_roundtrip` 6         |
| `rds-project`            | 48        | **52**    | 新增 `project_service_lifecycle` 5       |
| `rds-scratchpad`         | 37        | **42**    | —                                      |
| `rds-shared`             | 22        | **19**    | doctest 忽略 9→0（示例移除）                   |
| `rds-settings`           | 21        | **29**    | —                                      |
| `rds-plugin`             | 11        | **175**   | sidecar 集成 61 全绿，**仍无调用方**（§5）         |
| `rds-paths`              | 11        | **16**    | —                                      |
| `rds-workbench-shell`    | 1         | **9**     | —                                      |
| `rds-app`                | 0         | **6**     | 来自 `src/main.rs` 单测                    |
| `rds-property`           | （新 crate） | **6**     | 属性面板独立 crate                           |
| `rds-search`             | （新 crate） | **5**     | —                                      |
| **合计**                   | **2005**  | **2775**  | 0 失败                                   |

**workbench 集成目标 33 → 36**：基线 33 个目标全部通过且多数数量增长（`data_source_lifecycle` 28→30 ·

`connection_render_matrix` 2→6 · `connection_type_driver` 7→9 · `ui_contract` 7→8 · `dialog_host_layer` 4→5 ·

`editor_session_real` 4→5 · `official_driver_real` 4→5 · `connection_staging` 6→7 · `connection_multi_save` 3→4 ·

`editor_exec_real` 1→2），`zz_fixture_probe` 1 ✅（本机诊断）；**新增 3 个**：`connection_rule12` 2（`27e854ac`）、

`nav_property_real` 2（`b12c65a5`）、`scratchpad_editor_entry` 1（`8e4a7a55`）—— 均为属性面板 / 草稿盘 / 连接规则线。

**engine 探针与新增集成**：8 个探针目标（§2.1）全 ✅；另 7 个新增集成目标（`acceptance_evidence_is_real` 2 ·

`dependency_citations_match_lock` 1 · `error_location_from_real_errors` 6 · `mysql_rsa_auth_probe` 2 ·

`read_only_flag_from_real_servers` 4 · `result_rows_reach_the_temp_table` 4 · `type_decode_fidelity` 4 = 23 项）全 ✅。

plugin 9 个真机 sidecar 集成目标（61 项）全 ✅。

**告警已清零（2026-10-14 第六轮复跑）**：第五轮（2026-10-14）曾记录 **103 行&#x20;**`warning`**&#x20;头**（未用 import / 变量 / 字段 / 方法 / 常量、`unexpected cfg test-support`、unused doc comment、`unused_must_use`、未构造变体；大头：`rds-scratchpad` lib 18 · `rds-workbench` 测试目标 41・`rds-workbench` lib 3 · `rds-engine` lib 2 · `rds-shared` lib 2 · `rds-settings` lib 1 · `rds-editor` lib 2 · `rds-analytics-resource` 测试 4・`rds-insight` / `rds-mock` lib test 6 / 3）。本轮逐条清理：



* **未用导入 / 参数 / 变量**：render 签名未用参数加 `_` 前缀（workbench/analytics/insight/mock 测试宿主）；测试目标删除未用 `FluentBuilder` / `ParentElement` 导入；`editor_exec` / `workbench view` 未用参数加 `_`；`metadata_cache_index` 删未用 `CommonError` 导入。

* **dead\_code 预留项**：`rds-scratchpad` 的 `NewReference` / `ScratchpadClipboard` / `scratchpad_relative_time` / `set_scratchpad_template` / `toolbar_drag` / `paste_scratchpad_clipboard` 为预留入口，`#[allow(dead_code)]` + 注释留痕；`chrome.rs` 删除全库零调用的 `tool_btn` 闭包。

* **结构性根因**：`rds-editor` 新增 `[features] test-support = []`（消 `unexpected cfg`，照抄 `paths` 模式）；`shared::i18n` 的 `ZH_JSON` / `ZH` 仅测试用，加 `#[cfg(test)]`；`settings` 未处理返回值显式 `let _ =`。

* **顺带修复**：`chrome.rs` 一处注释换行损坏吞掉了 `.child(case_btn)`（case 按钮未渲染 + 未用变量告警）—— 恢复代码行与中文注释，UI 按钮恢复渲染。

复跑 `cargo check --workspace --all-targets -j 2`：**0 行 warning 头**，`Finished` 1m 17s。

**口径与完整性**：116 目标 = 21 单测二进制 + 77 集成 / 探针二进制 + 18 Doc-tests 目标；2775 = 2368 单测 + 407 集成 + 0 doctest；

44 忽略 = engine 单测 24 + Doc-tests 20（connection 1・engine 16・insight 1・plugin 1・workbench\_shell 1）。

完整性硬证据：116 个 `Running` / `Doc-tests` ↔ 116 个 `test result:` **全部配对**，0 `FAILED`，日志尾部为 cargo 最终输出

（`all doctests ran`）。包装进程（Start-Process 外层）被外部因素终止、未输出 `EXIT_CODE=`—— 已知现象，不影响配对完整性。

代码规模（行数 / `.rs` 文件数）本轮未重测，§0 仍为 09-19 快照。

#### 6.5 全仓第六次全量复跑（2026-10-15）：119 目标 / 2908 通过

**基线已更新为本次**（§0 / §2 即其数字）。命令与 §1 相同（`cargo test --workspace -j 2 --no-fail-fast`，绕 rustup shim）；

测试日志 `.rds/tmp/test-all-final.log`（3914 行），check 日志 `.rds/tmp/check-all-2.log`（0 error / **0 行 warning 头**），

clippy 日志 `.rds/tmp/clippy-all-2.log`（exit 0；lint 告警 3462 条为存量，CI 不设硬门禁）。

**本次代码改动（2 处，均测试文件，生产代码零改动）**：

① `crates/workbench/tests/nav_property_real.rs` 在 `counts: &[],` 后补 `candidates: &[],`（1 行）——

HEAD `f8f2bf35`（2026-10-15 19:04「feat (property) 属性面板优化」）为 `property::ProviderInput`

新增必填字段 `candidates`，该已提交测试未同步 → E0063 曾阻塞 check /clippy/test 三扇门（测试目标构建阶段整体中止）。

② `crates/database/src/nav_jobs.rs` flake 测试加固（新增 `settle_pending_loads`，两处计数断言改用 5s 归零窗口，见事件 2）。

**逐包变化（lib + 集成，通过数）**：



| 包                                                               | 基线（10-14）          | 本次（10-15） | 说明                                                 |
| --------------------------------------------------------------- | ------------------ | --------- | -------------------------------------------------- |
| `rds-engine`                                                    | 591                | **626**   | lib 536→571（+35）；集成 55 不变                          |
| `rds-editor`                                                    | 590                | **627**   | lib 585→622（+37）；`offline_baseline` 5 不变           |
| `rds-workbench`                                                 | 296                | **236**   | lib 142→81（−60，重构迁移）；集成 154→155（`ui_contract` 8→9） |
| `rds-insight`                                                   | 241                | **246**   | lib +1；集成 14→18（新增 `ui_contract` 4）                |
| `rds-mock`                                                      | 265                | **262**   | 集成 52→49（5→4 目标，1 个目标移除）                           |
| `rds-analytics-resource`                                        | 197                | **197**   | =                                                  |
| `rds-database`                                                  | 128                | **166**   | lib 113→151（+38）                                   |
| `rds-connection`                                                | 108                | **108**   | =                                                  |
| `rds-connection-dialog`                                         | —                  | **69**    | 新 crate 入工作区（crate 数 18→19）                        |
| `rds-project`                                                   | 52                 | **54**    | lib 43→45（+2）                                      |
| `rds-scratchpad` / `shared` / `settings`                        | 42 / 19 / 29       | 同左        | =                                                  |
| `rds-plugin`                                                    | 175                | **185**   | lib 114→124（+10）；sidecar 61 全绿，**仍无调用方**（§5）       |
| `rds-paths` / `workbench-shell` / `app` / `property` / `search` | 16 / 9 / 6 / 6 / 5 | 同左        | =                                                  |
| **合计**                                                          | **2775**           | **2908**  | 0 失败（+133）                                         |

**其他事件**：



1. **plugin 并发重构自愈**：测试窗口内（19:48–20:01）`crates/plugin/src/` 被持续写入（`loader.rs` 删除、manager/lib/installer 等修改），

   首跑 `loader.rs:220` 的 E0277（`deactivate_plugin(...)?` 未 `.await`）随文件删除消失；终态复跑基于稳定后的树。

   ⚠️ 原 `loader.rs:241` 的 `let _ = manager.unload_plugin(plugin_id)` 会丢弃 async Future（卸载静默失效）——**需在重构后的新实现中确认卸载路径仍生效**。

2. **1 次负载 flake（非终态）**：`rds-database --lib` 的 `nav_jobs::tests::worker_isolates_panics_and_recovers_from_a_dead_channel`

   在并发负载下断言 `!has_pending_loads()` 失败（计数配平，15s 窗口）。**已加固**：根因 = worker 先回填结果、

   后 `fetch_sub`（微窗口）+ 兄弟测试在途任务令全局计数瞬时非零；测试改为断言前 5s 归零窗口

   （新增 `settle_pending_loads`，两处断言均改用）。加固后 5× 隔离 + 2× 全量 `rds-database --lib`（151）全部通过。

3. `nav_property_real`**&#x20;纳入并通过**：修复后该目标 2 项全绿（§2.2 已列）。

4. **冒烟**：`cargo build -p rds-app` 成功（116 MB exe），启动连接真实 MySQL（`mall_business`）导航 / L1 缓存正常、日志无 error/panic、

   PrintWindow 截图确认 UI 渲染（项目树 / 标签页 / SQL 编辑器 / 工具栏）；测试实例已停止。截图 `docs/tmp/rds-app-smoke-2026-10-15.png`。

5. **格式**：`cargo fmt --check` 漂移 329 文件 / 1958 hunk（已知存量，CI 有意不跑）。

**口径与完整性**：119 目标 = 22 单测 / 探针二进制 + 78 集成目标 + 19 Doc-tests 目标；2908 = 2499 单测 + 409 集成 + 0 doctest；

44 忽略 = engine 单测 24 + Doc-tests 20（connection 1・engine 16・insight 1・plugin 1・workbench\_shell 1）。

配对硬证据：119 个 `Running`/`Doc-tests` ↔ 120 条 `test result:`（其中 1 条为个别目标重复输出，经逐包合计复核不影响数字），0 `FAILED`；

逐包数字按「目标名 ↔ test result」配对提取并经目录清单（`crates/*/tests/`）复核，合计 2499 + 409 = **2908**。

代码规模（行数 / `.rs` 文件数）本轮未重测，§0 仍为 09-19 快照。

### 6.6 2026-10-16 第七次复跑：a11y 修复两批 + gpui-kit headless 断言（排除 `rds-plugin`）

**背景**：2026-10-15 冒烟启动日志发现 21 条 a11y INFO 告警（9 个不同 FocusId）——聚焦元素缺无障碍节点。根因：全 `crates/` 零 `.role(`（gpui 聚焦元素须 `.id(...)` + `.role(...)` 才建节点）。

**修复（两批，共 12 个文件）**：

1. **批次 1（6 元素 8 处 role）**：`property-host-panel`→Group、`search-results-host`×2→Group、`metadata-search-host`×2→Group、`analytics-resource-panel`→List、`quick-open-entry`→Button、`status-mock-cancel`→Button。
2. **批次 2（无 id 7 元素补 id+role）**：`sidebar-panel`、`right-sidebar-panel`、`editor-panel`、`editor-host-panel`、`nav-panel`、`scratchpad-panel`、`mock-detail`（另 `mock-draft-open`/`mock-result-open` 补 Button role，`settings-page` 补 Group）。

**gpui-kit headless 断言（用户点名路径，替代 OCR 定位）**：

- workbench 主界面 7 个已修元素补 `.test_support()`（`gpui_base::observe.rs` 转发：非 `test-support` 构建零成本返回原元素，`test-support` 构建包 `Observed` 进观测注册表）。
- 新增集成目标 `a11y_roles`（§2.2，4 项全过）：`sidebar-panel`/`right-sidebar-panel`/`editor-panel` 断言 `Role::Group` + 可见；`quick-open-entry` 断言 `Role::Button` + 可见。
- `window.focused(cx)` 探针：headless 窗口启动无焦点（142v1 的“启动即焦点”是真机特有路径）。

**真机复核（重启 + UIA 激活）**：

- 原 21 条告警 → **1 条残留**：`FocusId(142v1)`（“有 id 无 role”，启动默认焦点）——39v3 已消失（`mock-draft-open` 修复生效）。
- 142v1 排查已到项目代码极限：crates/ 内 8 个 `track_focus` 宿主全部补齐 id+role 仍报；启动路径无 `focus` 调用（main.rs / view.rs / nav 预连）；headless `window.focused()` = None；UIA 系统键盘焦点不在 rds-app；vendor 内 “id+track_focus 无 role” 仅在测试代码。**结论**：142v1 不在项目代码可修范围（最大嫌疑：vendor/gpui-component 组件内部或 gpui 窗口激活时 accesskit 初始树的焦点 fallback）——vendor 属第三方库，不改（保持可升级性）。

**本次复跑（第七次，2026-10-16）**：

- 命令：`cargo test --workspace --exclude rds-plugin -j 2`（`test-all` 别名等价）。
- **排除 plugin 原因**：`crates/plugin/src/render.rs:387` 测试断言 raw string `r#"..."#` 内含 `"#123456"` 提前闭合（`r#...#` 定界符冲突）——**用户未提交在途改动**（`render.rs` +94 行，渲染计算轨），用户选择跳过 plugin。
- 结果：**108 目标 / 2727 通过 / 0 失败 / 43 忽略**（项目自身套件 107 目标 / 2726 项，去掉 `zz_fixture_probe`）。
- 对比 10-15（119/2908）：减项 = plugin 目标与忽略数；增项 = `a11y_roles`（+1 目标 +4 项）。
- **并发 flake 记录**：首跑（含 plugin 缓存）`rds-editor --lib` 的 `lsp::client` 2 项（`publish_diagnostics_becomes_an_event` / `other_notifications_are_passed_through`，均为“事件未收到”）在 `-j 2` 下偶发；单跑 `--test-threads=1` 6/6 全过——**时序类 flake（非代码缺陷）**，与 §6.5 的 nav_jobs flake 同类。
- 非测试构建：`cargo check -p rds-app -j 1` 通过（test_support 零成本转发确认）；`cargo test -p rds-workbench -j 1` exit=0（含新 `a11y_roles` 与既有 connection 套件）。

**待办**：plugin `render.rs` 语法错误由用户修（或确认后我改 `r#"` → `r##"`）；修好后补跑 `-p rds-plugin` 可恢复全量口径。

#### 6.7 全仓第八次全量复跑（2026-10-21）：107 目标 / 3074 通过（含 plugin 首次全量全绿）

**基线已更新为本次**（§0 / §2 即其数字）。命令：`cargo test --workspace -j 2 --no-fail-fast`（含 `rds-plugin`——第七次的 `render.rs` raw string 语法冲突已在用户后续提交中解决，本轮 plugin 全量纳入并全绿）。

测试日志 `.rds/tmp/test-2026-10-21-final2.log`（全绿），check 日志 `.rds/tmp/check-2026-10-21.log`（`Finished dev profile in 5m 19s`，0 error / **3 行 warning 头**），clippy 日志 `.rds/tmp/clippy-2026-10-21.log`（`Finished in 2m 18s`，0 error；lint 告警为存量风格债，大头 `result_large_err` 1310 · `collapsible_if` 256 · closure large err 88 · `complex_type` 73，CI 不设硬门禁）。

**回归修复（本次 2 处，均为测试先行发现）**：

1. **`rds-insight --test ui_contract` 3 项失败**：`view/overview.rs`（用户新增右栏「摘要」Tab 视图，工作树未提交）未登记进契约测试的 `include_str` 分支 → 清单守卫 panic。已补 `"view/overview.rs"` 分支；该文件本身符合尺寸/颜色契约（无裸 px / 无裸色）。

2. **`rds-workbench --test mock_panel_probe` 4 项失败**：`Shared::open_right_panel`（导航右键 / 命令面板 / 测试宿主共用的无窗口上下文入口）只改 `active_right` 状态、不动 dock 树；真面板化后 `apply_right_mode` 仅在 dock 缺失时 `select_panel`，dock 已存在时切 tab 不生效 → 打开 Mock 面板仍渲染 Insight。修复：`Shared::open_right_panel` 同步登记 `right_open_panels`（与 `WorkbenchView::open_right_panel` 对齐）；`apply_right_mode` Expanded 分支补「active 面板未挂载 → `add_panel_view`、已挂载 → `select_panel`」（幂等，render 权威同步点同先例）。修复后该目标 5/5 全绿。

**并发 flake 记录**：`rds-mock --test mock_engine_tests::ordered_preview_returns_the_global_top_rows` 在 `-j 2` 全量满载时偶发（DuckDB 全局内存库跨 binary 竞争）；单独跑该目标 38/38 全过——**时序类 flake（非代码缺陷）**，与 §6.5/§6.6 的 nav_jobs / editor lsp flake 同类。基线按全量 0 失败记录（末次全量复跑未复现）。

**plugin 首次全量**：`rds-plugin` lib 单测 + 9 个真机 sidecar 集成目标全绿（§2.1 探针口径不变），`crates/plugin/src/sidecar/{health_checker,hot_reload_manager}.rs` 0 字节待补（§3/§5 不变，仍无调用方）。

#### 6.8 全仓第九次全量复跑（2026-10-22）：109 目标 / 3160 通过（0 失败，含 plugin 全量）

**基线已更新为本次**（§0 / §2 即其数字）。命令：`cargo test --workspace -j 2 --no-fail-fast`（含 `rds-plugin`）。测试日志 `.rds/tmp/test-2026-10-22.log`（全绿，exit 0）；check 日志 `.rds/tmp/check-2026-10-22-c.log`（0 error，**3 行既有 warning 头**：plugin `supervisor.rs` mut 不必要 ×2 · workbench `history_editor_entry` unused import · kernel `registry.rs` unused var）；clippy 日志 `.rds/tmp/clippy-2026-10-22.log`（exit 0，0 error，`Finished dev profile in 5m 11s`；63 个 crate lint 头为存量风格债，CI 不设硬门禁，本轮不逐条清理）。

**本轮工作区改动（测试先行，随 0.2.4 一并提交）**：

1. **SQL 命名参数绑定**：新增 `crates/engine/src/sql/bind.rs`（`param_names` / `substitute_params`，`:name` 扫描与字面替换）；editor 宿主以 `substitute` 替换执行目标。新增 2 项窗口测试：`view::tests::named_parameters_hold_execution_until_filled`（参数未填满前保持不执行）、`view::tests::cancelling_bind_aborts_the_run`（取消绑定即中止本轮执行）；bind.rs 内 8 项纯函数单测（同名多处 / `::` 类型转换 / 字符串与 `''` 转义 / 行块注释跳过 / 空值→NULL / 引号 `''` 转义 / 无参原样）随 engine lib 目标全绿。
2. **洞察内存管理链**：settings 新增 `Insight` 分组（`memory_limit_mb` / `full_scan_confirm` / `work_set_limit` / `status_usage`）+ registry 登记 + `set_memory_limit_mb` 持久化通知；engine/duckdb 新增 `MemoryStats` / `apply_memory_limit_mb` / `set_memory_limit_mb` / `memory_stats` / `parse_human_size`，配套 3 项单测：`test_parse_human_size`、`test_memory_stats_and_set_limit`、`test_configure_connection_applies_memory_and_temp_settings`；insight 新增 3 项：`request_full_waits_for_host_confirmation`（全量统计等待宿主确认）、`request_full_is_noop_for_temp_table_targets`（临时表目标全量统计为空操作）、`apply_insight_prefs_updates_fields`（偏好装配落字段）；workbench 接入全量统计确认弹窗订阅与偏好装配，设置变更回调推送洞察偏好。
3. **资产库全局实例分发**：analytics_resource 新增 `RowScope` / `InstanceFilter`（label）/ `to_global_rows`；`ResourcesHost` trait 新增 `request_distribute_from_global`；`resource_view` 接入 `set_instance` 与分发接线；workbench `resource_host` 实现该 trait 方法，`resource_jobs` 刷新任务配合。
4. **测试夹具适配与 warning 清理**：首轮 `cargo check-all` 在 `rds-analytics-resource` 的 `panel_window` 编译失败 3 处——`RecordingHost` 缺 `request_promote_to_global` / `request_distribute_from_global` 两个新 trait 方法、`ArchiveRow` 字面量缺 `scope: RowScope`、`ResourcesSnapshot` 字面量缺 `global_rows: Vec<ArchiveRow>`；已补齐夹具。另清理本轮新增 5 条 warning：editor `host.rs` 三个仅测试调用方法补 `#[cfg(any(test, feature = "test-support"))]` 门控；`resource_view.rs` 空 `if is_global { menu = menu; }` 自赋值反转为 `if !is_global` 并把 `move_ids` 移入块内；`editor_completion.rs` 删 unused import `ColumnAnnotation` 与未用变量 `root_s`；`editor/host.rs` 删除无调用点的冗余 `open_bind_dialog_with_window`。
5. **左 dock 命令面板修复**：`apply_left_mode` 对称右 dock 补 `active_tab` / `active_id` 句柄，Expanded 分支补「面板未挂载 → `add_panel_view`、已挂载 → `select_panel`」；`Action::OpenLeftPanel` 先登记 `left_open_panels`、置 `active_left` / `left_mode=Expanded`，再调 `apply_left_mode` 强制宿主这拍同步。右 dock 链路核对自洽（`Shared::open_right_panel` 状态登记 + `apply_right_mode` 对称补丁已于 §6.7 落地）。

**数字口径（日志逐条 `Running` / `Doc-tests` ↔ `test result:` 配对复核）**：**109 目标** = 108 个 Running 目标 + 1 行合并 Doc-tests；**3160 passed · 0 failed · 48 ignored**。忽略 48 = lib 28（`rds-engine` 真机探针 24 · `rds-editor` 1 · `rds-kernel` 3）+ Doc-tests 20（engine 16 · connection / insight / plugin / workbench_shell 各 1）。项目自身套件（去掉本机诊断 `zz_fixture_probe` 1 项）= **108 目标 / 3159 项通过**。

**用例增量说明**：本轮工作区新增 16 个用例（editor 窗口 2 + insight 3 + engine 11：DuckDB 内存 3 + 参数绑定 8，均见上文明细）；通过数较 §6.7 的 3074 增加 86，其余增量来自 0.2.3 tag 之后已提交的 32 个 commit（结果网格单行转置 / 隐藏列 / BLOB 摘要 / 列统计 / 历史面板接线 / 洞察联动 T18–T21 / 双实例服务层 / 用户自定驱动 jar / 分析模式串行队列 / gpui-kit 0.8.0 升级等，commit 清单见 Release 说明）。本轮全量 0 失败，未复现 §6.7 的 `rds-mock` 时序类 flake。

#### 6.9 全仓第十次全量复跑（2026-10-22）：108 Running + 21 Doc-tests 段 / 3161 通过（0 失败，含 plugin 全量）

**基线已更新为本次**（§0 即其数字）。命令：`cargo test --workspace -j 2 --no-fail-fast`（含 `rds-plugin`）。测试日志 `.rds/tmp/test-all-2026-10-22.log`（全绿，exit 0）；check 日志 `.rds/tmp/check-all-2026-10-22.log`（0 error，3 行既有 warning 头，本轮未新增）；clippy 日志 `.rds/tmp/clippy-all-2026-10-22.log`（exit 0，0 error，205 条存量风格 lint，CI 不设硬门禁）。

**本轮工作区改动分两条线（随 0.2.4 一并提交）**：

**A. 应用日志 ERROR/WARN 处置（2 处真 bug 修复 + 1 类 vendor 噪音写库过滤 + 1 处级别降级）**：

1. **`metadata_cache.rs` mmap_size PRAGMA 误用 execute（真 bug，历史累计 498 条 WARN）**：`PRAGMA mmap_size=268435456` 返回一行结果集，rusqlite `execute` 遇结果集即报 `Execute returned results - did you mean to call query?`。改为 `query_row("PRAGMA mmap_size=268435456", [], |_| Ok(()))`，失败仅 `tracing::warn!` 容错，口径对齐同仓 `metadata_cache_pool.rs`；不再构造被 `let _` 丢弃的 `CoreError`。同文件其余 `cache_size` / `foreign_keys` / `synchronous` PRAGMA 无结果集、历史未报错，最小改动未动。
2. **`secret_integration.rs` 本地文件源错误注册 Secret（真 bug，历史累计 64 条 WARN：`Secret type 'sqlite' not found`）**：`db_type_to_secret_type` 错误地给 `sqlite` / `duckdb` 映射了 secret type；本地文件源无凭据概念，DuckDB 也不存在这两个 secret type。已删除这两个分支落到 `_ => None`，本地文件源在类型层即被 `ensure_secret_registered_at` 的 `secret_type_of().is_none()` 提前 return；scanner 门控 `has_federable_extension`（本地加速 ATTACH 文件的另一道闸）保持不变。`test_db_type_mapping` 补 `sqlite / duckdb → None` 断言。
3. **GPUI vendor 窗口关闭/失焦时序 ERROR（噪音，历史 38 条，target=`log`）**：`window not found`、`无效的窗口句柄。 (0x80070578)`、`无效的窗口句柄 (0x80040102)`、`unable to get cursor position: 拒绝访问。 (0x80070005)`、`操作成功完成。 (0x00000000)`，均来自 gpui-pre-windows vendor 的 window/events/platform/dispatcher，是关闭瞬间向已销毁窗口发事件，非产品缺陷。`DatabaseLogLayer::on_event` 写库前加纯函数 `is_dismissible_vendor_noise(target, message)`——**仅当 `target == "log"`** 且消息等于 `window not found` 或包含上述四类文案才跳过写库；stderr 与文件层是独立 Layer 不受影响、排障仍可见；业务模块同名消息不被吞。新增单测 `vendor_window_noise_is_filtered_only_for_log_target`（5 类噪音 log target 命中、业务 target 同名不吞、非噪音放行、业务 target 噪音文案放行）。
4. **`nav_runtime.rs` 导航探针建连失败级别降级（环境/数据性状态，历史 9 条 ERROR）**：离线 / 内网不可达 / 网络档案缺失时探针建连失败，失败结果已经 `submit_connect_outcome(false)` 回填导航面板，不属进程缺陷，`tracing::error!` 降为 `tracing::warn!`。同文件「桥接运行时失败」（L113）与「load_entry_with 失败」（L83）属进程运行时缺陷 / 元数据前置读取失败且错误已向上冒泡，保留 `error!`。

**B. 应用图标替换（新版修复母版）**：

- 母版 `assets/icons/light/app-icon-coral-clean.png`（2048×2048 RGBA，橙红渐变圆角方块 + 白色 Σ 数据库，圆角外透明）。重新生成 `assets/icons/icon.ico`（7 帧 PNG：16 / 24 / 32 / 48 / 64 / 128 / 256，32bpp、planes=1，手写 ICO 容器，目录项自洽与偏移链校验通过）与 `assets/icons/icon.icns`（8 块 PNG：ic07 / ic08 / ic09 / ic10 / ic11 / ic12 / ic13 / ic14，回读 1024×1024 RGBA；旧版 il32 / l8mk / is32 / s8mk BMP+mask 块按 macOS 11+ 口径不再生成）。旧版备份于 `.rds/tmp/backup-2026-10-22/`（过程产物，不入库）。
- 标题栏图标不再写死 light：`crates/workbench/src/view.rs` 的 `render_title_bar` 按 `theme.is_dark()` 选 `dark/32x32.png` 或 `light/32x32.png`；`ActiveTheme::theme()` / `Theme::is_dark()` 签名经 vendor 0.8.0 源码核实。exe 任务栏 / 窗口类图标仍由 `crates/app/build.rs` 经 winresource 把 `icon.ico` 嵌入资源 ID=1；macOS 包由 `tools/package-release.sh` 拷贝 `icon.icns`。
- 本轮未动：数据源品牌标 `icons/db/<type_id>.svg`、插件清单 `icon = "icon.png"`、README 文档 logo `assets/public/rds-icon-{light,dark}.png`（已是新版）。

**C. 代码规模重测**：脚本 `.rds/tmp/count_code.py`（`crates/**/*.rs`，物理行含空行与注释，不含 `target/`、`v1/`、`vendor/`）实测 **591 个 `.rs` 文件 / 333,742 物理行 / 307,851 非空行**；§0 旧值（221,971 行 / 440 文件，2026-09-19 快照）已替换为本轮口径。

**数字口径（日志逐条 `Running` / `Doc-tests` ↔ `test result:` 配对复核）**：**108 个 Running 目标 + 21 个 crate 的 Doc-tests 段**；**3161 passed · 0 failed · 48 ignored**。较 §6.8 的 3160 增加 1 = engine 新增 `vendor_window_noise_is_filtered_only_for_log_target` 1 项；connection-dialog `test_db_type_mapping` 在既有测试函数内补 2 条断言，不增用例计数。忽略 48 构成与 §6.8 完全一致：lib 28（engine 真机探针 24 · editor 1 · kernel 3）+ Doc-tests 20（engine 16 · connection / insight / plugin / workbench_shell 各 1）。项目自身套件（去掉本机诊断 `zz_fixture_probe` 1 项）= **107 Running 目标 / 3160 项通过**。

**真机复核记录（2026-10-22 21:55:47 重启 rds-app debug 构建，PID 18812）**：

- **启动阶段增量日志**：以重启前 `MAX(timestamp)=2026-10-22T10:04:14.672Z`（存量 5743 条：INFO 5133 / WARN 562 / ERROR 48）为基线，启动后增量 **64 条 INFO、0 WARN、0 ERROR**；专项复现核对七类在增量内均为 0：`mmap_size` PRAGMA、`Secret type 'sqlite'`、`window not found`、`无效的窗口句柄`、`unable to get cursor position`、`操作成功完成`、探针「引用的网络配置无法解析」。增量 target 覆盖 `rds_engine::persistence::project_db`（项目 SQLite 表迁移完成）、`rds_connection_dialog::services::connection_service`、`rds_workbench::services::nav_runtime`（导航探针运行时）——缓存库打开与连接服务启动路径已走到，排除「无告警 = 路径未走到」的假阴性。
- **图标嵌入硬证据**：ctypes `FindResourceW` 回读 `target/debug/rds-app.exe` 资源段，`RT_GROUP_ICON(14)` name=1、`RT_ICON(3)` name=1..7，七帧 16 / 24 / 32 / 48 / 64 / 128 / 256、32bpp，字节数 757 / 1440 / 2267 / 4333 / 6900 / 20905 / 69113，与新 `icon.ico` 逐帧一致；`.NET ExtractAssociatedIcon` 回读 32px 为新橙红底白色 Σ 图标。标题栏深色主题 `dark/32x32.png`（深灰底珊瑚橙标）渲染肉眼验收通过。
- **仍待收尾**：① 标题栏**浅色**主题 32px 图标切换肉眼验收（设置页切浅色后核对 `light/32x32.png` 橙底白标）；② 一轮手动功能操作后**正常关闭应用**，再按增量基线复核窗口关闭 / 失焦时序噪音不再写库（纯函数过滤已有单测 `vendor_window_noise_is_filtered_only_for_log_target` 覆盖）；③ 探针建连失败降级本次启动处于在线 / 未复现失败场景，代码分级保留「桥接运行时失败 / load_entry_with 失败 = ERROR，探针建连失败 = WARN」。

**真机交互链路复核（2026-10-22 晚，命令面板驱动，15 连接全离线）**：

- **命令面板呼出与分档**：先点中央编辑器区交还焦点再 `Ctrl+P`（焦点在编辑器内直按可能不弹）。默认名称档分组「文件（草稿箱）8 条 + 连接 8 条」共 24 条，右侧分类标签 notes / data / duckdb / sqlite，底部操作提示「↑↓ 选择 · ↵ 打开 · Shift+↵ 保留面板 · Esc 关闭」；输入 `>` 进命令档再按关键词过滤，命令项右侧渲染快捷键槽位（`打开设置` = `Ctrl+,`，0.7.1+ 行为，已实测可见）。
- **左 dock 三档命令链路真机验收通过**（§6.8 第 5 条 `apply_left_mode` 对称补丁真机有效）：`>资产` → 资产库，1 条已归档「数据修复 220 B · 8 天前」，状态行「1 项 · 已归档 1」，工具条「搜索存档 / 全部 / 筛选 / 名称↑」；`>草稿` → 草稿箱，草稿 10 条（data / mock / notes / sql / 测试 分组 + 1.py / 1.sql / ceshi.sql / config.yaml / README.md 带字节大小），引用 0 · 回收站 0；`>导航` → 数据库导航，三档互斥切换、tab 句柄登记正确。
- **右 dock 三档命令链路真机验收通过**：`>洞察` → 六 Tab（摘要 / 列 / 表 / 多列 / 结构 / 历史），空态引导「在结果列表头或导航树上右键，选择洞察」，状态「已洞察表 0 / 30」；`>Mock` → 1 张草稿 `mock_data` 1,000 行 · 未生成，状态五态（未生成 / 生成中 / 已生成 / 已落库 / 引用的表没落库），四出口（持久化到项目分析库 / 追加到既有表 / 保存到草稿箱 / 另存为），底部「生成历史 (6) · 用户模板 (0)」，面板固定文案「数据只写入项目分析库（项目）/.RSmeta/analytics.duckdb 与文件，不回传源库（M7）」；`>历史` → 搜索框 + 清空 + 胶囊（全部 / 成功 / 失败 · 库:全部），存量记录含语句 · 时间 · 耗时 · 返回行 · 引擎（SQLITE / POSTGRES / MYSQL），失败条红色、行尾删除。
- **编辑器三模式真机验收**：**SQL**（`Ctrl+N`）→ 「SQL」徽标 + 执行位置「源库 · 未绑定连接」+ ▶ 执行按钮；**Analysis**（`>新建笔记`）→ 「分析」徽标 + 「跟随当前连接」+ 状态栏 `NOTE` 徽标，中央「分析笔记」说明 + `+ SQL 单元` / `+ 文本单元` 两按钮；**General**（`>新建文件`）→ 「通用」徽标 + 「选择环境 ▾ · 运行」+ 状态栏「通用」徽标，不绑连接、无 SQL 执行工具栏。已知小瑕疵：General 空态复用 SQL 欢迎文案「开始编写 SQL」，模式徽标区分不受影响，留待后续优化空态文案。
- **设置页真机验收**：`>设置`（`Ctrl+,`）模态对话框，左侧 9 个分组（外观 / 数据源导航 / 资产库 / 连接默认值 / 项目 / 日志 / 启动 / 草稿箱 / 洞察），右侧外观：主题模式（浅色 / 深色 / 跟随系统，当前深色选中）· 界面字号（13 / 14 / 15 / 16 默认 / 17）· 界面语言（中文 / English）· 标签栏最大宽度（120 / 160 / 200 默认 / 240 / 320 px）· 标签栏样式（标签页 / 描边 / 胶囊 / 分段 / 下划线），底部「修改即时生效并持久化到 settings.json」。

**本轮复核中纠正的两处 README 口径（代码实证，非观感）**：

1. **洞察实为六 Tab（旧注「五 Tab」过期）**：`crates/insight/src/model.rs` `PanelTab::ALL = [Overview, Column, Table, MultiColumn, Schema, History]`，label = 摘要 / 列 / 表 / 多列 / 结构 / 历史；`crates/insight/src/insight_view.rs:8` 文件头注释「列 │ 表 │ 多列 │ 结构 │ 历史」五 Tab 为 Overview 加入前的过期注释（仅注释，不影响行为）。README 亮点⑤、M8 进度行、洞察截图说明已由「五 Tab」改为「六 Tab（摘要 / 列 / 表 / 多列 / 结构 / 历史）」。
2. **M7「跨库直写」语义澄清（消除与亮点④「不写源库」的表面矛盾）**：`MockEngine::write_temp_table_to_database(db_path, …)`（`crates/mock/src/engine.rs:506`）是内存 DuckDB 会话经 `ATTACH` 直写到**项目分析库** `{项目}/.RSmeta/analytics.duckdb`（测试 `crates/workbench/tests/mock_jobs.rs:227` 落库目标即 `dir/analytics.duckdb`）；「追加到既有表」目标同为分析库既有表；`crates/mock/src/lib.rs:4` 约束「不回传各源数据库（M7）」，代码不存在写回用户源业务库的路径。README M7 进度行原「跨库直写落库」措辞易误解为跨业务库直写，已改为「落库到项目分析库（内存临时表 ATTACH 直写 · 可追加分析库既有表 · 不回写源业务库）」，与亮点④「代码里没有任何一条写回源库的路径」口径一致。

**标签管理入口定位（避免在连接行右键盲点）**：「管理标签…」不在连接行右键菜单，挂在导航面板标题栏「更多（⋯）」按钮的 dropdown_menu（`crates/database/src/nav_view/chrome.rs:372` `Button::new("nav-more").icon(Ellipsis)`，L524 `PopupMenuItem::new("管理标签…")` → `open_tag_manager`）。连接行右键菜单实际为：连接 / 测试连接 / 编辑连接… / 查看属性 / 移动到分组… / 复制连接（模板）… / 共享至项目 / 删除连接 / 复制名称 / 刷新元数据 / 在 SQL 编辑器中打开 / 查看洞察。标签管理对话框真机打开、连接行标签 chip 编辑、`tag:` facet 筛选留手动验收。

**手动收尾清单（2026-10-22 晚待执行）**：① 设置页切浅色肉眼验收标题栏 `light/32x32.png` 橙底白标后切回深色；② 导航「⋯ → 管理标签…」对话框、连接标签 chip 打标 / 改名、`tag:` 搜索 facet；③ 深度交互（连库执行 SQL、SQL 命名参数 `:name` 绑定对话框、联邦只读查询、洞察 / Mock 落库出口）；④ 从标题栏 ✕ 正常退出（不强杀）后，按 `.rds/tmp/log-baseline-before-manual.json`（max_ts `2026-10-22T14:04:39.638Z`）复核关闭瞬间五类 vendor 噪音不再写库。

**测试命令口径注记（2026-10-22 复核）**：权威门禁命令是 `cargo test --workspace -j 2 --no-fail-fast`（§0 3161 通过即此口径，本轮两个新单测 `vendor_window_noise_is_filtered_only_for_log_target`、`secret_integration::tests::test_db_type_mapping` 均在该日志内 ok；`view.rs` 标题栏改动 mtime 19:05 早于基线日志 19:31，已被全量覆盖）。单独 `cargo test -p rds-workbench` 会在集成测试 `tests/a11y_roles.rs` 报 `E0463 can't find crate for rds_workbench / rds_settings`——这是单包测试 target 对主依赖别名（`settings.workspace = true`）解析不全的既有口径现象，全量 workspace 构建下该 target 编译通过、用例全绿（基线日志 `a11y_roles` target 仅 1 条 linker warning）；排查单包报错前先跑全量口径比对，不要据此判定为代码回归。

**发布前第十一次复跑（2026-10-22 晚，0.2.4 打包前复核，数字与 §6.9 基线一致、无回归）**：

- `cargo check --workspace --all-targets`：**0 error**，3 个既有 warning 头（`rds-kernel` unused variable `venv` · `rds-workbench` test `history_editor_entry` unused import `AppContext` · `rds-plugin` lib test `mut` 不必要 ×2），与 §6.9 既有口径一致，本轮未新增 warning；日志 `.rds/tmp/check-all-024-final.log`（exit 0）。
- `cargo test --workspace -j 2 --no-fail-fast`：**3161 passed · 0 failed · 48 ignored**（108 Running 目标 + 21 crate Doc-tests 段），与 §6.9 基线逐条配对一致；日志 `.rds/tmp/test-all-024-final.log`（exit 0）。项目自身套件（去掉 `zz_fixture_probe`）= 107 Running 目标 / 3160 项通过。
- `cargo clippy --workspace --all-targets`：**0 error**；存量风格 lint（`result_large_err` / `collapsible_if` 等）为既有技术债，CI 不设硬门禁，本轮不逐条清理；日志 `.rds/tmp/clippy-all-024-final.log`（exit 0）。
- 口径说明：本次复核前工作区除 §6.9 已编译进 exe 的 5 个 `.rs` 修复（日志 4 处 + 标题栏明暗图标）外，仅文档（`README.md` / `README.en.md` / 本台账）改动；三道门禁确认 0.2.4 打包前代码状态全绿。

#### 6.10 第二轮全量复跑（2026-10-23）：真机反馈两问题代码补丁 + 两处并行测试竞态修复，108 Running + 21 Doc-tests 段 / 3162 通过 · 1 失败（现象 1 暂停用例）· 48 忽略

**本轮工作区改动（随 0.2.4 一并提交；现象 2 回执隔离修复已于 2026-10-23 真机复测闭环，现象 1 命令面板鼠标点击问题经用户明确暂停、0.2.4 不修，详见 A.3 与下文卡点段）**：

**A. 真机反馈两个问题（现象 2 已闭环；现象 1 代码层补右 dock 同步但鼠标点击路径未接通，暂停至下一版本）**：

1. **Ctrl+P 右侧三模块命令点了进不去面板（任务 #21，0.2.4 暂停未修）**：`quick_open` 选中「洞察 / Mock / 历史」后，旧 `Action::OpenRightPanel` 分支只调 `Shared::open_right_panel(panel, cx)`，布局同步依赖 `notify_host` 的 defer 重绘；弱引用升级失败或事件时序错位时右 dock 停在收起态，表现为「命令点了进不去」。对照左分支 `Action::OpenLeftPanel`（登记 `left_open_panels` → `active_left` → `left_mode=Expanded` → Resources 刷新 → 同步 `apply_left_mode`），右分支补齐为：先 `open_right_panel` 登记与激活，再**同步** `self.apply_right_mode(window, cx)` 这拍挂载 / 激活 / 展开；补 `tracing::info!`（target=`rds_workbench::quick_open`，字段 panel / right_mode / open_before / open_after）。改动 `crates/workbench/src/view.rs`。**真机与 headless 复核结论**：该同步补丁改善了命令经宿主端口触发后的 dock 装配，但鼠标点击行本身未抵达 `delegate.confirm` / 宿主执行端口（headless 用例 `clicking_a_row_executes_the_selected_action` 断言 `executed=[]`，即现象 1 唯一失败用例）；**键盘回车可正常进入右 dock 面板**。鼠标点击路径定位与修复列入下一版本，0.2.4 不阻塞发布，不把该问题写成已修复。
2. **编辑器「源库」通道无法执行 SQL、状态混乱、缺日志（任务 #22，先补全链路埋点定位断点，控制流未改）**：
   - `crates/editor/src/view/host.rs` `execute_labeled`：入口 `info!`（doc / mode / execute_allowed / channel / connection / statements / kind / sql 首条前 80 字符）；模式不可执行、通道不可用或写被拦、项目只读三道闸分别 `warn!`（带 reason）；参数挂起 `info!`；submit Ok `info!`、Err `warn!`。`rds-editor` crate 此前未声明 `tracing` 依赖（E0433，7 个错误全在 host.rs），`crates/editor/Cargo.toml` 补 `tracing.workspace = true`，`Cargo.lock` 反向边 +1。
   - `crates/workbench/src/services/editor_exec.rs` `run`：入口 `info!`（channel / connection / schema / use_transaction / sql 摘要）；源库成功 `info!`（rows / cols / elapsed_ms）、失败 `error!`（connection / schema / sql 摘要 / elapsed_ms / error）；`run_on_accel`、`run_on_federation` 主路径失败改 match 记 `error!`。
   - 真机复测按 `global.db` 中 target=`rds_editor::exec` / `rds_workbench::exec` 记录判定断点落在哪一道闸（模式不可执行 / 通道不可用或写被拦 / 项目只读 / 源库执行失败），再决定是否需要补通道能力代码。

**A.3 现象 2 根因定位与回执隔离修复（任务 #26，本地门禁全绿，2026-10-23 真机复测已闭环；控制流已改）**：

1. **根因（真机日志实证）**：`EditorShared.exec: Rc<RefCell<Option<ExecQueue>>>` 全应用一份队列（`crates/editor/src/shared.rs` L121）；每个 `EditorHostPanel` 各自起 60ms 回执泵。旧逻辑每拍 `drain_exec()` 全量取走队列所有回执，未提交过 SQL 的空闲幽灵面板泵连续抢先 `drain`，把别人文档的回执取走后按 `is_mine=false` 丢弃，被抢回执永久丢失——表现为成功结果不回填、失败错误也不回界面，成功 / 失败两路都空。连接、通道、模式、入队、执行器本身均正常（执行器侧已回传 rows / cols / elapsed）。
2. **修复（方案：队列按文档隔离排空，三处落盘）**：
   - `crates/editor/src/execution.rs`：紧邻 `drain()`（L1420）新增 `ExecQueue::drain_for(document)`（L1433），只返回归属该文档的回执。
   - `crates/editor/src/shared.rs`：紧邻 `drain_exec()`（L361）新增 `EditorShared::drain_exec_for(document)`（L390）透传，沿用 `ExecReceipt` 留档口径（同文档回执进 `receipts`，`MAX_EXEC_RECEIPTS=64` 溢出淘汰队首）。
   - `crates/editor/src/view/host.rs`：`drain_exec_results` 改调 `drain_exec_for(&self.document)`（调用点 L5273）；认领循环重构为埋点后 `if !is_mine { continue; }` 直接跳过非归属回执，成功路径与事务快照 / notice 提取顺接。
   - **兼容保留未删**：原全量 `drain_exec()` 与 `drain_exec_receipts()` 保留——`crates/workbench/tests/editor_exec_real.rs:212`、`crates/workbench/tests/insight_editor_entry.rs:166` 仍用全量 `drain_exec()`；`scratchpad_meta.rs:26` 用 `drain_exec_receipts()`（宿主回执给草稿箱记最近连接，独立用途）。
3. **本地门禁实证（r8 口径，日志留档 `.rds/tmp/`，均可重跑复现）**：
   - `cargo check -p rds-editor`：**exit 0**，2m 11s，0 error 0 warning（`check-editor-r8.log/.exit`、`check-editor-r8-tail.txt`）。
   - `cargo build -p rds-app`：**exit 0**，51.82s；产物 `target/debug/rds-app.exe` 170,162,688 B，mtime 2026-10-23 04:53:52；唯一 warning 是链接器创建 `.lib/.exp` 的中文 stdout 提示（非代码告警，既有口径）（`build-r8.log/.exit`）。
   - editor lib 单测：`cargo test -p rds-editor --lib -- --test-threads=1`，**exit 0 · 685 passed · 0 failed · 1 ignored · 21.01s**（`test-editor-r8.log/.exit`、`test-editor-r8-diag.txt`）。
   - **rlib 符号实证**（ar 归档解包扫描；MSVC 下符号 mangle、调试信息走外置 PDB，exe 明文搜不到属正常现象，`exe-symbols-r8.out` 全 MISS 即此口径）：`librds_editor-eaeee667d77eb8f9.rlib`（04:53:23，261 成员，41,244,980 B）含 `drain_exec_for`×26、`drain_for`×147 及 host/shared/execution 源码路径；`librds_workbench-85bc255189f23a7b.rlib`（04:53:35，261 成员，114,071,116 B）含 `editor_exec.rs`、`view.rs` 埋点源码；源码 → rlib → exe 时间戳逐层闭环（`rlib-symbols-r8.out`、`rebuild-check-r8.txt`、`exe-mtime-r8.txt`）。
   - **启动冒烟**：r8 基线 `max_id=19245`（`log-baseline-r8.json`，复测增量按 `id > 19245`）；新进程启动期 **66 条全 INFO · 0 WARN · 0 ERROR · 0 panic**（`startup-r8-summary.txt`）；启动预连只自动建立 `G_conn_SQLite_Example`，`P_real_mysql` 未自动连（`conn-r8.out`，#19302–19310），真机复测需在导航树手动连接。
4. **r9 修复后全量门禁复跑（2026-10-23，日志 `.rds/tmp/check-all-r9.*` / `clippy-all-r9.*` / `test-all-r9.*`，脚本 `gate-r9.ps1`、解析 `parse-test-r9.py`）**：
   - `cargo check-all`：**exit 0**，4m 36s；本轮 9 个改动文件在 check 告警中 **0 命中**；残余 warning 仅 §6.9 存量三类（kernel `registry.rs` unused `venv` · plugin `supervisor.rs` 两处多余 mut · workbench `history_editor_entry` unused import `AppContext`）。
   - `cargo clippy-all`：**exit 0**，2m 47s，**0 error**；本轮改动文件命中的 38 处位置全部落在既有函数体、均为存量风格 lint 类别（`type_complexity` · `collapsible_if` · `redundant_closure` · `too_many_arguments` · `clone_on_copy(PanelId)` · `needless_borrow` · `bool_assert_comparison` · doc 缩进等，与 r2 基线 205 条存量风格 lint 同类，CI 不设硬门禁，归类见 `.rds/tmp/clippy-r9-lint.txt`）；现象 2 新增的 `drain_for` / `drain_exec_for` / 认领 `continue` 分支未引入新 clippy 告警。
   - `cargo test-all --no-fail-fast`：**3162 passed · 1 failed · 48 ignored**，**129 段配对**（108 Running 目标 + 21 crate Doc-tests 段，与基线口径一致）；较 r2 基线 3161 增加 1 个非失败用例。退出码 101 由唯一失败用例导致，**非编译错误**（日志无 `error[E` / `could not compile`）。
     - 唯一失败：`quick_open::tests::clicking_a_row_executes_the_selected_action`（`crates/workbench/src/quick_open/tests.rs:231`，断言 `left=0, right=1`：鼠标点击行未经宿主端口执行，`executed=[]`）——即现象 1 命令面板鼠标点击问题（任务 #21/#25），**用户已明确暂停、0.2.4 不修**；`rds-workbench --lib` 91 passed / 1 failed。
     - 现象 2 修复面无回归：`rds-editor --lib` **685 passed · 0 failed · 1 ignored**（与 r8 单包口径一致）；`rds-engine` 605 passed / 24 ignored；ignored 合计 **48** 结构与基线完全一致（lib 28：engine 24 · editor 1 · kernel 3；Doc 20：engine 16 · connection/insight/plugin/workbench_shell 各 1）。项目自身套件（去掉 `zz_fixture_probe` 1 项）= 107 Running 目标 / 3161 项通过。
5. **真机复测闭环（2026-10-23，PID 15024，基线 `max_id=19398`，增量日志 `.rds/tmp/log-exec-r8.txt` / `.json`，探针 `probe_exec_r8.py`）**：
   - 总量：增量 84 条，执行链 35 条；链路 `提交 5 → 入队 5 → 执行器接收 5 → 源库成功 3 / 失败 2 → 回执泵 5 → 认领 5 → 投影 5`，全程闭合无缺环。
   - **回执归属（修复目标）**：认领 `is_mine=true=5`，**跨面板误抢 `is_mine=false=0`**。
   - **双文档隔离实证**：`doc-10` 两条成功（#19438 `...order LIMIT 200;` 200 行 16 列 55ms；#19447 `...order LIMIT 20` 20 行 16 列 54ms），认领 `receipt_doc=doc-10`；`doc-7` 两条失败 + 一条成功（#19455 `connection=None` 本地方言报错；#19463 选区 `order` MySQL 1064；#19473 `...order` 1000 行 16 列 140ms），认领 `receipt_doc=doc-7`；两文档回执互不串号，结果各自回填。
   - **投影形状**：成功 3 条 `has_entry=true · has_grid=true · failed=false`（cols=16，rows=200 / 20 / 1000）；失败 2 条 `has_entry=true · has_grid=false · failed=true`（错误卡片回填、无结果网格）。
   - **2 条 ERROR 定性**：均为输入 SQL 自身语法问题被执行器如实捕获回传、回填失败卡片，**非程序崩溃 / 非连接异常 / 非未捕获错误**——#19458 文档未绑定连接（`connection=None`）落到本地方言解析，`order` 保留字触发 `near "order": syntax error`；#19466 选区仅提交标识符 `order`，MySQL 1064 语法错误。
   - 与清单出入说明：清单拟用 `SELECT 1` / `SELECT 1 AS v` 等简单语句，真机以 `mall_business.order` 查询为主；核心验收点（成功网格回填、失败错误卡片回填、双文档隔离、零抢回执）全部覆盖通过。
   - **遗留观察（不阻塞 0.2.4）**：`connection=None`（未绑定且无活动连接）时错误信息来自本地方言而非目标库，后续可在未绑定场景补一条明确提示；归入下一版本观察项。
6. **遗留同根因债（不阻塞 0.2.4，列入下一版本阶段 1）**：`host.rs` 的 `tx_notes` / `commit_notes` / `source_notes` / `export_notes` 四通道仍是「先全量 `drain` 再按文档 `continue` 过滤」，且 pending 计数在过滤前就 `saturating_sub(1)`，存在与现象 2 同类的跨面板丢回执风险；本轮未改，处置口径见 A.4 决策文档阶段 1。

**A.4 执行上下文模型决策（下一版本规划，不进 0.2.4 代码线）**：

- 已完成 DBeaver 社区版 `devel` 分支源码调研（非产品行为转述；`master` ref 不存在，社区版主分支为 `devel`），对照本仓现状代码实测形成决策文档 [`../connection/execution-context-model.md`](../connection/execution-context-model.md)，并登记 `connection/README.md` §6 文档地图、`editor-backlog.md` 第二节。
- 核心结论：**不做通用连接池**（桌面查询无并发吞吐压力；池化归还必须重置临时表 / `SET` 变量 / `search_path` / 残留事务 / 未关闭游标等会话状态，复杂度高收益低；DBeaver 社区版 `Driver.connect` 新建、`connection.close()` 直接关闭，同样不池化）。
- 目标模型：Source 档 per-context 独立物理连接 + 独立事务 + 独立取消（惰性建连 / 关闭释放 / 空闲超时回收 / 语句锁 / 独立元数据上下文）；Accelerated、Federated 两档本地嵌入式会话维持现有复用键（`conn_id` / owner+源指纹），不拆 per-editor 独立实例。
- 现状差距（实测）：物理连接、事务、取消令牌按 `conn_id` 跨文档共享，执行队列全局一份、单 `busy` 原子位 + 单工作线程 + 单 `running_connection` 槽；结果集 `ResultStore.documents[]` 已按文档分槽（这一层做对了，目标模型维持不动）。
- 方案 A（per-context 独立连接，推荐）/ B（单连接 + 仅 UI 归属修正）/ C（连接池复用，明确不推荐）与待拍板项（方案选型、独立连接上限与空闲超时时长、元数据上下文是否同批、是否提供单连接模式偏好、事务内语句取消 / 超时缺口）见决策文档 §4、§6；迁移分阶段 0–3 见其 §5，0.2.4 仅含阶段 0（回执按文档隔离排空，即 A.3）。

**B. 两处全量并行下的测试隔离竞态（与本轮真机 bug 生产代码无关，均为存量隔离缺陷被 `-j 2` 并行调度触发；单包复跑均全绿）**：

1. **settings 越界字号用例（`crates/settings/src/lib.rs::out_of_range_font_size_is_rejected`）**：`init` 读共享 `settings_path()`；同进程 `font_size_applies_global_and_persists` 在隔离根上 `set_font_size(14.0)` 的中间态被并行越界用例 init 读到，旧断言硬编码「拒绝后值 = 16.0」失败。`paths` test-support 按进程 id 隔离（`rds-test-root-<pid>`），同一测试进程内所有用例共享同一 settings.json。修复：断言改为保持 `before = SettingsService::font_size(cx)`（init 读到的值），拒绝语义是「不写入」，与读到何值无关；单包 `cargo test -p rds-settings --lib` 31 passed 全绿。
2. **mock 未知列用例（`crates/mock/tests/mock_engine_tests.rs::ordered_preview_returns_the_global_top_rows`）**：`try_preview_sample` 在内存库 `Mutex<Connection>` 拿锁 `WouldBlock` 时非阻塞返回 `Ok(None)`（`crates/mock/src/engine.rs:333`），绕过 DuckDB `prepare` 对未知排序列的报错；并行竞争锁时测试 `is_err()` 误判。同文件 `sample_preview_when_idle`（L556）已有 200 次 ×20ms 等锁重试兜底，唯独未知列断言那行直调没等锁。修复：未知列断言改为等空闲重试循环——`Err(_)` 记为已拒绝并 break，`Ok(Some(_))` 直接 panic（未知列不能被放行），`Ok(None)` 睡眠 20ms 重试；循环结束断言 `unknown_column_rejected`。单包 `cargo test -p rds-mock --test mock_engine_tests` 38 passed 全绿。

**第二轮门禁（日志 `.rds/tmp/check-024-r2.log` / `clippy-024-r2.log` / `test-024-r2.log`，exit 均 0）**：

- `cargo check --workspace --all-targets`：**0 error**；warning 仍是 §6.9 存量三类（kernel `registry.rs` unused `venv` · plugin `supervisor.rs` 两处多余 mut · workbench `history_editor_entry` unused import `AppContext`），本轮改动文件零告警。
- `cargo clippy --workspace --all-targets`：**0 error**；命中全是存量 `result_large_err`（CoreError 大 Err）与 MySQL / PG rollback 文案，无一条指向本轮改动的五个文件。
- `cargo test --workspace -j 2 --no-fail-fast`：**3161 passed · 0 failed · 48 ignored**（108 Running 目标 + 21 crate Doc-tests 段），与 §6.9 基线逐条配对一致；settings `out_of_range_font_size_is_rejected`、mock `ordered_preview_returns_the_global_top_rows` 在全量并行下均 ok。项目自身套件（去掉 `zz_fixture_probe`）= 107 Running 目标 / 3160 项通过。（此为竞态修复时 r2 复跑口径；**最新 r9 全量基线见 A.3 第 4 点：3162 passed · 1 failed · 48 ignored**，唯一失败为现象 1 暂停用例。）
- `cargo build`：debug `rds-app.exe` 170,149,888 B，2026-10-23 02:01:51 新构建，含本轮补丁，1m41s Finished dev profile，无 error。

**发布卡点闭环状态（2026-10-23）**：现象 2 回执隔离修复真机复测已闭环（A.3 第 5 点）；现象 1 命令面板鼠标点击问题经用户明确暂停、0.2.4 不修，不阻塞发布。以下为启动冒烟、环境留痕与素材归档实况。

- **启动冒烟（本地已完成，2026-10-23 02:05:37 新进程 PID 16492）**：清理断电残留旧进程 PID 19536（共用 `global.db` 会写串日志）后单实例启动；按上一轮基线 `max_ts=2026-10-22T14:04:39.638Z` 分段统计，新进程启动期 **66 条 INFO · 0 WARN · 0 ERROR · 0 panic**；target 覆盖 `rds_app`（启动）· `gpui_component::theme::registry`（主题注册）· `rds_engine::persistence::project_db`（项目库打开/迁移）· `rds_connection_dialog::services::connection_service`（连接服务）· `rds_workbench::services::nav_runtime`（导航探针）· `rds_database::nav_view::actions`（导航树加载）· `rds_plugin::sidecar::runtime`（包结构初始化，本轮不接入 plugin），核心启动路径完整走到，排除「无告警 = 路径未走到」假阴性；启动期无 `mmap_size` / Secret type / vendor 窗口关闭失焦噪音写库。分段日志 `.rds/tmp/log-startup-segments.json/.txt`，探查脚本 `.rds/tmp/probe_startup_segment.py`。
- **环境备注（本轮自动化桌面操作留痕）**：结束残留进程 19536、Win32 `ShowWindow`/`BringWindowToTop`/`SetForegroundWindow` 置前 PID 16492 主窗口（hwnd 1900760，rect −7,−3,1542,1044，非最小化、Responding True）、关闭第三方 PowerToys 错误弹窗；桌面另有第三方开始菜单增强浮层处于置顶状态，自动化截图与坐标点击无法可靠露出本项目窗口（无 `test_support` 标记、坐标盲点不可靠，与 §6.9 标题栏「更多」按钮同类限制），故交互复测与截图按分工交用户手动，自动化侧只做进程/窗口/日志层面的冒烟核验。
- **现象 1（0.2.4 暂停，不阻塞发布）**：`Ctrl+P` → `>洞察` / `>Mock` / `>历史` **键盘回车**可正常展开右侧 dock 面板；日志 target=`rds_workbench::quick_open` 记 open_before / open_after。**鼠标点击行未抵达宿主确认端口**（headless 用例 `clicking_a_row_executes_the_selected_action` 即此问题，唯一失败用例），定位与修复列入下一版本；`insight.gif` / `mock.gif` / `history.gif` 画面待该问题闭环后重录。
- **现象 2（回执隔离修复，已真机闭环，详见 A.3 第 5 点）**：绑定 `P_real_mysql` 的 SQL 文档执行成功网格回填、失败错误卡片回填、双文档结果互不覆盖、零跨文档误抢，均已在 PID 15024 真机复测中实证通过（基线 `max_id=19398`，增量日志 `.rds/tmp/log-exec-r8.txt` / `.json`，探针 `probe_exec_r8.py`）。复测前启动进程 PID 15024，启动期 66 条全 INFO · 0 WARN · 0 ERROR · 0 panic（`startup-r8-summary.txt`）。遗留观察：未绑定连接（`connection=None`）时错误信息来自本地方言而非目标库，下一版本补明确提示；`tx_notes` / `commit_notes` / `source_notes` / `export_notes` 四通道回执隔离列入下一版本阶段 1（A.3 第 6 点）。
- **素材归档实况（2026-10-23，`assets/screenshots/RDS-beta-0.2.4/`，hex 魔数 / 尺寸 / 帧数实测留档 `.rds/tmp/media-archive-r9.txt`）**：已归档 6 份画面不受现象 1 / 2 影响的素材——`main-window.png`（3150×2014）、`quick-open.png`（3150×2014）、`database-navigator.gif`（1698×1080 · 186 帧）、`analytics-resource.gif`（1698×1080 · 182 帧）、`scratchpad.gif`（1698×1080 · 114 帧）、`settings.gif`（1698×1080 · 102 帧）。**未归档**：`editor-sql.png` 为现象 2 修复前旧件，复测通过后需重录成功网格画面；`editor-error.png` 缺失，需补拍失败错误卡片画面；`insight.gif` / `mock.gif` / `history.gif` 属现象 1 暂停项，闭环后重录。README 截图表本轮只挂已归档 6 份，缺图模块在表下注明待补，不以修复前旧件冒充新成果。



***
***

## 7. 维护约定



1. **数字变了就重跑 §1 的命令**，并更新 §0 / §2；不要手改数字。

2. **新增测试目标**（`tests/*.rs`）请同时更新 §2 的「集成目标」列。

3. **本机专用诊断一律用&#x20;**`zz_`**&#x20;前缀**（`.gitignore` 已挡住）；它们会出现在**原始命令输出**里，所以 §2 的合计按原样记录，但**必须同时给出「项目自身套件」口径**（去掉那一行）—— 两个数都写，别只写一个。

4. **接通状态变化时**（某能力从「零调用」变「活」）：更新 §3 / §5，并在模块文档留痕。

5. 本文件**只放能复现的数字**；口径不清的（例如跨 crate 组合出来的「N 个目标 / M 用例」）要么删掉，要么在 §2 里拆成可归属的逐目标数。