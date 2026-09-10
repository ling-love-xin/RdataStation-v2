# 数据库导航栏优化方案 Spec

## Why

经过 V10.5 系列修复，导航栏的缓存链路和后端命令已完整打通。但前端 Store（1267行）和 Tree Loader（1047行）存在大量重复代码、状态管理混乱、错误处理不一致等问题。需要一次结构性优化，提升可维护性、性能和用户体验。

## What Changes

- 重构 Store：拆分大 Store 为按领域的子 Store，消除 8 处重复的 catalog→schema→table 遍历模式
- 引入通用懒加载工厂：替代 `loadCatalogs`/`loadSchemas`/`loadTables`/`loadColumns` 中重复的「检查缓存→从缓存加载→从DB加载」三段式代码
- 错误状态改为 Map：支持每个节点独立错误显示，不再全局覆盖
- Loading 状态独立化：`loadProcedures` 等不再复用 `loadingTables`
- 统一树遍历工具函数：消除 `catalogs.find→schemas.find→mutate` 的 8 处重复
- 左侧树按需预加载：Schema 节点展开时批量预加载 tables+views，而非逐个文件夹独立请求
- 修复 `loadViews` 的冗余调用和 `loadSchemasFromCache`/`loadTablesFromCache` 的未使用方法
- 节点状态订阅优化：消除 `connectionCatalogs.value = new Map(currentMap)` 的反模式，改为直接 mutation

## Impact

- Affected specs: 数据库导航栏、元数据缓存
- Affected code:
  - `src/extensions/builtin/database/ui/stores/database-navigator-store.ts`（重构）
  - `src/extensions/builtin/database/ui/composables/use-database-tree-loader.ts`（精简）
  - `src/extensions/builtin/database/ui/components/database-navigator.vue`（适配新 Store）
  - `src/extensions/builtin/database/ui/api/database-api.ts`（移除未使用方法）

## ADDED Requirements

### 需求1：通用懒加载工厂（LazyLoader）

系统应当提供统一的懒加载工厂函数，封装 L2 缓存 → API 查询的加载策略，消除 `loadCatalogs`/`loadSchemas`/`loadTables`/`loadColumns` 中重复的三段式代码。

```typescript
function createLazyLoader<T>(config: {
  loadFromCache: () => Promise<T | null>
  loadFromApi: () => Promise<T>
  updateStore: (data: T) => void
  cacheKey: string
  loadingSet: Ref<Set<string>>
}): () => Promise<T>
```

#### 场景：首次展开 Catalog 节点

- **WHEN** 用户展开 Catalog 节点，且 L2 缓存有效
- **THEN** 从 L2 缓存加载 schemas，跳过 API 调用
- **WHEN** L2 缓存无效
- **THEN** 调用 API 加载 schemas 后写入 Store

#### 场景：防重复请求

- **WHEN** 同一 catalog 的 schemas 正在加载中，用户再次触发加载
- **THEN** 第二次请求被忽略，不发送新的网络请求

### 需求2：独立 Loading 状态

每个文件夹类型（procedures-folder、functions-folder、sequences-folder、triggers-folder）应当使用独立的 loading 状态集合，不再复用 `loadingTables`。

#### 场景：同时展开 Procedures 和 Functions 文件夹

- **WHEN** 用户先展开 Procedures 文件夹，Procedures 正在加载
- **THEN** 用户可以同时展开 Functions 文件夹，两个加载互不阻塞
- **WHEN** Procedures 加载完成
- **THEN** Functions 仍在加载中的状态下，Procedures 文件夹不再显示加载状态

### 需求3：按节点 Key 存储错误

错误状态从单个 `error` 字符串改为 `Map<string, string>`，按节点 key 存储每个节点的独立错误。

#### 场景：Schema A 加载失败，Schema B 加载成功

- **WHEN** 用户展开 Schema A，但后端返回错误
- **THEN** Schema A 节点显示红色错误图标
- **WHEN** 用户展开 Schema B
- **THEN** Schema B 正常加载，不影响 Schema A 的错误状态

### 需求4：treeMutation 通用工具函数

提供 `mutateTreeNode(path, updater)` 工具函数，统一 `connectionCatalogs → catalog → schema → field` 的树遍历和修改逻辑。

```typescript
function mutateTreeNode<T>(
  connectionId: string,
  path: TreeMutationPath,
  updater: (node: T) => void
): void
```

#### 场景：为某个 schema 写入 procedures 列表

- **WHEN** `loadProcedures` 从后端返回数据
- **THEN** 使用 `mutateTreeNode(connectionId, { catalogName, schemaName }, (schema) => { schema.procedures = data })` 替代 10 行手动遍历代码

### 需求5：Schema 节点批量预加载

Schema 节点展开时，一次性批量发起 tables + views 的加载请求，利用 `Promise.all` 并行化。

#### 场景：展开 PostgreSQL 的 public schema

- **WHEN** 用户展开 public schema 节点
- **THEN** 同时发起 `loadTables` + `loadViews` 的 API 请求
- **WHEN** 两个请求都返回后再创建 Schema 下的对象文件夹节点

### 需求6：移除未使用代码和冗余逻辑

- **删除 `loadViews`**：它只是 `loadTables` 的无操作包装，tree loader 中 `views-folder` 展开已直接调用 `loadTables`
- **删除 `loadSchemasFromCache`** 和 **`loadTablesFromCache`**：已被 `loadCatalogsFromCache` 和 `getTablesFromCache` 的直接调用替代
- **消除 `connectionCatalogs.value = new Map(currentMap)` 反模式**：Vue 3 的 `reactive` Map 可以直接 set，不需要创建新 Map

## MODIFIED Requirements

### 需求7：database-navigator-store 拆分

原 Store（1267行）按领域拆分为：

| 文件                          | 职责                                         | 预估行数 |
| ----------------------------- | -------------------------------------------- | -------- |
| `use-catalog-loader.ts`       | catalogs + schemas 加载                      | ~200     |
| `use-table-loader.ts`         | tables + views 加载                          | ~250     |
| `use-object-loader.ts`        | procedures/functions/sequences/triggers 加载 | ~250     |
| `use-column-loader.ts`        | columns + indexes + constraints 加载         | ~200     |
| `database-navigator-store.ts` | 状态聚合、对外暴露统一 API                   | ~250     |

#### 场景：其他模块导入 navigatorStore

- **WHEN** 其他模块通过 `useDatabaseNavigatorStore()` 获取 Store
- **THEN** 接口保持不变（向后兼容），内部委托给子模块

## REMOVED Requirements

### 需求8：移除全局错误状态

**Reason**: 单个 `error.value` 在多并发场景下会被覆盖，导致用户看到错误的错误提示。
**Migration**: 改为 `Map<string, string>` 按节点 key 存储。`database-navigator.vue` 中 `currentError` 变更为按选中节点 key 查询。
