# 分析资源管理器 — API 接口参考

> 版本：v1.7
> 最后更新：2026-05-12
> 状态：✅ 26 个 Tauri Command（25 resource + 1 IPC version）+ 25 个前端 API 方法

---

## 一、架构概览

```
Frontend (TS)                    IPC                     Backend (Rust)
─────────────────────────────────────────────────────────────────────────
analytics-resource-api.ts  →  tauri.invoke()  →  analytics_resource_commands.rs
        │                                                 │
        │                                          AnalyticsResourceState
        │                                          (Arc<Mutex<Option<Store>>>)
        │                                                 │
        │                                    analytics_resource_store.rs
        │                                    (rusqlite → project.db)
```

### 通信协议

- **传输层**: Tauri 2.x IPC Bridge
- **序列化**: Serde JSON（自动）
- **并发控制**: tokio::sync::Mutex（Rust 侧），async/await（TS 侧）
- **错误格式**: `Result<T, String>`（Rust 侧自动转 `Result<T, string>`）

---

## 二、数据类型

### 2.1 核心类型（Rust ⇄ TypeScript 对应）

| Rust                | TypeScript            | 说明                  |
| ------------------- | --------------------- | --------------------- |
| `String`            | `string`              | 文本                  |
| `Option<String>`    | `string \| undefined` | 可选文本              |
| `i64` / `u64`       | `number`              | 整数（JSON 安全范围） |
| `bool`              | `boolean`             | 布尔                  |
| `Vec<T>`            | `T[]`                 | 数组                  |
| `DateTime<Utc>`     | `string`（ISO 8601）  | 时间戳                |
| `serde_json::Value` | `Record<string, any>` | JSON 对象             |

### 2.2 AnalyticsResource（资源实体）

```typescript
interface AnalyticsResource {
  id: string // 格式: "ar_{uuid}"
  resource_type: ResourceType // "connection" | "table" | "file"
  name: string
  alias?: string
  config: Record<string, any> // JSON 配置（连接ID/表名/文件路径等）
  scope: ResourceScope // "global" | "project" | "session"
  row_count?: number
  column_count?: number
  file_size?: number
  version: number // 默认 1，每次更新 +1
  parent_version_id?: string // 上一版本资源 ID
  parent_resource_id?: string // 克隆来源资源 ID
  source_query?: string // 数据来源 SQL
  created_at: string // ISO 8601
  updated_at: string // ISO 8601，触发器自动更新
  created_by?: string
  deleted_at?: string // 非 null 表示已软删除
}
```

### 2.3 AnalyticsFolder（文件夹）

```typescript
interface AnalyticsFolder {
  id: string // 格式: "af_{uuid}"
  name: string
  scope: ResourceScope
  parent_folder_id?: string // 自引用树形结构
  sort_order: number
  color?: string
  icon?: string
  created_at: string
  updated_at: string
  deleted_at?: string
}
```

### 2.4 AnalyticsTag（标签）

```typescript
interface AnalyticsTag {
  id: string // 格式: "at_{uuid}"
  name: string
  color?: string
  icon?: string
  scope: ResourceScope
  created_at: string
  deleted_at?: string
}
```

### 2.5 AnalyticsRecycleItem（回收站条目）

```typescript
interface AnalyticsRecycleItem {
  id: string // 格式: "rb_{uuid}"
  resource_id: string // 原始资源 ID
  resource_type: string
  resource_name: string
  resource_data: Record<string, unknown> // 删除时的完整快照
  deleted_by?: string
  deleted_at: string
}
```

### 2.6 ResourceVersion（版本快照）

```typescript
interface ResourceVersion {
  id: string // 格式: "arv_{uuid}"
  resource_id: string
  version: number
  snapshot: Record<string, unknown> // 该版本的完整 JSON 快照
  created_at: string
}
```

### 2.7 请求类型

```typescript
interface CreateResourceRequest {
  resource_type: ResourceType
  name: string
  alias?: string
  config: Record<string, any>
  scope: ResourceScope
  row_count?: number
  column_count?: number
  file_size?: number
  parent_resource_id?: string
  source_query?: string
}

interface CreateFolderRequest {
  name: string
  scope: ResourceScope
  parent_folder_id?: string
  color?: string
  icon?: string
}

interface CreateTagRequest {
  name: string
  color?: string
  icon?: string
  scope: ResourceScope
}
```

### 2.8 输入/输出类型

```typescript
interface ListResourcesInput {
  scope?: string
  resource_type?: string
  folder_id?: string
  search?: string
  pagination?: { page?: number; pageSize?: number }
  sort?: { sortBy?: string; sortOrder?: string }
}

interface ListResourcesOutput {
  items: AnalyticsResource[]
  total: number
  page: number
  pageSize: number
  totalPages: number
}

interface ListFoldersInput {
  scope?: string
  parent_folder_id?: string
}

interface ListTagsInput {
  scope?: string
}
```

---

## 三、资源 API（8 个命令）

### 3.1 create_analytics_resource

创建新资源。

| 属性             | 值                                    |
| ---------------- | ------------------------------------- |
| **Rust Command** | `create_analytics_resource`           |
| **TS 函数**      | `createAnalyticsResource(input)`      |
| **参数**         | `input: CreateResourceRequest`        |
| **返回**         | `AnalyticsResource`                   |
| **错误**         | `"分析资源存储未初始化"` / 数据库错误 |

```typescript
import { createAnalyticsResource } from './analytics-resource-api'

const resource = await createAnalyticsResource({
  resource_type: 'table',
  name: 'supplier_master_v3',
  scope: 'project',
  config: {
    table_name: 'supplier_master_v3',
    duckdb_schema: 'persist',
  },
  row_count: 45000,
  column_count: 12,
})
```

---

### 3.2 update_analytics_resource

更新已有资源。更新前自动保存当前版本快照。

| 属性             | 值                                                             |
| ---------------- | -------------------------------------------------------------- |
| **Rust Command** | `update_analytics_resource`                                    |
| **TS 函数**      | `updateAnalyticsResource(id, input)`                           |
| **参数**         | `id: string`, `input: CreateResourceRequest`                   |
| **返回**         | `AnalyticsResource`                                            |
| **副作用**       | 自动保存旧版本快照到 `analytics_resource_versions`，version +1 |

```typescript
const updated = await updateAnalyticsResource('ar_xxx', {
  ...existingConfig,
  name: 'supplier_master_v4',
  row_count: 46000,
})
```

---

### 3.3 get_analytics_resource

获取单个资源详情。

| 属性             | 值                         |
| ---------------- | -------------------------- |
| **Rust Command** | `get_analytics_resource`   |
| **TS 函数**      | `getAnalyticsResource(id)` |
| **参数**         | `id: string`               |
| **返回**         | `AnalyticsResource`        |
| **错误**         | 资源不存在时抛错           |

---

### 3.4 list_analytics_resources

列出资源（不带分页）。

| 属性             | 值                                              |
| ---------------- | ----------------------------------------------- |
| **Rust Command** | `list_analytics_resources`                      |
| **TS 函数**      | `listAnalyticsResources(input)`                 |
| **参数**         | `input: { scope?, resource_type?, folder_id? }` |
| **返回**         | `AnalyticsResource[]`                           |
| **注意**         | 使用参数化查询，排除已删除资源                  |

---

### 3.5 list_analytics_resources_paginated

分页列出资源（推荐）。

| 属性             | 值                                       |
| ---------------- | ---------------------------------------- |
| **Rust Command** | `list_analytics_resources_paginated`     |
| **TS 函数**      | `listAnalyticsResourcesPaginated(input)` |
| **参数**         | `input: ListResourcesInput`              |
| **返回**         | `ListResourcesOutput`                    |

```typescript
const result = await listAnalyticsResourcesPaginated({
  scope: 'project',
  resource_type: 'table',
  search: 'supplier',
  pagination: { page: 1, pageSize: 20 },
  sort: { sortBy: 'created_at', sortOrder: 'desc' },
})
// result = { items: [...], total: 100, page: 1, pageSize: 20, totalPages: 5 }
```

---

### 3.6 delete_analytics_resource

软删除资源（移入回收站）。

| 属性             | 值                                                                            |
| ---------------- | ----------------------------------------------------------------------------- |
| **Rust Command** | `delete_analytics_resource`                                                   |
| **TS 函数**      | `deleteAnalyticsResource(id)`                                                 |
| **参数**         | `id: string`                                                                  |
| **返回**         | `void`                                                                        |
| **副作用**       | 插入回收站快照 → 标记 `deleted_at` → 清理 `resource_folder` + `resource_tags` |

```typescript
await deleteAnalyticsResource('ar_xxx')
```

---

### 3.7 batch_delete_analytics_resources

批量软删除资源（事务保护）。

| 属性             | 值                                   |
| ---------------- | ------------------------------------ |
| **Rust Command** | `batch_delete_analytics_resources`   |
| **TS 函数**      | `batchDeleteResources(ids)`          |
| **参数**         | `ids: string[]`                      |
| **返回**         | `void`                               |
| **事务**         | BEGIN → 逐个处理 → COMMIT / ROLLBACK |

```typescript
await batchDeleteResources(['ar_xxx', 'ar_yyy', 'ar_zzz'])
```

---

### 3.8 clone_analytics_resource

克隆资源。

| 属性             | 值                                                        |
| ---------------- | --------------------------------------------------------- |
| **Rust Command** | `clone_analytics_resource`                                |
| **TS 函数**      | `cloneAnalyticsResource(id, newName?)`                    |
| **参数**         | `id: string`, `newName?: string`                          |
| **返回**         | `AnalyticsResource`（新 ID，`parent_resource_id` 指向源） |

---

## 四、文件夹 API（5 个命令）

### 4.1 create_analytics_folder

| 属性             | 值                             |
| ---------------- | ------------------------------ |
| **Rust Command** | `create_analytics_folder`      |
| **TS 函数**      | `createAnalyticsFolder(input)` |
| **参数**         | `input: CreateFolderRequest`   |
| **返回**         | `AnalyticsFolder`              |

```typescript
const folder = await createAnalyticsFolder({
  name: '供应链分析',
  scope: 'project',
  parent_folder_id: undefined,
  color: '#165DFF',
  icon: 'folder',
})
```

---

### 4.2 get_analytics_folder

| 属性             | 值                       |
| ---------------- | ------------------------ |
| **Rust Command** | `get_analytics_folder`   |
| **TS 函数**      | `getAnalyticsFolder(id)` |
| **参数**         | `id: string`             |
| **返回**         | `AnalyticsFolder`        |

---

### 4.3 list_analytics_folders

| 属性             | 值                            |
| ---------------- | ----------------------------- |
| **Rust Command** | `list_analytics_folders`      |
| **TS 函数**      | `listAnalyticsFolders(input)` |
| **参数**         | `input: ListFoldersInput`     |
| **返回**         | `AnalyticsFolder[]`           |

---

### 4.4 add_analytics_resource_to_folder

将资源关联到文件夹（多对多，同资源可属于多个文件夹）。

| 属性             | 值                                                   |
| ---------------- | ---------------------------------------------------- |
| **Rust Command** | `add_analytics_resource_to_folder`                   |
| **TS 函数**      | `addAnalyticsResourceToFolder(resourceId, folderId)` |
| **参数**         | `resourceId: string`, `folderId: string`             |
| **返回**         | `void`                                               |

---

### 4.5 remove_analytics_resource_from_folder

从文件夹解除资源关联。

| 属性             | 值                                                        |
| ---------------- | --------------------------------------------------------- |
| **Rust Command** | `remove_analytics_resource_from_folder`                   |
| **TS 函数**      | `removeAnalyticsResourceFromFolder(resourceId, folderId)` |
| **参数**         | `resourceId: string`, `folderId: string`                  |
| **返回**         | `void`                                                    |

---

## 五、标签 API（4 个命令）

### 5.1 create_analytics_tag

| 属性             | 值                          |
| ---------------- | --------------------------- |
| **Rust Command** | `create_analytics_tag`      |
| **TS 函数**      | `createAnalyticsTag(input)` |
| **参数**         | `input: CreateTagRequest`   |
| **返回**         | `AnalyticsTag`              |

---

### 5.2 list_analytics_tags

| 属性             | 值                         |
| ---------------- | -------------------------- |
| **Rust Command** | `list_analytics_tags`      |
| **TS 函数**      | `listAnalyticsTags(input)` |
| **参数**         | `input: ListTagsInput`     |
| **返回**         | `AnalyticsTag[]`           |

---

### 5.3 add_analytics_tag_to_resource

| 属性             | 值                                             |
| ---------------- | ---------------------------------------------- |
| **Rust Command** | `add_analytics_tag_to_resource`                |
| **TS 函数**      | `addAnalyticsTagToResource(resourceId, tagId)` |
| **参数**         | `resourceId: string`, `tagId: string`          |
| **返回**         | `void`                                         |

---

### 5.4 remove_analytics_tag_from_resource

| 属性             | 值                                                  |
| ---------------- | --------------------------------------------------- |
| **Rust Command** | `remove_analytics_tag_from_resource`                |
| **TS 函数**      | `removeAnalyticsTagFromResource(resourceId, tagId)` |
| **参数**         | `resourceId: string`, `tagId: string`               |
| **返回**         | `void`                                              |

### 4.5 get_analytics_tag

| 属性             | 值                    |
| ---------------- | --------------------- |
| **Rust Command** | `get_analytics_tag`   |
| **TS 函数**      | `getAnalyticsTag(id)` |
| **参数**         | `id: string`          |
| **返回**         | `AnalyticsTag`        |

---

## 六、回收站 API（3 个命令）

### 6.1 get_analytics_recycle_bin

| 属性             | 值                          |
| ---------------- | --------------------------- |
| **Rust Command** | `get_analytics_recycle_bin` |
| **TS 函数**      | `getAnalyticsRecycleBin()`  |
| **参数**         | 无                          |
| **返回**         | `AnalyticsRecycleItem[]`    |

---

### 6.2 restore_analytics_resource_from_recycle

从回收站恢复资源（事务保护）。

| 属性             | 值                                                  |
| ---------------- | --------------------------------------------------- |
| **Rust Command** | `restore_analytics_resource_from_recycle`           |
| **TS 函数**      | `restoreAnalyticsResourceFromRecycle(recycleId)`    |
| **参数**         | `recycleId: string`                                 |
| **返回**         | `AnalyticsResource`                                 |
| **事务**         | 从快照重建资源 → 清除 `deleted_at` → 删除回收站条目 |

```typescript
const restored = await restoreAnalyticsResourceFromRecycle('rb_xxx')
```

---

### 6.3 permanent_delete_analytics_resource

永久删除（不可恢复）。

| 属性             | 值                                            |
| ---------------- | --------------------------------------------- |
| **Rust Command** | `permanent_delete_analytics_resource`         |
| **TS 函数**      | `permanentDeleteAnalyticsResource(recycleId)` |
| **参数**         | `recycleId: string`                           |
| **返回**         | `void`                                        |

---

## 七、版本历史 API（1 个命令）

### 7.1 get_resource_versions

获取资源的所有历史版本。

| 属性             | 值                                          |
| ---------------- | ------------------------------------------- |
| **Rust Command** | `get_resource_versions`                     |
| **TS 函数**      | `getResourceVersions(resourceId)`           |
| **参数**         | `resourceId: string`                        |
| **返回**         | `ResourceVersion[]`（按 version DESC 排序） |

---

## 八、双向查询 API（2 个命令）🆕

### 8.1 get_tags_for_resource

获取某个资源关联的标签列表。

| 属性             | 值                               |
| ---------------- | -------------------------------- |
| **Rust Command** | `get_tags_for_resource`          |
| **TS 函数**      | `getTagsForResource(resourceId)` |
| **参数**         | `resourceId: string`             |
| **返回**         | `AnalyticsTag[]`                 |

```typescript
const tags = await getTagsForResource('ar_xxx')
// tags = [{ id: 'at_1', name: '供应链', color: '#165DFF', ... }]
```

---

### 8.2 get_resources_by_tag

获取某个标签关联的资源列表。

| 属性             | 值                         |
| ---------------- | -------------------------- |
| **Rust Command** | `get_resources_by_tag`     |
| **TS 函数**      | `getResourcesByTag(tagId)` |
| **参数**         | `tagId: string`            |
| **返回**         | `AnalyticsResource[]`      |

---

## 九、初始化 API（1 个命令）

### 9.1 init_analytics_resource_store

初始化分析资源存储（每个项目启动时调用一次）。

| 属性             | 值                                    |
| ---------------- | ------------------------------------- |
| **Rust Command** | `init_analytics_resource_store`       |
| **TS 函数**      | `initAnalyticsResourceStore()`        |
| **参数**         | 无（从 `project_state` 获取项目路径） |
| **返回**         | `void`                                |
| **幂等**         | 已初始化时直接返回                    |

---

## 十、错误处理

### 10.1 错误格式

所有命令返回 `Result<T, String>`，TS 侧为 `Promise<T>`（成功）或 throw `string`（失败）。

```typescript
try {
  const resource = await getAnalyticsResource('ar_xxx')
} catch (error) {
  // error 类型为 string
  console.error('获取资源失败:', error)
}
```

### 10.2 常见错误信息

| 错误信息                    | 原因                                   | 解决                 |
| --------------------------- | -------------------------------------- | -------------------- |
| `"分析资源存储未初始化"`    | `init_analytics_resource_store` 未调用 | 确保 App 启动时调用  |
| `"项目存储未初始化"`        | `project_state` 未就绪                 | 检查项目加载流程     |
| `"Resource not found: xxx"` | 资源 ID 不存在或已删除                 | 检查 ID 或恢复回收站 |
| `"Folder not found: xxx"`   | 文件夹不存在                           | 检查 ID              |
| `"Tag not found: xxx"`      | 标签不存在                             | 检查 ID              |

---

## 十一、完整命令索引

| #   | Rust Command                              | TS 函数                                   | 模块     | v1.1 |
| --- | ----------------------------------------- | ----------------------------------------- | -------- | ---- |
| 1   | `init_analytics_resource_store`           | `initAnalyticsResourceStore()`            | 初始化   | ✅   |
| 2   | `create_analytics_resource`               | `createAnalyticsResource(input)`          | 资源     | ✅   |
| 3   | `update_analytics_resource`               | `updateAnalyticsResource(id, input)`      | 资源     | ✅   |
| 4   | `get_analytics_resource`                  | `getAnalyticsResource(id)`                | 资源     | ✅   |
| 5   | `list_analytics_resources`                | `listAnalyticsResources(input)`           | 资源     | ✅   |
| 6   | `list_analytics_resources_paginated`      | `listAnalyticsResourcesPaginated(input)`  | 资源     | ✅   |
| 7   | `delete_analytics_resource`               | `deleteAnalyticsResource(id)`             | 资源     | ✅   |
| 8   | `batch_delete_analytics_resources`        | `batchDeleteResources(ids)`               | 资源     | ✅   |
| 9   | `clone_analytics_resource`                | `cloneAnalyticsResource(id, name?)`       | 资源     | ✅   |
| 10  | `create_analytics_folder`                 | `createAnalyticsFolder(input)`            | 文件夹   | ✅   |
| 11  | `get_analytics_folder`                    | `getAnalyticsFolder(id)`                  | 文件夹   | ✅   |
| 12  | `list_analytics_folders`                  | `listAnalyticsFolders(input)`             | 文件夹   | ✅   |
| 13  | `add_analytics_resource_to_folder`        | `addAnalyticsResourceToFolder(...)`       | 文件夹   | ✅   |
| 14  | `remove_analytics_resource_from_folder`   | `removeAnalyticsResourceFromFolder(...)`  | 文件夹   | ✅   |
| 15  | `create_analytics_tag`                    | `createAnalyticsTag(input)`               | 标签     | ✅   |
| 16  | `list_analytics_tags`                     | `listAnalyticsTags(input)`                | 标签     | ✅   |
| 17  | `add_analytics_tag_to_resource`           | `addAnalyticsTagToResource(...)`          | 标签     | ✅   |
| 18  | `remove_analytics_tag_from_resource`      | `removeAnalyticsTagFromResource(...)`     | 标签     | ✅   |
| 19  | `get_analytics_tag`                       | `getAnalyticsTag(id)`                     | 标签     | 🆕   |
| 20  | `get_analytics_recycle_bin`               | `getAnalyticsRecycleBin()`                | 回收站   | ✅   |
| 21  | `restore_analytics_resource_from_recycle` | `restoreAnalyticsResourceFromRecycle(id)` | 回收站   | ✅   |
| 22  | `permanent_delete_analytics_resource`     | `permanentDeleteAnalyticsResource(id)`    | 回收站   | ✅   |
| 23  | `get_resource_versions`                   | `getResourceVersions(resourceId)`         | 版本     | 🆕   |
| 24  | `get_tags_for_resource`                   | `getTagsForResource(resourceId)`          | 双向查询 | 🆕   |
| 25  | `get_resources_by_tag`                    | `getResourcesByTag(tagId)`                | 双向查询 | 🆕   |

---

## 十二、注意事项

1. **必须先调用 `init_analytics_resource_store`**，其他命令依赖此初始化
2. 所有命令都是 **async**，不要在阻塞上下文中调用
3. **删除是软删除**，资源移入 `analytics_recycle_bin`，`deleted_at` 标记时间
4. **更新资源自动保存版本快照**，`version` 字段自增
5. 资源 → 文件夹是 **多对多** 关系，同资源可属于多个文件夹
6. **无文件大小限制** — 本模块仅管理元数据指针，大文件分析由 DuckDB 引擎承担
7. TS 侧建议通过 **Store（Pinia）** 间接调用，而非直接使用 api 函数，以获得缓存和状态一致性
