# 数据库导航栏整体架构深度分析

> 版本：v1.1
> 日期：2026-06-09
> 状态：已完成 3/3 问题修复

---

## 一、模块总览

数据库导航栏（Database Navigator）是 RdataStation 的核心交互模块，提供类似 DBeaver/DataGrip 的树形数据库结构浏览能力，支持连接节点 → Catalog → Schema → 对象文件夹 → 表/视图 → 列/索引/约束 的多级懒加载展开。

### 1.1 核心文件清单

| 层级               | 文件                                       | 行数  | 职责                                            |
| ------------------ | ------------------------------------------ | ----- | ----------------------------------------------- |
| **主组件**         | `components/database-navigator.vue`        | ~1200 | Vue 组件，VirtualTree 宿主，事件分发            |
| **树加载器**       | `composables/use-database-tree-loader.ts`  | ~1100 | 虚拟树节点构建 + 懒加载调度 `loadChildren()`    |
| **主 Store**       | `stores/database-navigator-store.ts`       | ~641  | Pinia Store，状态聚合 + 子 loader 委派          |
| **Catalog Loader** | `stores/nav-loaders/use-catalog-loader.ts` | ~350  | catalogs + schemas 加载 + L2 缓存               |
| **Table Loader**   | `stores/nav-loaders/use-table-loader.ts`   | ~300  | tables + views 加载 + Schema 统计计算           |
| **Object Loader**  | `stores/nav-loaders/use-object-loader.ts`  | ~250  | procedures/functions/sequences/triggers         |
| **Column Loader**  | `stores/nav-loaders/use-column-loader.ts`  | ~320  | columns/indexes/constraints                     |
| **API 层**         | `api/database-api.ts`                      | ~483  | 前端→Tauri 命令桥接，typed specta 绑定          |
| **缓存服务**       | `services/metadata-cache-service.ts`       | ~700+ | L2 磁盘缓存状态检查/清理                        |
| **树工具**         | `utils/tree-mutation.ts`                   | ~136  | 树节点遍历与变更（mutateTreeNode/getTreeNode）  |
| **懒加载工厂**     | `utils/lazy-loader.ts`                     | ~62   | Cache-first 通用工厂 `createLazyLoader<T>()`    |
| **状态持久化**     | `utils/navigator-persistence.ts`           | ~70   | localStorage 展开状态保存/恢复（800ms 防抖）    |
| **类型定义**       | `types/nav-types.ts`                       | ~109  | 全模块共享类型（单源真）                        |
| **虚拟树类型**     | `types/virtual-tree.ts`                    | ~123  | VirtualTreeNode / NodeKeyEncoder / 节点类型枚举 |

### 1.2 数据关系图

```
VirtualTree (UI)
    ↑ VirtualTreeNode[]
    │
useDatabaseTreeLoader (composable)
    │ loadChildren() 懒加载调度
    │ create*Node() 节点工厂函数
    ↓
useDatabaseNavigatorStore (Pinia)
    │ 状态聚合 + 委派
    ├── useCatalogLoader  ─── loadCatalogs() / loadSchemas()
    ├── useTableLoader     ─── loadTables()
    ├── useObjectLoader    ─── loadProcedures() / loadFunctions() / loadSequences() / loadTriggers()
    └── useColumnLoader    ─── loadColumns() / loadIndexes() / loadConstraints()
           ↓
    database-api.ts  ─── typed(commands.loadCatalogs(...)) ─── Tauri IPC
           ↓
    metadata_commands.rs  ─── #[tauri::command] ─── CacheManager → Driver → information_schema
```

---

## 二、数据模型（树节点层次）

### 2.1 层次结构

```
connection (根)
  └── catalog
        ├── schema (有 Schema 的 DB: PostgreSQL/DuckDB)
        │     ├── tables-folder → table
        │     │     ├── columns-folder → column
        │     │     ├── indexes-folder → index
        │     │     └── constraints-folder → constraint
        │     ├── views-folder → view → column
        │     ├── procedures-folder → procedure
        │     ├── functions-folder → function
        │     ├── sequences-folder → sequence
        │     └── triggers-folder → trigger
        │
        └── [无 Schema: MySQL] 直接挂 tables-folder → table → ...
```

### 2.2 nav-types.ts 完整类型

```typescript
// catalog → [schema] → 对象
CatalogNode {
  name: string
  schemas: SchemaNode[]           // PostgreSQL 等有 Schema
  tables?: TableNode[]            // MySQL 无 Schema 直接挂表
}

SchemaNode {
  name: string
  tables: TableNode[]
  views: ViewNode[]
  procedures?: ProcedureNode[]    // lazy
  functions?: FunctionNode[]      // lazy
  sequences?: SequenceNode[]      // lazy
  triggers?: TriggerNode[]        // lazy
  totalTables?: number            // 统计聚合字段
  totalViews?: number
  totalSizeBytes?: number
  rowCountTotal?: number
}

TableNode {
  name: string
  type: string                    // 'TABLE' | 'VIEW'
  columns: ColumnNode[]
  indexes?: IndexNode[]           // lazy
  constraints?: ConstraintNode[]  // lazy
  rowCount?: number | null
  dataLength?: number | null
  indexLength?: number | null
}

ColumnNode {
  name: string
  dataType: string
  nullable?: boolean
  defaultValue?: string
  isPrimaryKey?: boolean
  charMaxLength?: number
  numericPrecision?: number
  numericScale?: number
}

IndexNode     { name, columns[], isUnique, isPrimary }
ConstraintNode { name, type: 'PRIMARY KEY'|'FOREIGN KEY'|'UNIQUE'|'CHECK', columns[] }
ViewNode      { name, type, columns[] }
ProcedureNode { name }
FunctionNode  { name }
SequenceNode  { name }
TriggerNode   { name, tableName?, event? }
```

### 2.3 VirtualTreeNode（UI 层）

```typescript
VirtualTreeNode {
  key: string              // NodeKeyEncoder.encode(['catalog', connId, 'mydb'])
  level: number            // 缩进级别 0..5
  isExpanded: boolean
  isLeaf: boolean
  label: string            // 显示文本
  type: VirtualTreeNodeType // connection|catalog|schema|table|view|column|tables-folder|...
  data: ITreeNodeData      // { connectionId, dbName, schemaName, tableName, ... }
  parentId: string | null
  childCount: number
  isLoading?: boolean
  connectionTags?: string[]
  connectionStatus?: 'connected' | 'connecting' | 'disconnected'
  isLoaded?: boolean       // 是否已加载过子节点
}
```

### 2.4 关键设计：NodeKeyEncoder

```typescript
// 编码：JSON.stringify(parts) → base64
NodeKeyEncoder.encode(['catalog', 'conn-1', 'mydb'])
// 解码：base64 → JSON.parse → string[]
NodeKeyEncoder.decode(node.key) // ['catalog', 'conn-1', 'mydb']
```

不同的节点类型使用不同的 key parts 结构：

| 节点类型      | key encode 格式                                              |
| ------------- | ------------------------------------------------------------ |
| connection    | `['connection', scope, connId]`                              |
| catalog       | `['catalog', connId, catName]`                               |
| schema        | `['schema', connId, dbName, schemaName]`                     |
| tables-folder | `['tables-folder', connId, dbName, schemaName?]`             |
| table         | `['table', connId, dbName, schemaName, tableName]`           |
| column        | `['column', connId, dbName, schemaName, tableName, colName]` |

---

## 三、Store 架构（Pinia）

### 3.1 状态结构

```typescript
const connectionCatalogs = ref<Map<string, CatalogNode[]>>(new Map()) // 核心数据
const selectedObject = ref<SelectedObject | null>(null) // 当前选中
const loadingCatalogs = ref<Set<string>>(new Set()) // 加载防重
const loadingSchemas = ref<Set<string>>(new Set())
const loadingTables = ref<Set<string>>(new Set())
const loadingColumns = ref<Set<string>>(new Set())
const error = ref<string | null>(null) // 全局错误（兼容）
const nodeErrors = ref<Map<string, string>>(new Map()) // 按节点 key 独立错误
const connectionTypes = ref<Map<string, 'global' | 'project'>>(new Map())
const connectionProjectPaths = ref<Map<string, string | undefined>>(new Map())
const connectionDbTypes = ref<Map<string, string>>(new Map()) // 如 'mysql', 'postgres'
const introspectionLevels = ref<Map<string, IntrospectionLevel>>(new Map())
const lastSyncTimes = ref<Map<string, number>>(new Map())
const syncModes = ref<Map<string, 'full' | 'incremental'>>(new Map())
```

### 3.2 子 Loader 委派模式

```typescript
// 子 loader 实例化时注入共享 Ref
const catalogLoader = useCatalogLoader(
  connectionCatalogs,
  connectionTypes,
  connectionProjectPaths,
  connectionDbTypes,
  loadingCatalogs,
  loadingSchemas,
  lastSyncTimes,
  nodeErrors
)
const tableLoader = useTableLoader(
  connectionCatalogs,
  connectionTypes,
  connectionProjectPaths,
  loadingTables,
  lastSyncTimes,
  nodeErrors
)
const objectLoader = useObjectLoader(
  connectionCatalogs,
  connectionTypes,
  connectionProjectPaths,
  nodeErrors
)
const columnLoader = useColumnLoader(
  connectionCatalogs,
  connectionTypes,
  connectionProjectPaths,
  loadingColumns,
  nodeErrors
)

// Store 只做薄层聚合，所有加载委托给子 loader
function loadTables(...args) {
  return tableLoader.loadTables(...args)
}
function loadProcedures(...args) {
  return objectLoader.loadProcedures(...args)
}
```

### 3.3 Loading 状态隔离

| 加载对象                    | loading Set 归属                                      | 防重 key 格式                                                  |
| --------------------------- | ----------------------------------------------------- | -------------------------------------------------------------- |
| catalogs                    | `loadingCatalogs`（主 Store）                         | `{connectionId}`                                               |
| schemas                     | `loadingSchemas`（主 Store）                          | `{connectionId}:{catalogName}`                                 |
| tables/views                | `loadingTables`（主 Store，注入 tableLoader）         | `{connectionId}:{catalogName}:{schemaName}`                    |
| columns/indexes/constraints | `loadingColumns`（主 Store，注入 columnLoader）       | `{connectionId}:{catalogName}:{schemaName}:{tableName}:{type}` |
| **procedures**              | `objectLoader.loadingProcedures`（objectLoader 内部） | `{connId}:{cat}:{schema}:procedures`                           |
| **functions**               | `objectLoader.loadingFunctions`（objectLoader 内部）  | `{connId}:{cat}:{schema}:functions`                            |
| **sequences**               | `objectLoader.loadingSequences`（objectLoader 内部）  | `{connId}:{cat}:{schema}:sequences`                            |
| **triggers**                | `objectLoader.loadingTriggers`（objectLoader 内部）   | `{connId}:{cat}:{schema}:triggers`                             |

procedures/functions/sequences/triggers 各自独立 loading 状态，展开 Functions 不会阻塞 Procedures。

### 3.4 Per-node 错误追踪

```typescript
// 写入（子 loader 的 catch 块）
nodeErrors.value.set('conn-1:mydb:public:procedures', '加载存储过程列表失败: timeout')

// 读取
navigatorStore.getNodeError('conn-1:mydb:public') // → string | null

// 管理
navigatorStore.setNodeError(key, msg)
navigatorStore.clearNodeError(key)
navigatorStore.clearAllNodeErrors()
```

### 3.5 对外暴露的公共 API

```typescript
return {
  // 状态
  connectionCatalogs,
  selectedObject,
  error,
  // 加载器
  loadCatalogs,
  loadCatalogsFromCacheSilent,
  loadSchemas,
  loadTables,
  loadProcedures,
  loadFunctions,
  loadSequences,
  loadTriggers,
  loadColumns,
  loadIndexes,
  loadConstraints,
  // 读取器
  getCatalogs,
  getCatalogSchemas,
  getSchemaTables,
  getSchemaViews,
  // Loading 状态
  isLoadingCatalogs,
  isLoadingSchemas,
  isLoadingTables,
  isLoadingProcedures,
  isLoadingFunctions,
  isLoadingSequences,
  isLoadingTriggers,
  // 错误
  getNodeError,
  setNodeError,
  clearNodeError,
  clearAllNodeErrors,
  // 搜索
  searchObjects,
  // 同步/缓存
  refreshMetadata,
  startCacheWarming,
  clearCache,
  getLastSyncTime,
  setLastSyncTime,
  setSyncMode,
  getSyncMode,
  // 连接管理
  setConnectionInfo,
  getDbType,
  getConnectionType,
  getProjectPath,
  disconnectConnection,
  setIntrospectionLevel,
  getIntrospectionLevel,
  setSelectedObject,
  clearError,
  executeSql,
  expandToNode,
  selectNode,
}
```

---

## 四、懒加载调度流程

### 4.1 `loadChildren()` — 核心调度器

位置：`use-database-tree-loader.ts` line 820

```typescript
async function loadChildren(node: VirtualTreeNode): Promise<VirtualTreeNode[]> {
  const [nodeType, ...parts] = NodeKeyEncoder.decode(node.key)
  const config = await getNavConfig(dbType)  // 根据 dbType 获取导航配置

  switch (nodeType) {
    case 'connection':
      // 在线: loadCatalogs() → 从 DB / L1 / L2 获取
      // 离线: loadCatalogsFromCacheSilent() → 仅从 L2 缓存
      if (config.hasCatalogs) return createCatalogNodes()
      else return createCatalogObjectNodes()  // MySQL: 跳过 catalog 层，直接对象文件夹

    case 'catalog':
      if (config.hasSchemas) → loadSchemas() + createSchemaNodes()
      else → createCatalogObjectNodes()  // MySQL: catalog 下直接挂对象文件夹

    case 'schema':
      → loadTables() + createSchemaObjectNodes()

    case 'tables-folder':
      → loadTables() + createTableNodes()

    case 'views-folder':
      → loadTables() + createViewNodes()

    case 'procedures-folder':
      → loadProcedures() + createProcedureNodes()

    case 'functions-folder':
      → loadFunctions() + createFunctionNodes()

    case 'sequences-folder':
      → loadSequences() + createSequenceNodes()

    case 'triggers-folder':
      → loadTriggers() + createTriggerNodes()

    case 'table':
      → loadColumns() + createTableSubFolderNodes()

    case 'view':
      → loadColumns() + createColumnNodes()

    case 'columns-folder':
      → createColumnNodes()  // 数据已在 loadColumns 时写入 Store

    case 'indexes-folder':
      → loadIndexes() + createIndexNodes()

    case 'constraints-folder':
      → loadConstraints() + createConstraintNodes()
  }
}
```

### 4.2 MySQL vs PostgreSQL 的差异处理

```typescript
// 通过 NavigationConfig 区分
const config = await getNavConfig(dbType) // 从 form-schemas/{dbtype}.json 加载

config.hasCatalogs // true: MySQL(数据库=catalog), PostgreSQL(数据库=catalog)
config.hasSchemas // true: PostgreSQL(public等schema), false: MySQL
config.systemSchemas // ['information_schema', 'pg_catalog', ...] — 过滤系统 schema
config.folders // { tables, views, functions, procedures, sequences, triggers } 每个有 enabled
```

### 4.3 NavigationConfig 配置驱动

```typescript
interface NavigationFolderConfig {
  enabled: boolean // 此 dbType 是否有此类对象
  label: string // 显示文本
}

interface NavigationConfig {
  hasCatalogs: boolean
  hasSchemas: boolean
  systemSchemas: string[]
  folders: {
    tables: NavigationFolderConfig
    views: NavigationFolderConfig
    functions: NavigationFolderConfig
    procedures: NavigationFolderConfig
    sequences: NavigationFolderConfig
    triggers: NavigationFolderConfig
  }
  tableChildren: {
    columns: boolean
    indexes: boolean
    constraints: boolean
    triggers: boolean
    foreignKeys: boolean
    references: boolean
  }
}
```

---

## 五、后端接口映射

### 5.1 Tauri Command 清单

所有 metadata 命令定义在 `src-tauri/src/commands/metadata_commands.rs`：

| Tauri Command             | 参数                                                  | 返回                  | 说明             |
| ------------------------- | ----------------------------------------------------- | --------------------- | ---------------- |
| `load_catalogs`           | connId, connType, projectPath                         | `Vec<CatalogMeta>`    | Catalog 列表     |
| `load_schemas`            | connId, catalogName, connType, projectPath            | `Vec<SchemaMeta>`     | Schema 列表      |
| `load_tables`             | connId, catalog, schema, connType, projectPath        | `Vec<TableMeta>`      | 表列表（含视图） |
| `load_views`              | connId, catalog, schema, connType, projectPath        | `Vec<ViewMeta>`       | 视图列表         |
| `load_columns`            | connId, catalog, schema, table, connType, projectPath | `Vec<ColumnMeta>`     | 列列表           |
| `load_indexes`            | connId, catalog, schema, table, connType, projectPath | `Vec<IndexMeta>`      | 索引列表         |
| `load_constraints`        | connId, catalog, schema, table, connType, projectPath | `Vec<ConstraintMeta>` | 约束列表         |
| `load_procedures`         | connId, catalog, schema, connType, projectPath        | `Vec<ProcedureMeta>`  | 存储过程列表     |
| `load_functions`          | connId, catalog, schema, connType, projectPath        | `Vec<FunctionMeta>`   | 函数列表         |
| `load_sequences`          | connId, catalog, schema, connType, projectPath        | `Vec<SequenceMeta>`   | 序列列表         |
| `load_triggers`           | connId, catalog, schema, connType, projectPath        | `Vec<TriggerMeta>`    | 触发器列表       |
| `load_routine_source`     | connId, catalog, schema, name, kind                   | `RoutineSourceMeta`   | DDL 源码         |
| `get_cache_stats`         | connId, connType                                      | `CacheStats`          | 缓存统计         |
| `set_introspection_level` | connId, level                                         | `void`                | 内省级别         |
| `get_introspection_level` | connId                                                | `String`              | 获取内省级别     |

所有命令的统一参数模式：

```
(connectionId, catalogName?, schemaName?, tableName?, connectionType, projectPath?)
```

### 5.2 后端缓存架构

```
请求进入
  ↓
L1 内存缓存 (MetadataCache: CTL-based LRU, 全局单例 CacheManager::instance())
  ├─ HIT → 返回（微秒级）
  └─ MISS ↓
L2 磁盘缓存 (SQLite: MetadataCacheManager → metadata_cache.db)
  ├─ HIT → 读取 + 回填 L1 → 返回（毫秒级）
  └─ MISS ↓
数据库实时查询 (Driver::get_catalogs → information_schema / pg_catalog)
  → 回填 L2 + 回填 L1 → 返回（秒级）
```

**缓存键结构：**

```rust
enum MetadataCacheKey {
    Catalogs { conn_id },
    Schemas { conn_id, database },
    Tables { conn_id, database, schema },
    Columns { conn_id, database, schema, table },
    Views / Procedures / Functions / Indexes / Constraints / Sequences / Triggers { ... },
    RoutineSource { conn_id, database, schema, name, kind },
    DataSourceMeta { conn_id },
}
```

**TTL 策略：**

- Catalogs/Schemas: 5 分钟
- Tables/Columns: 10 分钟
- Indexes/Constraints: 15 分钟
- 序列/触发器: 30 分钟

**连接类型路由：**

```rust
enum ConnectionType { Global, Project }

// Global: L2 缓存存储在 ~/.rdata-station/cache/
// Project: L2 缓存存储在 {projectPath}/.rdata-station/cache/
```

### 5.3 驱动层元数据接口（traits）

```rust
trait Database {
    fn get_catalogs(&self) -> Result<Vec<CatalogMeta>, CoreError>;
    fn get_schemas(&self, catalog: &str) -> Result<Vec<SchemaMeta>, CoreError>;
    fn get_tables(&self, catalog: &str, schema: Option<&str>) -> Result<Vec<TableMeta>, CoreError>;
    fn get_table_detail(...) -> Result<TableDetail, CoreError>;
    fn list_tables(...) -> Result<Vec<NodeInfo>, CoreError>;
    fn list_columns(...) -> Result<Vec<ColumnDetail>, CoreError>;
    fn list_indexes(...) -> Result<Vec<IndexDetail>, CoreError>;
    fn list_constraints(...) -> Result<Vec<ConstraintDetail>, CoreError>;
    fn list_procedures(...) -> Result<Vec<ProcedureMeta>, CoreError>;
    fn list_functions(...) -> Result<Vec<FunctionMeta>, CoreError>;
    fn list_sequences(...) -> Result<Vec<SequenceMeta>, CoreError>;
    fn list_triggers(...) -> Result<Vec<TriggerMeta>, CoreError>;
    fn get_routine_source(...) -> Result<RoutineSourceMeta, CoreError>;
}
```

---

## 六、请求生命周期（以"展开 Tables 文件夹"为例）

```
1. 用户点击 Tables-folder 节点
     ↓
2. VirtualTree 触发 @toggle 事件
     ↓
3. database-navigator.vue handleVirtualTreeToggle(node)
     ↓
4. treeLoader.loadChildren(node)  // composable 调度
     ↓ node.type === 'tables-folder'
5. navigatorStore.loadTables(connId, dbName, schemaName)  // Store 委派
     ↓
6. tableLoader.loadTables(connId, dbName, schemaName)     // 子 loader
     ↓
7. loadingTables.add(key)  // 防重入
     ↓
8. 检查 L2 缓存 getMetadataCacheStatus(connId, ..., 'tables', schemaName)
     ├─ is_valid=true → loadTablesFromCache() → 直接写入 connectionCatalogs
     └─ is_valid=false ↓
9. databaseApi.loadTables(connId, dbName, schemaName, connType, projectPath)
     ↓
10. typed(commands.loadTables(...))  // tauri-specta typed binding
     ↓
11. #[tauri::command] load_tables(conn_id, catalog, schema, conn_type, project_path)
     ↓
12. MetadataService::list_tables(conn_id, catalog, schema)   // 查 L1 内存缓存
     ├─ L1 HIT → 返回
     └─ L1 MISS ↓
13. MetadataCacheManager (L2 SQLite) 查询
     ├─ L2 HIT → 写入 L1 → 返回
     └─ L2 MISS ↓
14. ConnectionManager.acquire(conn_id) → Box<dyn Database>
     ↓
15. db.list_tables(catalog, schema)  // MySQL: SHOW TABLES / PostgreSQL: information_schema.tables
     ↓
16. 写入 L2 磁盘缓存 + L1 内存缓存
     ↓
17. Result<Vec<TableMeta>, CoreError> 返回前端
     ↓
18. tableLoader 将 TableMeta[] 转换为 TableNode[]，写入 catalog.schemas[i].tables
     ↓
19. loadingTables.delete(key)  // 释放 loading 状态
     ↓
20. treeLoader.createTableNodes() → VirtualTreeNode[]
     ↓
21. virtualTreeNodes.value.push(...children)  // Vue 响应式更新
     ↓
22. VirtualTree 渲染新的子节点
```

---

## 七、关键工具函数

### 7.1 tree-mutation.ts

```typescript
// 一行替代 10 行手动遍历
mutateTreeNode(
  connectionCatalogs.value,   // Map<string, CatalogNode[]>
  connectionId,
  { catalogName: 'mydb', schemaName: 'public' },
  (schema) => { schema.procedures = [{ name: 'sp_foo' }] }
)

// 只读查找
const schema = getTreeNode(catalogs, connId, { catalogName, schemaName })

// 直接操作 catalog（修改 schemas 等）
mutateCatalogNode(catalogs, connId, 'mydb', (cat) => { cat.schemas = [...] })
```

### 7.2 lazy-loader.ts

```typescript
// 通用「查缓存→缓存有效返回→缓存无效调 API」工厂
const loaderFn = createLazyLoader({
  cacheKey: `${connId}:${dbName}`,
  loadingSet: myLoadingSet, // Set<string> 防重入
  checkCache: () => getCacheStatus(connId, 'tables'),
  loadFromCache: () => loadFromDisk(connId, dbName),
  loadFromApi: () => databaseApi.loadTables(connId, dbName, schema),
  onLoaded: data => {
    /* 写入 connectionCatalogs */
  },
})

await loaderFn() // 一次调用完成 cache-first 全流程
```

### 7.3 navigator-persistence.ts

```typescript
// localStorage 保存/恢复树展开状态
saveConnectionNavigatorState(
  connId,
  {
    expandedKeys: ['key1', 'key2'],
    selectedKey: 'key3',
    filterText: '',
    lastUpdated: Date.now(),
    version: 1,
  },
  projectPath
)

// 800ms 防抖自动保存
const restored = getConnectionNavigatorState(connId, projectPath)
// restoring expandedKeys → virtualTreeNodes.forEach(n => n.isExpanded = true)
```

---

## 八、缓存预热（Cache Warming）

当用户刷新元数据且未指定具体的 catalog/schema 时，触发全量缓存预热：

```typescript
async function startCacheWarming(connectionId: string) {
  // Phase 1: 并行加载所有 Catalog 的 Schema
  await Promise.allSettled(catalogs.map(cat => loadSchemas(connectionId, cat.name)))

  // Phase 2: 收集所有 Schema，并行加载表
  const tablePromises = []
  for (const cat of catalogs) {
    for (const schema of getCatalogSchemas(connectionId, cat.name)) {
      tablePromises.push(loadTables(connectionId, cat.name, schema.name))
    }
  }
  await Promise.allSettled(tablePromises)

  // Phase 3: 按内省级别决定是否加载列（BATCH_SIZE=20 分批）
  if (introspectionLevel !== 'level1') {
    for (const batch of chunk(columnPromises, 20)) {
      await Promise.allSettled(batch)
    }
  }
}
```

**内省级别：**

- `level1`: 仅名称（只预热到表级）
- `level2`: 名称 + 列元数据（不含例程源码）
- `level3`: 全部（含例程源码）

---

## 九、组件结构（database-navigator.vue）

```
database-navigator.vue
├── NavigatorToolbar        — 工具栏（新建连接/刷新/断开/搜索/筛选切换）
├── NavigatorSearch         — 对象全局搜索（基于 searchObjects()，内存遍历匹配）
├── NavigatorFilter         — 显示筛选（Tables/Views/Columns/System Schemas）
├── VirtualTree             — 虚拟滚动树（核心渲染）
│     ↑ virtualTreeNodes: VirtualTreeNode[]
│     ↑ loadChildren() 懒加载
│     ↑ @select / @toggle / @context-menu / @dblclick 事件
├── NavigatorContextMenuV2  — 右键菜单（基于节点类型动态生成菜单项）
├── NavigatorError          — 错误提示弹窗（showError + currentError + retry）
├── NavigatorGroupDialog    — 连接分组对话框
├── NavigatorPropertiesDialog — 对象属性面板
└── NavigatorStatus         — 底部状态栏（连接状态/事务指示）
```

---

## 十、已知问题与设计约束

### 10.1 已知问题

| #   | 问题                                                                   | 影响                                                | 状态     |
| --- | ---------------------------------------------------------------------- | --------------------------------------------------- | -------- |
| 1   | `loadChildren()` schema 节点下重复调用两次 `loadTables`                | 第 872-875 行重复调用，浪费一个往返                 | 待修复   |
| 2   | 树展开状态不跨会话持久化（localStorage 按 connId 维度，跨连接不恢复）  | 切换连接后需要重新展开                              | 设计中   |
| 3   | `searchObjects()` 是 O(N) 内存遍历，大数据量下可能卡顿                 | 1000+ 表时搜索有延迟                                | 设计中   |
| 4   | 视图和表共用 `loadTables` 加载（后端 `load_tables` 返回 TABLE + VIEW） | 前端的 `loadViews()` API 仍然存在但未被树加载器使用 | 设计如此 |

### 10.2 设计约束

- VirtualTree 使用按键路径 `['type', connId, dbName, schemaName, ...]` 编码节点标识
- 配置驱动：每个 dbType 对应一个 `form-schemas/{dbType}.json` 导航配置文件
- 前端→后端通过 `tauri-specta` 生成类型安全的 typed commands，禁止手动 `invoke()`
- 子 loader 通过接收 `Ref<Map<string, string>>` 写入 `nodeErrors`，不是返回 error

---

## 十一、模块演进历史

| 版本      | 日期        | 关键变更                                                                                         |
| --------- | ----------- | ------------------------------------------------------------------------------------------------ |
| V10.0     | 2026-04     | 初始实现：VirtualTree + Pinia Store + Tauri Commands                                             |
| V10.1     | 2026-04     | 添加 NodeKeyEncoder (base64 编码)、配置驱动 NavigationConfig                                     |
| V10.2     | 2026-05     | 添加 L2 磁盘缓存、cache-first 加载策略                                                           |
| V10.3     | 2026-05     | 添加 navigator-persistence（localStorage 展开状态保存/恢复）                                     |
| V10.4     | 2026-05     | 添加 startCacheWarming（三阶段全量预热）                                                         |
| V10.5     | 2026-05     | 添加 IntrospectionLevel（三级内省控制）                                                          |
| **V10.7** | **2026-06** | **Bug 修复: schema loadTables 去重 + lastActive 跨会话恢复 + 搜索 debounceAsync 300ms**          |
| **V10.6** | **2026-06** | **Store 拆分(4 子 loader) + tree-mutation 工具 + LazyLoader 工厂 + loading 独立化 + nodeErrors** |
