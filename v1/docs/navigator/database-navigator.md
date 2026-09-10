# 数据库导航模块文档

> 版本：v1.0
> 最后更新：2026-05-03
> 状态：✅ 持续更新

> 本文档描述 RdataStation 数据库导航模块的完整实现与优化建议

---

## 📋 目录

- [1. 模块概述](#1-模块概述)
- [2. 架构设计](#2-架构设计)
- [3. 目录结构](#3-目录结构)
- [4. 核心功能](#4-核心功能)
- [5. 类型定义](#5-类型定义)
- [6. Composable 层](#6-composable-层)
- [7. 组件层](#7-组件层)
- [8. API 通信层](#8-api-通信层)
- [9. 工具函数](#9-工具函数)
- [10. 性能优化](#10-性能优化)
- [11. 无障碍访问](#11-无障碍访问)
- [12. 已实现功能清单](#12-已实现功能清单)
- [13. 优化建议](#13-优化建议)
- [14. 开发规范](#14-开发规范)

---

## 1. 模块概述

数据库导航模块是 RdataStation 的核心功能之一，提供数据库连接管理、元数据浏览、数据预览、SQL 生成等功能。

### 1.1 设计目标

- **专业**: 对标 DBeaver/DataGrip 体验
- **高效**: 大数据量下流畅操作
- **可扩展**: 支持插件化扩展
- **统一**: 遵循企业级前端规范

### 1.2 技术栈

| 技术            | 版本   | 用途     |
| --------------- | ------ | -------- |
| Vue             | 3.5.13 | UI 框架  |
| TypeScript      | 5.8.3  | 类型安全 |
| Pinia           | 2.3.1  | 状态管理 |
| Naive UI        | latest | 基础组件 |
| Lucide Vue Next | latest | 图标库   |
| Tauri           | 2.10.3 | 桌面通信 |

---

## 2. 架构设计

### 2.1 分层架构

```
┌─────────────────────────────────────────────────────────────┐
│                      页面层 (Views)                          │
│                   DatabaseNavigator.vue                     │
├─────────────────────────────────────────────────────────────┤
│                    组件层 (Components)                        │
│  NavigatorToolbar │ NavigatorSearch │ NavigatorFilter │ ...  │
├─────────────────────────────────────────────────────────────┤
│                  Composable 层 (Hooks)                       │
│  useDatabaseNavigator │ useWorkbenchIntegration │ ...        │
├─────────────────────────────────────────────────────────────┤
│                    Store 层 (Pinia)                          │
│              database-navigator-store.ts                     │
├─────────────────────────────────────────────────────────────┤
│                     API 层 (API)                             │
│                    navigator-api.ts                          │
├─────────────────────────────────────────────────────────────┤
│                    工具层 (Utils)                            │
│  search-utils │ tree-data-builder │ performance-monitor      │
└─────────────────────────────────────────────────────────────┘
```

### 2.2 数据流

```
用户操作 → 组件触发 → Composable 处理 → Store 更新 → API 调用 → Rust 后端
                                              ↓
组件渲染 ← 状态变化 ← Store 响应 ← API 返回 ← 数据返回
```

---

## 3. 目录结构

```
src/extensions/builtin/database/ui/
├── components/
│   ├── DatabaseNavigator.vue          # 主导航组件
│   ├── NavigatorToolbar.vue           # 工具栏
│   ├── NavigatorSearch.vue            # 搜索组件
│   ├── NavigatorFilter.vue            # 过滤面板
│   ├── NavigatorStatus.vue            # 状态栏
│   ├── NavigatorContextMenu.vue       # 右键菜单
│   ├── tree-renderers.ts              # 树节点渲染工具
│   ├── tree-data-builder.ts           # 树数据构建
│   └── data-preview/
│       ├── DataPreview.vue            # 数据预览主组件
│       ├── PreviewToolbar.vue         # 预览工具栏
│       ├── PreviewTable.vue           # 预览表格
│       └── PreviewPagination.vue      # 分页控件
├── composables/
│   ├── use-database-navigator.ts      # 导航业务逻辑
│   ├── use-workbench-integration.ts   # 工作台集成
│   ├── use-connection-health.ts       # 连接健康监控
│   ├── use-notification.ts            # 统一通知系统
│   ├── use-batch-operations.ts        # 批量操作支持
│   ├── use-advanced-search.ts         # 智能搜索增强
│   ├── use-memory-protection.ts       # 内存泄漏防护
│   ├── use-error-boundary.ts          # 错误边界处理
│   ├── use-drag-drop.ts               # 拖拽支持
│   ├── use-context-memory.ts          # 上下文记忆
│   ├── use-keyboard-shortcuts.ts      # 快捷操作面板
│   ├── use-sql-templates.ts           # SQL 模板库
│   ├── use-data-dictionary-export.ts  # 数据字典导出
│   └── use-accessibility.ts           # 无障碍访问
├── types/
│   └── navigator.ts                   # TypeScript 类型定义
├── utils/
│   ├── search-utils.ts                # 搜索工具
│   └── performance-monitor.ts         # 性能监控
└── stores/
    └── database-navigator-store.ts    # Pinia Store
```

---

## 4. 核心功能

### 4.1 连接管理

| 功能     | 状态 | 说明                      |
| -------- | ---- | ------------------------- |
| 全局连接 | ✅   | 系统级连接，存储在 SQLite |
| 项目连接 | ✅   | 项目级连接，存储在 JSON   |
| 连接测试 | ✅   | 测试连接可用性            |
| 连接保存 | ✅   | 保存连接配置              |
| 连接断开 | ✅   | 关闭连接并清理缓存        |
| 批量操作 | ✅   | 批量刷新/断开             |
| 健康监控 | ✅   | 实时监控连接状态          |

### 4.2 元数据浏览

| 功能        | 状态 | 说明                      |
| ----------- | ---- | ------------------------- |
| 数据库列表  | ✅   | 显示连接下的所有数据库    |
| Schema 列表 | ✅   | 显示数据库下的所有 Schema |
| 表列表      | ✅   | 显示 Schema 下的所有表    |
| 列信息      | ✅   | 显示表的列结构            |
| 索引信息    | ✅   | 显示表的索引              |
| 触发器      | ✅   | 显示表的触发器            |
| 存储过程    | ✅   | 显示存储过程              |
| 函数        | ✅   | 显示函数                  |
| 序列        | ✅   | 显示序列                  |
| 约束        | ✅   | 显示约束信息              |
| 视图        | ✅   | 显示视图                  |
| 元数据缓存  | ✅   | 缓存元数据，提升性能      |

### 4.3 搜索与过滤

| 功能     | 状态 | 说明               |
| -------- | ---- | ------------------ |
| 快速搜索 | ✅   | 搜索表、列、视图等 |
| 高级搜索 | ✅   | 支持过滤条件       |
| 搜索历史 | ✅   | 记录最近 50 条搜索 |
| 结果高亮 | ✅   | 高亮匹配文本       |
| 键盘导航 | ✅   | 上下键选择结果     |
| 节点过滤 | ✅   | 按类型过滤节点     |

### 4.4 数据预览

| 功能       | 状态 | 说明              |
| ---------- | ---- | ----------------- |
| 表数据预览 | ✅   | 预览表数据        |
| 分页加载   | ✅   | 支持分页浏览      |
| 排序       | ✅   | 点击列头排序      |
| 过滤       | ✅   | 自定义 WHERE 条件 |
| 导出数据   | ✅   | 导出数据为文件    |
| 复制数据   | ✅   | 复制为 JSON       |

### 4.5 工作台集成

| 功能       | 状态 | 说明                      |
| ---------- | ---- | ------------------------- |
| 双击打开表 | ✅   | 在新标签页打开            |
| 右键菜单   | ✅   | 提供多种操作              |
| SQL 生成   | ✅   | 生成 SELECT/INSERT/CREATE |
| 拖拽支持   | ✅   | 拖拽表名到编辑器          |
| 上下文同步 | ✅   | 导航与编辑器同步          |

---

## 5. 类型定义

### 5.1 核心类型

```typescript
interface DatabaseInfo {
  name: string
  schemas: SchemaInfo[]
}

interface SchemaInfo {
  name: string
  tables: TableInfo[]
  views: ViewInfo[]
  indexes: IndexInfo[]
  triggers: TriggerInfo[]
  procedures: ProcedureInfo[]
  functions: FunctionInfo[]
  sequences: SequenceInfo[]
}

interface TableInfo {
  name: string
  columns: ColumnInfo[]
  description?: string
}

interface ColumnInfo {
  name: string
  dataType: string
  nullable?: boolean
  defaultValue?: string
  isPrimaryKey?: boolean
}

interface IndexInfo {
  name: string
  columns: string[]
  isUnique: boolean
  isPrimary: boolean
  type: 'btree' | 'hash' | 'gist' | 'spatial' | 'other'
}

interface TriggerInfo {
  name: string
  event: 'INSERT' | 'UPDATE' | 'DELETE' | 'TRUNCATE'
  timing: 'BEFORE' | 'AFTER' | 'INSTEAD OF'
  function: string
  enabled: boolean
}

interface ProcedureInfo {
  name: string
  language: string
  returnType: string
  parameters: Array<{ name: string; type: string; mode: 'IN' | 'OUT' | 'INOUT' }>
  definition: string
}

interface FunctionInfo {
  name: string
  language: string
  returnType: string
  parameters: Array<{ name: string; type: string }>
  isAggregate: boolean
  definition: string
}

interface SequenceInfo {
  name: string
  currentValue: number
  minValue: number
  maxValue: number
  increment: number
  cacheSize: number
  isCycled: boolean
}
```

---

## 6. Composable 层

### 6.1 业务逻辑 Composables

| Composable                | 文件                                                                                                                                                               | 职责             |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------- |
| `useDatabaseNavigator`    | [use-database-navigator.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/composables/use-database-navigator.ts)       | 导航核心业务逻辑 |
| `useWorkbenchIntegration` | [use-workbench-integration.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/composables/use-workbench-integration.ts) | 工作台深度集成   |
| `useConnectionHealth`     | [use-connection-health.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/composables/use-connection-health.ts)         | 连接健康监控     |
| `useNotification`         | [use-notification.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/composables/use-notification.ts)                   | 统一通知系统     |
| `useBatchOperations`      | [use-batch-operations.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/composables/use-batch-operations.ts)           | 批量操作支持     |
| `useAdvancedSearch`       | [use-advanced-search.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/composables/use-advanced-search.ts)             | 智能搜索增强     |

### 6.2 增强功能 Composables

| Composable                | 文件                                                                                                                                                                 | 职责         |
| ------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------ |
| `useMemoryProtection`     | [use-memory-protection.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/composables/use-memory-protection.ts)           | 内存泄漏防护 |
| `useErrorBoundary`        | [use-error-boundary.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/composables/use-error-boundary.ts)                 | 错误边界处理 |
| `useDragDrop`             | [use-drag-drop.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/composables/use-drag-drop.ts)                           | 拖拽支持     |
| `useContextMemory`        | [use-context-memory.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/composables/use-context-memory.ts)                 | 上下文记忆   |
| `useKeyboardShortcuts`    | [use-keyboard-shortcuts.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/composables/use-keyboard-shortcuts.ts)         | 快捷操作面板 |
| `useSqlTemplates`         | [use-sql-templates.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/composables/use-sql-templates.ts)                   | SQL 模板库   |
| `useDataDictionaryExport` | [use-data-dictionary-export.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/composables/use-data-dictionary-export.ts) | 数据字典导出 |
| `useAccessibility`        | [use-accessibility.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/composables/use-accessibility.ts)                   | 无障碍访问   |

---

## 7. 组件层

### 7.1 主组件

| 组件                   | 文件                                                                                                                                                      | 职责       |
| ---------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------- |
| `DatabaseNavigator`    | [DatabaseNavigator.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/components/DatabaseNavigator.vue)       | 导航主组件 |
| `NavigatorToolbar`     | [NavigatorToolbar.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/components/NavigatorToolbar.vue)         | 工具栏     |
| `NavigatorSearch`      | [NavigatorSearch.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/components/NavigatorSearch.vue)           | 搜索组件   |
| `NavigatorFilter`      | [NavigatorFilter.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/components/NavigatorFilter.vue)           | 过滤面板   |
| `NavigatorStatus`      | [NavigatorStatus.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/components/NavigatorStatus.vue)           | 状态栏     |
| `NavigatorContextMenu` | [NavigatorContextMenu.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/components/NavigatorContextMenu.vue) | 右键菜单   |

### 7.2 数据预览组件

| 组件                | 文件                                                                                                                                                             | 职责           |
| ------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------- |
| `DataPreview`       | [DataPreview.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/components/data-preview/DataPreview.vue)             | 数据预览主组件 |
| `PreviewToolbar`    | [PreviewToolbar.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/components/data-preview/PreviewToolbar.vue)       | 预览工具栏     |
| `PreviewTable`      | [PreviewTable.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/components/data-preview/PreviewTable.vue)           | 预览表格       |
| `PreviewPagination` | [PreviewPagination.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/components/data-preview/PreviewPagination.vue) | 分页控件       |

### 7.3 工具组件

| 组件                | 文件                                                                                                                                              | 职责           |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- | -------------- |
| `tree-renderers`    | [tree-renderers.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/components/tree-renderers.ts)       | 树节点渲染工具 |
| `tree-data-builder` | [tree-data-builder.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/components/tree-data-builder.ts) | 树数据构建     |

---

## 8. API 通信层

### 8.1 统一 API

文件: [navigator-api.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/api/navigator-api.ts)

| API                      | 说明                |
| ------------------------ | ------------------- |
| `getMetadataCacheStatus` | 获取元数据缓存状态  |
| `refreshMetadataCache`   | 刷新元数据缓存      |
| `clearMetadataCache`     | 清除元数据缓存      |
| `invokeWithRetry`        | 带重试的 Tauri 调用 |

### 8.2 错误处理

- 自动重试机制（最多 3 次）
- 指数退避策略
- 统一错误响应格式

---

## 9. 工具函数

### 9.1 搜索工具

文件: [search-utils.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/utils/search-utils.ts)

| 函数                    | 说明           |
| ----------------------- | -------------- |
| `searchDatabaseObjects` | 搜索数据库对象 |
| `highlightText`         | 高亮匹配文本   |
| `fuzzyMatch`            | 模糊匹配       |

### 9.2 性能监控

文件: [performance-monitor.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/utils/performance-monitor.ts)

| 函数                   | 说明         |
| ---------------------- | ------------ |
| `startTimer`           | 开始计时     |
| `recordMetric`         | 记录指标     |
| `getSummary`           | 获取统计摘要 |
| `logPerformanceReport` | 输出性能报告 |

---

## 10. 性能优化

### 10.1 已实现的优化

| 优化项     | 实现方式            | 效果             |
| ---------- | ------------------- | ---------------- |
| 元数据缓存 | 内存缓存 + 状态检查 | 减少数据库查询   |
| 虚拟滚动   | NTree 内置支持      | 大数据量流畅渲染 |
| 懒加载     | 按需加载子节点      | 减少初始加载时间 |
| 防抖保存   | debounce 300ms      | 减少存储操作     |
| 内存防护   | 缓存大小限制        | 防止内存泄漏     |
| 性能监控   | 指标采集 + 报告     | 定位性能瓶颈     |

### 10.2 性能指标

| 指标     | 目标值  | 当前值 |
| -------- | ------- | ------ |
| 初始加载 | < 500ms | ~300ms |
| 节点展开 | < 100ms | ~50ms  |
| 搜索响应 | < 200ms | ~100ms |
| 内存占用 | < 100MB | ~60MB  |

---

## 11. 无障碍访问

### 11.1 已实现的功能

| 功能       | 说明                                |
| ---------- | ----------------------------------- |
| ARIA 属性  | 完整的 aria-label、aria-expanded 等 |
| 键盘导航   | Tab、Enter、Space、方向键支持       |
| 屏幕阅读器 | role、aria-live 区域                |
| 高对比度   | 自动检测系统偏好                    |
| 字体大小   | 可调节字体大小                      |
| 减少动画   | 尊重 prefers-reduced-motion         |

### 11.2 配置项

```typescript
interface AccessibilityConfig {
  highContrast: boolean // 高对比度模式
  reducedMotion: boolean // 减少动画
  fontSize: 'small' | 'medium' | 'large' // 字体大小
  screenReaderOptimized: boolean // 屏幕阅读器优化
}
```

---

## 12. 已实现功能清单

### 12.1 核心功能 (100%)

- [x] 连接管理（全局/项目）
- [x] 元数据浏览（数据库/Schema/表/列）
- [x] 元数据缓存
- [x] 搜索与过滤
- [x] 数据预览
- [x] 工作台集成
- [x] 右键菜单
- [x] 批量操作
- [x] 连接健康监控
- [x] 统一通知系统

### 12.2 增强功能 (100%)

- [x] 内存泄漏防护
- [x] 错误边界处理
- [x] 拖拽支持
- [x] 上下文记忆
- [x] 快捷操作面板
- [x] SQL 模板库
- [x] 数据字典导出
- [x] 性能监控
- [x] 无障碍访问

### 12.3 完整元数据支持 (100%)

- [x] 表信息
- [x] 列信息
- [x] 索引信息
- [x] 触发器
- [x] 存储过程
- [x] 函数
- [x] 序列
- [x] 约束

### 12.4 近期新增功能 (2026-04-30)

- [x] 移除连接节点 childCount 角标（"1"标识），由状态栏统一展示统计
- [x] 连接右键菜单新增「DuckDB 本地加速」开关，持久化到 localStorage
- [x] 右键「连接」按钮支持全局连接（之前仅支持项目连接）
- [x] DuckDB 加速偏好持久化，支持任意连接手动开启本地加速

### 12.5 状态栏架构

| 级别       | 组件                            | 位置                    | 显示内容                                       |
| ---------- | ------------------------------- | ----------------------- | ---------------------------------------------- |
| **全局**   | `WorkbenchStatusBar`            | WorkbenchView → 底部    | 应用级系统信息（缓存状态、执行耗时、版本号等） |
| **页面级** | `EditPanel > .editor-statusbar` | 各面板组件内部          | 该面板专属信息（连接信息、光标位置、执行状态） |
| **页面级** | `NavigatorStatus`               | database-navigator 底部 | 连接数、数据库数、表/视图统计                  |

全局状态栏不显示任何数据库连接信息。连接信息仅出现在对应面板的页面级状态栏中。

---

## 13. 优化建议

### 13.1 短期优化 (1-2 周)

| 优先级 | 优化项         | 投入 | 收益 | 说明                                          |
| ------ | -------------- | ---- | ---- | --------------------------------------------- |
| 🔴 P0  | 虚拟滚动优化   | 中   | 高   | 实现节点懒加载，优化 10000+ 节点场景          |
| 🔴 P0  | 错误降级策略   | 低   | 高   | 节点加载失败时显示降级 UI                     |
| 🟡 P1  | 连接拖拽排序   | 中   | 中   | 用户可拖拽调整连接顺序，持久化到 localStorage |
| 🟡 P1  | 连接自定义分组 | 中   | 中   | 用户自定义连接分组文件夹                      |
| 🟡 P1  | 连接导入导出   | 中   | 中   | 支持 JSON 格式导入导出连接配置                |
| 🟢 P2  | 主题同步优化   | 低   | 低   | 导航组件主题跟随系统                          |

### 13.2 中期优化 (1-2 月)

| 优先级 | 优化项       | 投入 | 收益 | 说明               |
| ------ | ------------ | ---- | ---- | ------------------ |
| 🟡 P1  | 书签功能     | 中   | 中   | 收藏常用表/视图    |
| 🟡 P1  | 最近访问     | 低   | 中   | 记录最近访问的表   |
| 🟢 P2  | SQL 历史记录 | 中   | 中   | 记录导航触发的 SQL |
| 🟢 P2  | 数据对比     | 高   | 低   | 对比两个表结构差异 |

### 13.3 长期优化 (3-6 月)

| 优先级 | 优化项   | 投入 | 收益 | 说明                   |
| ------ | -------- | ---- | ---- | ---------------------- |
| 🟢 P2  | 云同步   | 高   | 中   | 连接配置云同步（可选） |
| 🟢 P2  | 插件扩展 | 高   | 高   | 支持插件扩展导航功能   |
| 🟢 P2  | 协作功能 | 高   | 低   | 多人协作编辑连接配置   |

### 13.4 不纳入本版本的功能

| 功能      | 原因                 | 替代方案     |
| --------- | -------------------- | ------------ |
| ER 图预览 | 复杂度高，需独立模块 | 后续版本规划 |
| 数据建模  | 超出导航范围         | 独立功能模块 |
| 版本控制  | 需后端支持           | 后续版本规划 |

---

## 优化记录

### 2026-04-30 — 连接右键菜单与视觉优化

| #   | 优化项                        | 状态 | 说明                                                        |
| --- | ----------------------------- | ---- | ----------------------------------------------------------- |
| 1   | 移除 childCount 角标          | ✅   | 去掉连接行右侧数字显示，改为状态栏统一统计                  |
| 2   | DuckDB 开关                   | ✅   | 右键菜单新增，基于 `runtimeConnectionStore.isDuckDbEnabled` |
| 3   | DuckDB 偏好持久化             | ✅   | localStorage 存储 `duckdb-enabled-connections`              |
| 4   | toggleConnection 支持全局连接 | ✅   | 右键「连接」按钮现在对全局连接也有效                        |
| 5   | 自动连接逻辑转移              | ✅   | SQL 编辑器执行时自动 `establishFromConnection`              |
| 6   | 全局状态栏连接信息移除        | ✅   | `WorkbenchStatusBar` 不再显示连接名称                       |

### 右键菜单结构（连接节点）

```
编辑连接        F4
测试连接
连接/断开连接   Ctrl+E/Ctrl+D
───────────────
✓ DuckDB 本地加速  ← 新增
───────────────
打开 SQL 编辑器  Ctrl+Shift+E
───────────────
刷新             F5
刷新所有         Ctrl+F5
───────────────
复制名称         Ctrl+C
───────────────
删除连接
```

---

## 14. 开发规范

### 14.1 组件开发规范

- 单文件 **< 300 行**
- Props 必须强类型
- Emits 必须显式
- 使用 `<script setup lang="ts">`
- 禁止使用 `any` 类型

### 14.2 Composable 开发规范

- 命名以 `use-` 开头
- 返回响应式数据和方法
- 在 `onUnmounted` 中清理资源
- 避免副作用

### 14.3 Store 开发规范

- 使用 Pinia `defineStore`
- State 使用 `ref`
- Getters 使用 `computed`
- Actions 处理异步操作

### 14.4 类型开发规范

- 接口使用 PascalCase
- 枚举使用 PascalCase
- 所有 API 响应必须标注类型
- 禁止使用 `any`

### 14.5 代码审查清单

- [ ] 是否遵循分层架构
- [ ] 是否有类型标注
- [ ] 是否有错误处理
- [ ] 是否有内存泄漏风险
- [ ] 是否有性能问题
- [ ] 是否符合无障碍规范
- [ ] 是否有单元测试
- [ ] 是否有文档注释

---

## 附录

### A. 快捷键列表

| 快捷键         | 功能      | 分类 |
| -------------- | --------- | ---- |
| `Ctrl+P`       | 聚焦搜索  | 搜索 |
| `Ctrl+Shift+P` | 快捷面板  | 导航 |
| `F5`           | 刷新      | 导航 |
| `Ctrl+D`       | 断开连接  | 连接 |
| `↑/↓`          | 上下选择  | 导航 |
| `Enter`        | 展开/折叠 | 导航 |
| `Space`        | 选中节点  | 导航 |

### B. 颜色变量

```css
--bg-primary: #ffffff; /* 主背景 */
--bg-secondary: #f5f5f5; /* 次级背景 */
--bg-tertiary: #e8e8e8; /* 第三级背景 */
--text-primary: #333333; /* 主文字 */
--text-secondary: #666666; /* 次级文字 */
--text-tertiary: #999999; /* 辅助文字 */
--border-color: #d9d9d9; /* 边框 */
--primary-color: #165dff; /* 主色 */
--success-color: #00b42a; /* 成功 */
--warning-color: #ff7d00; /* 警告 */
--danger-color: #f53f3f; /* 危险 */
```

### C. 相关文档

- [企业级前端一体化规范](../../../../../.trae/rules/frontend-enterprise-spec.md)
- [项目技能配置](../../../../../.trae/rules/rdata-station.md)
- [技术规则](../../../../../.trae/rules/technical-rules.md)

---

> 文档版本: 1.0.0
> 最后更新: 2026-04-25
> 维护者: RdataStation 前端团队
