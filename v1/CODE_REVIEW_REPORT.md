# RdataStation 全项目不足与改进空间分析

> **审查日期**: 2026-06-09 | **审查范围**: 全项目 (Rust ~150 文件 + TS/Vue ~200 文件 + 配置/迁移)
> **原则**: 本文档只列问题和改进，不列出已符合规范的部分

---

## 一、Rust 后端 — 逐模块不足分析

---

### 1.1 core/driver/native/ — 六种驱动实现

#### 1.1.1 MySqlDatabase (sqlx) — [mysql.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/native/mysql.rs)

| #   | 问题                                                                           | 严重度 | 改进方案                 |
| --- | ------------------------------------------------------------------------------ | ------ | ------------------------ |
| 1   | `pool_status()` 返回默认值，sqlx Pool 实际支持 `pool.size()`/`pool.num_idle()` | 中     | 实现真实连接池状态       |
| 2   | `names_to_schema_objects` 只取 `batches.first()`，多批次结果丢失后续批次数据   | **高** | 遍历所有 batches         |
| 3   | `max_connections`/`min_connections` 硬编码为 10/0                              | 低     | 从配置读取或从 Pool 获取 |

#### 1.1.2 PostgresDatabase (sqlx) — [postgres.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/native/postgres.rs)

| #   | 问题                                                                     | 严重度 | 改进方案                                 |
| --- | ------------------------------------------------------------------------ | ------ | ---------------------------------------- |
| 4   | `pool_status()` 只返回 `size` 和 `available`，未区分 idle/active/waiting | 中     | 细化状态字段                             |
| 5   | `list_triggers` 依赖 `information_schema.triggers` 但未处理权限不足场景  | 低     | 加 fallback 查询 `pg_catalog.pg_trigger` |

#### 1.1.3 MySqlNativeDatabase (mysql_async) — [mysql_native.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/native/mysql_native.rs)

| #   | 问题                                                                    | 严重度 | 改进方案              |
| --- | ----------------------------------------------------------------------- | ------ | --------------------- |
| 6   | 使用 `mysql_async` 非池化连接，每次查询可能新建连接，无连接复用         | **高** | 包装为连接池模式      |
| 7   | 缺少 `list_sequences` / `list_triggers` 实现（回退到 trait 默认空返回） | 中     | 实现 MySQL 触发器查询 |

#### 1.1.4 PostgresNativeDatabase (tokio-postgres) — [postgres_native.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/native/postgres_native.rs)

| #   | 问题                                                                              | 严重度 | 改进方案                                   |
| --- | --------------------------------------------------------------------------------- | ------ | ------------------------------------------ |
| 8   | 非池化，每次 `acquire` 创建新连接                                                 | **高** | 接入 `deadpool-postgres` 或 `bb8-postgres` |
| 9   | `known_hosts` 检查在 Windows 上直接返回错误 (见 connector.rs L504 `unreachable!`) | 中     | 实现 Windows SSH known_hosts               |

#### 1.1.5 SqliteDatabase (rusqlite) — [sqlite.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/native/sqlite.rs)

| #   | 问题                                                  | 严重度 | 改进方案                     |
| --- | ----------------------------------------------------- | ------ | ---------------------------- |
| 10  | `Arc<Mutex<Connection>>` 在并发查询场景下全部串行化   | 中     | 评估 `WAL` 模式 + 多连接读取 |
| 11  | 不支持 `query_with_cancel` (同步 driver 无法真正取消) | 低     | 文档说明限制即可             |

#### 1.1.6 DuckDbDatabase (duckdb-rs) — [duckdb.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/native/duckdb.rs)

| #   | 问题                                 | 严重度 | 改进方案                         |
| --- | ------------------------------------ | ------ | -------------------------------- |
| 12  | 同样 `Arc<Mutex<Connection>>` 串行化 | 中     | 考虑 `duckdb` 多 connection 支持 |

---

### 1.2 core/driver/connection/ — 网络连接层

#### 1.2.1 [connector.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/connection/connector.rs)

| #   | 问题                                                                           | 严重度 | 改进方案             |
| --- | ------------------------------------------------------------------------------ | ------ | -------------------- |
| 13  | L504: `unreachable!("Windows 上 connect_ssh_agent 总是返回 Err")` — 不应 panic | **高** | 改为返回 `CoreError` |
| 14  | L825: 使用了 `#[allow(deprecated)]` 标记废弃 API 调用                          | 低     | 升级到非废弃 API     |

#### 1.2.2 [known_hosts.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/connection/known_hosts.rs)

| #   | 问题                           | 严重度 | 改进方案              |
| --- | ------------------------------ | ------ | --------------------- |
| 15  | Windows 平台已知主机检查不支持 | 中     | 实现 Windows 兼容路径 |

---

### 1.3 core/services/ — 服务层

#### 1.3.1 [insight_engine.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/services/insight_engine.rs)

| #   | 问题                                                     | 严重度 | 改进方案                              |
| --- | -------------------------------------------------------- | ------ | ------------------------------------- |
| 16  | L647, L692, L718: 三处 `panic!()` — 数据匹配失败直接崩溃 | **高** | 返回 `CoreError::Internal` 代替 panic |
| 17  | 统计分析器缺少对 NULL 值列的边界检查                     | 中     | 加 NULL-only 列检测                   |

#### 1.3.2 [connection_service.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/services/connection_service.rs)

| #   | 问题                                                              | 严重度 | 改进方案         |
| --- | ----------------------------------------------------------------- | ------ | ---------------- |
| 18  | L839: `unreachable!()` 在匹配 SSH 连接类型                        | **高** | 返回 `CoreError` |
| 19  | L505: `#[allow(dead_code)]` 函数 `build_url_from_config` 未被使用 | 低     | 删除或启用       |
| 20  | 方法参数过多（如 L1723-1732 闭包嵌套 4 层 clone）                 | 中     | 提取为结构体参数 |

#### 1.3.3 [sql_service.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/services/sql_service.rs)

| #   | 问题                                                         | 严重度 | 改进方案                      |
| --- | ------------------------------------------------------------ | ------ | ----------------------------- |
| 21  | L598: `panic!("expected Ok")` — 测试代码但不应 panic         | 中     | 改用 `assert!` / `unwrap_err` |
| 22  | `value_to_sql` / `execute_update` 标记 `#[allow(dead_code)]` | 低     | 删除或用 feature gate 启用    |

#### 1.3.4 [connection_manager.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/services/connection_manager.rs)

| #   | 问题                                                           | 严重度 | 改进方案                |
| --- | -------------------------------------------------------------- | ------ | ----------------------- |
| 23  | `connections: RwLock<HashMap<ConnId, DynDatabase>>` — 全局写锁 | 中     | 考虑 `dashmap` 或分段锁 |
| 24  | 连接空闲回收定时器使用 `tokio::spawn` 但无优雅关闭             | 低     | 加入 shutdown signal    |

---

### 1.4 core/persistence/ — 持久化层

#### 1.4.1 [metadata_cache.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/persistence/metadata_cache.rs)

| #   | 问题                                                       | 严重度 | 改进方案                                               |
| --- | ---------------------------------------------------------- | ------ | ------------------------------------------------------ |
| 25  | **文件 5030+ 行** — 严重超出 800 行拆分阈值                | **高** | 拆分为 catalog/schema/table/column/invalidation 子模块 |
| 26  | 5 处 `#[allow(clippy::too_many_arguments)]` — 方法签名过长 | 中     | 使用 Builder 模式或配置结构体                          |
| 27  | L233, L240: 2 处 `#[allow(dead_code)]` 未使用字段          | 低     | 清理或启用                                             |

#### 1.4.2 [global_db.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/persistence/global_db.rs)

| #   | 问题                                                  | 严重度 | 改进方案                              |
| --- | ----------------------------------------------------- | ------ | ------------------------------------- |
| 28  | 文件 2048+ 行 — 超过 800 行阈值                       | 中     | 拆分为 migrations/schema/query 子模块 |
| 29  | L2044, L2048: 测试中有 2 处 `unwrap()` (测试中可接受) | 低     | 改为 `expect()` 给出上下文            |

#### 1.4.3 [metadata_cache_pool.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/persistence/metadata_cache_pool.rs)

| #   | 问题                                         | 严重度 | 改进方案           |
| --- | -------------------------------------------- | ------ | ------------------ |
| 30  | L314, L322: 2 处 `expect()` 在生产代码中     | 中     | 改为 `?` 操作符    |
| 31  | `Arc::clone` 使用 6 处，部分可优化为引用传递 | 低     | 减少不必要的 clone |

#### 1.4.4 [connection_store.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/persistence/connection_store.rs)

| #   | 问题                                                                 | 严重度 | 改进方案                       |
| --- | -------------------------------------------------------------------- | ------ | ------------------------------ |
| 32  | `system_time_secs()` 使用 `unwrap_or(0)` — 时间获取失败返回 0 不合理 | 低     | 返回 `Option<u64>` 或 `Result` |

---

### 1.5 core/plugin/ — 插件系统

#### 1.5.1 [manifest.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/plugin/manifest.rs)

| #   | 问题                                                                          | 严重度 | 改进方案         |
| --- | ----------------------------------------------------------------------------- | ------ | ---------------- |
| 33  | L235-236: `File::create().unwrap()` + `write_all().unwrap()` — 文件操作 panic | **高** | 返回 `CoreError` |
| 34  | L457: `toml::from_str().unwrap()` — 解析失败 panic                            | **高** | 返回 `CoreError` |
| 35  | L307, L321, L325, L556, L557: 5 处测试 `unwrap()` (测试中可接受)              | 低     | -                |

#### 1.5.2 [storage.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/plugin/storage.rs)

| #   | 问题                             | 严重度 | 改进方案       |
| --- | -------------------------------- | ------ | -------------- |
| 36  | L103: `TODO: 实际项目中需要实现` | 中     | 实现持久化存储 |

#### 1.5.3 [manager.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/plugin/manager.rs)

| #   | 问题                                                      | 严重度 | 改进方案              |
| --- | --------------------------------------------------------- | ------ | --------------------- |
| 37  | L304: `TODO: Add Sidecar plugins` — 缺少 Sidecar 插件注册 | 中     | 实现 Sidecar 注册逻辑 |

---

### 1.6 core/duckdb/ — DuckDB 集成

#### 1.6.1 [manager.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/duckdb/manager.rs)

| #   | 问题                                                       | 严重度 | 改进方案                                          |
| --- | ---------------------------------------------------------- | ------ | ------------------------------------------------- | ------ | ------------- |
| 38  | L66: `unwrap_or_else(                                      | e      | panic!("初始化全局 DuckDB 内存实例失败: {}", e))` | **高** | 返回 `Result` |
| 39  | L68: 同上 `panic!` — 配置全局 DuckDB 连接失败              | **高** | 返回 `Result`                                     |
| 40  | L153: `TODO: 未来可集成 SqlEngine 进行更严格的验证`        | 低     | 按计划集成                                        |
| 41  | L357: `#[allow(dead_code)]` — `get_connection_pool` 未使用 | 低     | 启用或删除                                        |

#### 1.6.2 [temp_table.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/duckdb/temp_table.rs)

| #   | 问题                                                              | 严重度 | 改进方案          |
| --- | ----------------------------------------------------------------- | ------ | ----------------- |
| 42  | L456, L459, L475: 3 处 `registry.write().unwrap()` — 锁中毒 panic | **高** | 使用 `?` 传播错误 |

#### 1.6.3 [explain.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/duckdb/explain.rs)

| #   | 问题                                      | 严重度 | 改进方案           |
| --- | ----------------------------------------- | ------ | ------------------ |
| 43  | L228: `#[deprecated]` 标记的旧 API 未移除 | 低     | 清理或标记删除版本 |

---

### 1.7 core/sql/ — SQL 引擎封装

#### 1.7.1 [builder.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/sql/builder.rs)

| #   | 问题                                                 | 严重度 | 改进方案                |
| --- | ---------------------------------------------------- | ------ | ----------------------- |
| 44  | L10: `#[allow(dead_code)]` — 有未使用的 builder 函数 | 低     | 确认是否需要 public API |

#### 1.7.2 整体

| #   | 问题                                                          | 严重度 | 改进方案         |
| --- | ------------------------------------------------------------- | ------ | ---------------- |
| 45  | SQL 引擎无任何单元测试（parser/formatter/transpiler/builder） | **高** | 补充核心场景测试 |

---

### 1.8 core/cache/ — 缓存系统

| #   | 问题                                                                                                                                                | 严重度 | 改进方案           |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------- | ------ | ------------------ |
| 46  | [lru_cache.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/cache/lru_cache.rs#L108): `#[allow(dead_code)]` 未使用方法 | 低     | 清理               |
| 47  | [memory_guard.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/cache/memory_guard.rs#L70): `#[allow(dead_code)]`       | 低     | 启用内存保护       |
| 48  | [query_cache.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/cache/query_cache.rs#L34): `#[allow(dead_code)]`         | 低     | 清理               |
| 49  | [minicatalogs.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/cache/minicatalogs.rs#L103): `TODO` 硬编码 YAML 替换    | 低     | 替换为配置文件读取 |

---

### 1.9 core/dbi/ — DBI 抽象层

| #   | 问题                                                                                                                                                       | 严重度 | 改进方案 |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ | -------- |
| 50  | [duckdb_engine.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/dbi/engine/duckdb_engine.rs#L647): `#[allow(dead_code)]`      | 低     | 清理     |
| 51  | [stream_engine.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/dbi/engine/stream_engine.rs#L274): `#[allow(dead_code)]`      | 低     | 清理     |
| 52  | [performance.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/dbi/performance.rs#L97): `#[allow(clippy::too_many_arguments)]` | 低     | 重构参数 |

---

### 1.10 adapters/ — 适配器层

#### 1.10.1 [adapters/sidecar/](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/adapters/sidecar/)

| #   | 问题                                                  | 严重度 | 改进方案     |
| --- | ----------------------------------------------------- | ------ | ------------ |
| 53  | `driver.rs` L118: `cancel_query` 未实现（TODO）       | **高** | 实现取消逻辑 |
| 54  | `driver.rs` L184: `parse_response` 返回空结果（TODO） | **高** | 实现响应解析 |
| 55  | `driver.rs` L295: `get_default_port` 硬编码（TODO）   | 中     | 补充端口映射 |

---

### 1.11 commands/ — 命令层

#### 1.11.1 [project_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/project_commands.rs)

| #   | 问题                                                      | 严重度 | 改进方案         |
| --- | --------------------------------------------------------- | ------ | ---------------- |
| 56  | L846: `TODO: 实际上加载这些插件到 PluginManager`          | 中     | 实现加载         |
| 57  | L1323: `panic!("Expected Local path")` — 路径不匹配 panic | **高** | 返回 `CoreError` |
| 58  | L1377: `panic!("Expected Remote path")` — 同上            | **高** | 返回 `CoreError` |

#### 1.11.2 [connection_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/connection_commands.rs)

| #   | 问题                                                        | 严重度 | 改进方案        |
| --- | ----------------------------------------------------------- | ------ | --------------- |
| 59  | L921: `unreachable!()` — 类型转换不完整                     | 中     | 改为 error 返回 |
| 60  | L417: `#[allow(dead_code)]` — `ping_connection_impl` 未使用 | 低     | 清理            |

#### 1.11.3 [cache_warming_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/cache_warming_commands.rs)

| #   | 问题                                                    | 严重度 | 改进方案             |
| --- | ------------------------------------------------------- | ------ | -------------------- |
| 61  | L434, L444: `abort_all()` 取消所有任务 — 不等待优雅完成 | 中     | 先发取消信号再 abort |

---

### 1.12 lib.rs — 主入口

| #   | 问题                                                                                                                     | 严重度 | 改进方案                              |
| --- | ------------------------------------------------------------------------------------------------------------------------ | ------ | ------------------------------------- | --- | ---------------- |
| 62  | 命令路由三处重复：`collect_commands!` (129-335)、`generate_handler!`×2 (341-471, 474-663)、match 分支 (674-793) 各自维护 | **高** | 宏自动化或统一 handler                |
| 63  | `get_log_dir()` 使用 `unwrap_or_else(                                                                                    |        | PathBuf::from("."))` — 降级到当前目录 | 低  | 至少 log warning |

---

### 1.13 mock/ — 模拟数据引擎

| #   | 问题                                                                                                                                 | 严重度 | 改进方案   |
| --- | ------------------------------------------------------------------------------------------------------------------------------------ | ------ | ---------- |
| 64  | [engine.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/mock/engine.rs#L255): `unreachable!()` 在生成器匹配 | 中     | 改为 error |
| 65  | `#[allow(dead_code)]` 1 处 (L694)                                                                                                    | 低     | 清理       |

---

## 二、前端 TypeScript/Vue — 逐模块不足分析

---

### 2.1 extensions/builtin/connection/ — 数据源管理

#### 2.1.1 类型安全

| #   | 文件                                                                                                                                               | 问题                | 改进方案                        |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------- | ------------------------------- |
| 66  | [NetworkTab.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/connection/ui/components/tabs/NetworkTab.vue#L1075) | 认证数据 `as any`   | 定义 `AuthData` interface       |
| 67  | [GeneralTab.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/connection/ui/components/tabs/GeneralTab.vue#L309)  | 表单字段 `any`      | 使用 `DynamicFormField` 泛型    |
| 68  | [connection-store.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/connection/ui/stores/connection-store.ts#L62)  | `as any[]` 类型硬转 | 为 project connections 定义接口 |

#### 2.1.2 功能完整性

| #   | 问题                                                              | 改进方案             |
| --- | ----------------------------------------------------------------- | -------------------- |
| 69  | `useAddDataSource` 的 `StagingItem` 19 字段需要在运行时验证完整性 | 添加 Zod schema 校验 |
| 70  | 连接测试失败时错误信息不够具体（如 DNS 失败 vs 密码错误未区分）   | 细化错误码           |

---

### 2.2 extensions/builtin/database/ — 数据库导航器

| #   | 文件                                                                                                                                                                  | 问题                                        | 改进方案             |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------- | -------------------- |
| 71  | [use-database-tree-loader.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/composables/use-database-tree-loader.ts#L100) | `children: None` 懒加载后未正确处理取消场景 | 添加 AbortController |
| 72  | [zod-validation.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/utils/zod-validation.ts#L38)                            | `z.any()` 削弱验证                          | 定义具体 schema      |

---

### 2.3 extensions/builtin/workbench/ — 工作台

#### 2.3.1 编辑器

| #   | 问题                                            | 改进方案                   |
| --- | ----------------------------------------------- | -------------------------- |
| 73  | 编辑器 Tab 状态恢复没有内存上限，长会话可能 OOM | 添加 Tab 数量/内容大小限制 |
| 74  | SQL 编辑器无自动补全元数据缓存过期机制          | 添加 cache TTL             |

#### 2.3.2 结果面板

| #   | 问题                                             | 改进方案                     |
| --- | ------------------------------------------------ | ---------------------------- |
| 75  | AG Grid 虚拟滚动在百万级行数下可能需要服务端分页 | 对接 `execute_sql_paginated` |
| 76  | 结果导出格式仅 CSV，缺少 JSON/Parquet/Excel      | 扩展导出格式                 |

---

### 2.4 extensions/builtin/query/ — 查询模块

| #   | 文件                                                                                                                                                | 问题                     | 改进方案                         |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------ | -------------------------------- |
| 77  | [ResultTable.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/query/ui/components/ResultTable.vue#L36)            | AG Grid API 类型用 `any` | 使用 `GridApi`, `ColDef`         |
| 78  | [SqlEditorToolbar.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/query/ui/components/SqlEditorToolbar.vue#L267) | iconMap 类型 `any`       | 定义 `Record<string, Component>` |

---

### 2.5 extensions/builtin/settings/ — 设置

| #   | 问题                                                          | 改进方案           |
| --- | ------------------------------------------------------------- | ------------------ |
| 79  | 设置持久化依赖 `tauri-plugin-store`，离线同步到项目级别未实现 | 实现项目级设置继承 |

---

### 2.6 前端架构共性不足

| #   | 问题                                                 | 改进方案                              |
| --- | ---------------------------------------------------- | ------------------------------------- |
| 80  | 缺少全局 Loading/Error 边界组件                      | 实现 `<ErrorBoundary>` + `<Suspense>` |
| 81  | Extension 之间通信通过全局 store，缺少类型化事件总线 | 使用 typed event emitter              |
| 82  | 前端 vitest 测试仅覆盖少量组件                       | 扩大测试覆盖                          |
| 83  | 缺少 E2E 测试 (tauri e2e 或 playwright)              | 引入 E2E 框架                         |

---

## 三、数据库迁移 — 不足分析

| #   | 问题                                                                       | 严重度 | 改进方案                    |
| --- | -------------------------------------------------------------------------- | ------ | --------------------------- |
| 84  | `connection_metadata/` 迁移编号跳跃 001→003 (缺 002)                       | 低     | 补齐缺失编号或文档说明      |
| 85  | 全局迁移 017 和项目迁移 015 之后无新迁移，v0.5.2+ 的部分字段变更可能未对齐 | 中     | 审计 migration 与代码一致性 |
| 86  | 迁移缺少回滚脚本                                                           | 低     | 补充 down migration         |
| 87  | `analytics_resources` 表无 FTS (全文搜索) 索引                             | 低     | 添加 FTS 索引               |

---

## 四、配置与依赖 — 不足分析

| #   | 问题                                               | 严重度 | 改进方案                               |
| --- | -------------------------------------------------- | ------ | -------------------------------------- |
| 88  | `once_cell` 依赖在 Rust 1.80+ 已可被 std 替代      | 低     | 移除依赖，统一用 `std::sync::OnceLock` |
| 89  | `thiserror` 停留在 1.0.69, 2.x 有 breaking changes | 低     | 评估升级收益                           |
| 90  | ESLint 8.x 已 EOL，应升级到 9.x flat config        | 中     | 迁移到 eslint 9 + flat config          |
| 91  | `vite.config.ts` / `tsconfig.json` 未检查推入仓库  | 低     | 确认配置完整                           |
| 92  | `capabilities/default.json` 可能包含过多权限       | 中     | 按最小权限原则审计                     |

---

## 五、测试覆盖 — 不足分析

### 5.1 Rust 缺失测试

| #   | 模块                                              | 缺失内容               | 优先级 |
| --- | ------------------------------------------------- | ---------------------- | ------ |
| 93  | `core/sql/` (builder/formatter/parser/transpiler) | 零测试                 | **P1** |
| 94  | `core/crypto.rs`                                  | 加密/解密循环测试      | **P1** |
| 95  | `core/plugin/`                                    | 加载/卸载/激活生命周期 | P2     |
| 96  | `core/duckdb/`                                    | 联邦查询集成测试       | P2     |
| 97  | `core/insight/`                                   | 各规则执行测试         | P2     |
| 98  | `core/stream.rs`                                  | 流式查询/取消          | P2     |
| 99  | `adapters/sidecar/`                               | 零测试                 | P2     |
| 100 | `adapters/wasm/`                                  | 零测试                 | P2     |

### 5.2 前端缺失测试

| #   | 模块                      | 缺失内容                            | 优先级 |
| --- | ------------------------- | ----------------------------------- | ------ |
| 101 | `connection/` composables | `useAddDataSource`, `useAuthConfig` | P2     |
| 102 | `database/` composables   | `useDatabaseNavigator`              | P2     |
| 103 | `workbench/` panels       | EditorPanel, QueryResultPanel       | P2     |
| 104 | E2E 测试                  | 完整的连接-查询-导出流程            | P3     |

---

## 六、文档与代码一致性 — 不足

| #   | 问题                                                          | 改进方案            |
| --- | ------------------------------------------------------------- | ------------------- |
| 105 | `docs/backend/IMPLEMENTATION_SUMMARY.md` 可能反映旧版本状态   | 更新或标记日期/版本 |
| 106 | `docs/frontend/editor-audit-issues.md` 审计问题需确认当前状态 | 逐项核对并更新      |
| 107 | 缺少 API 变更日志 (CHANGELOG)                                 | 维护 `CHANGELOG.md` |
| 108 | `docs/README.md` 索引可能过期                                 | 更新引用路径        |

---

## 七、安全加固建议

| #   | 风险点                                  | 改进方案                                                  |
| --- | --------------------------------------- | --------------------------------------------------------- |
| 109 | `ConnectionConfig.url` 可能包含明文密码 | 禁止在 URL 中传递密码，强制通过 `password_encrypted` 字段 |
| 110 | SQLite/DuckDB 文件无权限检查            | 添加文件权限检查（Unix mode / Windows ACL）               |
| 111 | 日志脱敏 `redact.rs` 覆盖范围未知       | 审计脱敏规则覆盖所有敏感字段                              |
| 112 | SSH 私钥存储在明文                      | 考虑加密存储私钥文件                                      |

---

## 八、Dead Code 统计

按 `#[allow(dead_code)]` 分布：

| 文件                     | 数量   | 建议                                 |
| ------------------------ | ------ | ------------------------------------ |
| `logging/config.rs`      | 4      | 启用或移除                           |
| `smart_pool.rs`          | 3      | 评估是否启用 SmartPool               |
| `metadata_cache.rs`      | 2      | 清理                                 |
| `connection_service.rs`  | 1      | 启用 `build_url_from_config`         |
| `sql_service.rs`         | 2      | 移除 `value_to_sql`/`execute_update` |
| `connection_commands.rs` | 1      | 移除                                 |
| `duckdb/explain.rs`      | 1      | 移除 deprecated API                  |
| `duckdb/manager.rs`      | 1      | 启用 `get_connection_pool`           |
| `duckdb/temp_table.rs`   | 1      | 移除                                 |
| `cache/lru_cache.rs`     | 1      | 移除                                 |
| `cache/memory_guard.rs`  | 1      | 启用                                 |
| `cache/query_cache.rs`   | 1      | 移除                                 |
| `dbi/duckdb_engine.rs`   | 1      | 移除                                 |
| `dbi/stream_engine.rs`   | 1      | 移除                                 |
| **总计**                 | **21** |                                      |

> 21 处 dead_code 中有 7 处可能是预留 API（SmartPool 3 处、memory_guard 1 处、connection_service 1 处、duckdb/manager 1 处）。其余 14 处应清理。

---

## 九、优先修复路线图

### 阶段一：稳定性修复 (本周) — 5 项

```
1. [HIGH] insight_engine.rs     → 3 处 panic! → CoreError
2. [HIGH] project_commands.rs   → 2 处 panic! → CoreError
3. [HIGH] duckdb/manager.rs     → 2 处 panic! → CoreError
4. [HIGH] connector.rs          → 1 处 unreachable! → CoreError
5. [HIGH] connection_service.rs → 1 处 unreachable! → CoreError
```

### 阶段二：健壮性修复 (本月) — 7 项

```
6. [HIGH] plugin/manifest.rs    → 3 处 unwrap → CoreError
7. [HIGH] duckdb/temp_table.rs  → 3 处 unwrap → CoreError
8. [HIGH] 清理 8 处 TODO 标记
9. [HIGH] sidecar/driver.rs     → 3 个未实现功能
10. [HIGH] lib.rs               → 命令路由去重
11. [HIGH] core/sql/            → 补充单元测试
12. [HIGH] core/crypto.rs       → 补充单元测试
```

### 阶段三：质量提升 (3 个月) — 10 项

```
13. [MED] metadata_cache.rs → 拆分为子模块 (5000+ 行)
14. [MED] global_db.rs → 拆分子模块 (2000+ 行)
15. [MED] MySQL/PostgreSQL native 驱动 → 连接池化
16. [MED] 前端 10 处 any → 类型化
17. [MED] metadata_cache_pool.rs → expect → ?
18. [MED] 15 处 dead_code 清理
19. [MED] ESLint 8 → 9 升级
20. [MED] 前端组件测试补充
21. [LOW] once_cell 移除
22. [LOW] 文档同步审计
```

---

## 十、总结

| 类别                                   | 数量       |
| -------------------------------------- | ---------- |
| 🔴 Critical (panic/unreachable/unwrap) | 15 处      |
| 🟡 Major (TODO/dead_code/大文件)       | 22 处      |
| 🟢 Minor (类型/文档/配置)              | 30 处      |
| ⚠️ Dead Code 注解                      | 21 处      |
| ❌ 缺失测试模块                        | 12 个      |
| **总计问题**                           | **112 项** |

**最紧迫**: 消除 15 处 `panic!`/`unreachable!`/`unwrap()` 可导致进程崩溃的风险点。

**最大技术债**: `metadata_cache.rs` (5030 行) 和 `global_db.rs` (2048 行) 的单文件规模。
