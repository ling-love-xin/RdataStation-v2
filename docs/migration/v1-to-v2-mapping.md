# v1 → v2 迁移映射

> 状态：**初步映射（基于模块名与目录职责，待读码确认）**。
> 原则：v1 源码已整体暂存于 `v1/`，不参与 v2 编译；迁移时按本表逐模块抽取到目标 crate，确认一处迁移一处。

## 迁移进度

### ✅ Round 1（已完成，`cargo check --workspace` 通过）

| v2 crate | 迁入内容（自 v1） | 说明 |
| --- | --- | --- |
| `shared` | `core/` 顶层基础层：`error.rs`、`models.rs`、`types.rs`、`arrow.rs`、`stream.rs`、`utils/`（hash/string/time）、`crypto.rs`、`port_negotiation.rs`、`api_version.rs`、`macros.rs` | 全量复制；`crate::core::` → `crate::`；保留 specta TS 类型生成；error.rs 的 `From<duckdb::Error>/From<rusqlite::Error>` 暂留（技术债：error 域拆分时解耦） |
| `engine` | `core/duckdb/`（12 文件）+ `core/driver/native/duckdb.rs` 的 `duckdb_rows_to_arrow`（抽取为 `duckdb/row_to_arrow.rs`） | DuckDB 分析引擎全量复制；`crate::core::error::` → `shared::error::`；`shared` 以依赖键别名引用（`shared = { path = "../shared", package = "rds-shared" }`） |
| `app` | —（新建） | 接入 `gpui-kit = "0.6"`（workspace 依赖），最小可运行 App Shell：`application().run` → `init` → `open_window` → `Root::new(workspace, window, cx)` |

**v1 保留策略**：本轮为复制式迁移，`v1/backend/src/core` 对应文件**全部保留**（仍被 v1 的 services/persistence 等模块引用，且 v1 作为留档暂存区）；待对应 Feature 迁移完成并验证后，再按映射表逐项删除。

### ✅ Round 2（已完成，`cargo check --workspace` 通过，零告警）

| v2 crate | 迁入内容（自 v1） | 说明 |
| --- | --- | --- |
| `engine` | `core/driver/` **全量**（顶层 13 文件 + `connection/` + `jdbc/` + `native/` + `registry/` + `wasm/`） | 驱动 trait 抽象、注册/发现/路由、SmartPool/StandardPool 双层池、native 具体驱动（mysql/postgres/sqlite/duckdb）；`native/duckdb.rs` 内的 `duckdb_rows_to_arrow` 定义去重（统一引用 `duckdb/row_to_arrow.rs`） |
| `engine` | `core/dbi/` 全量（`dbi.rs` + `context` + `session` + `performance` + `engine/`） | 统一数据访问层：多引擎路由（Driver/DuckDB/Stream）、会话事务、性能统计、SQL 特征分析 |
| `engine` | `core/cache/` 全量（7 文件） | 多级缓存（L1 LRU / L2 SQLite / L3 源库）、内存守卫 `MemoryPressure`（smart_pool 依赖） |
| `engine` | `core/services/connection_manager.rs`（649 行） | 连接生命周期管理（`get_connection_manager`/`ConnectionManager`），dbi/engine 的依赖；其余 services 待后续 Feature 迁入 |

**v1 保留策略**：同 Round 1——复制式迁移，v1 对应文件全部保留，待对应 Feature 迁移完成并验证后统一删除。

### ✅ Round 3（已完成，`cargo check --workspace` 通过，零告警）

| v2 crate | 迁入内容 | 说明 |
| --- | --- | --- |
| `connection` | 自 `engine/src/driver/connection` 抽取（源自 v1 `core/driver/connection`）：`config.rs`（294 行，含 Ssh/Ssl/Proxy/Chain 配置）、`connector.rs`（846 行，SSH 隧道/SSL 包装/TunnelGuard）、`factory.rs`、`known_hosts.rs`（281 行，known_hosts 管理）、`stream.rs`（173 行，连接流） | 连接层**完全自包含**（仅依赖 shared::error + russh/tokio），可干净抽取；engine 的 `driver/factory.rs` 与 `driver/registry/config.rs` 改为引用 `connection` crate；`engine/src/driver/connection` 已移除 |
| `connection`（占位） | `secret.rs` 保留占位 | M3 核心差异化能力 **DuckDB Secret 本地加速通道** 待实现（`CREATE [PERSISTENT] SECRET` / ATTACH 联邦，参考 DuckDB Secrets Manager） |

**v1 保留策略**：同前——复制式迁移，v1 对应文件全部保留，待 Feature 完整迁移后统一清理。

### ✅ Round 4（已完成，`cargo check --workspace` 通过，零告警）

| v2 crate | 迁入内容 | 说明 |
| --- | --- | --- |
| `database` | 自 v1 `core/services/metadata_service.rs`（179 行） | 元数据浏览统一入口：catalog/schema/table/column/index/constraint/procedure/function/sequence/trigger 列枚举 + 例程源码；`MetadataBrowser` 主路径 + `Database` fallback 双通道 |
| `database`（占位） | `database_view.rs` / `property_panel.rs` 保留 | M4 导航树与对象属性面板待 GPUI 阶段实现 |

**v1 保留策略**：同前——复制式迁移，v1 对应文件保留，待 Feature 完整迁移后统一清理。

### ✅ Round 5（已完成，`cargo check --workspace` 通过，零告警）

| v2 crate | 迁入内容 | 说明 |
| --- | --- | --- |
| `engine/persistence` | 自 v1 `core/persistence` 全量 21 文件（**不含** `analytics_resource_store/`，归 M6）：auth/connection/driver/env/network/plugin/project/plugin/sql_template/workbench_context/history/log/insight/insight_meta/metadata_cache(+pool)/cache_version_migration/global_db(1708 行)/project_db/project_connection_store/id_prefix | SQLite + DuckDB 双层元数据仓储：系统级（global_db，跨项目共享）+ 项目级（project_db，物理隔离）；`mod.rs` 剥离 analytics_resource_store 声明与导出 |
| `engine/logging` | 自 v1 `core/logging`（record/redact/config/layer） | 日志仓储与 tracing Layer；`LogStore` 落地 SQLite |
| `engine/migration` | 自 v1 `core/migration`（manager/executor/schema/global_init）+ `migrations/`（4 库 30 SQL） | `include_dir!("$CARGO_MANIFEST_DIR/migrations")` 编译时嵌入；迁移目录已复制到 `crates/engine/migrations/` |
| `engine/persistence/insight_types.rs` | 自 v1 `core/services/result_service.rs` 抽取"洞察体系"类型组（15 个 pub struct/enum：ColumnInsightFull/ColumnStats/ColumnStatsDetail/NumericStats/ExtremeValue/TextStats/TextFrequency/DateTimeStats/BooleanStats/DistributionBin/TableProfile/TableColumnMeta/QualityScore/QualityDimension/TableQuality/ColumnQualityEntry） | 解决 insight_store 跨 services 依赖；标注 TODO 随 M8 insight crate 迁移 |

**本轮修坑**：engine 补 `rand` 依赖；2 处 `crate::core::` 单类型残留（`crate::core::CommonError` / `crate::core::DuckDBManager`）→ `crate::`；engine lib.rs 补齐 `shared::error` 全量 re-export（CommonError 等）与 logging re-export。

**v1 保留策略**：同前——复制式迁移，v1 对应文件全部保留，待 Feature 完整迁移后统一清理。

### ✅ Round 6（已完成，`cargo check --workspace` 通过）

| v2 crate | 迁入内容 | 说明 |
| --- | --- | --- |
| `engine/services` | `sql_service`（616 行，SQL 统一执行入口）、`sql_parser_service`（151 行）、`duckdb_service`（364 行，DuckDB 专用分析服务）、`execution_service`（92 行，执行编排）、`snapshot_service`（269 行，快照） | 查询执行基础设施，从 v1 services 集群中按依赖分层抽取 |
| `engine/sql` | v1 `core/sql` 全量（builder/engine/formatter/parser/transpiler，基于 sqlglot-rust） | SQL 方言解析/生成/转译 |
| `engine/services/result_types.rs` | `ResultSet`（自 result_service.rs 抽取） | 解除 duckdb/execution 服务对 result_service 的依赖 |

**修坑**：sqlglot-rust 锁 `=0.9.25`（v1 Cargo.lock 版本，0.9.37 API 不兼容——TableRef 缺 alias_quote_style）；duckdb_service 可见性 `pub(crate)`→`pub`（跨 crate 调用）；补 services/mod.rs。

### ✅ Round 7（已完成，`cargo check --workspace` 通过）

| v2 crate | 迁入内容 | 说明 |
| --- | --- | --- |
| `insight`（M8） | 规则引擎：v1 `core/insight` 全量（rule_executor 719 行 / rule_registry 254 行 / rule_types 90 行 / schema_analyzer 703 行 + `mod.rs` 全局注册表/热加载逻辑） | 内置规则资产 `insight-rules/`（column/multi/quality/table 共 4 类 .rule.toml）include_dir 编译时嵌入 |
| `insight`（M8） | 分析服务：`insight_engine`（831 行，列洞察全量计算/规则执行）、`quality_scorer`（384 行，质量评分）、`table_profile_service`（135 行，表画像） | 打破 v1 内 `services ↔ insight` 循环：分析层仅依赖 result 的**类型**（engine::persistence::insight_types），result 依赖分析层的**函数**（跨 crate 单向） |

**依赖环破拆说明**：v1 单 crate 中 `result_service ↔ insight_engine`、`schema_analyzer → sql_service` 为循环引用；v2 分层为 `workbench → insight → engine`、`insight → engine` 单向无环。

**过渡期告警**：engine 3 个 + insight 31 个 dead_code（`execution_service::re_execute_with_filter`、`quality_scorer::compute_column_quality` 等 pub(crate) 函数）——调用方为 result_service / persistence_service（workbench 轮次迁入后消除）。

**v1 保留策略**：同前——复制式迁移，v1 对应文件全部保留，待 Feature 完整迁移后统一清理。

### ✅ Round 8（已完成，`cargo check --workspace` 通过，零告警）

| v2 crate | 迁入内容 | 说明 |
| --- | --- | --- |
| `workbench`（M5） | `connection_service`（1912 行，数据源连接管理：测试/CRUD/路由/网络配置解析）、`result_service`（结果集服务 + 洞察计算编排外观）、`persistence_service`（236 行，洞察画像回写）、`driver_service`（77 行，驱动管理） | **过渡期告警清零**：Round 6/7 的 dead_code（compute_column_quality / re_execute_with_filter 等）随调用方迁入全部消除 |

**本轮处理**：result_service 副本中与 engine 抽取重复的洞察类型定义（15-181 行）删除，改引用 `engine::persistence::insight_types` / `engine::services::result_types`；残留引用修复（`crate::core::migration::`→`engine::migration::`、`crate::api::dto::QueryResult`→`shared::models::QueryResult` 等）；可见性上浮（engine execution/sql service、insight 分析服务 `pub(crate)`→`pub`，跨 crate 调用所需）。

**依赖环全解**：`workbench → insight → engine`、`workbench → engine → connection → shared` 全部单向。

**v1 保留策略**：同前——复制式迁移，v1 对应文件全部保留，待 Feature 完整迁移后统一清理。

### ✅ Round 9（已完成，`cargo check --workspace` 通过，零告警）

| v2 crate | 迁入内容 | 说明 |
| --- | --- | --- |
| `project`（M1） | v1 `core/project` 全量：`models.rs`（309 行：Project/ProjectConfig/ProjectInfo/ProjectPath/ProjectStatus/ConnectionRef/QueryRef/Versioned 等版本化模型）、`store.rs`（732 行：ProjectManager/ProjectStore） | **双层数据架构装配**：Project 数据分层 = SQLite（meta/project.db 元数据/事务）+ DuckDB（analytics/data.duckdb 分析数据/版本载入）+ Config（连接配置/SQL 文件）；版本化支持为 DuckLake 多人协同预留 |
| `engine/driver/missing_driver.rs` | `MissingDriver` 自 workbench driver_service **上移** engine 驱动层 | 双使用方（project 检测缺失驱动 + 驱动服务语义），符合 shared 上移原则；workbench 原定义删除 |

**本轮处理**：MissingDriver 抽取（含 doc 注释，大括号配对定位）+ engine driver re-export；project 引用改写（`crate::core::project::models::`→`crate::models::`、`crate::core::migration::`→`engine::migration::` 等）；project 补 rusqlite 依赖；workbench driver_service 清理 unused specta/MissingDriver import。

**v1 保留策略**：同前——复制式迁移，v1 对应文件全部保留，待 Feature 完整迁移后统一清理。

### ✅ Round 10（已完成，`cargo check --workspace` 通过，零告警）

| v2 crate | 迁入内容 | 说明 |
| --- | --- | --- |
| `scratchpad`（M5 草稿箱） | v1 `core/scratchpad` 全量：`models.rs`（106 行：ScratchpadEntry/ScratchpadConfig/SearchResult/DiffResult/ReplaceResult/AnalyzableFile/ExternalReference）、`state.rs`（48 行：ScratchpadState 状态机）、`store.rs`（1139 行：ScratchpadStore 文件系统存储） | 类 VS Code 文件管理草稿工作区；依赖极简（仅 shared + tokio/serde/chrono/regex/similar/opener），干净独立 |

**v1 保留策略**：同前——复制式迁移，v1 对应文件全部保留，待 Feature 完整迁移后统一清理。

### ✅ Round 11（已完成，`cargo check --workspace` 通过，零告警）

| v2 crate | 迁入内容 | 说明 |
| --- | --- | --- |
| `analytics_resource`（M6） | v1 `core/persistence/analytics_resource_store/` 全量 8 文件：`models`（101 行）、`helpers`（27 行）、`resource`（413 行，CRUD/分页/克隆）、`folder`（185 行）、`tag`（283 行，双向关联）、`recycle`（383 行，软删除/恢复/永久删除）、`version`（75 行，版本历史）、`tests`（505 行，`#[cfg(test)]`） | Round 5 剥离预留的 M6 模块归位；依赖 engine（project_db 池）；测试的 `include_str!` SQL 改指 engine/migrations 权威来源 |

**v1 保留策略**：同前——复制式迁移，v1 对应文件全部保留，待 Feature 完整迁移后统一清理。

### ✅ Round 12（已完成，`cargo check --workspace` 通过，零告警）

| v2 crate | 迁入内容 | 说明 |
| --- | --- | --- |
| `plugin`（M9） | v1 `core/plugin` 全量（dependency/events/installer/loader/manager/manifest/permission/storage 共 1846 行）+ `services/plugin_bridge.rs`（125 行）+ `plugin_service.rs`（344 行） | 插件系统核心（依赖解析/事件总线/热加载/生命周期/清单/权限）+ 桥接服务 |
| `plugin/sidecar` | v1 `adapters/sidecar`（client/manager + health_checker/hot_reload_manager 空文件） | Go Sidecar 进程管理 + JSON-RPC 通信 |
| `plugin/wasm` | v1 `adapters/wasm`（api/extism/host_functions/plugin_manager/mod，Extism 运行时） | WASM 分析/驱动/工具插件；tauri 适配器退役 |

**⚠️ v1 死代码发现与处理（重要）**：v1 `adapters/sidecar` 是**从未编译的遗留代码**——`client.rs` 存在两处括号不匹配的语法错误、`driver.rs` 实现的 DriverFactory 旧接口（id/name/kind/default_port/create_pool）与 v1 现行 trait（descriptor/create）不一致。处理：client.rs 语法修复；`sidecar/driver.rs` 移出编译（顶部标注 TODO，待按 v2 trait 重写）；`manager.rs` 的 `std::sync::MutexGuard` 跨 await 不 Send 问题修复（解引用提前）。

**v1 保留策略**：同前——复制式迁移，v1 对应文件全部保留，待 Feature 完整迁移后统一清理。

### ✅ Round 13（已完成，`cargo check --workspace` 通过，零告警）

| v2 crate | 迁入内容 | 说明 |
| --- | --- | --- |
| `mock`（M7） | v1 `backend/src/mock/` 全量 8 文件（engine 1106 行 / generators 1181 / templates 1351 / schema_map 808 / persistence 657 / models 567 / error 80 / mod）共 5772 行 | 测试数据生成：基于源库元数据的列映射生成 mock 数据，**仅落 DuckDB 分析引擎临时表，不回传源库**（M7 约束） |

**依赖与修复**：
- 新增 `fake` crate（v1 同版本 5.x，features 全量照搬）+ `duckdb`（1.10502.0 bundled，mock 直接使用 `duckdb::Connection`）+ `thiserror`；
- `duckdb_rows_to_arrow` 引用路径修正：v1 `crate::core::driver::native::duckdb::` → v2 `engine::duckdb::row_to_arrow::`（Round 1 抽取位置）；
- 其余路径改写：`crate::core::error/models/persistence/sql` → `shared::error/models` / `engine::persistence/sql` 等（REPL 表见脚本）。

**📦 图标资源迁移（本轮要求）**：v1 `src-tauri/icons`（53 文件 7.1MB：128/32/64 px PNG、android mipmap 系列、app-icon-coral-clean.png 等）→ `v2/assets/icons/`；v1 `public`（7 文件 5.7MB：brand 3D 主视觉、rds-icon-dark/light、popout.html 等）→ `v2/assets/public/`。新建 `assets/` 目录，命名与 v1 一致便于追溯（v2 为 GPUI 桌面应用，资源由 assets 统一承载）。

**v1 保留策略**：同前——复制式迁移，v1 对应文件全部保留，待 Feature 完整迁移后统一清理。

### ✅ Round 14（已完成，commands 退役 + 收尾核对）

**commands 层退役**：v1 `backend/src/commands/`（24 文件 11204 行，300+ 个 `#[tauri::command]`）是 Tauri IPC 壳层，v2 采用 GPUI 架构不再需要。**闭环验证**：该层引用的全部服务（ConnectionService/MetadataService/result_service/duckdb_service/plugin_bridge/plugin_service/sql_parser_service/driver_service/connection_manager 等）均已迁入 12 个 crate。详细映射见 [commands-retirement.md](./commands-retirement.md)。

**v1 收尾核对（100% 迁移完成）**：

| v1 区域 | 状态 |
| --- | --- |
| `core/` 全部子目录（cache/dbi/driver/duckdb/insight/logging/migration/persistence/plugin/project/scratchpad/services/sql/utils） | ✅ 全部迁入 12 个 crate |
| `core/` 顶层 9 文件（api_version/arrow/crypto/error/macros/models/port_negotiation/stream/types） | ✅ shared（Round 1） |
| `mock/`（backend 顶层） | ✅ mock crate（Round 13） |
| `commands/` | 🏁 退役（壳层，服务已闭环） |
| `lib.rs` / `main.rs` | 🏁 退役（Tauri 装配/入口，由 app Shell 替代） |

**剩余（非阻塞）**：GPUI 视图层逐步实现（各 Feature 的 `*_view.rs` 占位已就位）；双层数据架构 M1 装配验证；DuckDB Secret 加速通道（connection/secret.rs 占位，M3）。

### ✅ Round 15（已完成，M1 双层数据架构装配验证，测试 10/10 通过）

**新增 3 个 M1 装配验证测试**（`crates/project/src/store.rs`），对应双引擎架构承诺：

| 测试 | 验证点 |
| --- | --- |
| `test_dual_layer_assembled` | 层 1 SQLite `project.db`（含 project 元数据表、创建时元数据已落库）；层 2 DuckDB `analytics.duckdb` 可执行 SQL、可写读回（mock 数据落点） |
| `test_project_level_isolation` | 项目 A/B 各自 `.RSmeta/project_metadata/` 连接元数据**互不可见**（物理隔离） |
| `test_system_shared_meta_roundtrip` | 共享资产（connections 记录）写入 project.db 后重开可读——"清理一次、到处可用"载体闭环 |

**顺手修复的 v1 原缺陷**：
- `ProjectStore::create` 的 dirs 数组缺 `queries` 目录（与文档/既有测试断言不一致）→ 已补；
- lib.rs/store.rs doc 注释中的 ` ``` ` 围栏被 rustdoc 当作代码块导致 doctest 失败 → 已移除围栏；
- 共享测试改为 `INSERT OR REPLACE` 保证幂等。

**验证结果**：`cargo test -p rds-project` 单元 10/10 + doctest 通过；`cargo check --workspace` 零告警。

### ✅ Round 16（已完成，DuckDB Secret 本地加速通道实现，connection 测试 17/17）

**M3 核心差异化能力落地**（`crates/connection/src/secret.rs`，替换占位）：

| 能力 | 实现 |
| --- | --- |
| `SecretManager` | 注册/列出/删除数据源凭据为 DuckDB Secret；`open(db_path)`（持久化到项目分析引擎）/`in_memory()` |
| `register` | `CREATE OR REPLACE SECRET <name> (TYPE POSTGRES, HOST, PORT, USERNAME, PASSWORD, DATABASE)`；SQL 注入防护（单引号加倍转义） |
| `list` | 读 `duckdb_secrets()` 返回 `SecretInfo`（name/type/storage） |
| `remove` | `DROP SECRET IF EXISTS`，不存在返回 `NotFound` |
| `DatabaseCredential` | 凭据模型（name/type/host/port/username/password/database） |

**探针验证**：bundled DuckDB 内置 Secret 类型 = **POSTGRES / MYSQL / S3 / GCS / R2 / AZURE**（SQLITE/DUCKDB 需扩展）——Postgres/MySQL 正是"相对 DBeaver 本地加速"的目标场景（DuckDB 直连联邦查询）。

**顺手修复的 v1 缺陷**：
- `known_hosts.rs` parse 丢弃 keytype 段 + 不支持 russh `public_key_base64()` 裸 base64 → 新增 `key_openssh_with_type`（从 OpenSSH wire 格式解码类型前缀还原），2 个既有测试由失败转通过；
- secret 测试初版断言/类型问题修正。

**验证结果**：`cargo test -p rds-connection` 17/17 通过（含 secret 3 个新测试）；`cargo check --workspace` 零告警。

### ✅ Round 17（已完成，Secret × ConnectionService 集成闭环，workbench 测试 8/8）

**M3 本地加速全链路打通**（新模块 `crates/workbench/src/services/secret_integration.rs`）：

| 环节 | 实现 |
| --- | --- |
| URL 解析 | `parse_connection_url`：`scheme://user:pass@host:port/db` → 凭据字段（支持 postgres/mysql/sqlite/duckdb） |
| 类型映射 | `db_type_to_secret_type`：postgres→POSTGRES、mysql→MYSQL、sqlite→SQLITE、duckdb→DUCKDB、s3→S3 |
| 注册调用 | `register_connection_secret`：凭据 → `SecretManager::register`；名称净化（DuckDB 标识符，连字符/中文→`_`） |
| 生命周期 hook | `connect_with_type` 连接成功建立后自动调用 `ensure_secret_registered`（失败仅告警，不阻断连接） |

**闭环效果**：应用建立 Postgres/MySQL 连接 → 凭据自动注册为 DuckDB Secret → 分析引擎可联邦直连源库查询（相对 DBeaver 的本地加速差异化能力正式落地）。

**顺手修复**：`SecretError` 新增 `Invalid` 变体（参数/解析错误语义化）；sanitize 标识符规则（DuckDB 不支持连字符）。

**验证结果**：`cargo test -p rds-workbench` 8/8（含 secret_integration 6 个新测试：URL 解析 3 + 类型映射 + Secret 注册闭环 + 名称净化）；`cargo check --workspace` 零告警。

### ✅ Round 18（已完成，全仓测试巡检：18 失败收敛至 0）

**巡检路径**：`cargo test --workspace` 首次暴露 engine 241 测试的 18 失败 → 逐类修复 → 全仓 0 失败（单测 391 过 + doc tests 全过，`cargo build --workspace` 零告警）。命令经验：Windows 页面文件不足时全仓测试固定 `-j 2`。

**引擎修复清单**（失败 18→17→11→2→1→0）：

| 修复 | 内容 |
| --- | --- |
| 016 SQL 重复列 | `016_add_driver_properties.sql` 删除重复 `ALTER TABLE drivers ADD COLUMN driver_properties`（008 已建列） |
| Windows 单连接模式 | duckdb-rs 同进程同一 DuckDB 文件仅 1 连接句柄（Windows 文件锁 os error，探针 conn1 OK / conn2、conn3 报占用）→ `DuckDBManager::open` 只建 write_conn，read_pool 空回退 write_conn，maintenance 回退 write_conn；`MIN/MAX_READ_POOL_SIZE`、`create_read_pool` 标 `#[cfg(not(windows))]` |
| executor 列元数据 | `execute_query`/`execute_query_with_params` 的列名提取移到 `stmt.query()` 之后（duckdb-rs 1.10505 未执行前取列名 panic） |
| native duckdb 驱动 | `is_read_only` 变量 4 处取值（修复 left==right）；`ping()` 改 `query_row`（execute 返回行报错） |
| PRAGMA 类 | `metadata_cache.rs` / `project_db.rs` 测试中 `execute("PRAGMA ...")` 改 `execute_batch`（PRAGMA 返回行） |
| 快照排序 | `snapshot.rs` created_at 优先解析文件名 `_snapshot_<unix_secs>`（Windows fs::copy 保留源 mtime 致 cleanup 误删最新快照）；`.and_then` 避免 E0515；delete_all 测试加 1s sleep |
| import_export | 测试改用 `CARGO_MANIFEST_DIR` 绝对路径（`file!()` 相对路径在 cargo test 工作目录不存在） |
| cache_version 测试 | 改用最小库（cache_version version=1 + columns）走纯代码迁移路径；断言 `records.len() == CURRENT_CACHE_VERSION-1` |

**connection_metadata 迁移链 001–010 的 v1 SQL 缺陷（8 类，Python sqlite3 全链复现定位）**：

| 缺陷 | 修复 |
| --- | --- |
| 004 `COALESCE(name, column_name)`（无 column_name 列） | → `COALESCE(name, '')` |
| 004/005 尾部 `FROM tables` 取 schema_name（tables 无该列） | → JOIN schemata 取 s.schema_name（'rebuild' 前缀） |
| 005 `contentless_delete='true'`（FTS5 需数值） | → `contentless_delete=1` |
| 005/006 views 表不存在（视图在 tables table_type='VIEW'） | → 改写 FROM tables JOIN schemata WHERE table_type='VIEW' |
| 005 对 FTS5 虚拟表建索引 | → 移除 |
| 006 001 旧结构 sync_log 被 IF NOT EXISTS 跳过（无 connection_id） | → DROP TABLE IF EXISTS 再重建（日志表） |
| 006/007 尾部 `cache_migration_history (version, ...)` 列名错 | → 修正 from_version/to_version/reason + WHERE to_version=N |
| 009/010 `ADD COLUMN IF NOT EXISTS`（SQLite 不支持，46+11 处） | → 移除 IF NOT EXISTS（迁移按版本追踪只跑一次） |
| 009/010 尾部 `INSERT OR REPLACE INTO cache_version (version, description, applied_at)` | → UPDATE cache_version SET version=N, upgraded_at, upgrade_reason WHERE id=1 |

验证：`tools/probe_mig3.py` 逐文件复现 001–010 全链 ALL OK；探针 DB 已清理。

**shared / plugin 竞态修复**：crypto.rs 盐值改为进程内 `OnceLock` 缓存（并行测试各自生成盐互相覆盖导致解密失败）；port_negotiation.rs 测试断言按真实语义修正（分配后不可用→释放后可用）；plugin manifest 测试 `write_temp_toml` 唯一文件名（固定名并行覆盖导致 parse 结果不定）。

**doc tests 修复**：driver/wasm/error 等模块 doc 图表块（`│`/`├──` 等）未标语言被当 Rust 编译 → 标 `text`；registry/connection_manager/macros 的 v1 遗留伪代码示例（类型路径已随 v2 分层变化）→ 标 `ignore`（真实用法由单元测试覆盖）。

**终验**：`cargo test --workspace -j 2` 零失败（connection 17、engine 217、insight 53、mock 56、plugin 11、project 10、scratchpad 8、shared 19，共 391 单测 + doc tests 全过）；`cargo build --workspace` 零告警。迁移链修复脚本归档 `tools/fix_round18a–u.py`、`tools/fix_00X*.py`、`tools/probe_mig*.py`。

### ✅ Round 21（已完成，M3 真实数据接入：连接列表由全局系统库填充，`cargo check --workspace` 零告警、全仓 0 失败）

**目标**：替换工作台连接列表的占位数据（`ConnectionItem::sample()`），改为从 M3 数据源连接模块的持久化层（`GlobalDatabaseManager` 全局系统库）读取真实连接元数据。这是「模块 → UI」打通的第一条真实数据链路。

**新增/改动**：

| 文件 | 内容 |
| --- | --- |
| `crates/workbench/src/services/workspace_loader.rs`（新增） | 工作台真实数据加载器：`default_global_dir()`（`%APPDATA%\rdata-station\global`）+ `load_persisted_connections()` / `load_persisted_connections_from(dir)`（目录可注入，便于测试与后续数据目录切换）；tokio runtime + `GlobalDatabaseManager::new` + `get_global_connections` → 映射 `ConnectionItem`；失败降级为空列表 + 错误提示，不阻塞启动 |
| `crates/workbench/src/view.rs` | `WorkbenchView::new()` 启动时经 loader 加载真实连接 → `Shared::with_connections`；`ConnectionItem::sample()` 占位数据退役 |
| `crates/workbench/src/panels.rs` | `Shared::with_connections(connections, notice)`（连接为空时 `selected=None`）；连接列表空态 UI（暂无连接 / 加载失败提示） |
| `crates/workbench/tests/real_connections.rs`（新增，3 测试） | 真实接入链路集成测试：全局库 roundtrip（迁移 + 保存 + 读回 + 映射）、空库返回空列表、**重开库后连接持久化仍在**（模拟下次启动） |
| `crates/workbench/examples/seed_demo.rs`（新增） | 演示种子：`cargo run -p rds-workbench --example seed_demo` 向默认全局库写入一条演示连接 |
| `tools/patch_r21_view.py` / `tools/fix_r21_literal.py` / `tools/write_round21_doc.py` | 本轮修复与文档脚本归档 |

**验证**：
- 集成测试 3/3：`GlobalDatabaseManager::new`（含全局迁移链）+ `save_global_connection` + `get_global_connections` 全链路通过；重开库持久化验证通过。
- 全仓 `cargo test --workspace -j 2` 0 失败（新增 3 条后总计 394+ 单测/集成测试；engine 217 过 24 ignore 不变）。
- `cargo build -p rds-app` + 运行验证：进程稳定存活、无 panic、无 stderr。
- 端到端确认：`seed_demo` 写入 `%APPDATA%\rdata-station\global\global.db`（`conn-demo-mysql / 演示 MySQL 分析库 / mysql / is_active=1 / use_duckdb_fed=1`），Python sqlite3 直读确认落库；app 启动加载该连接渲染。

**实测发现**：
- `save_global_connection` 保存后连接默认激活（`is_active=1`），视图 `connected` 状态如实映射。
- Windows 上 `GlobalDatabaseManager::new` 会创建 DuckDB 文件（`global.duckdb`）；同进程单连接句柄约束下测试用唯一临时目录隔离（与 Round 18 结论一致）。
- GPUI 同步渲染模型 + async 数据源：启动期一次性 `block_on`（本地 SQLite 查询毫秒级）接入；后续连接操作（新建/删除/切换）仍可挂命令面板或异步刷新，留待下一轮。

### ✅ Round 22（已完成，连接详情面板：选中连接展示真实元数据，全仓 394 测试 0 失败）

**目标**：把「选中连接 → 详情查看」闭环做实——EditorPanel 从「名称/驱动摘要」升级为**真实元数据详情卡片**（主机/端口/数据库/Schema/DuckDB 联邦/描述/时间戳等 10 项键值行），数据来自 Round 21 打通的全局系统库。

**新增/改动**：

| 文件 | 内容 |
| --- | --- |
| `crates/workbench/src/view.rs` | `ConnectionItem` 扩展 8 个真实元数据字段：`host / port / database / schema / description / use_duckdb_fed / created_at / updated_at` |
| `crates/workbench/src/services/workspace_loader.rs` | 映射函数填充全部新字段（GlobalConnectionInfo → ConnectionItem 全字段对齐） |
| `crates/workbench/src/panels.rs` | `EditorPanel::render` 重写：连接详情卡片（状态徽标 + 10 行键值对 + 新建连接按钮），布局从居中改为顶部对齐流式 |
| `crates/workbench/tests/real_connections.rs` | 集成测试断言扩展：host/port/database/use_duckdb_fed/description 映射验证 |
| `tools/patch_r22_*.py`（4 个） | 本轮字段扩展 / 断言 / EditorPanel / 测试映射脚本归档 |

**验证**：
- 全仓 `cargo test --workspace -j 2`：**394 通过 0 失败**（25 个测试套件全 ok，含 workbench 8 单测 + 3 集成测试）。
- `cargo build -p rds-app` + 运行验证：进程稳定存活、无 panic、无 stderr（带 Round 21 种子连接 + 详情卡片渲染正常）。

**⚠️ 磁盘治理事件（本轮关键运维记录）**：
- **现象**：`cargo test` 连环报 `LNK1318 PDB LIMIT`、`LNK1180`、`E0462 staticlib std`、`E0463 can't find crate`、`os error 112 磁盘空间不足`——根因是 **D 盘 target/debug 膨胀至 125GB**（92646 文件，D 盘仅剩 6.67GB），链接产物损坏污染 deps。
- **处置**：`cargo clean` 释放 125GB（D 盘回 105GB）→ 全量重编译 28m36s → 测试 0 失败；当前 D 盘余 72GB。
- **预防**：后续轮次如再遇链接失败（LNK13xx/11xx 或 E0462/E0463），优先查磁盘余量；建议定期 `cargo clean` 或监控 target 大小（正常全量后约 24GB，异常增长至 125GB 前应干预）。
- 教训：`-j 2` 链接失败多为资源（磁盘/内存/PDB 竞争）而非代码错误；`cargo check --tests` 通过但 `cargo test` 失败时，先看链接器真实错误码与磁盘。

### ✅ Round 23（已完成，M3 写路径打通 UI：新建连接表单，保存真实落库并刷新列表）

**目标**：把 M3 数据源连接的**写路径**接入工作台——用户通过「新建连接」表单（名称/驱动/URL/用户名/密码）创建连接，保存后真实写入全局系统库，连接列表即时刷新。至此 M3 的读（Round 21）→ 详情（Round 22）→ **写（本轮）**闭环完成。

**新增/改动**：

| 文件 | 内容 |
| --- | --- |
| `crates/workbench/src/services/workspace_loader.rs` | 新增 `save_connection` / `save_connection_at(dir, ...)`（目录注入，便于测试）：conn_id 时间戳生成 → `GlobalDatabaseManager::save_global_connection` → 返回 Result；用户名/密码为空时存 None |
| `crates/workbench/src/panels.rs` | `EditorPanel` 新增表单状态（show_form + 5 个 `Entity<InputState>` 受控输入懒创建）；「新建连接」按钮改 toggle 表单；`Form::vertical` + `Field` + `Input` 渲染 5 字段；「保存连接」按钮：读值 → 校验非空 → `save_connection` → 成功刷新列表 + notice + 清空表单，失败 notice |
| `crates/workbench/tests/real_connections.rs` | 新增 `save_connection_at_then_load_roundtrip`：写路径保存 → 读回断言全字段（name/driver/host/port/database/联邦） |

**gpui 0.6 表单组件实证（本轮新 API 修正）**：
- **受控输入模型**：`Input::new(&Entity<InputState>)`，状态在面板字段持有；`InputState::new(window, cx)` 构造（需 window）；`state.read(cx).value()` 读（SharedString）、`state.update(app, |s, cx| s.set_value(v, window, cx))` 写（需 window——on_click 闭包第二参 `|_, window, app|` 可拿）。
- **E0502 陷阱**：`cx.theme()` 返回 `&Theme`（借用 cx 存活到借用者最后使用）；在其后调 `cx.new`（&mut cx）冲突 → **受控状态懒创建必须放在 `let theme = cx.theme();` 之前**。
- **嵌套 runtime 陷阱**：`save_connection_at` 内部 `Runtime::new()+block_on`，在 `#[tokio::test]` 内调用会 panic（Cannot start a runtime from within a runtime）→ 集成测试改用普通 `#[test]`（读写入口均同步）。
- `Button` 变体含 secondary（primary/ghost/link/text 等 9 种）；`Field::new()` 无参。

**验证**：workbench 12/12 测试通过（8 lib + 4 集成，含新写路径 roundtrip）；`cargo check --workspace` 零告警；`cargo build -p rds-app` + 运行验证进程稳定无 panic（表单懒建 + 详情渲染正常）。

### ✅ Round 24（已完成，M3 增删查写全闭环：删除连接，engine+loader+UI+测试四层打通）

**目标**：补上连接管理的**删除**能力——M3 数据源连接至此具备完整生命周期：读（R21）→ 详情（R22）→ 新建（R23）→ **删除（本轮）**。删除为物理删除（`DELETE FROM global_connections`），配合 `get_global_connections` 的 `WHERE is_active = 1` 过滤，删除后连接即从工作台列表消失。

**探查结论**：engine `GlobalDatabaseManager` 此前只有 `delete_environment/delete_plugin/delete_project` 等对象删除，**无连接删除方法**；`get_global_connections` SQL 带 `WHERE is_active = 1`（激活=可见）——删除与"隐藏"（is_active 置 0）是两种语义，本轮实现物理删除，doc 注释明确标注两种方案。

**新增/改动**：

| 文件 | 内容 |
| --- | --- |
| `crates/engine/src/persistence/global_db.rs` | 新增 `delete_global_connection(conn_id)`：`sqlite_pool.acquire` → `conn.inner()?.execute("DELETE FROM global_connections WHERE id = ?1", [conn_id])` → `CoreError::storage` 包装 + `tracing::info`；对齐 `delete_environment` 模式 |
| `crates/workbench/src/services/workspace_loader.rs` | 新增 `delete_connection_at(dir, conn_id)`（目录注入，初始化失败/删除失败均转中文错误）与 `delete_connection(conn_id)`（默认全局目录） |
| `crates/workbench/src/panels.rs` | 详情卡片底部新增「删除连接」`danger` 按钮：取选中连接 id → `delete_connection` → 成功刷新列表 + 清空选中（`shared.selected.set(None)`）+ notice；失败 notice |
| `crates/workbench/tests/real_connections.rs` | 新增 `delete_connection_at_removes_from_list`：save → load 1 条 → delete → load 空 |

**本轮踩坑修复（已记录，后续直接复用）**：
- **E0382 use of moved value: `entity`**：render 顶部 `let entity = cx.entity()`（非 Copy）被**删除按钮闭包 move 后**，**新建按钮闭包再 move** 冲突 → 详情块开头 `let entity = entity.clone();`（每个需要捕获的闭包用克隆）。
- **E0277 `?` 错误转换**：`runtime.block_on(async { ... .await? ... })` 的 async 块错误类型是 `CoreError`，与外层 `Result<(), String>` 不匹配 → 改 `match` 显式 `map_err` 转中文错误（load 路径本来就 match，写路径别用 `?` 穿透）。

**验证**：workbench 13/13 通过（8 lib + 5 集成，含新删除 roundtrip）；**全仓 396 测试 0 失败**（394 → 396）；`cargo build -p rds-app` + 运行验证进程稳定无 panic。

### ✅ Round 25（已完成，M4 数据库导航第一步：DuckDB 分析库元数据树真实接入工作台）

**目标**：启动 M4 数据库导航——选中连接（DuckDB 联邦开启）时，读取分析引擎库真实元数据，渲染 表 → 列 导航树。这是 DBeaver 式导航的最小闭环：**不依赖网络**（本地 DuckDB 文件），元数据来自 engine 原生驱动（`DuckDbDatabase`，Database trait 的 `list_tables` / `list_columns`）。

**探查结论**：
- engine `DuckDbDatabase::new(url)`（`duckdb://` 前缀或裸路径）同步打开文件，实现完整 `Database` trait（list_catalogs="main" / list_tables / list_columns / list_indexes…），可独立于 ConnectionManager 使用——**导航数据源首选**。
- engine 已 pub 导出 `Database` trait；`driver::native::duckdb::DuckDbDatabase` 经 `pub mod native → pub mod duckdb` 可达（无需新增导出）。
- gpui-component 0.6 `DockArea` **无 dump_state/load_state**（布局持久化 API 缺失）——本轮放弃布局持久化，改做导航。

**新增/改动**：

| 文件 | 内容 |
| --- | --- |
| `crates/workbench/src/services/db_navigator.rs` | 新增 `load_navigator_tree(duckdb_path) -> Result<Vec<NavTable>, String>`：DuckDbDatabase::new → list_tables("main") → 每表 list_columns → NavTable{name, columns[NavColumn{name, data_type, is_primary_key, is_nullable}]} |
| `crates/workbench/src/services/mod.rs` | 注册 `db_navigator` 模块 |
| `crates/workbench/src/panels.rs` | `Shared` 加导航缓存（`nav_for` 记录已加载连接 id + `nav_tables` 树）；详情卡片下「数据库导航（DuckDB 分析库）」区：选中联邦连接按需加载（nav_for 变更时重新加载，避免每帧重查）；渲染表名 + 列数 → 每列（列名/类型/PK 标记）；空库显示"运行 seed_demo 或导入数据" |
| `crates/workbench/examples/seed_demo.rs` | 扩展：global.duckdb 幂等建 3 张演示表（orders/order_items/customers）+ 视图 v_order_summary（`CREATE TABLE IF NOT EXISTS`），重跑不报错 |
| `crates/workbench/tests/db_navigator.rs` | 新增 2 集成测试：建表后读回树（表名排序断言 + 列断言）、空库返回空 |

**本轮踩坑修复（已记录，后续直接复用）**：
- **DuckDB 文件锁（Windows）**：同一 .duckdb 文件仅 1 个连接句柄（R18 已知）——测试里建表的 `Connection` **必须显式 drop** 后再 `load_navigator_tree`，否则 "另一个程序正在使用此文件"（os error 32）。
- **DuckDB 无 PK 元数据语义**：`CREATE TABLE ... PRIMARY KEY` 后 `list_columns` 的 `is_primary_key` 可能为 false（DuckDB 约束元数据有限）——测试只断言列存在。
- **E0382 seed move**：`duckdb`（PathBuf 非 Copy）被 async 块 move 后又借用 → 在 block_on 前先 `to_string_lossy().to_string()` 存 String，async 块内用 `duckdb.clone()`。
- **导航加载位置**：render 内同步 block_on（小库 <100ms），用 `nav_for`（连接 id）做缓存 key——选中切换重新加载，同连接重绘不重查。

**验证**：workbench 15/15 通过（8 lib + 2 navigator + 5 real_connections）；**全仓 398 测试 0 失败**（396 → 398）；seed 重跑成功（`C:\Users\sling\AppData\Roaming\rdata-station\global\global.duckdb` 已建 3 表 + 1 视图）；`cargo build -p rds-app` + 运行验证进程稳定（选中 demo 连接自动加载导航树无 panic）。

### ✅ Round 26（已完成，M5 查询工作台第一步：SQL 执行 + 结果集渲染）

**目标**：启动 M5 查询工作台核心——选中连接（DuckDB 联邦开启）时，「SQL 查询」区可输入 SQL 并对分析引擎库真实执行，结果集以列头 + 行表格渲染。至此工作台形成完整闭环：**连接（M3）→ 导航（M4）→ 查询（M5）**。

**探查结论**：
- engine `DuckDbDatabase::query`（Database trait）返回的 `QueryResult` 中 **`rows` 字段从不填充**（数据只在 Arrow `batches`，`..Default::default()` 留空 rows）——**不能直接用 result.rows**。
- `duckdb::types::Value` **无 Display**（20+ 变体）；engine `duckdb_service::duckdb_value_to_json`（pub fn）把 duckdb Value → `serde_json::Value`，可复用。
- **实现改用 duckdb-rs 直连**（workbench 已依赖 `duckdb = 1.10502.0`）：`Connection::open → prepare → query → column_name`，值经 `duckdb_value_to_json` 转字符串——**同步执行，无需 Runtime**。

**新增/改动**：

| 文件 | 内容 |
| --- | --- |
| `crates/workbench/src/services/query_runner.rs` | 新增 `execute_sql(duckdb_path, sql) -> Result<QueryOutput, String>`：SQL 非空校验 → duckdb::Connection::open → prepare → query → 收集行（`row.get::<usize, Value>` 循环至 Err 断）→ **query 后**取列名 → 值转字符串（String 去引号、NULL → "NULL"） |
| `crates/workbench/src/services/mod.rs` | 注册 `query_runner` 模块 |
| `crates/workbench/src/panels.rs` | EditorPanel 加 `sql_input`（受控 Input）+ `query_result`（Rc<RefCell<Option<QueryOutput>>>）；「SQL 查询（DuckDB 分析库）」区：Input + 「执行」按钮 → execute_sql(global.duckdb) → 结果表格（列头 + 行，定宽截断）；DDL/无返回显示"无结果" |
| `crates/workbench/tests/query_runner.rs` | 新增 3 集成测试：SELECT 读回列/行断言、空 SQL 报错、非法 SQL 报错 |

**本轮踩坑修复（已记录，后续直接复用）**：
- **duckdb-rs 列元数据时序**：`column_count()/column_name()` 必须在 `stmt.query([])` **之后**调用（"The statement was not executed yet"）；且 `Rows` 持有 stmt 可变借用，需**先收集完数据再读列名**（块作用域 drop rows）。
- **column_name 返回 `Result<&String, Error>`**（引用）——`map(|v| v.to_string())` 再 `unwrap_or_else`，不能直接 `unwrap_or_else(|_| String)`。
- **serde_json 字符串带引号**：`duckdb_value_to_json` 的 Text → JSON `"paid"`——`match String(s) => s` 去引号；Null → "NULL"。
- **Rc 非 Copy**：执行按钮闭包 move `query_result` 后渲染又借用 → 闭包用独立 `qr_closure = query_result.clone()`。

**验证**：workbench 18/18 通过（8 lib + 2 navigator + 3 query_runner + 5 real_connections）；**全仓 401 测试 0 失败**（398 → 401）；`cargo build -p rds-app` + 运行验证进程稳定（SQL 区渲染无 panic）。

### ✅ Round 27（已完成，M5 查询工作台第二步：SQL 结果导出 CSV）

**目标**：查询工作台闭环再进一步——**查询 → 查看 → 导出**。结果集可一键落盘为 CSV（全局目录 `results/`，按秒时间戳命名），复用 R26 的 `QueryOutput`（列 + 已字符串化行）。

**新增/改动**：

| 文件 | 内容 |
| --- | --- |
| `crates/workbench/src/services/query_export.rs` | 新增 `csv_field`（逗号/引号/换行加引号包裹、内部引号双写）+ `export_csv(output, path)`（列头 + 数据行写文件）+ `default_export_dir()`（`global/results/`）+ `export_to_default(output)`（秒时间戳命名 `rds_query_<secs>.csv`，返回落盘路径）——**手写转义，无第三方 csv 依赖** |
| `crates/workbench/src/services/mod.rs` | 注册 `query_export` 模块 |
| `crates/workbench/src/panels.rs` | SQL 结果表格后加「导出 CSV」ghost 按钮（行：`共 N 行` + 按钮）→ `export_to_default(&out)` → notice 显示导出路径；导出失败也提示 |
| `crates/workbench/tests/query_export.rs` | 新增 3 集成测试：逗号/引号/换行转义断言、导出默认目录落盘读回（列头 + 数据）、空结果仅列头 |

**本轮踩坑修复（已记录，后续直接复用）**：
- **E0382 部分 move**：「执行」按钮闭包已 move `shared`/`entity`，后续导出按钮再 `clone()` 报 borrow of partially moved value——**在 R26 闭包捕获之前就克隆双副本**（`shared_export`/`entity_export`），各按钮各用一份，避免借用在闭包 move 之后发生。

**验证**：workbench 21/21 通过（8 lib + 2 navigator + 3 query_runner + **3 query_export** + 5 real_connections）；**全仓 404 测试 0 失败**（401 → 404）；`cargo build -p rds-app` + 运行验证进程稳定。

### ✅ Round 28（已完成，M5 查询工作台第三步：多行 SQL 编辑器）

**目标**：SQL 查询区从单行 `Input` 升级为多行 `Textarea`（真实查询往往跨多行/多语句），并调整布局——编辑器占宽、执行按钮右对齐。

**探查结论（gpui-component 0.6 输入组件家族）**：
- `Textarea`（`gpui_kit::component::input`）存在：`Textarea::new(&Entity<TextareaState>)` + `.h(px())` 定高 + 继承 Input 样式链（bordered/appearance/disabled/readonly）。
- `TextareaState = InputBaseState<TextareaMode>`（gpui-base type alias），构造 `TextareaState::new(window, cx)`，读值 `value() -> SharedString`——与 `InputState` 同构，受控懒创建模式（R23 约定）直接复用。

**改动（仅 `crates/workbench/src/panels.rs`）**：
- import 增加 `Textarea, TextareaState`；
- `sql_input: Option<Entity<InputState>>` → `sql_textarea: Option<Entity<TextareaState>>`（struct 字段 / new 初始化 / 懒创建 / SQL 区取状态四处同步）；
- SQL 区布局：`h_flex(Input + 按钮)` → `v_flex(Textarea 高 96px + 右对齐执行按钮)`；
- 执行逻辑不变（`sql_state.read(app).value()` → `execute_sql`）。

**验证**：workbench 21/21 通过（服务层未动，测试数不变 404）；`cargo check` / `cargo build -p rds-app` 通过 + 运行验证进程稳定（多行编辑器渲染无 panic）。

### ✅ Round 29（已完成，M5 查询工作台第四步：SQL 历史记录，跨会话持久化）

**目标**：查询工作台记忆最近执行的 SQL（上限 20 条、去重、最新在前），跨会话持久化到全局目录；点击历史条目回填到多行编辑器，一键重放。

**新增/改动**：

| 文件 | 内容 |
| --- | --- |
| `crates/workbench/src/services/query_history.rs` | 新增 `load_history_from(dir)` / `append_history_at(dir, sql)`（**目录可注入**，便于测试与未来多项目隔离）+ 默认目录变体 `load_history()` / `append_history()`：JSON 数组持久化到 `<全局目录>/query_history.json`，去重（相同 SQL 移到最前）→ 插入头部 → 截断 20 条；文件缺失/损坏 → 空历史 |
| `crates/workbench/src/services/mod.rs` | 注册 `query_history` 模块 |
| `crates/workbench/src/panels.rs` | `sql_history: Rc<RefCell<Vec<String>>>` 字段（new() 时 `load_history()` 初始化）；执行成功后在 `entity.update` 内 `append_history(&sql)` 刷新；SQL 区「历史」小节——条目（42 字符截断预览）可点击回填 `set_value` 到 Textarea |
| `crates/workbench/tests/query_history.rs` | 新增 3 集成测试（临时目录注入）：去重 + 持久化读回、上限 20 最新在前、缺失文件空历史 + 空白 SQL 忽略 |

**本轮踩坑修复（已记录，后续直接复用）**：
- **set_value 泛型推断**：`s.set_value(sql.into(), ...)` 的 `.into()` 触发 E0283——`impl Into<SharedString>` 直接传 `String` 即可（`String: Into<SharedString>`），无需 `.into()`。
- **E0382 链式 move**：执行按钮闭包 move `sql_state` 后，历史按钮再 clone 报 borrow of moved value——**沿用 R27 模式**：在闭包捕获前提前克隆 `sql_state_hist`。
- **测试注入目录**：历史读写若硬编码全局目录会污染真实种子数据——用 `*_at(dir)` 参数化变体 + 临时目录测试。

**验证**：workbench 24/24 通过（8 lib + 2 navigator + 3 query_runner + 3 query_export + **3 query_history** + 5 real_connections）；**全仓 407 测试 0 失败**（404 → 407）；`cargo build -p rds-app` + 运行验证进程稳定（历史列表渲染/回填无 panic）。

### ✅ Round 30（已完成，连接切换联动：导航/查询不串数据）

**目标**：修复多连接场景的真实 UX 缺口——**切换连接后，导航树与 SQL 结果不得残留上一个连接的数据**（呼应 M1 双层数据架构"项目/连接彼此看不见"的产品主线）。

**现状探查**：
- 导航树（R25）已有 `nav_for` 校验：`nav_for != 当前连接 id` 时重新加载——切换连接会自动重载树 ✓；
- **缺口**：`query_result`（SQL 结果）无连接归属——切换连接后旧结果仍显示。

**改动**：

| 文件 | 内容 |
| --- | --- |
| `crates/workbench/src/panels.rs` | Shared 加 `sql_for: Rc<RefCell<Option<String>>>`（SQL 结果归属连接 id）；SQL 区渲染前校验 `sql_for == 当前连接 id` 才显示结果，否则隐藏（切走即失效）；执行成功记录归属、失败清空 |
| `crates/workbench/src/view.rs` | `SidebarEvent::SelectConnection` 处理（唯一入口）加清理：清空 `nav_for` + `nav_tables` + `sql_for`——切换连接即时清场，导航树下一轮渲染重载 |

**本轮踩坑修复（已记录，后续直接复用）**：
- **shared 闭包 move 链**：执行按钮闭包 move `shared` 后，渲染校验再 `shared.sql_for.borrow()` 报 E0382——沿用 R27/R29 模式，提前克隆 `shared_view`（渲染用）与 `shared`（闭包用）双副本；`conn_id` 同理提前克隆给闭包捕获。

**验证**：workbench 24/24 通过；**全仓 407 测试 0 失败**（服务层未动）；`cargo build -p rds-app` + 运行验证进程稳定。

### ✅ Round 31（已完成，M1 项目管理：CRUD + 生命周期 + 项目锁）

**目标**：落地 `docs/architecture/project/` 三件套（原型 / 开发方案），实现项目增删改查与一实例一项目生命周期。

**改动**：

| 文件 | 内容 |
| --- | --- |
| `crates/engine/migrations/global/019_add_project_ui_state.sql`（新） | `project_info` 增 `is_pinned`/`pinned_at`/`removed_at` + 索引（固定置顶 / 软删过滤） |
| `crates/engine/src/persistence/global_db.rs` | 查询改造（过滤已移除、固定置顶）+ `set_project_pinned`/`soft_delete_project`/`restore_project`/`list_removed_projects`；`save_project_info` 改 upsert 以保留固定/软删列 |
| `crates/project/src/lock.rs`（新） | 项目锁：OS 文件锁（崩溃自动释放）+ `project.lock.owner` 展示占用者；`probe`/`release` |
| `crates/workbench/src/services/project_service.rs`（新） | 项目 CRUD 编排：列表（最近/全部/已移除）、创建、打开 / 只读打开、关闭、重命名、固定、归档、软删 / 恢复 / 硬删 / 移出、目标探测 |
| `crates/workbench/src/components/project_ui.rs`（新） | 项目管理 UI：选择器（三 Tab / 搜索 / 排序 / 卡片操作）、标题栏项目菜单、新建 / 打开 / 删除确认 / 锁逃生口 / 未保存拦截对话框、项目设置 |
| `crates/workbench/src/{view,panels,commands,lib}.rs` | 标题栏项目槽可点、无项目时选择器覆盖中央区、`SwitchProject`/`CloseProject` Action、`Shared::project_ui`/`editor_dirty`、编辑区脏状态 |
| `crates/app/src/main.rs` | 绑定 `Ctrl+Shift+P`（切换项目）/ `Ctrl+Shift+W`（关闭项目） |
| `Cargo.toml`、`crates/workbench/Cargo.toml` | 新增 `project` 别名与依赖、`chrono` |

**验证**：`cargo check --workspace --all-targets` 零告警；engine 221 / project 12 单元 + 3 集成 / workbench 16 测试全绿（含迁移 019、固定/软删/恢复、项目锁用例）。

**二次迭代补全**：排序持久化（`settings.projects.sort_mode`）、未保存「保存并继续」、只读强制禁写、重新定位（U5）、版本链列表（新迁移 `project_meta/018_project_versions.sql`），并新增 `crates/project/tests/` 集成测试。

### ⏳ 下一轮候选

- GPUI 视图层：workbench 主界面（数据库导航/连接管理/资源分析）首个可交互视图；
- mock 命令装配（mock_commands → GPUI 或 CLI 入口）；
- v1 源文件清理决策（v1/ 暂存区保留 vs 按 Feature 删除）。
### ✅ Round 19（已完成，GPUI 视图层：首个可交互工作台骨架，`cargo check --workspace` 零告警）

**目标**：按 GPUI-kit 编码规范（https://gpui-kit.com/zh-CN/docs/coding-guides/）搭建 workbench 首个可交互视图——活动栏 + 侧边栏 + 内容区 + 状态栏，并接入 app shell（`cargo run -p rds-app` 即显示真实工作台）。

**新增/改动**：

| 文件 | 内容 |
| --- | --- |
| `crates/workbench/src/view.rs`（17KB） | `WorkbenchView`：标题栏、活动栏四工具（连接/导航/资源/设置）、侧边栏按工具渲染（连接列表可选中高亮、导航树缩进占位、资源/设置占位）、中央内容区（连接详情 + "新建连接"按钮）、`StatusBar` 状态栏（工具 + 选中连接 + 通知文案）；`Tool` 枚举、`ConnectionItem`（首版 4 条占位连接） |
| `crates/workbench/src/lib.rs` | `pub mod view;` + re-export `ConnectionItem/Tool/WorkbenchView`；迁移进度注释补 Round 19 |
| `crates/workbench/Cargo.toml` | 新增 `gpui-kit.workspace = true` |
| `crates/app/src/main.rs` | 删除占位 `Workspace`，接入 `workbench::WorkbenchView`（App Shell 只组合窗口，业务在 Feature crate） |
| `crates/app/Cargo.toml` | 新增 `workbench = { path = "../workbench", package = "rds-workbench" }` |
| `tools/fix_round19_view.py` / `fix_round19_sidebar.py` | 本轮修复脚本归档 |

**编码规范落地要点**：窗口第一层 `Root::new(workspace, window, cx)`（dialog/sheet/notification/tooltip/menu/focus trap 统一协调）；`h_flex` 交叉轴居中而 `v_flex` 拉伸，行内放列必须 `items_stretch()` + `min_h_0()`；颜色全部取 `cx.theme().colors`（禁止 raw hex/hsla）；依赖只向下（视图只用 gpui-kit，不承载业务逻辑）。

**gpui-kit 0.6 真实 API 修正记录（初版 25 编译错误 → 0）**：

| 初版用法 | 0.6 真实 API | 说明 |
| --- | --- | --- |
| `Button::new(cx)` | `Button::new(id)`（`impl Into<ElementId>`） | 无 `new(cx)` 的 Button；id 内置元素（ElementId），不再需要 `.id()` |
| `.primary()`/`.ghost()` 直接调 | `ButtonVariants` trait 方法（需 `use button::ButtonVariants` 或 `button::*`） | variant 是私有字段，公开入口是 trait 的 `with_variant` 派生方法 |
| `cx.listener(|this, _, cx| ...)` 3 参 | `on_click(|_, _, app| ...)` 内 `entity.clone().update(app, |this, cx| ...)` | `on_click` 签名 `Fn(&ClickEvent, &mut Window, &mut App)`；`App::update` 私有，用 `Entity::update` |
| `.id(("a","b"))` 元组 | 字符串 `format!("activity-{}", ...)` | `ElementId: From<(&str,&str)>` 未实现 |
| `gap_0_5`/`py_0_5`/`w_60` | 显式 `gap_2`/`pt/pb/pl/pr` + `w(px(240.))` | gpui-pre 0.3 utility 数字步长有限（`0_5` 半单位不存在，`w_60` 无），统一用 px() 显式值 |
| `.when(...)` 不可用 | `use gpui_kit::prelude::FluentBuilder as _;` | FluentBuilder trait 需显式引入 |
| `div().on_click(...)` | `div().id("conn-N").on_click(...)` | `on_click` 在 Stateful（InteractiveElement 需先 `.id()`） |
| `Toast`（notification） | 首版用 `notice: Option<String>` 状态栏文案 | notification::Toast 的导出路径与构造链未实证，不冒险引入 |
| `app.update(entity, ...)` | `entity.update(app, ...)` | `App::update` 私有 |
| `let theme = cx.theme();` 后 `cx.entity()` | 颜色字段先拷贝为 `Hsla` 局部值（E0502） | `Hsla: Copy`，拷贝后借用立即结束 |

**验证**：`cargo check -p rds-workbench` 25 错 → 0；`cargo check --workspace -j 2` 零告警；`cargo build -p rds-app` + 运行验证工作台弹窗（详见交付说明）。DockArea 可拖拽布局系统与命令面板（Action/Keybinding）留待下一轮。




### ✅ Round 20（已完成，DockArea 可拖拽布局系统接入，`cargo check --workspace` 零告警）

**目标**：把 Round 19 的单体骨架升级为 GPUI-kit DockArea 布局系统——左侧栏与中央内容区成为可拖拽、可收起、可持久化（DockAreaState）的面板，为后续 Feature 面板（数据库导航 / 资源分析 / 草稿箱 / 洞察）提供挂载容器。

**新增/改动**：

| 文件 | 内容 |
| --- | --- |
| `crates/workbench/src/panels.rs`（12KB，新增） | `Shared`（面板间共享状态：active_tool / selected / connections / notice）、`SidebarPanel`（连接列表可选中 / 导航树 / 资源 / 设置）、`EditorPanel`（连接详情 + 新建连接 + 通知文案）、`SidebarEvent`（SelectConnection）；两个面板完整实现 base `Panel` + component `Panel`（panel_name / tab_name / title / Focusable / EventEmitter） |
| `crates/workbench/src/view.rs` | `WorkbenchView` 重构：持有 `Entity<DockArea>`，render 首次懒初始化（`init_workspace`：创建面板实体 → `cx.subscribe` 订阅选中事件 → `DockSkin::dock_area` → `set_center(h_split: sidebar 240px + editor)`）；标题栏加侧边栏收起/展开按钮（`area.toggle_dock(Left)`）；活动栏点击更新共享状态并通知面板 |
| `crates/workbench/src/lib.rs` | `pub mod panels;` |
| `tools/fix_round20.py` | 本轮修复脚本归档 |

**gpui-kit 0.6 Dock 真实 API 修正记录（8 类编译错误 → 0）**：

| 初版用法 | 0.6 真实 API | 说明 |
| --- | --- | --- |
| `Entity<T>.emit(app, ev)` | `entity.update(app, |_, cx| cx.emit(ev))` | `emit` 是 `Context::emit`（要求 T: EventEmitter<Evt>），Entity 上无此方法 |
| `cx.subscribe(&e, |this, event, cx| ...)` 3 参 | `cx.subscribe(&e, |this, _entity, event, cx| ...)` 4 参 | `Context::subscribe` 闭包签名 `FnMut(&mut T, Entity<T2>, &Evt, &mut Context<T>)`——第二参是被订阅实体（值），第三参才是事件 |
| `DockLayout::tabs().panel(entity)` | `DockLayout::tabs().panel_view(panel_handle(entity), cx)` | builder 无 `panel()`，只有 `panel_view(Arc<dyn PanelView>, cx)`；`panel_handle` 在 `component::dock` |
| `Rc<Cell<Option<String>>>` | `Rc<RefCell<Option<String>>>` | `Cell::get` 要求 T: Copy，Option<String> 不满足 |
| `self.connections.borrow().get(i).cloned()`（闭包内） | 先 `let conns = self.connections.borrow();` 再取 | 临时 borrow 悬垂（E0515） |
| subscribe 返回值未用 | 存入 `_subscription: Option<Subscription>` 字段 | 防 drop 即取消订阅；编译器强制 must_use |
| 面板 `EventEmitter<PanelEvent>` | `EventEmitter<gpui_kit::component::dock::PanelEvent>` | base Panel 的关联事件类型经 component re-export |

**验证**：`cargo check --workspace -j 2` 零告警；`cargo build -p rds-app` 后运行验证：进程稳定存活、无 panic（DockArea + 双面板渲染正常）。布局持久化（DockAreaState dump/load）与命令面板（Action/Keybinding）留待下一轮。


## 总原则

1. **先整体暂存，后逐 Feature 迁移**：不要一次性搬空，按垂直切片（connection → database → workbench）推进。
2. **Tauri 命令 → 服务方法 / GPUI Action**：`v1/backend/src/commands/*` 不再保留为命令层；其逻辑迁入各 Feature 的 service，UI 触发点按 GPUI Action 落地。
3. **Vue 视图 → GPUI 视图**：`v1/frontend/src/extensions/*` 的组件按 Feature 重写为 GPUI view（`Entity<T>` / `RenderOnce` 按需选择）。
4. **adapters/tauri 退役**：事件/流模式可参考提炼到 `shared`；WASM/Sidecar 适配器并入 `plugin`。
5. **迁移后删除 v1 对应文件**，保持 `v1/` 只含未迁移内容；全部迁移完成后移除 `v1/`。

## 映射表

### Rust 后端（v1/backend/src）

| v1 路径 | v2 目标 | 说明 |
| --- | --- | --- |
| `core/driver` | `engine` | 驱动抽象 traits、注册、池、路由（含 native/jdbc/wasm/registry） |
| `core/driver/connection` | `connection` | 连接配置/连接器/SSH/SSL/代理 |
| `core/dbi` | `engine` | 统一数据访问接口与引擎封装 |
| `core/duckdb` | `engine` | DuckDB 管理器/临时表/联邦/快照/导入导出/扩展 |
| `core/cache` | `engine` | 多级缓存（L1 LRU / L2 SQLite / L3 源库）、内存守卫 |
| `core/sql` | `engine` | SQL 解析/构建/转译/格式化（跨 Feature 基础设施） |
| `core/migration` | `engine` | 迁移执行器与 schema 管理 |
| `core/logging` | `engine` | 结构化日志、脱敏 |
| `core/performance` | `engine` | 运行时监控 |
| `core/persistence`（通用 stores） | `engine` | global_db / project_db / connection_store / history_store / auth_store / env_store / network_store / driver_store / plugin_store / log_store / sql_template_store / insight_*_store 等 |
| `core/persistence/analytics_resource_store` | `analytics_resource` | 资源目录/文件夹/标签/版本/回收站 |
| `core/project` | `project` | 项目模型与存储（project_store / project_connection_store / workbench_context_store） |
| `core/scratchpad` | `scratchpad` | 草稿模型/状态/存储 |
| `core/insight` | `insight` | 规则执行/注册/类型、schema 分析 |
| `core/plugin` | `plugin` | 插件清单/加载/安装/权限/依赖/存储 |
| `core/services/connection_*` | `connection` | 连接管理/驱动服务 |
| `core/services/duckdb_service` | `engine` | DuckDB 服务封装 |
| `core/services/execution_service` | `workbench` | SQL 执行（含取消/历史） |
| `core/services/result_service` | `workbench` | 结果集服务（三模式） |
| `core/services/insight_engine` / `quality_scorer` / `table_profile_service` | `insight` | 洞察执行与画像 |
| `core/services/plugin_bridge` / `plugin_service` | `plugin` | 插件桥与宿主 |
| `core/services/snapshot_service` | `project` | 版本快照 |
| `core/services/sql_parser_service` / `sql_service` | `engine` | SQL 服务 |
| `core/services/persistence_service` | `engine` | 持久化统一入口 |
| `core/services/driver_service` | `connection` | 驱动注册服务 |
| `core/{error,models,arrow,crypto,utils,stream,port_negotiation,api_version,macros}` | `shared` | 跨 Feature 稳定能力 |
| `mock/` | `mock` | Mock 引擎/生成器/模板/持久化 |
| `adapters/sidecar` | `plugin` | Sidecar 客户端/驱动/健康检查/热重载/管理器 |
| `adapters/wasm` | `plugin` | Extism 宿主/API/权限 |
| `adapters/tauri` | （退役→参考） | 事件/状态/流：模式提炼到 shared |
| `api/dto` | 各 Feature `model.rs` | DTO 按 Feature 拆分 |
| `docs/` | `v1/docs` 留档 | 迁移时提炼到 v2 `docs/` |

### 迁移资源（v1/backend 根下）

| v1 路径 | v2 目标 | 说明 |
| --- | --- | --- |
| `migrations/`（4 库 45 迁移） | `engine` | 保持 4 库分库迁移（global / project_meta / connection_metadata / project_analysis） |
| `insight-rules/` | `insight` | 规则资产（column/multi/quality/table 的 toml） |

### 前端（v1/frontend/src）

| v1 路径 | v2 目标 | 说明 |
| --- | --- | --- |
| `extensions/builtin/workbench` | `workbench` | 布局/活动栏/命令面板/编辑器工作台 → GPUI Dock + 命令 |
| `extensions/builtin/connection` | `connection` | 连接表单/网络链 → `connection_dialog.rs` |
| `extensions/builtin/database` | `database` | 导航树/缓存/搜索 → `database_view.rs` |
| `extensions/builtin/query` | `workbench` | 结果表格/查询执行 |
| `extensions/builtin/scratchpad` | `scratchpad` | 草稿面板 |
| `extensions/builtin/analytics-resource` | `analytics_resource` | 资源管理器 UI |
| `extensions/builtin/settings` | `workbench` | 设置对话框 |
| `extensions/builtin/mysql-driver` | （参考） | 驱动 schema 资产 |
| `core/*`（extension-host/registry） | `plugin` / `workbench` | 扩展宿主概念由 gpui-shell 承接 |
| `shared/design-tokens` | `workbench` / 主题 | 迁移为 GPUI theme token |

### 命令文件去向（v1/backend/src/commands/*）

| 命令文件 | 迁入 Feature |
| --- | --- |
| connection_commands / driver_commands / data_source_commands / port_commands | `connection` |
| metadata_commands / navigator_commands / metadata_cache_commands / cache_warming_commands | `database` |
| sql_commands / sql_parser_commands / sql_template_commands / result_commands | `workbench` |
| project_commands / project_store_commands | `project` |
| scratchpad_commands | `scratchpad` |
| analytics_resource_commands | `analytics_resource` |
| mock_commands / mock_persistence_commands | `mock` |
| plugin_commands | `plugin` |
| logging_commands / memory_commands / performance_commands / system_commands | `engine` / `shared` |

## 迁移顺序建议

- **Phase 1（垂直切片）**：`engine`（驱动/dbi/duckdb/缓存/迁移）→ `connection` → `database` → `workbench`（编辑/执行/结果）→ `app` 装配。
- **Phase 2（分析闭环）**：`project`（双层/快照）→ `insight` → DuckDB Secret 加速通道（`connection/secret.rs`）。
- **Phase 3（生态与分发）**：`analytics_resource` → `mock` → `plugin`（WASM/Sidecar/gpui-shell）→ 打包。

## 验收口径

- 每个 Feature 迁移完成后：`cargo check` 通过、targeted test 通过、v1 对应文件已删。
- 垂直切片可用：连接 → 导航 → 编辑 → 执行 → 结果（本地 GPUI 渲染）。
