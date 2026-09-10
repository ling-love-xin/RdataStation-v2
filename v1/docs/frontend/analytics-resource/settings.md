# 分析资源管理器 - 设置功能文档

> 版本：v1.7
> 最后更新：2026-05-12
> 状态：✅ 设置联动生效 + 快捷键全部激活 + IPC 版本协商 (const ANALYTICS_RESOURCE_API_VERSION)

---

## 概述

分析资源管理器提供了丰富的设置选项，允许用户自定义资源管理的行为和外观。设置功能分为四个主要类别：通用设置、显示设置、缓存设置和快捷键。

**所有设置保存后立即生效**，无需刷新页面。

## 访问设置

点击页面右上角的 **⚙️ 设置** 按钮即可打开设置面板。

---

## 设置类别

### 1. 通用设置

| 设置项       | 说明                       | 默认值     | 联动位置                             |
| ------------ | -------------------------- | ---------- | ------------------------------------ |
| 默认作用域   | 新建资源时的默认作用域     | project    | `CreateResourceModal` 初始化         |
| 每页显示数量 | 资源列表每页显示的资源数量 | 20         | `loadResourcesPaginated` 的 pageSize |
| 默认排序字段 | 资源列表默认排序字段       | created_at | 初始化 `sortBy`                      |
| 默认排序方向 | 资源列表默认排序方向       | desc       | 初始化 `sortOrder`                   |

**作用域说明：**

- **🌍 全局**：资源在所有项目中可见
- **📂 项目**：资源仅在当前项目中可见（推荐）
- **📌 会话**：资源仅在当前会话期间存在

### 2. 显示设置

| 设置项         | 说明                           | 默认值  | 联动位置                     |
| -------------- | ------------------------------ | ------- | ---------------------------- |
| 显示资源图标   | 在资源列表中显示资源类型图标   | ✅ 开启 | `ResourceList` 图标 `v-if`   |
| 显示作用域标签 | 在资源列表中显示作用域标签     | ✅ 开启 | `ResourceList` 标签 `v-if`   |
| 显示资源元数据 | 显示行数、列数、文件大小等信息 | ✅ 开启 | `ResourceList` 元数据 `v-if` |
| 启用虚拟滚动   | 大数据量时启用虚拟滚动优化性能 | ✅ 开启 | 控制虚拟滚动 / 普通列表切换  |

**开关说明**：

- 关闭图标后，资源列表变得更紧凑
- 关闭元数据后，仅显示资源名称
- 关闭虚拟滚动后，使用普通 DOM 列表渲染

### 3. 缓存设置

| 设置项       | 说明                       | 默认值  | 联动位置                                         |
| ------------ | -------------------------- | ------- | ------------------------------------------------ |
| 启用查询缓存 | 缓存查询结果以提升响应速度 | ✅ 开启 | `resourceCache.enabled` 和 `folderCache.enabled` |
| 缓存过期时间 | 缓存自动过期时间（秒）     | 30      | `resourceCache.ttl` 和 `folderCache.ttl`         |
| 最大缓存数量 | 缓存的最大条目数           | 50      | `resourceCache.maxSize` 和 `folderCache.maxSize` |

**缓存机制说明：**

- 缓存采用 LRU（最近最少使用）策略
- 当资源发生变更（创建/更新/删除）时，缓存自动失效
- 禁用缓存后，每次查询都会直接访问后端
- 可点击 **清除缓存** 按钮手动清除所有缓存

### 4. 快捷键（全部已实现）

| 快捷键         | 功能           | 状态         |
| -------------- | -------------- | ------------ |
| `Ctrl+N`       | 新建资源       | ✅ 已实现    |
| `Ctrl+E`       | 编辑选中的资源 | ✅ 已实现    |
| `Ctrl+D`       | 删除选中的资源 | ✅ 已实现    |
| `Ctrl+Shift+C` | 克隆选中的资源 | ✅ 已实现    |
| `Ctrl+Shift+V` | 查看版本历史   | ✅ 已实现 🆕 |
| `Ctrl+F`       | 聚焦搜索框     | ✅ 已实现    |
| `Ctrl+A`       | 全选资源       | ✅ 已实现    |
| `Delete`       | 删除选中的资源 | ✅ 已实现    |

**使用方式**：

- `Ctrl+E`、`Ctrl+Shift+C` 和 `Ctrl+Shift+V` 需要先选中一个资源
- `Ctrl+D` 和 `Delete` 需要至少选中一个资源
- `Ctrl+N` 和 `Ctrl+F` 无需选中资源

---

## 设置联动机制

### 加载流程

```
1. initStore() 调用
2. loadSettings() → 从 localStorage 读取设置
3. applySettingsToState() → 将设置应用到运行时状态
    ├── pageSize = settings.general.defaultPageSize
    ├── sortBy = settings.general.defaultSortField
    ├── sortOrder = settings.general.defaultSortOrder
    ├── resourceCache.updateConfig({ enabled, ttl, maxSize })
    └── folderCache.updateConfig({ enabled, ttl, maxSize })
4. 显示设置通过 props 传递给 ResourceList
```

### 保存流程

```
1. SettingsModal 点击保存
2. handleSaveSettings() 调用 store.saveSettings()
3. localStorage.setItem() 持久化
4. applySettingsToState() 立即应用
5. UI 自动响应变化
```

---

## 设置持久化

所有设置会自动保存到浏览器 localStorage，键名为 `analytics_resource_settings`。

刷新页面后，`initStore()` 会自动加载并应用已保存的设置。

## 重置设置

点击设置面板底部的 **重置为默认** 按钮可恢复所有设置为默认值，并立即生效。

---

## API 接口

### 设置数据结构

```typescript
interface AnalyticsResourceSettings {
  general: {
    defaultScope: 'global' | 'project' | 'session'
    defaultPageSize: number // 10 | 20 | 50 | 100
    defaultSortField: 'name' | 'created_at' | 'updated_at' | 'row_count' | 'file_size'
    defaultSortOrder: 'asc' | 'desc'
  }
  display: {
    showIcons: boolean
    showScopeTags: boolean
    showMetadata: boolean
    enableVirtualScroll: boolean
  }
  cache: {
    enabled: boolean
    ttlSeconds: number // 10 - 3600
    maxSize: number // 5 - 200
  }
}

const DEFAULT_SETTINGS: AnalyticsResourceSettings = {
  general: {
    defaultScope: 'project',
    defaultPageSize: 20,
    defaultSortField: 'created_at',
    defaultSortOrder: 'desc',
  },
  display: {
    showIcons: true,
    showScopeTags: true,
    showMetadata: true,
    enableVirtualScroll: true,
  },
  cache: {
    enabled: true,
    ttlSeconds: 30,
    maxSize: 50,
  },
}
```

### Store 方法

| 方法                     | 说明                           | 参数                                  |
| ------------------------ | ------------------------------ | ------------------------------------- |
| `loadSettings()`         | 从 localStorage 加载设置到内存 | 无                                    |
| `saveSettings(settings)` | 保存并立即应用设置             | `settings: AnalyticsResourceSettings` |
| `resetSettings()`        | 重置为默认值并立即应用         | 无                                    |
| `applySettingsToState()` | 将设置应用到运行时状态         | 无（内部方法）                        |

### 缓存 API

```typescript
interface CacheConfig {
  maxSize?: number;   // 最大缓存条目
  ttl?: number;       // 过期时间（毫秒）
  enabled?: boolean;  // 是否启用
}

// useCache 暴露的方法
{
  get(key: string): T | null;
  set(key: string, data: T): void;
  has(key: string): boolean;
  delete(key: string): void;
  clear(): void;
  updateConfig(config: Partial<CacheConfig>): void;  // 动态更新配置
  stats(): { hits, misses, total, hitRate, size };
}
```

---

## 组件 Props

### ResourceList 新增 Props

```typescript
interface ResourceListProps {
  displaySettings?: AnalyticsResourceDisplaySettings
  // ... 其他 props
}
```

---

## 文件结构

```
src/extensions/builtin/analytics-resource/
├── ui/
│   ├── components/
│   │   ├── SettingsModal.vue               # 设置模态框组件
│   │   ├── ResourceList.vue               # 资源列表（响应显示设置）
│   │   └── AnalyticsResourceManager.vue    # 主管理器（键盘快捷键）
│   ├── stores/
│   │   └── analytics-resource-store.ts     # 设置状态管理 + 联动
│   └── composables/
│       └── use-cache.ts                    # LRU 缓存 + updateConfig
├── types/
│   └── index.ts                            # 设置类型定义 + DEFAULT_SETTINGS
└── infrastructure/
    └── api/
        └── analytics-resource-api.ts       # 后端 API
```

---

## 最新变更记录（v1.1）

### 新增功能

- ✅ 设置保存后立即生效，无需刷新
- ✅ 通用设置联动：pageSize、sortBy、sortOrder 动态更新
- ✅ 显示设置联动：图标、标签、元数据、虚拟滚动动态切换
- ✅ 缓存设置联动：缓存开关、TTL、容量动态调整
- ✅ 全部快捷键已实现：Ctrl+N/E/D/Shift+C/Shift+V/F/A

### 技术变更

- 新增 `applySettingsToState()` 方法
- 新增 `useCache.updateConfig()` 动态配置方法
- `ResourceList` 新增 `displaySettings` prop
- `initStore()` 自动加载并应用设置

---

## 注意事项

1. 设置变更立即生效，影响当前页面的所有实例
2. 缓存设置仅影响前端，不影响后端数据
3. 建议根据实际使用场景调整缓存过期时间
4. 虚拟滚动在资源数量 > 100 时效果明显
5. 禁用缓存会导致每次查询都访问后端，增加延迟
