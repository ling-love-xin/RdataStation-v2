# RdataStation v2 整体联动集成测试报告（2026-10-14）

## 0. 摘要

| 项 | 基线（2026-09-19） | 本次（2026-10-14） | 变化 |
| --- | --- | --- | --- |
| 工作区测试目标 | 85 | **116** | +31 |
| 通过 | 2005 | **2775** | +770 |
| 失败 | 0 | **0** | = |
| 忽略 | 51（真机探针 24 + Doc-tests 27） | **44**（engine 单测 24 + Doc-tests 20） | −7 |
| 编译告警 | 声称零告警 | **103 行 warning 头**（见 §3） | 漂移 |
| 修复 | — | 1 处（`mock_generator.rs` 回归，见 §1） | — |

- 命令：`cargo test --workspace -j 2 --no-fail-fast`（绕 rustup shim，显式 RUSTC/RUSTDOC）
- 日志：`.rds/tmp/rds-integration-workspace-2026-10-14.log`（4158 行）
- 完整性：116 个 `Running`/`Doc-tests` 目标 ↔ 116 个 `test result:` 结果**全部配对**，0 个 `FAILED`，日志尾部为 cargo 最终输出（`all doctests ran`）。包装进程被外部因素终止未输出 `EXIT_CODE=`，但配对完整 + 尾部正常 = 跑批完整结束。

## 1. 本次唯一代码修复：`mock_generator` 回归

- **根因**：今日提交 `4d5051db`（Mock 模块 S1-S5 收口）把 `crates/mock/src/schema_map.rs::parse_data_type` 签名从 `ColumnDataType` 改为 `MockResult<ColumnDataType>`（返回 `Result`）。mock crate 自身测试已适配，但 `crates/workbench/tests/mock_generator.rs::parse_data_type_maps_common_types`（2026-09-17 后未改）的 6 处调用未跟上 → 6 个 E0308，导致全量批在**测试目标构建阶段**中止（`--no-fail-fast` 只管运行阶段，构建失败则一个测试都不执行）。
- **修复**：6 处 `parse_data_type("...")` → `.unwrap()`，与 mock crate 自身测试风格一致。已核对实现：INTEGER / VARCHAR(64) / DECIMAL(12,2) / BOOLEAN / TIMESTAMP / WEIRD 全部返回 `Ok(...)`（未知类型回退 `Ok(Text)`；仅 `DECIMAL` 标度>精度或精度为 0 才 Err，用例不涉及），`.unwrap()` 安全。
- **验证**：`cargo test -p rds-workbench --test mock_generator --no-run` 编译通过；全量跑批中该目标 12 项全绿。
- **改动面**：仅此 1 个文件（`git status` 其余 3 个改动为既有未提交内容，未触碰）。

## 2. 全量结果（116 目标 / 2775 通过 / 0 失败 / 44 忽略）

### 2.1 逐包对照（lib 单测 + tests/ 集成，通过数）

| 包 | 基线 2026-09-19 | 本次 2026-10-14 | 变化 | 说明 |
| --- | --- | --- | --- | --- |
| `rds-engine` | 484 | **591** | +107 | 集成 32→55（8→15 目标） |
| `rds-editor` | 377 | **590** | +213 | 新增 `offline_baseline` 5 |
| `rds-workbench` | 251 | **296** | +45 | 集成 135→154（33→36 目标） |
| `rds-insight` | 241 | **241** | = | 含 `column_profile_e2e` 14 |
| `rds-mock` | 238 | **265** | +27 | 集成 48→52（4→5 目标） |
| `rds-analytics-resource` | 152 | **197** | +45 | `panel_window` 25 · `dialog_window` 10 |
| `rds-database` | 59 | **128** | +69 | 新增 4 个真机集成目标 |
| `rds-connection` | 52 | **108** | +56 | lib 102 + `tunnel_roundtrip` 6 |
| `rds-project` | 48 | **52** | +4 | 新增 `project_service_lifecycle` 5 |
| `rds-scratchpad` | 37 | **42** | +5 | — |
| `rds-shared` | 22 | **19** | −3 | doctest 忽略 9→0（示例移除） |
| `rds-settings` | 21 | **29** | +8 | — |
| `rds-plugin` | 11 | **175** | +164 | sidecar 协议 61 集成全绿，**仍无调用方**（§4） |
| `rds-paths` | 11 | **16** | +5 | `test_support_is_wired` 1 |
| `rds-workbench-shell` | 1 | **9** | +8 | — |
| `rds-app` | 0 | **6** | +6 | — |
| `rds-property` | （新） | **6** | 新 | 属性面板独立 crate |
| `rds-search` | （新） | **5** | 新 | — |
| **合计** | **2005** | **2775** | +770 | 0 失败 |

### 2.2 `rds-workbench` 36 个集成目标逐个（33 基线 + 3 新增，全部 ✅）

基线 33 目标（134 项）全部通过，且多数数量增长：

| 目标 | 基线→本次 | | 目标 | 基线→本次 |
| --- | --- | --- | --- | --- |
| `data_source_lifecycle` | 28→30 ✅ | | `insight_entry` | 2→2 ✅ |
| `mock_generator` | 12→12 ✅（修复目标） | | `insight_editor_entry` | 2→2 ✅ |
| `mock_jobs` | 11→11 ✅ | | `query_export` | 3→3 ✅ |
| `connection_project_picker` | 7→7 ✅ | | `query_history` | 3→3 ✅ |
| `connection_type_driver` | 7→9 ✅ | | `connection_dialog_ui` | 4→4 ✅ |
| `ui_contract` | 7→8 ✅ | | `dialog_host_layer` | 4→5 ✅ |
| `connection_staging` | 6→7 ✅ | | `editor_session_real` | 4→5 ✅ |
| `real_connections` | 5→5 ✅ | | `connection_multi_save` | 3→4 ✅ |
| `connection_tunnel_cleanup` | 1→1 ✅ | | `connection_render_matrix` | 2→6 ✅ |
| `connection_drafts_persist` | 1→1 ✅ | | `connection_scope_and_state` | 2→2 ✅ |
| `connection_edit_backfill` | 1→1 ✅ | | `connection_template` | 2→2 ✅ |
| `db_navigator` | 2→2 ✅ | | `global_service_singleton` | 2→2 ✅ |
| `editor_exec_real` | 1→2 ✅ | | `insight_source_real` | 2→2 ✅ |
| `official_driver_real` | 4→5 ✅ | | `insight_schema_real` | 1→1 ✅ |
| `federation_sources` | 1→1 ✅ | | `log_dialog_layer` | 1→1 ✅ |
| `oracle_federation` | 1→1 ✅ | | `mock_job_cancel` | 1→1 ✅ |
| `zz_fixture_probe` | 1→1 ✅（本机诊断） | | | |

**新增 3 目标（2026-10-03/10-09 提交引入，属性面板线）**：

| 目标 | 数量 | 提交 |
| --- | --- | --- |
| `connection_rule12` | 2 ✅ | `27e854ac` |
| `nav_property_real` | 2 ✅ | `b12c65a5` |
| `scratchpad_editor_entry` | 1 ✅ | `8e4a7a55` |

### 2.3 `rds-engine` 8 个探针目标（基线口径，全部 ✅）

| 目标 | 基线→本次 | 性质 |
| --- | --- | --- |
| `sqlglot_capabilities` | 10→10 ✅ | 报告式 |
| `transaction_affinity` | 12→12 ✅ | 无端点时跳过 |
| `duckdb_accel_probe` | 3→3 ✅ | 本地加速 |
| `duckdb_export_probe` | 3→3 ✅ | Parquet/XLSX 往返 |
| `duckdb_extensions_probe` | 1→1 ✅ | 联网约 18s，本次通过 |
| `federation_probe` | 1→1 ✅ | 跨源挂载 |
| `federation_credentials_probe` | 1→1 ✅ | 凭据脱敏 |
| `oracle_probe` | 1→1 ✅ | Oracle 扩展腿 |

引擎另有 7 个新增集成目标（`acceptance_evidence_is_real` 2 · `dependency_citations_match_lock` 1 · `error_location_from_real_errors` 6 · `mysql_rsa_auth_probe` 2 · `read_only_flag_from_real_servers` 4 · `result_rows_reach_the_temp_table` 4 · `type_decode_fidelity` 4 = 23 项）全部 ✅。plugin 9 个真机 sidecar 集成目标（61 项）也全部 ✅。

## 3. 告警审计（基线「零告警」已漂移）

`cargo check --workspace --all-targets -j 2`（日志 `.rds/tmp/rds-check-workspace-2026-10-14.log`，`Finished dev profile in 5m 08s`，无 error）**共 103 行 warning 头**。基线「零告警」声明不再成立，主要告警：

| 位置 | 数量 | 内容 |
| --- | --- | --- |
| `rds-scratchpad` (lib) | 18（9 建议） | `scratchpad_relative_time` / `paste_scratchpad_clipboard` / `scratchpad_group_header` / `set_scratchpad_template` / `label` 未用；`mode`/`paths` 字段未读；未用 import 等 |
| `rds-workbench` (lib) | 3 | `editor_exec.rs:601/602`、`view.rs:1590` 未用参数 |
| `rds-workbench` 各测试目标 | 41 | `_cx`/`_window`/`_panel` 未用参数（connection_*、dialog_host_layer、log_dialog_layer、nav_property_real 等） |
| `rds-engine` (lib) | 2 | `CommonError` 未用 import（`metadata_cache_index.rs:23`）、unused doc comment（`:25`） |
| `rds-shared` (lib) | 2 | 常量 `ZH_JSON` / `ZH` 未用（`i18n.rs:71/81`） |
| `rds-settings` (lib) | 1 | `unused_must_use`（`lib.rs:459`） |
| `rds-editor` (lib) | 2 | `unexpected_cfgs test-support`（`host.rs:2271`）、未用 `cx` |
| `rds-analytics-resource` (test) | 4 | `dialog_window` 未用 import |
| `rds-insight` / `rds-mock` (lib test) | 6 / 3 | 同上模式（`_cx` 等） |

典型类型：未用 import/变量/字段/方法/常量、`unexpected cfg test-support`、unused doc comment、`unused_must_use`、未构造变体 `NewReference`。

## 4. 联动缺口（对照 `docs/architecture/module-status.md` §3/§5，状态不变）

- **M9 `plugin` ⛔ 未接通**：测试 11→175 全绿，但整包仍**无任何 crate 依赖**，等 beta3 立项；`sidecar/{health_checker,hot_reload_manager}.rs` 为 0 字节。
- **`engine` L2 缓存零调用**：`CacheLevel`、`l2_enabled`/`l3_enabled`、`CacheVersionManager` 无生产调用；`use_cache` 默认 `false`；L2 遗留 5 张 v1 表无消费者。权威清单见 `data-layer-wiring-matrix.md` §2.2/§7。
- **缓存身份指纹未接**：`metadata_identity` 已写并有 13 项单测，但 L2 仍按 `conn_{id}` 命名，`meta_{fp}.sqlite` 未接线。
- **增量同步 / 快照表**：有意不补，等真实消费者。
- **联邦查询 🟡**：L3 桥接 · SQL Server 真机 · 扫描量可见（`duckdb/federation`）。
- **M6 引用档**：Phase 5 引用档 / 内容预览未接 UI。
- **M4 虚拟列表**：实施规格已就绪（`database-nav-dev-plan.md` §2.5）未落地。
- **Quick Open**：视图/例程定义未进 FTS；`@` 限定当前连接；最近使用/空态建议（Phase 2）。
- **settings `effect` 字段**无人消费；SSH 主机密钥默认放行；Secret 无门控不清理。

**本次联动增益**：自基线以来新接线了 3 个 workbench 集成目标（`connection_rule12`、`nav_property_real`、`scratchpad_editor_entry`），属性面板已抽为独立 `rds-property` crate 并带 6 项单测；`rds-search` 5 项单测上线。

## 5. 口径与日志

- 数字口径：本次 116 目标 = 21 单测二进制 + 77 集成/探针二进制 + 18 Doc-tests 目标；2775 = 2368 单测 + 407 集成 + 0 doctest 通过。44 忽略 = engine 单测 24 + Doc-tests 20。基线「85/2005/51」为 2026-09-19 第四次复跑口径（含 24 真机探针 + 27 doctest 忽略），引用时须写明版本。
- 过程产物：`.rds/tmp/rds-integration-workspace-2026-10-14.log`（测试）、`.rds/tmp/rds-check-workspace-2026-10-14.log`（check）、脚本 `extract_pairs.ps1`/`per_pkg.ps1`/`final_check.ps1` 等。
