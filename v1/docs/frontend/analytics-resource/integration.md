# 分析资源管理器 — 前端集成指南

> 版本：v1.7
> 最后更新：2026-05-12
> 状态：✅ 25 个前端 API + 18 Tauri Command + IPC 版本协商 + 15 单元测试 + 3 故障注入测试

---

## 一、概述

本文档面向需要在其他模块/插件中**集成分析资源管理功能**的开发者。无论你是要在查询结果中自动创建资源、从文件导入创建资源，还是只想读取当前项目的资源列表，本文档都提供了标准化的接入方式。

---

## 二、集成层级选择

```
需要集成分析资源？
├── 仅读取数据 → 使用 Store (Pinia)         ← 推荐
├── 需要写入数据 → 使用 Store (Pinia)         ← 推荐
├── 跨插件/跨模块 → 使用 API 层直接调用       ← 备选
└── 自定义 UI 集成 → 引入组件 + Store
```

| 方式       | 适用场景                  | 优点                   | 缺点                   |
| ---------- | ------------------------- | ---------------------- | ---------------------- |
| **Store**  | 绝大多数场景              | 缓存、响应式、状态一致 | 需确认已初始化         |
| **API 层** | 跨插件调用、非 Vue 上下文 | 轻量、直接             | 无缓存、需自行管理状态 |
| **组件**   | 嵌入到其他页面            | 开箱即用               | 耦合度较高             |

---

## 三、方式一：通过 Pinia Store（推荐）

### 3.1 基础用法

```typescript
import { useAnalyticsResourceStore } from '@/extensions/builtin/analytics-resource/ui/stores/analytics-resource-store'

// 在任何 setup / composable 中使用
const store = useAnalyticsResourceStore()

// 确保已初始化
if (!store.initialized) {
  await store.initStore()
}

// 读取资源列表
const resources = store.resources // AnalyticsResource[]
const folders = store.folders // AnalyticsFolder[]
const tags = store.tags // AnalyticsTag[]
const loading = store.loading // boolean

// 读取过滤后的资源
const filtered = store.filteredResources // 根据 scope/type/folder 过滤
```

### 3.2 创建资源（来自查询结果）

最常见的集成场景：用户执行 SQL 查询后，将结果集注册为分析资源。

```typescript
const store = useAnalyticsResourceStore()

async function saveQueryResultAsResource(queryResult: {
  tableName: string
  rowCount: number
  columnCount: number
  sql: string
}) {
  const resource = await store.createResource({
    resource_type: 'table',
    name: queryResult.tableName,
    scope: 'project',
    config: {
      table_name: queryResult.tableName,
      duckdb_schema: 'temp',
    },
    row_count: queryResult.rowCount,
    column_count: queryResult.columnCount,
    source_query: queryResult.sql,
  })

  console.log('资源已创建:', resource.id)
  return resource
}
```

### 3.3 创建资源（来自文件导入）

```typescript
const store = useAnalyticsResourceStore()

async function registerFileAsResource(filePath: string, fileName: string, fileSize: number) {
  const resource = await store.createResource({
    resource_type: 'file',
    name: fileName,
    scope: 'project',
    config: {
      file_path: filePath,
      file_name: fileName,
      file_size: fileSize,
    },
    file_size: fileSize,
  })

  return resource
}
```

### 3.4 创建资源（来自连接/表提取）

```typescript
const store = useAnalyticsResourceStore()

async function registerTableAsResource(
  connectionId: string,
  tableName: string,
  rowCount?: number,
  columnCount?: number
) {
  const resource = await store.createResource({
    resource_type: 'table',
    name: tableName,
    scope: 'project',
    config: {
      connection_id: connectionId,
      table_name: tableName,
    },
    row_count: rowCount,
    column_count: columnCount,
  })

  return resource
}
```

### 3.5 分页加载

```typescript
const store = useAnalyticsResourceStore()

// 第一页（自动使用 store 中的 pageSize/sortBy/sortOrder 设置）
await store.loadResourcesPaginated({
  scope: 'project',
  resource_type: 'table',
  search: 'supplier',
})

// 翻页
store.nextPage()
await store.loadResourcesPaginated()

// 跳转到指定页
store.setPage(3)
await store.loadResourcesPaginated()

// 修改每页数量
store.setPageSize(50)
await store.loadResourcesPaginated()

// 访问分页信息
console.log(store.total, store.page, store.pageSize, store.totalPages)
```

### 3.6 设置排序

```typescript
// 按创建时间排序
store.setSort('created_at', 'desc')
await store.loadResourcesPaginated()

// 按名称排序
store.setSort('name', 'asc')
await store.loadResourcesPaginated()

// 切换当前排序方向
store.toggleSortOrder()
await store.loadResourcesPaginated()
```

### 3.7 文件夹操作

```typescript
// 创建文件夹
const folder = await store.createFolder({
  name: '供应链分析',
  scope: 'project',
  color: '#165DFF',
})

// 将资源添加到文件夹（同资源可属于多个文件夹）
await store.addResourceToFolder('ar_xxx', folder.id)

// 从文件夹移除
await store.removeResourceFromFolder('ar_xxx', folder.id)

// 查看某资源所在的所有文件夹
const folderIds = store.getResourceFolders('ar_xxx')
```

### 3.8 回收站操作

```typescript
// 加载回收站
await store.loadRecycleBin()

// 恢复资源
const restored = await store.restoreResource('rb_xxx')

// 永久删除
await store.permanentDeleteResource('rb_xxx')
```

### 3.9 设置管理

```typescript
// 读取当前设置
const currentSettings = store.settings

// 保存新设置（自动持久化到 localStorage 并立即生效）
store.saveSettings({
  ...currentSettings,
  general: {
    ...currentSettings.general,
    defaultPageSize: 50,
    defaultSortField: 'updated_at',
    defaultSortOrder: 'desc',
  },
  display: {
    ...currentSettings.display,
    showMetadata: false,
  },
})

// 重置为默认
store.resetSettings()

// 清除缓存
store.clearCache()
```

---

## 四、方式二：通过 API 层直接调用

适用于跨插件调用或非 Vue 上下文的场景。

```typescript
import * as analyticsApi from '@/extensions/builtin/analytics-resource/infrastructure/api/analytics-resource-api'

// 确保先初始化
await analyticsApi.initAnalyticsResourceStore()

// 创建资源
const resource = await analyticsApi.createAnalyticsResource({
  resource_type: 'table',
  name: 'query_result_2024',
  scope: 'project',
  config: { table_name: 'query_result_2024' },
})

// 查询资源
const result = await analyticsApi.listAnalyticsResourcesPaginated({
  scope: 'project',
  resource_type: 'table',
  search: 'supplier',
  pagination: { page: 1, pageSize: 20 },
  sort: { sortBy: 'created_at', sortOrder: 'desc' },
})

// 删除资源
await analyticsApi.deleteAnalyticsResource('ar_xxx')
```

---

## 五、Store 完整 API 参考

### 5.1 状态（State）

| 属性                | 类型                             | 说明             |
| ------------------- | -------------------------------- | ---------------- |
| `resources`         | `Ref<AnalyticsResource[]>`       | 当前页资源列表   |
| `folders`           | `Ref<AnalyticsFolder[]>`         | 全部文件夹       |
| `tags`              | `Ref<AnalyticsTag[]>`            | 全部标签         |
| `recycleBin`        | `Ref<AnalyticsRecycleItem[]>`    | 回收站条目       |
| `loading`           | `Ref<boolean>`                   | 加载状态         |
| `initialized`       | `Ref<boolean>`                   | 是否已初始化     |
| `selectedResources` | `Ref<string[]>`                  | 已选资源 ID 列表 |
| `settings`          | `Ref<AnalyticsResourceSettings>` | 当前设置         |

### 5.2 计算属性（Computed）

| 属性                | 类型                  | 说明                                |
| ------------------- | --------------------- | ----------------------------------- |
| `filteredResources` | `AnalyticsResource[]` | 根据 scope/type/folder 过滤后的资源 |
| `totalPages`        | `number`              | 总页数                              |

### 5.3 方法（Actions）

#### 初始化

| 方法        | 签名                  | 说明                             |
| ----------- | --------------------- | -------------------------------- |
| `initStore` | `() => Promise<void>` | 初始化 Store，加载设置和初始数据 |

#### 资源

| 方法                     | 签名                                                                       | 说明                         |
| ------------------------ | -------------------------------------------------------------------------- | ---------------------------- |
| `loadResources`          | `(input?: ListResourcesInput) => Promise<void>`                            | 加载资源（含缓存）           |
| `loadResourcesPaginated` | `(input?: ListResourcesInput) => Promise<void>`                            | 分页加载资源                 |
| `createResource`         | `(input: CreateResourceRequest) => Promise<AnalyticsResource>`             | 创建资源                     |
| `updateResource`         | `(id: string, input: CreateResourceRequest) => Promise<AnalyticsResource>` | 更新资源（自动保存版本快照） |
| `deleteResource`         | `(id: string) => Promise<void>`                                            | 软删除资源                   |
| `batchDeleteResources`   | `(ids: string[]) => Promise<void>`                                         | 批量软删除                   |
| `cloneResource`          | `(id: string, newName?: string) => Promise<AnalyticsResource>`             | 克隆资源                     |

#### 分页与排序

| 方法              | 签名                                                    | 说明                          |
| ----------------- | ------------------------------------------------------- | ----------------------------- |
| `setPage`         | `(newPage: number) => void`                             | 设置页码                      |
| `setPageSize`     | `(size: number) => void`                                | 设置每页数量（重置到第 1 页） |
| `nextPage`        | `() => void`                                            | 下一页                        |
| `prevPage`        | `() => void`                                            | 上一页                        |
| `setSort`         | `(field: SortField \| null, order?: SortOrder) => void` | 设置排序                      |
| `toggleSortOrder` | `() => void`                                            | 切换排序方向                  |

#### 文件夹

| 方法                       | 签名                                                       | 说明                       |
| -------------------------- | ---------------------------------------------------------- | -------------------------- |
| `loadFolders`              | `(input?: ListFoldersInput) => Promise<void>`              | 加载文件夹                 |
| `createFolder`             | `(input: CreateFolderRequest) => Promise<AnalyticsFolder>` | 创建文件夹                 |
| `addResourceToFolder`      | `(resourceId: string, folderId: string) => Promise<void>`  | 资源添加到文件夹           |
| `removeResourceFromFolder` | `(resourceId: string, folderId: string) => Promise<void>`  | 从文件夹移除资源           |
| `getResourceFolders`       | `(resourceId: string) => string[]`                         | 获取资源所属文件夹 ID 列表 |

#### 标签

| 方法                    | 签名                                                   | 说明                |
| ----------------------- | ------------------------------------------------------ | ------------------- |
| `loadTags`              | `(input?: ListTagsInput) => Promise<void>`             | 加载标签            |
| `createTag`             | `(input: CreateTagRequest) => Promise<AnalyticsTag>`   | 创建标签            |
| `addTagToResource`      | `(resourceId: string, tagId: string) => Promise<void>` | 资源添加标签        |
| `removeTagFromResource` | `(resourceId: string, tagId: string) => Promise<void>` | 从资源移除标签      |
| `getAnalyticsTag`       | `(id: string) => Promise<AnalyticsTag>`                | 获取单个标签详情 🆕 |

#### 标签缓存（前端本地）

| 方法               | 签名                                       | 说明                          |
| ------------------ | ------------------------------------------ | ----------------------------- |
| `loadResourceTags` | `(resourceIds: string[]) => Promise<void>` | 批量加载资源标签到本地缓存 🆕 |
| `getResourceTags`  | `(resourceId: string) => AnalyticsTag[]`   | 从本地缓存同步获取资源标签 🆕 |

#### 版本与双向查询

| 方法                  | 签名                                                 | 说明               |
| --------------------- | ---------------------------------------------------- | ------------------ |
| `getResourceVersions` | `(resourceId: string) => Promise<ResourceVersion[]>` | 获取资源版本历史   |
| `getTagsForResource`  | `(resourceId: string) => Promise<AnalyticsTag[]>`    | 获取资源关联的标签 |
| `getResourcesByTag`   | `(tagId: string) => Promise<AnalyticsResource[]>`    | 获取标签关联的资源 |

#### 回收站

| 方法                      | 签名                                                | 说明       |
| ------------------------- | --------------------------------------------------- | ---------- |
| `loadRecycleBin`          | `() => Promise<void>`                               | 加载回收站 |
| `restoreResource`         | `(recycleId: string) => Promise<AnalyticsResource>` | 恢复资源   |
| `permanentDeleteResource` | `(recycleId: string) => Promise<void>`              | 永久删除   |

#### 选择

| 方法             | 签名                                       | 说明           |
| ---------------- | ------------------------------------------ | -------------- |
| `selectResource` | `(id: string, multiple?: boolean) => void` | 选择/取消资源  |
| `clearSelection` | `() => void`                               | 清除所有选择   |
| `selectScope`    | `(scope: string \| null) => void`          | 设置作用域筛选 |
| `selectType`     | `(type: string \| null) => void`           | 设置类型筛选   |
| `selectFolder`   | `(folderId: string \| null) => void`       | 设置文件夹筛选 |

#### 设置

| 方法            | 签名                                            | 说明                     |
| --------------- | ----------------------------------------------- | ------------------------ |
| `loadSettings`  | `() => void`                                    | 从 localStorage 加载设置 |
| `saveSettings`  | `(settings: AnalyticsResourceSettings) => void` | 保存并立即应用设置       |
| `resetSettings` | `() => void`                                    | 重置为默认设置           |
| `clearCache`    | `() => void`                                    | 清除所有前端缓存         |

---

## 六、初始化顺序要求

```
应用启动 / 打开项目
  │
  ├─→ 1. project_store 初始化（项目数据库就绪）
  │
  ├─→ 2. analytics_store.initStore()
  │     ├─ loadSettings() → localStorage
  │     ├─ applySettingsToState() → 联动设置
  │     ├─ initAnalyticsResourceStore() → Rust 后端
  │     └─ loadResources() + loadFolders() + loadTags()
  │
  └─→ 3. 其他插件可以安全访问 store.resources/...
```

### 在其他模块中使用前检查

```typescript
const store = useAnalyticsResourceStore()

// 方式一：等待初始化
if (!store.initialized) {
  await store.initStore()
}

// 方式二：响应式等待
watch(
  () => store.initialized,
  ready => {
    if (ready) {
      // 安全访问
      console.log(store.resources)
    }
  }
)
```

---

## 七、集成场景示例

### 7.1 SQL 查询结果自动注册为资源

```typescript
// 在 query 模块中
import { useAnalyticsResourceStore } from '@/extensions/builtin/analytics-resource/ui/stores/analytics-resource-store'

async function onQueryExecuted(result: QueryResult, sql: string) {
  const analyticsStore = useAnalyticsResourceStore()

  if (!analyticsStore.initialized) return

  const resource = await analyticsStore.createResource({
    resource_type: 'table',
    name: `result_${Date.now()}`,
    scope: 'session',
    config: {
      table_name: `result_${Date.now()}`,
      duckdb_schema: 'temp',
    },
    row_count: result.rowCount,
    column_count: result.columnCount,
    source_query: sql,
  })

  return resource
}
```

### 7.2 文件导入注册为资源

```typescript
// 在文件管理器或导入模块中
import { useAnalyticsResourceStore } from '@/extensions/builtin/analytics-resource/ui/stores/analytics-resource-store'

async function onFileImported(fileInfo: {
  path: string
  name: string
  size: number
  type: string
}) {
  const analyticsStore = useAnalyticsResourceStore()

  const resource = await analyticsStore.createResource({
    resource_type: 'file',
    name: fileInfo.name,
    scope: 'project',
    config: {
      file_path: fileInfo.path,
      file_name: fileInfo.name,
      file_type: fileInfo.type,
    },
    file_size: fileInfo.size,
  })

  // 可进一步用 DuckDB 分析该文件
  // await analyzeFileWithDuckDB(fileInfo.path);
}
```

### 7.3 数据库表提取为分析资源

```typescript
// 在 connection 或 database navigator 模块中
import { useAnalyticsResourceStore } from '@/extensions/builtin/analytics-resource/ui/stores/analytics-resource-store'

async function onTableExtracted(tableInfo: {
  connectionId: string
  schema: string
  tableName: string
  rowCount: number
  columnCount: number
}) {
  const analyticsStore = useAnalyticsResourceStore()

  await analyticsStore.createResource({
    resource_type: 'table',
    name: tableInfo.tableName,
    scope: 'project',
    config: {
      connection_id: tableInfo.connectionId,
      schema: tableInfo.schema,
      table_name: tableInfo.tableName,
    },
    row_count: tableInfo.rowCount,
    column_count: tableInfo.columnCount,
  })
}
```

### 7.4 标签管理与筛选

```typescript
// 在分析资源页面内
import { useAnalyticsResourceStore } from '@/extensions/builtin/analytics-resource/ui/stores/analytics-resource-store'

async function tagWorkflow() {
  const store = useAnalyticsResourceStore()

  // 创建新标签
  const tag = await store.createTag({
    name: '供应链',
    color: '#165DFF',
    scope: 'project',
  })

  // 给资源打标签
  await store.addTagToResource('ar_xxx', tag.id)

  // 按标签筛选资源
  const taggedResources = await store.getResourcesByTag(tag.id)

  // 查看资源有哪些标签
  const resourceTags = await store.getTagsForResource('ar_xxx')
}
```

### 7.5 版本历史查看

```typescript
// 查看资源的所有历史版本
const store = useAnalyticsResourceStore()

const versions = await store.getResourceVersions('ar_xxx')
// versions = [
//   { id: 'arv_1', resource_id: 'ar_xxx', version: 3, snapshot: {...}, created_at: '...' },
//   { id: 'arv_2', resource_id: 'ar_xxx', version: 2, snapshot: {...}, created_at: '...' },
//   { id: 'arv_3', resource_id: 'ar_xxx', version: 1, snapshot: {...}, created_at: '...' },
// ]
```

### 7.6 通过 EventBus 跨插件通信

```typescript
// 发送方（如 query 插件）
import { eventBus } from '@/extensions/core/event-bus'

eventBus.emit('analytics:resource:created', {
  resourceId: 'ar_xxx',
  resourceType: 'table',
  resourceName: 'supplier_master_v3',
})

// 接收方（如 workbench 插件）
import { eventBus } from '@/extensions/core/event-bus'

eventBus.on('analytics:resource:created', payload => {
  // 自动刷新资源视图
  const analyticsStore = useAnalyticsResourceStore()
  analyticsStore.loadResourcesPaginated()
})
```

---

## 八、缓存机制

### 8.1 缓存架构

```
resourceCache (LRU)
├── maxSize: 20（可配置 5-200）
├── ttl: 30s（可配置 10-3600s）
└── enabled: true（可配置）

folderCache (LRU)
├── maxSize: 10（可配置 5-200）
├── ttl: 60s（可配置 10-3600s）
└── enabled: true（可配置）
```

### 8.2 缓存失效时机

| 操作                         | 自动失效            |
| ---------------------------- | ------------------- |
| `createResource`             | ✅                  |
| `updateResource`             | ✅                  |
| `deleteResource`             | ✅                  |
| `batchDeleteResources`       | ✅                  |
| `cloneResource`              | ✅                  |
| `saveSettings`（改缓存配置） | 自动 `updateConfig` |
| `clearCache`（手动）         | ✅                  |

### 8.3 手动管理缓存

```typescript
const store = useAnalyticsResourceStore()

// 清除所有缓存
store.clearCache()

// 重新加载（绕过缓存）
store.invalidateResourceCache() // 内部方法
await store.loadResourcesPaginated() // 此时缓存已清，直接查后端
```

---

## 九、错误处理最佳实践

```typescript
import { useAnalyticsResourceStore } from '@/extensions/builtin/analytics-resource/ui/stores/analytics-resource-store'

async function safeCreateResource(input: CreateResourceRequest) {
  const store = useAnalyticsResourceStore()

  try {
    if (!store.initialized) {
      await store.initStore()
    }

    const resource = await store.createResource(input)
    return { success: true, data: resource }
  } catch (error) {
    console.error('创建分析资源失败:', error)
    return {
      success: false,
      error: typeof error === 'string' ? error : '未知错误',
    }
  }
}
```

---

## 十、注意事项

1. **必须初始化**: 使用 Store 前确保 `initStore()` 已调用（通常在 App 启动时）
2. **无文件大小限制**: 本模块只管理元数据指针，大文件分析由 DuckDB 引擎承担
3. **软删除机制**: `deleteResource` 不是真正删除，资源移入回收站
4. **多对多文件夹**: 同资源可属于多个文件夹（播放列表模式）
5. **缓存一致性**: Store 方法内部已处理缓存失效，无需手动管理
6. **类型安全**: 所有 API 均有完整 TypeScript 类型定义
7. **避免循环依赖**: 不要在 analytics-resource 插件内部引用其他插件的 store
8. **通过 EventBus 跨插件**: 如需通知其他插件资源变更，使用 event-bus 而非直接调用
