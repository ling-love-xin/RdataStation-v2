# 元数据缓存与预热系统实现总结

## 概述

本文档总结了对元数据模块和缓存模块的完整审计与修复工作。

## 已完成的修复工作

### 1. 数据库 Schema 修复

**问题**: `privileges` 表的外键引用了不存在的表 `connections`

**修复内容**:

- 移除了 `src-tauri/migrations/connection_metadata/009_jdbc_metadata_alignment.sql` 中的错误外键
- 简化了表结构，移除了 `connection_id` 字段（因为每个连接有独立的元数据缓存库）

**修改文件**:

- [`src-tauri/migrations/connection_metadata/009_jdbc_metadata_alignment.sql`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src-tauri/migrations/connection_metadata/009_jdbc_metadata_alignment.sql)

### 2. 代码质量修复

**问题**: `cache_manager.rs` 中的 `metadata_cache_mut` 函数有 `unimplemented!()`

**修复内容**:

- 移除了 `src-tauri/src/core/cache/cache_manager.rs` 中的有问题函数
- 保持使用线程安全的 `Arc<Mutex<MetadataCache>>` 设计

**修改文件**:

- [`src-tauri/src/core/cache/cache_manager.rs`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src-tauri/src/core/cache/cache_manager.rs)

### 3. 前后端 API 对齐

**问题**: 前后端 API 完全不对齐，类型定义不一致

**修复内容**:

- 完整更新了前端服务，添加了所有后端已实现的 API 调用
- 添加了类型定义，与后端保持一致
- 支持全局/项目双架构模式

**修改文件**:

- [`src/extensions/builtin/database/ui/services/metadata-cache-service.ts`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src/extensions/builtin/database/ui/services/metadata-cache-service.ts)

### 4. 类型自动生成集成

**问题**: 前后端类型需要手动维护，容易不一致

**修复内容**:

- 添加了 `ts-rs` 依赖到 `Cargo.toml`
- 创建了 `types.rs` 统一导出模块
- 添加了 `build.rs` 构建脚本
- 更新了 `core/mod.rs` 导出新模块

**新增/修改文件**:

- [`src-tauri/Cargo.toml`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src-tauri/Cargo.toml)
- [`src-tauri/src/core/types.rs`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src-tauri/src/core/types.rs)（新建）
- [`src-tauri/build.rs`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src-tauri/build.rs)（新建）
- [`src-tauri/src/core/mod.rs`](file:///e:/myapps/tauirapps/RdataStation/个人工作台-插件/rdata-station/src-tauri/src/core/mod.rs)

## 完整的 API 功能

### 元数据加载 API (metadata_commands.rs)

- `loadDatabases` / `loadCatalogs` - 加载数据库列表
- `loadSchemas` - 加载 Schema 列表
- `loadTables` / `loadViews` - 加载表/视图
- `loadColumns` - 加载列
- `loadProcedures` / `loadFunctions` - 加载存储过程/函数
- `loadRoutineSource` - 加载例程源码
- `loadIndexes` - 加载索引
- `loadConstraints` - 加载约束
- `invalidateMetadataCache` - 缓存失效
- `getCacheStats` / `resetCacheStats` - 缓存统计
- `setIntrospectionLevel` / `getIntrospectionLevel` / `removeIntrospectionLevel` - 内省级别管理

### 元数据缓存管理 API (metadata_cache_commands.rs)

- `getMetadataCacheStatus` - 获取缓存状态
- `refreshMetadataCache` / `clearMetadataCache` - 刷新/清除缓存
- `saveTableMetadataToCache` / `saveColumnMetadataToCache` - 保存元数据
- `saveTablesBatchToCache` / `saveColumnsBatchToCache` - 批量保存
- `getTablesFromCache` / `getColumnsFromCache` - 从缓存读取
- `notifyDDLEvent` - DDL 事件通知
- `getSyncStatus` / `cancelSync` - 同步状态管理

### 缓存预热 API (cache_warming_commands.rs)

- `buildCacheIndex` - V7: 构建缓存索引（支持增量模式）
- `startCacheWarming` / `cancelCacheWarming` - 预热控制
- `getWarmingProgress` - 预热进度
- `checkCacheVersion` / `executeCacheMigration` - 版本管理
- `getCacheMigrationHistory` - 迁移历史
- `getIntrospectLevelSuggestion` - V7: 内省级别建议
- `getSchemaObjectCounts` - V7: 对象统计

## 全局/项目双架构

系统完整支持：

- **全局连接**: 元数据缓存存储在用户系统目录
- **项目连接**: 元数据缓存存储在项目内部目录
- 所有 API 都接受 `connectionType` 和 `projectPath` 参数

## 架构特性

### 1. 三层缓存架构

- **L1**: 内存 LRU 缓存（进程内）
- **L2**: SQLite 持久化缓存（可选）
- **L3**: 数据库直接查询

### 2. 差异化过期策略

- Catalogs: 1小时
- Schemas: 30分钟
- Tables/Views: 10分钟
- Columns/Routines: 1小时

### 3. 完整的版本迁移

- 10个迁移版本（新增 V10：企业级统计 + 显示控制）
- 自动升级检测
- 迁移历史追踪

### 4. 高性能优化

- WAL 模式
- Memory-Mapped I/O (256MB)
- 批量写入
- JoinSet 并行查询

### 5. 内省级别 (V7)

- Level 1: 基础信息（快速）
- Level 2: 标准信息（中等）
- Level 3: 完整信息（慢但详细）

### 6. 企业级元数据统计（V10 新增）

- **schemata 聚合统计列**：total_tables, total_views, total_procedures, total_functions, total_size_bytes, row_count_total
- **tables 显示控制列**：display_order, hidden, favorite, color_label, user_comment
- **schema_stats 视图**：实时聚合每个 Schema 的对象数量和数据大小
- **connection_stats 视图**：跨 Schema 的连接级统计汇总
- **save_table_with_stats**：保存表元数据时自动更新所属 Schema 的聚合统计
- **list_schemas_with_stats**：查询 Schema 列表时返回完整统计信息

### 7. 导航树修复（V10 同步）

- **childCount 动态计算**：Schema 节点按 NavigationConfig 启用的文件夹数计算；Tables/Views 文件夹按实际已加载数据计数；表节点按 columns + 配置的 tableChildren 计算
- **离线浏览支持**：`loadCatalogsFromCacheSilent` 方法，连接断开后仍可从 L2 SQLite 缓存构建导航树
- **数据库类型自适应**：PostgreSQL（catalog+schema）、MySQL（catalog→tables 直接）、DuckDB/SQLite（connection→tables 直接）

### 8. 前后端一致性修复（V10.1）

- **Rust TableMeta 扩展**：新增 7 个 V10 字段（row_count_estimate, data_length, index_length, display_order, hidden, favorite, color_label, user_comment），使用 `skip_serializing_if` 可选序列化
- **Rust SchemaMeta 扩展**：新增 6 个 Schema 聚合统计字段（total_tables, total_views, total_procedures, total_functions, total_size_bytes, row_count_total）及 `SchemaMeta::basic()` 构造函数
- **前端 metadata-cache-service.ts**：SchemaMeta 新增 6 个可选统计字段，与 Rust 完全对齐
- **生成绑定同步**：`src/generated/specta/bindings.ts` 的 SchemaMeta/TableMeta 类型同步更新
- **所有 TableMeta/SchemaMeta 构造点**：统一使用 `TableMeta::basic()` / `SchemaMeta::basic()` 工厂方法
- **ColumnMeta 验证**：前后端 8 字段（name/dataType/isNullable/defaultValue/isPrimaryKey/isForeignKey/comment）经 serde rename 完全对齐

### 9. 导航栏全链路打通修复（V10.2）

- **P0-1 indexes/constraints 加载修复**：Store 新增 `loadIndexes`/`loadConstraints` 方法，写入 TableNode.indexes/constraints；Tree Loader 展开 indexes-folder/constraints-folder 时先调用 Store 加载再渲染
- **P0-2 sequences/triggers 文件夹修复**：Tree Loader `loadChildren` 新增 `sequences-folder`/`triggers-folder` 分支，之前展开时静默返回空
- **P0-3 后端 commands 新增**：
  - `load_sequences` Tauri 命令（返回 SequenceMeta）
  - `load_triggers` Tauri 命令（返回 TriggerMeta，含 tableName/event）
  - Specta 类型绑定 + lib.rs 命令注册（3 处）
- **P0-4 驱动实现补齐**：
  - Database trait 新增 `list_sequences` / `list_triggers` 默认方法
  - PostgreSQL 实现：`pg_catalog.pg_proc` 查询序列/触发器
  - MySQL 实现：`SHOW INDEX FROM` 查询索引
  - MetadataBrowser trait 新增 `get_sequences` / `get_triggers` 方法
  - MetadataService 新增 `list_sequences` / `list_triggers` 方法
- **SchemaObject 扩展**：新增 `Sequence` / `Trigger` 变体；新增 `table_name` / `event` 可选字段；`names_to_schema_objects` 支持触发器 3 列解析
- **前端 API 补齐**：`database-api.ts` 新增 `loadSequences` / `loadTriggers`（tauri.invoke 直调），`SequenceMeta` / `TriggerMeta` 类型定义
- **前端 Store 接口扩展**：`SchemaNode.sequences?` / `triggers?` 字段，`SequenceNode` / `TriggerNode` 接口
- **Tree Loader 新增**：`createSequenceNodes` / `createTriggerNodes` 渲染函数

### 10. 后端 Schema 对齐审计 + 驱动索引实现（V10.3）

- **三库 Schema 全链路对齐验证**：
  - `global_connections`（global.db）25 列完整：id, name, driver, host, port, database, schema_name, username, password_encrypted, options, tags, use_duckdb_fed, metadata_path, is_active, server_version, description, driver_id, environment_id, auth_config_id, auth_method, network_config_id, driver_properties, advanced_options, created_at, updated_at
  - `connections`（project.db）25 列完整：字段与 global_connections 一一对应，driver 列替代旧 db_type
  - 列演化路径验证：global（001:16列 → 005:+server_version → 008:+7列 → 010:+auth_method = 25列），project（001:13列 → 002:重建17列 → 010:+8列 → 012:+auth_method = 25列）
- **Rust 结构体与 DB 对齐**：
  - `GlobalConnectionInfo`（25 字段）↔ `global_connections`（25 列）✅
  - `ProjectConnection`（25 字段）↔ `connections`（25 列）✅
  - `AuthConfig`（9 字段）↔ 项目 `auth_configs`（9 列，含 origin/source_id/snapshot_at），全局 `auth_configs`（6 列，无快照字段）✅
  - `NetworkConfig`（10 字段）↔ 项目 `network_configs`（10 列，含 origin + auth_config_id），全局 `network_configs`（7 列，无快照字段）✅
- **认证/网络配置 INSERT 一致性**：
  - `create_global_auth_config`：6 列（无 origin 字段）✅
  - `create_auth_config`（项目）：9 列（含 origin/source_id/snapshot_at）✅
  - `create_global_network_config`：7 列（含 auth_config_id，无 origin）✅
  - `create_network_config`（项目）：10 列（含 origin + auth_config_id）✅
- **DuckDB `list_indexes` 实现**：使用 `duckdb_indexes()` 表函数查询索引名、唯一性、类型、表达式，从 expression 字段（如 `"CREATE INDEX idx ON t (col1, col2)"`）正则解析列名列表；避免 `Vec<i64>` 与 duckdb-rs `FromSql` trait 不兼容问题
- **SQLite `list_indexes` 实现**：`PRAGMA index_list(table)` 获取索引列表（seq/name/unique/origin），`PRAGMA index_info(index_name)` 获取列名；通过 origin 列（"pk"）检测主键
- **前端类型修复**：`useAddDataSource.ts` 第 440 行 `project_id: params.projectId ?? null` → `params.projectId ?? undefined`（修复 `string | null` 不可赋值给 `string | undefined` 的类型错误）
- **Quality Gates**：`cargo clippy -- -D warnings`（0 errors）、`cargo fmt`（passed）、`pnpm vue-tsc`（0 errors）

## 后续建议

1. **启用类型自动生成**
   - 在首次调试构建时运行 `cargo build` 重新生成 specta bindings
   - 切换前端使用自动生成的类型而不是临时定义

2. **进一步优化**
   - 在 `load_tables` 命令中从驱动层读取 row_count_estimate / data_length / index_length
   - 调用 `save_table_with_stats()` 将统计信息写入 L2 缓存
   - 完善权限表的读写逻辑
   - 添加更多单元测试

3. **文档完善**
   - 添加架构图
   - 添加使用示例
   - 添加性能基准

## 文件清单

### 新增文件

- `src-tauri/migrations/connection_metadata/010_enterprise_statistics_and_display.sql`
- `docs/metadata-cache-implementation.md` (本文件)

### 修改文件 (V10)

- `src-tauri/src/core/persistence/metadata_cache.rs` — SchemaInfo/TableDetailInfo 扩展 + save_table_with_stats / update_schema_stats / get_schema_stats / list_schemas_with_stats
- `src/extensions/builtin/database/ui/services/metadata-cache-service.ts` — TableMeta + SchemaMeta 扩展
- `src/extensions/builtin/database/ui/stores/database-navigator-store.ts` — loadCatalogsFromCacheSilent
- `src/extensions/builtin/database/ui/composables/use-database-tree-loader.ts` — childCount 修复 + countEnabledFolders/countTableChildren + 离线支持

### 修改文件 (V10.1 一致性修复)

- `src-tauri/src/commands/metadata_commands.rs` — TableMeta/SchemaMeta 扩展 + basic()/from_detail() 工厂方法
- `src/generated/specta/bindings.ts` — SchemaMeta/TableMeta 类型同步

### 修改文件 (V10.2 导航栏全链路打通)

- `src-tauri/src/core/driver/traits.rs` — SchemaObjectKind 新增 Sequence/Trigger；SchemaObject 新增 table_name/event；Database trait 新增 list_sequences/list_triggers；MetadataBrowser 新增 get_sequences/get_triggers
- `src-tauri/src/core/driver/native/postgres.rs` — 实现 list_sequences/list_triggers；names_to_schema_objects 支持 3 列触发器解析
- `src-tauri/src/core/driver/native/mysql.rs` — 实现 list_indexes via SHOW INDEX FROM
- `src-tauri/src/commands/metadata_commands.rs` — 新增 SequenceMeta/TriggerMeta + load_sequences/load_triggers 命令
- `src-tauri/src/core/services/metadata_service.rs` — 新增 list_sequences/list_triggers
- `src-tauri/src/lib.rs` — 命令注册（3 处）+ 权限允许列表
- `src/extensions/builtin/database/ui/stores/database-navigator-store.ts` — 新增 loadIndexes/loadConstraints/loadSequences/loadTriggers；SchemaNode 新增 sequences/triggers；新增 SequenceNode/TriggerNode 接口
- `src/extensions/builtin/database/ui/api/database-api.ts` — 新增 loadSequences/loadTriggers + SequenceMeta/TriggerMeta 类型
- `src/extensions/builtin/database/ui/composables/use-database-tree-loader.ts` — indexes/constraints 先加载再渲染；新增 sequences/triggers folder 和 node 处理

### 修改文件 (V10.3 Schema 审计 + 驱动索引实现)

- `src-tauri/src/core/driver/native/duckdb.rs` — 实现 `list_indexes`（`duckdb_indexes()` 表函数 + expression 列解析列名）
- `src-tauri/src/core/driver/native/sqlite.rs` — 实现 `list_indexes`（`PRAGMA index_list` + `PRAGMA index_info`）
- `src/extensions/builtin/connection/ui/composables/useAddDataSource.ts` — 修复 `null` → `undefined` 类型错误（第 440 行）

### 审计验证（V10.3 无代码变更，仅验证通过）

- `src-tauri/migrations/global/001-016` — 25 列 global_connections 完整对齐 ✅
- `src-tauri/migrations/project_meta/001-015` — 25 列 connections 完整对齐 ✅
- `src-tauri/migrations/connection_metadata/001-010` — V10 企业级统计字段完整 ✅
- `src-tauri/src/core/persistence/global_db.rs` — GlobalConnectionInfo 25 字段 ↔ 25 列 ✅
- `src-tauri/src/core/persistence/project_connection_store.rs` — ProjectConnection 25 字段 ↔ 25 列 ✅
- `src-tauri/src/core/persistence/auth_store.rs` — AuthConfig 9 字段，全局/项目 INSERT 列数一致 ✅
- `src-tauri/src/core/persistence/network_store.rs` — NetworkConfig 10 字段，全局/项目 INSERT 列数一致 ✅

### 11. 缓存预热 + 导航树统计增强（V10.4）

- **P1-1 缓存预热实际化**：
  - `start_cache_warming` Tauri 命令从空壳变为真实后台任务：接受 `AppState` + `AppHandle`，通过 `WarmingTaskManager` 创建任务并 `tokio::spawn` 后台执行
  - 按 `IntrospectionLevel` 分级加载：Level 1 仅加载 Schema，Level 2 加载表，Level 3 加载列
  - 进度实时推送：通过 `warming_manager.update_progress()` + `app_handle.emit()` 将进度推送到前端
  - 支持取消：`cancel_token.is_cancelled()` 检查后立即退出后台循环
  - `WarmCacheInput` 扩展：新增 `source_connection_id: String` 和 `introspection_level: Option<i32>` 字段

- **P1-2 表节点 Tooltip 统计**：
  - `virtual-tree-node.vue` tooltip 新增 `table` 类型：显示行数（formatNumber）、数据大小（formatBytes）、索引大小（formatBytes）
  - 数据来源：`ITreeNodeData.rowCount` / `dataLength` / `indexLength`，由 `createTableNodes` 从 `TableNode` 传入
  - `TableNode` 扩展：新增 `rowCount?: number | null` / `dataLength?: number | null` / `indexLength?: number | null` 字段
  - 缓存路径 `loadTablesFromCache`：从 `TableMeta.rowCountEstimate` / `dataLength` / `indexLength` 映射到 `TableNode`

- **P2 Schema 节点统计摘要**：
  - `virtual-tree-node.vue` tooltip 新增 `schema` 类型：显示表数量、视图数量、总大小、总行数
  - `computeSchemaStats(schema: SchemaNode)` 新增辅助函数：从 `schema.tables` 计算 `totalTables`/`totalViews`/`totalSizeBytes`/`rowCountTotal`
  - `updateSchemaTables` 在写入表后自动调用 `computeSchemaStats`，保证 Schema 统计随数据实时更新
  - `createSchemaNodes` 将 `SchemaNode.totalTables` / `totalViews` / `totalSizeBytes` / `rowCountTotal` 传递到 `ITreeNodeData`

- **辅助函数**：`formatNumber`（千/百万/十亿缩写）和 `formatBytes`（B/KB/MB/GB）内置于 `virtual-tree-node.vue`

- **修改文件**：
  - `src-tauri/src/commands/cache_warming_commands.rs` — start_cache_warming 真实后台任务实现
  - `src/extensions/builtin/database/ui/components/virtual-tree-node.vue` — table/schema tooltip + formatNumber/formatBytes
  - `src/extensions/builtin/database/ui/composables/use-database-tree-loader.ts` — createSchemaNodes 传递 schema 统计数据
  - `src/extensions/builtin/database/ui/stores/database-navigator-store.ts` — computeSchemaStats + updateSchemaTables 自动统计
  - `src/extensions/builtin/database/ui/types/virtual-tree.ts` — ITreeNodeData 扩展 7 个统计字段

- **Quality Gates**：`cargo clippy -- -D warnings`（0 errors）、`cargo fmt`（passed）、`pnpm typecheck`（0 errors）

### 12. 数据导航栏全链路修复（V10.5）

**审计发现**：6 层全栈审计（Driver → Cache → Tauri Command → Frontend API → Store → Tree Loader → UI）共发现 10 项遗漏。

#### P0 — 关键 Bug 修复

- **P0-1: `loadProcedures` / `loadFunctions` 传参错误**：
  - 缺陷：Store 将 `connectionDbTypes.value.get()` 返回的驱动类型名（如 "postgresql"）错误地当作 `catalogName` 传给后端
  - 修复：`database-navigator-store.ts` 中 `loadProcedures` 和 `loadFunctions` 改为直接传递 `catalogName` 参数
  - 影响：修复后 PostgreSQL/MySQL 的存储过程和函数列表能正常加载

- **P0-2: 视图列无法加载**：
  - 缺陷①：`view` 节点展开时未调用 `loadColumns`，直接跳到 `createColumnNodes`
  - 缺陷②：`createColumnNodes` 只查 `getSchemaTables`，不查 `getSchemaViews`，永远找不到视图
  - 修复：`use-database-tree-loader.ts` 中添加 `await navigatorStore.loadColumns(...)` 调用，且 `createColumnNodes` 增加了 `getSchemaViews` 回退查找
  - 影响：修复后视图展开能正常显示列列表

- **P0-3: `load_sequences` / `load_triggers` 无 L1 缓存**：
  - `metadata_cache.rs` 新增 `Sequences` / `Triggers` key 变体、getter/setter 方法
  - `metadata_commands.rs` 中 `load_sequences` / `load_triggers` 添加完整 `check_l1_cache` + `write_l1_cache` 缓存链路
  - `estimated_memory_usage()` 匹配新增的 key 变体

#### P1 — 数据完整性与架构修复

- **P1-1: MetadataService 新增 MetadataBrowser 回退路径**：
  - `list_sequences` / `list_triggers` 增加 `as_metadata_browser()` 优先路径（NodeInfo → SchemaObject 转换）
  - 与 `list_tables` / `list_indexes` / `list_constraints` 保持一致的双路径架构

- **P1-2: `load_procedures` / `load_functions` 新增 `connection_type`/`project_path` 参数**：
  - Rust 侧：4 个命令签名统一添加 `connection_type: Option<String>, project_path: Option<String>`
  - 参数暂未用于 L2 缓存（需后续实现 L2 procedures/functions 表），但为 Specta 类型生成和 L2 打通预留

- **P1-3: 前端 `loadSequences`/`loadTriggers` 连接参数链路**：
  - API 层 `loadSequences`/`loadTriggers` 已支持 `connectionType`/`projectPath` 参数（raw invoke）
  - Store 层 `loadSequences`/`loadTriggers` 已从 `connectionTypes`/`connectionProjectPaths` 中获取并传递

- **P1-6: 默认 NavigationConfig 开放**：
  - 文件夹默认全开：`functions`/`procedures`/`sequences`/`triggers` 由 `enabled: false` → `enabled: true`
  - 表子文件夹默认开：`indexes`/`constraints` 由 `false` → `true`
  - 影响：PostgreSQL 连接可见全部对象文件夹，表节点下 Indexes/Constraints 可展开

#### P2 — 体验改善

- **P2-2: 视图节点 Tooltip**：
  - `virtual-tree-node.vue` 新增 `view` 类型 tooltip（"类型: 视图" + 行数/数据大小，当数据可用时）

- **修改文件**：
  - `src-tauri/src/core/cache/metadata_cache.rs` — 新增 Sequences/Triggers key + getter/setter
  - `src-tauri/src/core/services/metadata_service.rs` — list_sequences/list_triggers 增加 MetadataBrowser 回退
  - `src-tauri/src/commands/metadata_commands.rs` — 4 个命令添加 connection_type/project_path 参数 + L1 缓存链路
  - `src/extensions/builtin/database/ui/stores/database-navigator-store.ts` — P0-1 传参修复
  - `src/extensions/builtin/database/ui/composables/use-database-tree-loader.ts` — P0-2 视图列加载修复
  - `src/extensions/builtin/database/ui/components/virtual-tree-node.vue` — P2-2 视图 tooltip
  - `src/extensions/builtin/connection/ui/utils/schema-loader.ts` — P1-6 默认导航配置开放

- **Quality Gates**：`cargo clippy -- -D warnings`（0 errors）、`cargo fmt`（passed）、`pnpm typecheck`（0 errors）

### 13. Specta 类型迁移完成（V10.5.1）

完成 Specta 类型绑定迁移，将 V10.5 中遗留的手动类型定义和 raw invoke 全部迁移到类型安全的 Specta typed commands。

#### Specta bindings 手动同步

> **背景**：由于 Specta 代码生成步骤（`cargo build`）在开发流程中未稳定执行，V10.5.1 采用手动同步策略：先修改 Rust 命令签名和类型定义，再手动同步 `bindings.ts` 以确保前后端类型一致。

- **Rust 侧新增类型**（`metadata_commands.rs`）：
  - `SequenceMeta { name: String }` — 序列元数据，标记 `#[derive(Type)]` + `#[specta::specta]`
  - `TriggerMeta { name, table_name: Option<String>, event: Option<String> }` — 触发器元数据，`serde(rename = "tableName"/"event")` 确保 camelCase 序列化

- **Rust 侧命令签名更新**（4 个命令）：
  - `load_sequences` / `load_triggers`：新增 `connection_type: Option<String>, project_path: Option<String>` 参数
  - `load_procedures` / `load_functions`：同上，新增连接参数
  - 参数暂不用于 L2 缓存查询，但为 L2 打通和 Specta 类型生成预留完整签名

- **`bindings.ts` 手动同步**：
  - `commands` 对象新增 `loadSequences` / `loadTriggers`（含 `connectionType`/`projectPath` 参数）
  - `commands.loadProcedures` / `commands.loadFunctions` 签名更新：增加 `connectionType`/`projectPath` 参数
  - 新增导出类型：`SequenceMeta { name }` / `TriggerMeta { name, tableName: string | null, event: string | null }`

#### 前端 API 层迁移

- **`database-api.ts` 迁移到 Specta typed commands**：
  - 移除手动定义的 `SequenceMeta` / `TriggerMeta` 接口
  - 改为从 `@/generated/specta/bindings` 统一导入所有元数据类型
  - `loadProcedures` / `loadFunctions` / `loadSequences` / `loadTriggers` 全部使用 `typed(commands.xxx(...))` 调用
  - 所有方法新增 `connectionType?: string, projectPath?: string` 可选参数，以 `?? null` 传递

- **`database-navigator-store.ts` 类型适配**：
  - `loadProcedures` / `loadFunctions` 调用传递 `connType` / `projectPath`
  - `loadSequences` / `loadTriggers` 调用传递 `connType` / `projectPath`
  - `TriggerMeta` 类型转换：`tableName: t.tableName ?? undefined` / `event: t.event ?? undefined`（`string | null` → `string | undefined`）

#### 验证结果

- `pnpm run typecheck`（vue-tsc --noEmit）：✅ 0 errors
- `cargo check`：✅ 0 errors（`Finished dev profile`）
- `cargo clippy -- -D warnings`：✅ 0 errors
- `cargo fmt`：✅ passed

#### 仍待完成

- **Specta 绑定的 Rust 源码级重新生成**：✅ **已完成**（V10.5.3）。`cargo build`（debug 模式）已自动重新生成 `bindings.ts`，输出与手动版本一致，确认 `SequenceMeta`/`TriggerMeta` 类型和 4 个命令的 `connectionType`/`projectPath` 参数均已正确导出。

- **修改文件（V10.5.1）**：
  - `src/generated/specta/bindings.ts` — 手动同步 SequenceMeta/TriggerMeta 类型和 4 个命令签名
  - `src/extensions/builtin/database/ui/api/database-api.ts` — 迁移到 Specta typed commands，移除手动类型
  - `src/extensions/builtin/database/ui/stores/database-navigator-store.ts` — 类型适配（null → undefined）

### 14. L2 缓存补齐 + 计数器统一（V10.5.2）

V10.5 遗留审计发现：`load_sequences` / `load_triggers` / `load_procedures` / `load_functions` 四个命令缺少 L2（SQLite）缓存回退路径，而 databases/schemas/tables/columns 均有完整 L2 链路。

#### L2 缓存补齐

- **4 个 `try_l2_*` 辅助函数**（[metadata_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/metadata_commands.rs)）：
  - `try_l2_procedures` — 查询 L2 `routines` 表，`routine_type = 'PROCEDURE'`，JOIN `schemata` 按 `catalog_name + schema_name` 过滤
  - `try_l2_functions` — 同上，`routine_type = 'FUNCTION'`
  - `try_l2_sequences` — 查询 L2 `sequences` 表，JOIN `schemata` 过滤
  - `try_l2_triggers` — 查询 L2 `triggers` 表，JOIN `tables` + `schemata`，返回 `(trigger_name, table_name, trigger_event)` 三列
  - 所有函数遵循 `try_l2_tables` / `try_l2_columns` 模式：`open_l2_cache` → `get_connection()` → raw SQL → 空结果返回 `Ok(None)`

- **4 个 `load_*` 命令接入 L2**：
  - 移除 `let _ = (connection_type, project_path);` 占位代码
  - 改为 `let ct = connection_type.as_deref().unwrap_or("global")` + `let pp = project_path.as_deref()`
  - L2 命中 → `L2_HIT_COUNT.fetch_add(1)` + 直接返回（L2 已有数据，不重复写 L1）
  - L2 未命中 → `L2_MISS_COUNT.fetch_add(1)` + 回退到 MetadataService 直接查询
  - 缓存链路完整：L1 → L2 → DB Query → write L1

#### 原子计数器统一

4 个 `load_*` 命令的 L1 命中路径补充 `L1_HIT_COUNT.fetch_add(1, Ordering::Relaxed)`（之前缺失），与 `load_databases` / `load_schemas` / `load_tables` / `load_columns` 保持一致的统计口径。

#### 前端类型断言说明（P2）

`database-api.ts` 中的 `as unknown as ICatalogMeta[]` 等类型断言是 Specta 生成类型 ↔ 自定义树形接口之间的必要桥接。自定义接口（`ICatalogMeta` / `ISchemaMeta` / `ITableMeta` / `IViewMeta` / `IColumnMeta`）扩展了 Specta 类型以支持树形导航的嵌套结构（`schemas?` / `tables?` / `views?` / `columns?` 可选字段），这些字段在 API 层返回时为 `undefined`，由 Store/Tree Loader 层按需填充。断言安全且无运行时开销。

#### 验证结果

- `cargo check`：✅ 0 errors
- `cargo clippy -- -D warnings`：✅ 0 warnings
- `cargo fmt`：✅ passed
- `pnpm typecheck`：✅ 0 errors

- **修改文件（V10.5.2）**：
  - `src-tauri/src/commands/metadata_commands.rs` — 新增 4 个 `try_l2_*` 函数 + 4 个 `load_*` 命令接入 L2 + L1 计数器补充
  - `docs/metadata-cache-implementation.md` — 本文档

---

**最后更新**: 2026-05-30 (V10.5.2 L2 缓存补齐 + V10.5.3 Specta 自动生成确认)

## V10.6 前端 Store 重构（2026-06-09）

### 重构动机

数据库导航栏 Store（`database-navigator-store.ts`）经过多次迭代膨胀至 1267 行，存在以下问题：

1. **代码重复**：8 处 `catalogs.find → schemas.find → mutate` 遍历模式
2. **加载逻辑重复**：`loadCatalogs`/`loadSchemas`/`loadTables`/`loadColumns` 中相同的「查缓存→从缓存加载→从DB加载」三段式
3. **状态共享**：procedures/functions/sequences/triggers 共享 `loadingTables`，互斥阻塞
4. **类型冲突**：子模块各自定义本地类型，与 `tree-mutation.ts` / `nav-types.ts` 不一致

### 重构方案

| 组件                          | 职责                                                                      | 行数 |
| ----------------------------- | ------------------------------------------------------------------------- | ---- |
| `use-catalog-loader.ts`       | catalogs + schemas 加载、缓存检查、从 DB 同步                             | ~220 |
| `use-table-loader.ts`         | tables + views 加载（含视图与表合并）、Schema 统计计算                    | ~260 |
| `use-object-loader.ts`        | procedures / functions / sequences / triggers 加载，各含独立 loading 状态 | ~250 |
| `use-column-loader.ts`        | columns + indexes + constraints 加载                                      | ~300 |
| `database-navigator-store.ts` | 状态聚合、对外 API 委派                                                   | ~600 |
| `tree-mutation.ts`            | 通用树遍历与变更工具                                                      | ~136 |
| `lazy-loader.ts`              | 通用懒加载工厂                                                            | ~70  |
| `nav-types.ts`                | 共享类型定义（单源真）                                                    | ~109 |

### 关键优化

- **Loading 状态独立化**：`useObjectLoader` 创建 4 个独立 `Ref<Set<string>>`，展开 Procedures 不会阻塞 Functions
- **Per-node 错误追踪**：`nodeErrors: Map<string, string>` 替代单一 `error` 字符串，每个节点独立显示错误状态
- **LazyLoader 工厂**：`createLazyLoader<T>()` 封装「L2 缓存有效 → 返回缓存 / L2 缓存无效 → 调用 API → 写入 Store」通用模式
- **treeMutation 工具**：`mutateTreeNode(catalogs, connId, { catalogName, schemaName }, updater)` 一行替代 10 行手动遍历
- **类型统一**：所有子 loader / tree loader / store 统一引用 `nav-types.ts`，消除类型冲突
