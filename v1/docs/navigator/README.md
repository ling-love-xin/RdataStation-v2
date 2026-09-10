# IVM 数据库导航栏设计文档

> 版本：v2.5
> 最后更新：2026-06-09
> 状态：✅ 持续更新

---

## 文档索引

| 文档                                                   | 说明                    | 状态 |
| ------------------------------------------------------ | ----------------------- | ---- |
| [01-ARCHITECTURE.md](./01-ARCHITECTURE.md)             | IVM 架构设计            | ✅   |
| [02-DATAFLOW.md](./02-DATAFLOW.md)                     | 数据流设计              | ✅   |
| [03-INTERFACES.md](./03-INTERFACES.md)                 | 接口规范                | ✅   |
| [04-IMPLEMENTATION.md](./04-IMPLEMENTATION.md)         | 实施步骤                | ✅   |
| [05-OPTIMIZATION.md](./05-OPTIMIZATION.md)             | 优化策略                | ✅   |
| [06-CACHE-OPTIMIZATION.md](./06-CACHE-OPTIMIZATION.md) | 缓存优化（V7 增量同步） | ✅   |

### 模块文档

| 文档                                                                         | 说明                   |
| ---------------------------------------------------------------------------- | ---------------------- |
| [database-navigator.md](./database-navigator.md)                             | 数据库导航模块完整实现 |
| [database-navigator-optimizations.md](./database-navigator-optimizations.md) | 导航栏优化更新         |
| [frontend-backend-alignment.md](./frontend-backend-alignment.md)             | 前后端对齐与职责划分   |

### 外部

| 文档                              | 说明         |
| --------------------------------- | ------------ |
| [COMPARISON.md](../COMPARISON.md) | 竞品对比分析 |

---

## 已知问题

### ✅ 已修复（2026-05-25）

| 问题                               | 修复内容                                                                                              | 涉及文件                                                                                                                                                                                                                                                              |
| ---------------------------------- | ----------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Indexes/Constraints 后端 Stub**  | `load_indexes` / `load_constraints` 从空数组改为真实 DB 查询 + L1 缓存                                | [metadata_commands.rs](../../src-tauri/src/commands/metadata_commands.rs), [traits.rs](../../src-tauri/src/core/driver/traits.rs), [mysql.rs](../../src-tauri/src/core/driver/native/mysql.rs), [metadata_cache.rs](../../src-tauri/src/core/cache/metadata_cache.rs) |
| **10 个右键菜单/事件处理器为空壳** | 实现全部处理器：复制名称、打开表/视图、展开/折叠全部、刷新 Schema/Database、创建对象、打开 SQL 编辑器 | [database-navigator.vue](../../src/extensions/builtin/database/ui/components/database-navigator.vue)                                                                                                                                                                  |
| **遗留双轨加载代码**               | 标记 `navigator-loader.ts` 为 @deprecated，指向当前主流程                                             | [navigator-loader.ts](../../src/extensions/builtin/database/domain/services/navigator-loader.ts)                                                                                                                                                                      |
| **树展开状态不持久化**             | 新增 `navigator-persistence.ts`，基于 localStorage，支持 global/project 双链路，800ms 防抖保存        | [navigator-persistence.ts](../../src/extensions/builtin/database/ui/utils/navigator-persistence.ts), [database-navigator.vue](../../src/extensions/builtin/database/ui/components/database-navigator.vue)                                                             |

### ✅ V10.6 Store 重构（2026-06-09）

| 问题                     | 修复内容                                                                                                                               | 涉及文件                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| ------------------------ | -------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Store 1267行代码冗余** | 拆分为 4 个子 loader：`useCatalogLoader` / `useTableLoader` / `useObjectLoader` / `useColumnLoader`，每种 loader 独立管理 loading 状态 | [nav-loaders/](../../src/extensions/builtin/database/ui/stores/nav-loaders/)                                                                                                                                                                                                                                                                                                                                                                                |
| **8 处重复树遍历模式**   | 新增 `tree-mutation.ts` 工具函数 `mutateTreeNode` / `getTreeNode`，统一 `catalogs.find → schemas.find → mutate` 遍历                   | [tree-mutation.ts](../../src/extensions/builtin/database/ui/utils/tree-mutation.ts)                                                                                                                                                                                                                                                                                                                                                                         |
| **加载代码三段式重复**   | 新增 `lazy-loader.ts` 通用工厂 `createLazyLoader`，封装「检查缓存 → 从缓存加载 → 从 DB 加载」策略                                      | [lazy-loader.ts](../../src/extensions/builtin/database/ui/utils/lazy-loader.ts)                                                                                                                                                                                                                                                                                                                                                                             |
| **loadingTables 被共享** | `useObjectLoader` 为 procedures / functions / sequences / triggers 各建独立 `Ref<Set<string>>` 加载状态                                | [use-object-loader.ts](../../src/extensions/builtin/database/ui/stores/nav-loaders/use-object-loader.ts)                                                                                                                                                                                                                                                                                                                                                    |
| **全局错误覆盖**         | 新增 `nodeErrors: Map<string, string>`，子 loader 按节点 key 写入独立错误，暴露 `getNodeError`/`setNodeError` API                      | [database-navigator-store.ts](../../src/extensions/builtin/database/ui/stores/database-navigator-store.ts)                                                                                                                                                                                                                                                                                                                                                  |
| **`loadViews` 冗余包装** | 删除 `loadViews` 函数（仅是 `loadTables` 空包装），3 个调用方全部改用 `loadTables`                                                     | [database-navigator-store.ts](../../src/extensions/builtin/database/ui/stores/database-navigator-store.ts), [use-database-tree-loader.ts](../../src/extensions/builtin/database/ui/composables/use-database-tree-loader.ts), [use-context-menu-actions.ts](../../src/extensions/builtin/database/ui/composables/use-context-menu-actions.ts), [use-incremental-refresh.ts](../../src/extensions/builtin/database/ui/composables/use-incremental-refresh.ts) |
| **类型定义不一致**       | 统一所有子 loader / tree loader / store 的类型导入源为 `nav-types.ts`，消除本地类型与共享类型冲突                                      | [nav-types.ts](../../src/extensions/builtin/database/ui/types/nav-types.ts), all nav-loaders                                                                                                                                                                                                                                                                                                                                                                |

### ✅ V10.8 深度审计修复（2026-06-09）

| 问题                                    | 修复内容                                                                                    | 涉及文件                                                                                                        |
| --------------------------------------- | ------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| **loadChildren catch 静默吞异常**       | catch 中设置 nodeError + 返回 `⚠ 加载失败：{msg}` 错误占位节点，用户可见可重试              | [use-database-tree-loader.ts](../../src/extensions/builtin/database/ui/composables/use-database-tree-loader.ts) |
| **debouncedPersistSave timer 泄漏**     | `onUnmounted` 添加 `clearTimeout(persistTimer)`                                             | [database-navigator.vue](../../src/extensions/builtin/database/ui/components/database-navigator.vue)            |
| **connection 节点取错 connectionId**    | connection 节点从 `keyParts[2]` 取 connId（非 `keyParts[1]`），`node.data.driver` 取 dbType | [use-database-tree-loader.ts](../../src/extensions/builtin/database/ui/composables/use-database-tree-loader.ts) |
| **tables/views-folder 重复 loadTables** | Schema 已加载表数据时，展开 folder 直接走 `create*Nodes()`，跳过 API 调用                   | [use-database-tree-loader.ts](../../src/extensions/builtin/database/ui/composables/use-database-tree-loader.ts) |

### ✅ V10.7 Bug 修复（2026-06-09）

| 问题                                | 修复内容                                                                                      | 涉及文件                                                                                                                                                                                                  |
| ----------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Schema 节点 loadTables 重复调用** | 删除 `Promise.all` 双调 `loadTables`，改为单次 `await`（节省一次 RTT）                        | [use-database-tree-loader.ts](../../src/extensions/builtin/database/ui/composables/use-database-tree-loader.ts:872)                                                                                       |
| **树展开状态不跨会话持久化**        | 新增 `saveLastActiveConnection` / `getLastActiveConnection`，`onMounted` 自动恢复上次活跃连接 | [navigator-persistence.ts](../../src/extensions/builtin/database/ui/utils/navigator-persistence.ts), [database-navigator.vue](../../src/extensions/builtin/database/ui/components/database-navigator.vue) |
| **搜索无防抖，逐字触发全树扫描**    | `onSearchQueryChange` 用 `debounceAsync` 包装，300ms 防抖                                     | [database-navigator.vue](../../src/extensions/builtin/database/ui/components/database-navigator.vue:394), [debounce.ts](../../src/extensions/builtin/database/ui/utils/debounce.ts)                       |

### 历史问题（已修复）

#### 数据库表列表不显示

**根因分析**（已修复）：前端 `database-api.ts` 调用 `invoke('load_tables', ...)` 加载表列表，但该 Tauri 命令在 Rust 后端缺失（未实现）。

**第1轮修复（2026-04-23）**：在 `use-database-tree-loader.ts` 的 `loadChildren` 中，MySQL 无 Schema 时用 `dbName` 替代 `schemaName`。

**第2轮修复（2026-04-23）**：`execute_sql` 返回 `unknown[][]`（数组的数组），修复 `loadTablesFromDb`、`loadColumnsFromDb` 共 4 处 array-vs-object 映射错误。

**第3轮修复（2026-04-23）**：MySQL 无 Schema 时 `updateSchemaTables` 崩溃。修复 `DatabaseNode` 增加 `tables` 字段，增加 `db.tables` 回退。

### SQL 编辑器补全报错

**根因**：`sql-editor-service.ts` 调用不存在的 `invoke('get_tables')` 和 `invoke('get_columns')`。

**第1轮修复（2026-04-23）**：改用 `invoke('execute_sql', ...)` 查询 `information_schema.tables`。

**第2轮修复（2026-04-23）**：SQLite 无 `information_schema`，新增 `dbType` 参数使用 `sqlite_master` + `PRAGMA table_info`。

---

## 概述

基于 IVM（增量视图维护）的数据库导航栏设计方案，打造高性能、实时响应、资源友好的桌面级数据库导航体验。

## 核心特性

- **增量更新**：只更新变更部分，而非全量刷新
- **虚拟滚动**：只渲染视口内节点，支持百万级数据
- **实时同步**：WebSocket 推送元数据变更
- **离线优先**：本地物化视图支持断网浏览
- **智能缓存**：三级缓存架构（内存/IndexedDB/SQLite）

## 技术栈

- **前端**：Vue 3 + TypeScript + Pinia
- **桌面**：Tauri + Rust
- **缓存**：SQLite（本地）+ IndexedDB（浏览器）

## 架构概览

```
┌─────────────────────────────────────────────────────────────────┐
│  Presentation Layer (Vue Components)                            │
│  ├── NavigatorTree.vue     导航树组件                           │
│  ├── NavigatorNode.vue     节点组件                             │
│  ├── ConnectionCard.vue    连接卡片                             │
│  └── SearchPanel.vue       搜索面板                             │
├─────────────────────────────────────────────────────────────────┤
│  View Layer (Incremental Views)                                 │
│  ├── ViewportView          视口视图                             │
│  ├── FilteredView          筛选视图                             │
│  ├── SortedView            排序视图                             │
│  └── AggregatedView        聚合视图                             │
├─────────────────────────────────────────────────────────────────┤
│  Engine Layer (IVM Core)                                        │
│  ├── ViewEngine            视图引擎                             │
│  ├── DeltaProcessor        增量处理器                           │
│  ├── ChangePropagator      变更传播器                           │
│  └── QueryOptimizer        查询优化器                           │
├─────────────────────────────────────────────────────────────────┤
│  Cache Layer (Multi-Level)                                      │
│  ├── L1 Cache (Memory)     内存缓存                             │
│  ├── L2 Cache (IndexedDB)  本地持久化                           │
│  └── L3 Cache (SQLite)     系统级缓存                           │
├─────────────────────────────────────────────────────────────────┤
│  Source Layer (Data Sources)                                    │
│  ├── WebSocket Source      实时数据源                           │
│  ├── HTTP Source           请求数据源                           │
│  └── Local Source          本地数据源                           │
└─────────────────────────────────────────────────────────────────┘
```

## 性能目标

| 指标     | 目标    | 说明           |
| -------- | ------- | -------------- |
| 初始加载 | < 100ms | 从本地缓存加载 |
| 滚动性能 | 60fps   | 虚拟滚动       |
| 搜索响应 | < 50ms  | 增量索引       |
| 内存占用 | < 50MB  | 视口内数据     |
| 实时延迟 | < 100ms | WebSocket 推送 |

## 相关资源

## 导航栏持久化架构（v2.1 新增）

### 存储选型

| 数据类型                       | 存储方案          | 理由                                     |
| ------------------------------ | ----------------- | ---------------------------------------- |
| **元数据**（表/列/索引/约束）  | SQLite（L2 缓存） | 结构化查询、事务安全、与现有缓存体系共用 |
| **树展开状态**（expandedKeys） | localStorage      | 数据量小 <10KB、同步读写、无需额外依赖   |

### 双链路设计

```
Global 连接
  → localStorage key: "rds:navigator:global:{connId}"
  → 作用域：所有项目共享

Project 连接
  → localStorage key: "rds:navigator:project:{base64(projectPath)}:{connId}"
  → 作用域：仅当前项目内有效
```

### 持久化时机

| 事件                        | 动作                                    | 防抖  |
| --------------------------- | --------------------------------------- | ----- |
| 用户展开/折叠节点           | `debouncedPersistSave(connId)`          | 800ms |
| 组件卸载 (onUnmounted)      | `saveAllNavigatorStates()`              | 同步  |
| 刷新连接 (Refresh Database) | `clearConnectionNavigatorState(connId)` | 即时  |

### 数据结构

```typescript
interface NavigatorStateEntry {
  expandedKeys: string[] // 已展开的节点 key 列表
  selectedKey: string | null // 当前选中节点
  filterText: string // 搜索框文本
  lastUpdated: number // 时间戳
  version: number // 格式版本号（当前=1）
}
```

### 文件清单

| 文件                                                                                                 | 职责                                                                   |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| [navigator-persistence.ts](../../src/extensions/builtin/database/ui/utils/navigator-persistence.ts)  | 持久化读写 API（localStorage）                                         |
| [database-navigator.vue](../../src/extensions/builtin/database/ui/components/database-navigator.vue) | 集成：restore on mount, debounced save on change, sync save on unmount |

- [前端架构](../frontend/ARCHITECTURE.md)
- [架构总览](../architecture/overview.md)

---

## 版本历史

| 版本     | 日期       | 说明                                                                                                                                                    |
| -------- | ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **v2.5** | 2026-06-09 | **V10.8: loadChildren 异常不再静默吞噬(错误占位节点) + persistTimer cleanup + connectionId 修正 + tables/views-folder 跳过重复请求**                    |
| **v2.4** | 2026-06-09 | **V10.7: 修复 schema loadTables 重复调用 + lastActive 跨会话恢复 + 搜索 300ms debounceAsync 防抖**                                                      |
| **v2.3** | 2026-06-09 | **V10.6: Store 拆分(1267→4子loader)、LazyLoader 通用工厂、独立 loading 状态、nodeErrors 按节点追踪、tree-mutation 工具函数、nav-types 统一类型**        |
| **v2.2** | 2026-05-25 | **R5: 四驱动列元数据(extra HashMap)全覆盖; L2 indexes/fks 回写主流程接入; 前端 Tooltip/属性面板消费索引约束新字段; SQL迁移009 JDBC对齐; 打通率30%→82%** |
| v2.1     | 2026-05-25 | 导航栏持久化架构（localStorage 双链路设计）                                                                                                             |
| v2.0     | 2026-05-20 | IVM 架构重构，增量视图维护                                                                                                                              |

---

## 完整字段映射表

### SchemaObject 三层映射

| Rust traits.rs                        | serde JSON | TS navigator.ts             | L2 SQLite 表                                           |
| ------------------------------------- | ---------- | --------------------------- | ------------------------------------------------------ |
| `name: String`                        | `name`     | `name: string`              | `schema_name` / `table_name` / `column_name`           |
| `kind: SchemaObjectKind`              | `kind`     | `type: string`              | `table_type` / `routine_type` / ...                    |
| `children: Option<Vec<SchemaObject>>` | `children` | `children?: SchemaObject[]` | N/A (独立查询)                                         |
| `comment: Option<String>`             | `comment`  | `comment?: string`          | `table_comment` / `column_comment` / `routine_comment` |

### ColumnDetail → ColumnMeta → ColumnInfo 完整映射

| Rust (traits.rs)                     | serde JSON       | TypeScript (TS)                  | L2 SQLite (columns) | Optional |
| ------------------------------------ | ---------------- | -------------------------------- | ------------------- | -------- |
| `name: String`                       | `name`           | `name: string`                   | `column_name`       | ❌       |
| `data_type: String`                  | `data_type`      | `dataType: string`               | `data_type`         | ❌       |
| `nullable: bool`                     | `nullable`       | `nullable: boolean`              | `is_nullable`       | ❌       |
| `is_primary_key: bool`               | `is_primary_key` | `isPrimaryKey: boolean`          | `is_primary`        | ❌       |
| `is_foreign_key: bool`               | `is_foreign_key` | `isForeignKey: boolean`          | FK join             | ❌       |
| `default_value: Option<String>`      | `default_value`  | `defaultValue?: string`          | `column_default`    | ✅       |
| `comment: Option<String>`            | `comment`        | `comment?: string`               | `column_comment`    | ✅       |
| **`extra: HashMap<String, String>`** | `extra`          | `extra?: Record<string, string>` | `extra` (JSON TEXT) | ✅       |

**extra 字段典型内容（驱动相关）：**

| 键名                | MySQL             | PostgreSQL                  | SQLite | DuckDB |
| ------------------- | ----------------- | --------------------------- | ------ | ------ |
| `char_max_length`   | ✅ VARCHAR 长度   | ✅ character_maximum_length | ✅     | ✅     |
| `numeric_precision` | ✅ INT 精度       | ✅                          | ✅     | ✅     |
| `numeric_scale`     | ✅ DECIMAL 小数位 | ✅                          | ✅     | ✅     |
| `auto_increment`    | ✅ AUTO_INCREMENT | ✅ SERIAL                   | ❌     | ❌     |
| `unsigned`          | ✅ UNSIGNED       | ❌                          | ❌     | ❌     |
| `on_update`         | ✅ ON UPDATE      | ❌                          | ❌     | ❌     |

### IndexDetail → IndexMeta → IndexInfo 完整映射

| Rust (traits.rs)             | serde JSON     | TypeScript (TS)         | L2 SQLite (indexes) | Optional |
| ---------------------------- | -------------- | ----------------------- | ------------------- | -------- |
| `name: String`               | `name`         | `name: string`          | `index_name`        | ❌       |
| `table_name: String`         | `table_name`   | `tableName: string`     | JOIN tables         | ❌       |
| `column_names: Vec<String>`  | `column_names` | `columnNames: string[]` | JOIN index_columns  | ❌       |
| `is_unique: bool`            | `is_unique`    | `isUnique: boolean`     | `is_unique`         | ❌       |
| `is_primary: bool`           | `is_primary`   | `isPrimary: boolean`    | `is_primary`        | ❌       |
| `index_type: Option<String>` | `index_type`   | `indexType?: string`    | `index_type`        | ✅       |
| `comment: Option<String>`    | `comment`      | `comment?: string`      | `index_comment`     | ✅       |

### ConstraintDetail → ConstraintMeta → ConstraintInfo 完整映射

| Rust (traits.rs)                   | serde JSON           | TypeScript (TS)               | L2 SQLite (foreign_keys/check_constraints) | Optional   |
| ---------------------------------- | -------------------- | ----------------------------- | ------------------------------------------ | ---------- |
| `name: String`                     | `name`               | `name: string`                | `constraint_name`                          | ❌         |
| `table_name: String`               | `table_name`         | `tableName: string`           | JOIN tables                                | ❌         |
| `constraint_type: String`          | `constraint_type`    | `constraintType: string`      | 表类型区分                                 | ❌         |
| `column_names: Vec<String>`        | `column_names`       | `columnNames: string[]`       | JOIN fk_columns                            | ❌         |
| `referenced_table: Option<String>` | `referenced_table`   | `referencedTable?: string`    | `ref_table_id` → JOIN                      | ✅ (仅 FK) |
| `referenced_columns: Vec<String>`  | `referenced_columns` | `referencedColumns: string[]` | `ref_column_name`                          | ✅ (仅 FK) |
| `update_rule: Option<String>`      | `update_rule`        | `updateRule?: string`         | `update_rule`                              | ✅ (仅 FK) |
| `delete_rule: Option<String>`      | `delete_rule`        | `deleteRule?: string`         | `delete_rule`                              | ✅ (仅 FK) |

---

## L2 缓存架构

### 两级缓存体系

```
┌─────────────────────────────────────────────────────────────────┐
│                      L1 内存缓存                                 │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ MetadataCache (in-memory HashMap)                       │   │
│  │ - key: "{catalog}/{schema}/{table}"                     │   │
│  │ - value: NodeDetail / Vec<ColumnDetail> / ...          │   │
│  │ - TTL-based eviction (可配置)                           │   │
│  │ - 命中延迟: < 0.1ms                                     │   │
│  └─────────────────────────────────────────────────────────┘   │
├─────────────────────────────────────────────────────────────────┤
│                      L2 磁盘缓存 (SQLite)                       │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ Per-Connection SQLite DB                                │   │
│  │ - 路径: {data_dir}/global_metadata/conn_{id}.sqlite     │   │
│  │       或 {project}/meta/connection_metadata/conn_{id}.sqlite │
│  │ - WAL 模式 + mmap_size=256MB                            │   │
│  │ - 命中延迟: < 5ms                                       │   │
│  └─────────────────────────────────────────────────────────┘   │
├─────────────────────────────────────────────────────────────────┤
│                      L3 数据源 (DB)                             │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ MySQL / PostgreSQL / SQLite / DuckDB                    │   │
│  │ - information_schema / sqlite_master / pg_catalog       │   │
│  │ - 命中延迟: 10-500ms (取决于网络和查询复杂度)            │   │
│  └─────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

### 读取优先级

```
请求元数据
    ↓
1. 查询 L1 内存缓存
    ├── 命中 → 直接返回 (< 0.1ms)
    └── 未命中 ↓
2. 查询 L2 SQLite 缓存
    ├── 命中 → 返回并写入 L1 (< 5ms)
    └── 未命中 ↓
3. 查询数据库 (DB)
    ├── 成功 → 写入 L2 + L1，返回结果 (10-500ms)
    └── 失败 → 返回错误
```

### 回写时机

| 事件                     | 动作                                       | 异步 | 说明                        |
| ------------------------ | ------------------------------------------ | ---- | --------------------------- |
| `load_tables` 完成       | 批量写入 L2 (tables + columns)             | ✅   | DB 查询后异步回写           |
| `load_indexes` 完成      | 写入 L2 (indexes + index_columns)          | ✅   | 按表维度写入                |
| `load_constraints` 完成  | 写入 L2 (foreign_keys + check_constraints) | ✅   | FK 和 CHECK 分开写          |
| `load_routines` 完成     | 写入 L2 (routines + parameters)            | ✅   | 存储过程/函数               |
| 用户刷新连接             | 清除 L1，保留 L2                           | 同步 | `invalidate_metadata_cache` |
| Introspection level 变更 | 标记 L2 过期                               | 同步 | 下次访问时重新加载          |

### 缓存失效策略

```rust
// 场景 1: 用户手动刷新
invalidate_metadata_cache(conn_id)  // 清除 L1，L2 标记 stale

// 场景 2: Introspection level 升级 (e.g., 1 → 3)
set_introspection_level(conn_id, level)  // 标记需要重新加载

// 场景 3: DDL 检测 (未来)
on_ddl_detected(conn_id, sql)  // 智能失效相关表
```

### L2 表结构概览

| 表名                | 用途           | 记录数级 (大型 DB) | 查询频率 |
| ------------------- | -------------- | ------------------ | -------- |
| `schemata`          | Schema/Catalog | 1-100              | 低       |
| `tables`            | 表/视图定义    | 1K-100K            | 中       |
| `columns`           | 列元数据       | 10K-1M             | **高**   |
| `indexes`           | 索引定义       | 1K-50K             | 中       |
| `index_columns`     | 索引列映射     | 10K-200K           | 中       |
| `foreign_keys`      | 外键约束       | 100-10K            | 低       |
| `check_constraints` | 检查约束       | 100-5K             | 低       |
| `routines`          | 存储过程/函数  | 100-10K            | 低       |
| `triggers`          | 触发器         | 10-1K              | 低       |
| `sequences`         | 序列           | 10-500             | 低       |

---

## 驱动能力矩阵

### 四驱动元数据获取能力对比

| 能力                                     | MySQL (sqlx)           | PostgreSQL (sqlx) | SQLite (rusqlite) | DuckDB (duckdb-rs) |
| ---------------------------------------- | ---------------------- | ----------------- | ----------------- | ------------------ |
| **list_columns**                         | ✅                     | ✅                | ✅                | ✅                 |
| &nbsp;&nbsp;└ extra (char_max_len)       | ✅                     | ✅ (R5)           | ✅ (R5)           | ✅ (R5)            |
| &nbsp;&nbsp;└ extra (numeric_prec/scale) | ✅                     | ✅                | ✅                | ✅                 |
| &nbsp;&nbsp;└ extra (auto_increment)     | ✅                     | ✅ (SERIAL)       | ❌                | ❌                 |
| **list_indexes**                         | ✅                     | ✅                | ✅                | ✅                 |
| &nbsp;&nbsp;└ index_type (BTREE/HASH)    | ✅                     | ✅                | ❌ (统一 BTREE)   | ❌                 |
| &nbsp;&nbsp;└ include_columns            | ✅                     | ❌                | ❌                | ❌                 |
| **list_constraints**                     | ✅                     | ✅                | ✅                | ✅                 |
| &nbsp;&nbsp;└ foreign_keys               | ✅                     | ✅                | ✅                | ✅                 |
| &nbsp;&nbsp;└ check_constraints          | ⚠️ (有限)              | ✅                | ❌                | ❌                 |
| &nbsp;&nbsp;└ unique_constraints         | ✅ (via indexes)       | ✅                | ✅                | ✅                 |
| **list_tables**                          | ✅                     | ✅                | ✅                | ✅                 |
| &nbsp;&nbsp;└ engine/stats               | ⚠️ (SHOW TABLE STATUS) | ⚠️ (pg_class)     | ❌                | ❌                 |
| &nbsp;&nbsp;└ table_comment              | ✅                     | ✅                | ❌                | ❌                 |
| **get_routine_source**                   | ⚠️ (mysql.proc)        | ✅ (pg_proc)      | ❌                | ❌                 |
| **list_triggers**                        | ✅                     | ✅                | ❌                | ❌                 |
| **list_sequences**                       | ❌ (auto_inc only)     | ✅                | ❌                | ❌                 |

### R5 打通详情 (v2.2 新增)

**目标**: 将四驱动的 list_columns 输出中的扩展信息（如 char_max_length、numeric_precision 等）完整写入 L2 的 `columns.extra` 字段，并在读取时正确反序列化。

**实现要点**:

1. **MySQL**: 从 `information_schema.COLUMNS` 获取 `CHARACTER_MAXIMUM_LENGTH`, `NUMERIC_PRECISION`, `NUMERIC_SCALE`, `EXTRA` (auto_increment 等)
2. **PostgreSQL**: 从 `information_schema.columns` 获取同上，加上 `identity_generation`
3. **SQLite**: 从 `PRAGMA table_info()` 获取基础类型，`PRAGMA table_xinfo()` 获取隐藏列
4. **DuckDB**: 从 `information_schema.columns` 获取（兼容 PG 模式）

**打通率提升**:

| 指标              | v2.1       | v2.2             | 提升   |
| ----------------- | ---------- | ---------------- | ------ |
| 列元数据完整度    | 30%        | 82%              | +52pp  |
| extra 字段覆盖率  | 0%         | 100% (4驱动)     | +100pp |
| 索引约束回写      | Stub       | 完整实现         | ✅     |
| 前端 Tooltip 展示 | 仅基础类型 | 含精度/长度/自增 | ✅     |

---

## 已知问题

### 🔴 当前剩余问题 (v2.2)

| 问题                                | 影响                     | 优先级 | 计划修复版本    |
| ----------------------------------- | ------------------------ | ------ | --------------- |
| **SQLite table_comment 缺失**       | 导航栏表节点无注释       | P1     | v2.3            |
| **DuckDB 无 trigger/sequence 支持** | 导航栏不显示这些对象类型 | P2     | v2.4 (等待上游) |
| **大表 columns 加载慢 (>1000 列)**  | 首次展开可能卡顿         | P1     | v2.3 (分页加载) |
| **L2 缓存无自动过期机制**           | 长时间运行后数据可能过时 | P2     | v2.3 (TTL 策略) |

### ✅ 已修复问题

#### v2.2 修复 (2026-05-25)

| 问题                                 | 修复内容                                                                                | 涉及文件                                                                    |
| ------------------------------------ | --------------------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| **ColumnDetail.extra 未从 L2 读取**  | `load_node_detail()` 和 `list_columns_normalized()` 增加 `extra` 列查询与 JSON 反序列化 | [metadata_cache.rs](../../src-tauri/src/core/persistence/metadata_cache.rs) |
| **ColumnDetailInfo 缺少 extra 字段** | 结构体新增 `extra: HashMap<String, String>`，`from_row()` 更新为 17 列映射              | [metadata_cache.rs](../../src-tauri/src/core/persistence/metadata_cache.rs) |
| **Trigger/Sequence 无 L2 写入方法**  | 新增 `save_trigger()` 和 `save_sequence()` 基础写入方法                                 | [metadata_cache.rs](../../src-tauri/src/core/persistence/metadata_cache.rs) |

#### v2.1 修复 (2026-05-25)

| 问题                               | 修复内容                                                                                              | 涉及文件                                                                                                                                                                                                                                                              |
| ---------------------------------- | ----------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Indexes/Constraints 后端 Stub**  | `load_indexes` / `load_constraints` 从空数组改为真实 DB 查询 + L1 缓存                                | [metadata_commands.rs](../../src-tauri/src/commands/metadata_commands.rs), [traits.rs](../../src-tauri/src/core/driver/traits.rs), [mysql.rs](../../src-tauri/src/core/driver/native/mysql.rs), [metadata_cache.rs](../../src-tauri/src/core/cache/metadata_cache.rs) |
| **10 个右键菜单/事件处理器为空壳** | 实现全部处理器：复制名称、打开表/视图、展开/折叠全部、刷新 Schema/Database、创建对象、打开 SQL 编辑器 | [database-navigator.vue](../../src/extensions/builtin/database/ui/components/database-navigator.vue)                                                                                                                                                                  |
| **遗留双轨加载代码**               | 标记 `navigator-loader.ts` 为 @deprecated，指向当前主流程                                             | [navigator-loader.ts](../../src/extensions/builtin/database/domain/services/navigator-loader.ts)                                                                                                                                                                      |
| **树展开状态不持久化**             | 新增 `navigator-persistence.ts`，基于 localStorage，支持 global/project 双链路，800ms 防抖保存        | [navigator-persistence.ts](../../src/extensions/builtin/database/ui/utils/navigator-persistence.ts), [database-navigator.vue](../../src/extensions/builtin/database/ui/components/database-navigator.vue)                                                             |

### 历史问题（已修复）

#### 数据库表列表不显示

**根因分析**（已修复）：前端 `database-api.ts` 调用 `invoke('load_tables', ...)` 加载表列表，但该 Tauri 命令在 Rust 后端缺失（未实现）。

**第1轮修复（2026-04-23）**：在 `use-database-tree-loader.ts` 的 `loadChildren` 中，MySQL 无 Schema 时用 `dbName` 替代 `schemaName`。

**第2轮修复（2026-04-23）**：`execute_sql` 返回 `unknown[][]`（数组的数组），修复 `loadTablesFromDb`、`loadColumnsFromDb` 共 4 处 array-vs-object 映射错误。

**第3轮修复（2026-04-23）**：MySQL 无 Schema 时 `updateSchemaTables` 崩溃。修复 `DatabaseNode` 增加 `tables` 字段，增加 `db.tables` 回退。

### SQL 编辑器补全报错

**根因**：`sql-editor-service.ts` 调用不存在的 `invoke('get_tables')` 和 `invoke('get_columns')`。

**第1轮修复（2026-04-23）**：改用 `invoke('execute_sql', ...)` 查询 `information_schema.tables`。

**第2轮修复（2026-04-23）**：SQLite 无 `information_schema`，新增 `dbType` 参数使用 `sqlite_master` + `PRAGMA table_info`。

---

## 附录：关键文件路径速查

### 后端 (Rust)

| 文件                                                                                                              | 职责                                           |
| ----------------------------------------------------------------------------------------------------------------- | ---------------------------------------------- |
| [metadata_cache.rs](../../src-tauri/src/core/persistence/metadata_cache.rs)                                       | L2 缓存管理器（读写 SQLite）                   |
| [traits.rs](../../src-tauri/src/core/driver/traits.rs)                                                            | 元数据接口定义（ColumnDetail/IndexDetail/...） |
| [mysql.rs](../../src-tauri/src/core/driver/native/mysql.rs)                                                       | MySQL 元数据实现                               |
| [postgres.rs](../../src-tauri/src/core/driver/native/postgres.rs)                                                 | PostgreSQL 元数据实现                          |
| [sqlite.rs](../../src-tauri/src/core/driver/native/sqlite.rs)                                                     | SQLite 元数据实现                              |
| [duckdb.rs](../../src-tauri/src/core/driver/native/duckdb.rs)                                                     | DuckDB 元数据实现                              |
| [009_jdbc_metadata_alignment.sql](../../src-tauri/migrations/connection_metadata/009_jdbc_metadata_alignment.sql) | SQL 迁移（JDBC 对齐）                          |

### 前端 (TypeScript/Vue)

| 文件                                                                                                 | 职责         |
| ---------------------------------------------------------------------------------------------------- | ------------ |
| [navigator.ts](../../src/extensions/builtin/database/domain/types/navigator.ts)                      | TS 类型定义  |
| [database-navigator.vue](../../src/extensions/builtin/database/ui/components/database-navigator.vue) | 导航栏主组件 |
| [navigator-persistence.ts](../../src/extensions/builtin/database/ui/utils/navigator-persistence.ts)  | 持久化工具   |
