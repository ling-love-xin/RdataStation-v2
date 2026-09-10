# 分析资源管理器 (Analytics Resource Manager) 实现文档

## 一、概述

分析资源管理器是 RdataStation 的核心组件之一，提供统一界面管理数据库资源（连接、表、文件），支持文件夹组织、标签筛选、资源生命周期管理（回收站）和多级作用域控制。

## 二、技术架构

### 2.1 整体架构

```
┌─────────────────────────────────────────────────────────────┐
│                      Frontend (Vue 3)                        │
│  ┌──────────────────────────────────────────────────────┐   │
│  │           Analytics Resource Extension                │   │
│  │  ┌─────────────┐ ┌─────────────┐ ┌────────────────┐  │   │
│  │  │  Components │ │   Stores    │ │   API Layer    │  │   │
│  │  │ - Manager   │ │ - Pinia     │ │ - Tauri Invoke │  │   │
│  │  │ - Modals    │ │   Store     │ │                │  │   │
│  │  └─────────────┘ └─────────────┘ └────────────────┘  │   │
│  └──────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                              │
                    Tauri IPC (invoke)
                              │
┌─────────────────────────────────────────────────────────────┐
│                      Backend (Rust)                          │
│  ┌──────────────────────────────────────────────────────┐   │
│  │              Tauri Commands Layer                     │   │
│  │  ┌────────────────────────────────────────────────┐  │   │
│  │  │         analytics_resource_commands.rs          │  │   │
│  │  │  - create_analytics_resource                    │  │   │
│  │  │  - list_analytics_resources                     │  │   │
│  │  │  - delete_analytics_resource                    │  │   │
│  │  │  - ...                                          │  │   │
│  │  └────────────────────────────────────────────────┘  │   │
│  └──────────────────────────────────────────────────────┘   │
│                              │                              │
│  ┌──────────────────────────────────────────────────────┐   │
│  │            Persistence Layer                          │   │
│  │  ┌────────────────────────────────────────────────┐  │   │
│  │  │        analytics_resource_store.rs              │  │   │
│  │  │  - SQLite based storage                        │  │   │
│  │  │  - CRUD operations for all entities           │  │   │
│  │  └────────────────────────────────────────────────┘  │   │
│  └──────────────────────────────────────────────────────┘   │
│                              │                              │
│  ┌──────────────────────────────────────────────────────┐   │
│  │              Database Schema                        │   │
│  │  ┌────────────────────────────────────────────────┐  │   │
│  │  │ 007_analytics_resources.sql                    │  │   │
│  │  │  - analytics_resources                         │  │   │
│  │  │  - analytics_folders                          │  │   │
│  │  │  - analytics_resource_folder                  │  │   │
│  │  │  - analytics_tags                              │  │   │
│  │  │  - analytics_resource_tags                     │  │   │
│  │  │  - analytics_recycle_bin                       │  │   │
│  │  └────────────────────────────────────────────────┘  │   │
│  └──────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

### 2.2 资源类型

| 类型         | 说明       | 示例                              |
| ------------ | ---------- | --------------------------------- |
| `connection` | 数据库连接 | MySQL、PostgreSQL、SQLite、DuckDB |
| `table`      | 数据表     | 查询结果集、DuckDB 表             |
| `file`       | 分析文件   | CSV、Parquet、JSON                |

### 2.3 作用域

| 作用域    | 说明       | 持久化位置         |
| --------- | ---------- | ------------------ |
| `global`  | 应用级全局 | 系统数据库         |
| `project` | 项目级     | 项目 SQLite 数据库 |
| `session` | 会话级临时 | 内存/会话结束清除  |

## 三、数据库设计

### 3.1 核心表结构

```sql
-- 资源表
CREATE TABLE analytics_resources (
    id                  TEXT PRIMARY KEY,
    resource_type       TEXT NOT NULL,       -- connection / table / file
    name                TEXT NOT NULL,       -- 显示名称
    alias               TEXT,                -- 用户自定义别名
    config              TEXT NOT NULL,       -- JSON 配置
    scope               TEXT NOT NULL,       -- global / project / session
    row_count           INTEGER,
    column_count        INTEGER,
    file_size           INTEGER,
    version             INTEGER DEFAULT 1,
    parent_version_id   TEXT,
    parent_resource_id  TEXT,
    source_query        TEXT,
    created_at          TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at          TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    created_by          TEXT,
    deleted_at          TIMESTAMP,
    FOREIGN KEY (parent_resource_id) REFERENCES analytics_resources(id)
);

-- 文件夹表
CREATE TABLE analytics_folders (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    scope           TEXT NOT NULL,           -- global / project / session
    parent_folder_id TEXT,
    sort_order      INTEGER DEFAULT 0,
    color           TEXT,
    icon            TEXT,
    created_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    deleted_at      TIMESTAMP,
    FOREIGN KEY (parent_folder_id) REFERENCES analytics_folders(id)
);

-- 资源-文件夹关联表 (多对多)
CREATE TABLE analytics_resource_folder (
    resource_id     TEXT NOT NULL,
    folder_id       TEXT NOT NULL,
    added_at        TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (resource_id, folder_id),
    FOREIGN KEY (resource_id) REFERENCES analytics_resources(id),
    FOREIGN KEY (folder_id) REFERENCES analytics_folders(id)
);

-- 标签表
CREATE TABLE analytics_tags (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL UNIQUE,
    color           TEXT,
    icon            TEXT,
    scope           TEXT NOT NULL,
    created_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    deleted_at      TIMESTAMP
);

-- 资源-标签关联表 (多对多)
CREATE TABLE analytics_resource_tags (
    resource_id     TEXT NOT NULL,
    tag_id          TEXT NOT NULL,
    added_at        TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (resource_id, tag_id),
    FOREIGN KEY (resource_id) REFERENCES analytics_resources(id),
    FOREIGN KEY (tag_id) REFERENCES analytics_tags(id)
);

-- 回收站表 (软删除)
CREATE TABLE analytics_recycle_bin (
    id              TEXT PRIMARY KEY,
    resource_id     TEXT NOT NULL,
    resource_type   TEXT NOT NULL,
    resource_name   TEXT NOT NULL,
    resource_data   TEXT NOT NULL,           -- JSON 序列化的完整资源数据
    deleted_by      TEXT,
    deleted_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (resource_id) REFERENCES analytics_resources(id)
);
```

### 3.2 索引设计

```sql
-- 资源表索引
CREATE INDEX idx_resources_scope ON analytics_resources(scope);
CREATE INDEX idx_resources_type ON analytics_resources(resource_type);
CREATE INDEX idx_resources_deleted ON analytics_resources(deleted_at);
CREATE INDEX idx_resources_created ON analytics_resources(created_at DESC);

-- 文件夹表索引
CREATE INDEX idx_folders_scope ON analytics_folders(scope);
CREATE INDEX idx_folders_parent ON analytics_folders(parent_folder_id);

-- 关联表索引
CREATE INDEX idx_resource_folder_resource ON analytics_resource_folder(resource_id);
CREATE INDEX idx_resource_folder_folder ON analytics_resource_folder(folder_id);
CREATE INDEX idx_resource_tags_resource ON analytics_resource_tags(resource_id);
CREATE INDEX idx_resource_tags_tag ON analytics_resource_tags(tag_id);

-- 回收站索引
CREATE INDEX idx_recycle_deleted ON analytics_recycle_bin(deleted_at DESC);
```

## 四、后端实现

### 4.1 模块结构

```
src-tauri/src/
├── core/
│   └── persistence/
│       └── analytics_resource_store.rs    # 持久化层
└── commands/
    └── analytics_resource_commands.rs     # Tauri 命令
```

### 4.2 核心类型

```rust
// 资源结构
pub struct AnalyticsResource {
    pub id: String,
    pub resource_type: String,
    pub name: String,
    pub alias: Option<String>,
    pub config: Value,                    // serde_json::Value
    pub scope: String,
    pub row_count: Option<i64>,
    pub column_count: Option<i32>,
    pub file_size: Option<i64>,
    pub version: i32,
    pub parent_version_id: Option<String>,
    pub parent_resource_id: Option<String>,
    pub source_query: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: Option<String>,
    pub deleted_at: Option<DateTime<Utc>>,
}

// 文件夹结构
pub struct AnalyticsFolder {
    pub id: String,
    pub name: String,
    pub scope: String,
    pub parent_folder_id: Option<String>,
    pub sort_order: i32,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

// 标签结构
pub struct AnalyticsTag {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub scope: String,
    pub created_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

// 回收站项结构
pub struct AnalyticsRecycleItem {
    pub id: String,
    pub resource_id: String,
    pub resource_type: String,
    pub resource_name: String,
    pub resource_data: Value,
    pub deleted_by: Option<String>,
    pub deleted_at: DateTime<Utc>,
}
```

### 4.3 Tauri Commands

| Command                                   | 说明               | 参数                             |
| ----------------------------------------- | ------------------ | -------------------------------- |
| `create_analytics_resource`               | 创建资源           | `CreateResourceRequest`          |
| `update_analytics_resource`               | 更新资源           | `id`, `CreateResourceRequest`    |
| `get_analytics_resource`                  | 获取资源详情       | `id`                             |
| `list_analytics_resources`                | 列出资源           | `ListResourcesInput`             |
| `list_analytics_resources_paginated`      | 分页列出资源       | `ListResourcesInput`             |
| `delete_analytics_resource`               | 删除资源（软删除） | `id`                             |
| `batch_delete_analytics_resources`        | 批量删除资源       | `ids: Vec<String>`               |
| `clone_analytics_resource`                | 克隆资源           | `id`, `new_name: Option<String>` |
| `create_analytics_folder`                 | 创建文件夹         | `CreateFolderRequest`            |
| `get_analytics_folder`                    | 获取文件夹详情     | `id`                             |
| `list_analytics_folders`                  | 列出文件夹         | `ListFoldersInput`               |
| `add_analytics_resource_to_folder`        | 添加资源到文件夹   | `input: {resourceId, folderId}`  |
| `remove_analytics_resource_from_folder`   | 从文件夹移除资源   | `input: {resourceId, folderId}`  |
| `create_analytics_tag`                    | 创建标签           | `CreateTagRequest`               |
| `list_analytics_tags`                     | 列出标签           | `ListTagsInput`                  |
| `add_analytics_tag_to_resource`           | 添加标签到资源     | `input: {resourceId, tagId}`     |
| `remove_analytics_tag_from_resource`      | 从资源移除标签     | `input: {resourceId, tagId}`     |
| `get_analytics_recycle_bin`               | 获取回收站         | -                                |
| `restore_analytics_resource_from_recycle` | 从回收站恢复       | `recycleId`                      |
| `permanent_delete_analytics_resource`     | 永久删除           | `recycleId`                      |
| `init_analytics_resource_store`           | 初始化存储         | -                                |

## 五、前端实现

### 5.1 目录结构

```
src/extensions/builtin/analytics-resource/
├── extension.ts                           # 扩展入口
├── package.json                           # 扩展元数据
├── types/
│   └── index.ts                          # TypeScript 类型定义
├── infrastructure/
│   └── api/
│       └── analytics-resource-api.ts     # API 调用层
└── ui/
    ├── composables/
    │   ├── use-debounce.ts               # 防抖工具
    │   ├── use-search.ts                 # 搜索逻辑
    │   └── use-toast.ts                  # Toast 通知
    ├── components/
    │   ├── AnalyticsResourceManager.vue  # 主管理器组件
    │   ├── SearchBar.vue                 # 搜索栏组件
    │   ├── FilterBar.vue                 # 筛选栏组件
    │   ├── Pagination.vue                 # 分页组件
    │   ├── FolderList.vue                # 文件夹列表组件
    │   ├── ResourceList.vue              # 资源列表组件（虚拟滚动）
    │   ├── ContextMenu.vue               # 右键菜单组件
    │   ├── ToastContainer.vue            # Toast 通知容器
    │   ├── CreateResourceModal.vue       # 创建资源弹窗
    │   ├── CreateFolderModal.vue         # 创建文件夹弹窗
    │   └── RecycleBinModal.vue          # 回收站弹窗
    └── stores/
        └── analytics-resource-store.ts   # Pinia 状态管理
```

### 5.2 核心组件

#### AnalyticsResourceManager

主面板组件，提供：

- 搜索栏：按名称/别名搜索资源
- 标签筛选栏：作用域筛选、类型筛选
- 文件夹列表：支持文件夹选择和创建
- 资源列表：虚拟滚动支持大数据量
- 底部操作栏：添加资源、打开回收站

#### CreateResourceModal

创建资源弹窗，提供：

- 资源类型选择（连接/表/文件）
- 名称和别名输入
- 作用域选择
- 扩展属性输入（行数/列数/文件大小）
- JSON 配置编辑

#### CreateFolderModal

创建文件夹弹窗，提供：

- 文件夹名称输入
- 作用域选择
- 颜色选择器
- 图标选择器

#### RecycleBinModal

回收站弹窗，提供：

- 已删除资源列表
- 恢复功能
- 永久删除功能

### 5.3 状态管理

```typescript
// Pinia Store
export const useAnalyticsResourceStore = defineStore('analytics-resource', () => {
  // State
  const resources = ref<AnalyticsResource[]>([])
  const folders = ref<AnalyticsFolder[]>([])
  const tags = ref<AnalyticsTag[]>([])
  const recycleBin = ref<AnalyticsRecycleItem[]>([])
  const loading = ref(false)
  const initialized = ref(false)

  // Selected items
  const selectedResources = ref<string[]>([])
  const selectedFolderId = ref<string | null>(null)
  const selectedScope = ref<string | null>(null)
  const selectedType = ref<string | null>(null)

  // Pagination state
  const total = ref(0)
  const page = ref(1)
  const pageSize = ref(20)
  const totalPages = computed(() => Math.ceil(total.value / pageSize.value))

  // Sorting state
  const sortBy = ref<SortField | null>(null)
  const sortOrder = ref<SortOrder>('asc')

  // Computed
  const filteredResources = computed(() => {
    let result = resources.value
    if (selectedScope.value) {
      result = result.filter(r => r.scope === selectedScope.value)
    }
    if (selectedType.value) {
      result = result.filter(r => r.resource_type === selectedType.value)
    }
    return result
  })

  // Actions
  async function initStore() {
    /* ... */
  }
  async function loadResources(input?: ListResourcesInput) {
    /* ... */
  }
  async function loadResourcesPaginated(input?: ListResourcesInput) {
    /* ... */
  }
  async function createResource(input: CreateResourceRequest) {
    /* ... */
  }
  async function updateResource(id: string, input: CreateResourceRequest) {
    /* ... */
  }
  async function deleteResource(id: string) {
    /* ... */
  }
  async function batchDeleteResources(ids: string[]) {
    /* ... */
  }
  async function cloneResource(id: string, newName?: string) {
    /* ... */
  }
  // ... 其他 actions

  return {
    /* ... */
  }
})
```

## 六、集成方式

### 6.1 注册到扩展系统

在 `builtin-extensions.ts` 中注册：

```typescript
import analyticsResourceExtension from '@/extensions/builtin/analytics-resource/extension'

export const builtinExtensions: BuiltinExtension[] = [
  // ... 其他扩展
  { id: 'analytics-resource', module: analyticsResourceExtension },
]
```

### 6.2 面板注册

扩展激活时通过 `context.window.registerViewProvider()` 注册面板：

```typescript
const panelDisposable = context.window.registerViewProvider('analytics-resource-manager', {
  component: AnalyticsResourceManager,
  title: '分析资源管理器',
  location: 'left',
  icon: '📊',
})
```

面板会自动注册到 `panelRegistry`，WorkbenchView 在初始化时从注册表读取并创建面板。

## 七、使用流程

### 7.1 创建资源

1. 用户点击「添加资源」按钮
2. 弹出 CreateResourceModal
3. 用户填写资源信息
4. 调用 `createResource()` API
5. 资源保存到数据库，列表刷新

### 7.2 组织资源

1. 用户可以创建文件夹对资源进行分组
2. 可以将资源添加到指定文件夹
3. 可以为资源添加标签进行分类

### 7.3 删除与恢复

1. 用户选中资源后点击「删除选中」
2. 资源移动到回收站（软删除）
3. 用户可以打开回收站查看已删除资源
4. 可以选择恢复或永久删除

## 八、扩展计划

### 8.1 短期扩展

- [x] 资源拖拽排序
- [x] 批量操作支持
- [x] 资源预览功能
- [x] 资源复制/克隆
- [x] 分页功能
- [x] 排序功能

### 8.2 长期扩展

- [ ] 资源版本历史
- [ ] 资源依赖分析
- [ ] SQL 引用追踪
- [ ] 资源导入/导出

## 九、注意事项

1. **命名规范**：前端 TypeScript 类型使用 camelCase（如 `resourceType`），后端 Rust 结构体使用 snake_case（如 `resource_type`），API 调用时自动转换

2. **作用域隔离**：不同作用域的资源存储在不同位置，查询时需要根据作用域过滤

3. **软删除**：资源删除时不会立即从数据库移除，而是移入回收站，保留 30 天后自动清理

4. **虚拟滚动**：资源列表使用虚拟滚动优化大数据量渲染性能

## 十、优化特性

### 10.1 组件拆分

主组件 `AnalyticsResourceManager` 拆分为多个小型组件，提高可维护性：

| 组件             | 职责                       |
| ---------------- | -------------------------- |
| `SearchBar`      | 搜索输入，支持 Ctrl+F 聚焦 |
| `FilterBar`      | 作用域和类型筛选           |
| `FolderList`     | 文件夹选择和创建           |
| `ResourceList`   | 资源列表（虚拟滚动）       |
| `ContextMenu`    | 右键菜单                   |
| `ToastContainer` | Toast 通知                 |

### 10.2 防抖搜索

```typescript
// SearchBar 组件内置 300ms 防抖
const debounceMs = 300
// 减少 API 调用，提升性能
```

### 10.3 Toast 通知

```typescript
import { useToast } from '@/composables/use-toast'

const toast = useToast()
toast.success('操作成功')
toast.error('操作失败')
toast.warning('警告信息')
toast.info('提示信息')
```

### 10.4 虚拟滚动

`ResourceList` 组件使用虚拟滚动，只渲染可视区域内的项目：

```typescript
const visibleRange = computed(() => {
  const start = Math.floor(scrollTop.value / itemHeight)
  const visibleCount = Math.ceil(containerHeight.value / itemHeight) + 2
  // ...
})
```

### 10.5 快捷键支持

| 快捷键     | 功能                |
| ---------- | ------------------- |
| `Ctrl + F` | 聚焦搜索框          |
| `Delete`   | 删除选中资源        |
| `Escape`   | 清空搜索 / 关闭弹窗 |

### 10.6 右键菜单

支持资源项右键菜单：

- 打开
- 编辑
- 复制
- 删除（危险操作红色标识）

### 10.7 分页功能

`Pagination` 组件提供完整的分页支持：

| 功能          | 说明                    |
| ------------- | ----------------------- |
| 页码显示      | 显示总页数和当前页码    |
| 页大小选择    | 支持 10/20/50/100 条/页 |
| 上一页/下一页 | 基础导航                |
| 跳转页码      | 点击页码直接跳转        |
| 省略号        | 超过7页时显示省略号     |

```typescript
interface PaginationInput {
  page?: number
  pageSize?: number
}

interface ListResourcesOutput {
  items: AnalyticsResource[]
  total: number
  page: number
  pageSize: number
  totalPages: number
}
```

**后端实现**：

```rust
pub async fn list_resources_paginated(
    &self,
    scope: Option<&str>,
    resource_type: Option<&str>,
    folder_id: Option<&str>,
    search: Option<&str>,
    page: i64,
    page_size: i64,
    sort_by: Option<&str>,
    sort_order: Option<&str>,
) -> Result<ListResourcesOutput, CoreError> {
    let sort_field = match sort_by {
        Some("name") => "name",
        Some("created_at") => "created_at",
        Some("updated_at") => "updated_at",
        Some("row_count") => "row_count",
        Some("file_size") => "file_size",
        _ => "created_at",
    };
    let sort_dir = match sort_order {
        Some("desc") => "DESC",
        _ => "ASC",
    };
    let offset = (page - 1) * page_size;
    // SQL 查询带 LIMIT 和 OFFSET
}
```

### 10.8 排序功能

支持多字段排序：

| 排序字段     | 说明       |
| ------------ | ---------- |
| `name`       | 按资源名称 |
| `created_at` | 按创建时间 |
| `updated_at` | 按更新时间 |
| `row_count`  | 按行数     |
| `file_size`  | 按文件大小 |

排序方向：`asc`（升序）或 `desc`（降序）

```typescript
interface SortInput {
  sortBy?: SortField
  sortOrder?: SortOrder
}
```

### 10.9 批量操作

支持多选资源的批量删除：

```typescript
async function batchDeleteResources(ids: string[]) {
  try {
    loading.value = true
    await analyticsApi.batchDeleteResources(ids)
    resources.value = resources.value.filter(r => !ids.includes(r.id))
    selectedResources.value = selectedResources.value.filter(id => !ids.includes(id))
    total.value -= ids.length
  } catch (error) {
    console.error('Failed to batch delete resources:', error)
    throw error
  } finally {
    loading.value = false
  }
}
```

**UI 交互**：

- 选中资源时显示选中数量
- 提供「批量删除」按钮
- 提供「清空选择」按钮
- 删除前二次确认

### 10.10 资源克隆

支持复制现有资源：

```typescript
async function cloneResource(id: string, newName?: string) {
  try {
    loading.value = true
    const cloned = await analyticsApi.cloneAnalyticsResource(id, newName)
    resources.value.push(cloned)
    total.value += 1
    return cloned
  } catch (error) {
    console.error('Failed to clone resource:', error)
    throw error
  } finally {
    loading.value = false
  }
}
```

克隆行为：

- 生成新的资源 ID
- 默认名称为「原名称 (副本)」
- 可指定自定义名称
- 复制所有配置和元数据
- 创建时间更新为当前时间

---

_文档版本：1.2.0_
_最后更新：2026-05-05_
