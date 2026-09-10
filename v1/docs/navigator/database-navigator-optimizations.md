# 数据库导航栏优化更新文档

> 版本: v1.0  
> 最后更新: 2026-05-07  
> 状态: ✅ 持续更新

---

## 目录

1. [错误修复](#一错误修复)
2. [功能增强](#二功能增强)
3. [性能优化](#三性能优化)
4. [新增工具](#四新增工具)
5. [API 变更](#五api变更)
6. [文件变更清单](#六文件变更清单)

---

## 一、错误修复

### 1.1 SQLite 持久化错误修复

**问题**：`PRAGMA mmap_size` 命令在某些 SQLite 版本中会返回结果，但代码使用了 `conn.execute()`（用于不返回结果的语句），导致连接建立失败。

**修复位置**：`src-tauri/src/core/persistence/metadata_cache.rs:123`

**修复内容**：将 `execute()` 改为 `query()`，并忽略返回结果

```rust
// 修复前
conn.execute("PRAGMA mmap_size=268435456", []).map_err(|e| ...)?;

// 修复后
let _ = conn.query("PRAGMA mmap_size=268435456", []).map_err(|e| {
    log::warn!("Failed to set mmap_size: {}", e);
    ...
});
```

### 1.2 Vue 初始化顺序错误修复

**问题**：`keyboardShortcuts` 在函数定义之前被初始化，导致 TDZ（暂时性死区）错误。

**修复位置**：`src/extensions/builtin/database/ui/components/database-navigator.vue`

**修复内容**：将 `keyboardShortcuts` 初始化移到所有函数定义之后

---

## 二、功能增强

### 2.1 错误提示优化

- **新增组件**：`navigator-error.vue`
- **功能**：在 UI 上显示友好的错误提示和重试按钮
- **特性**：
  - 显示错误标题和消息
  - 支持重试操作
  - 支持关闭提示

### 2.2 键盘快捷键支持

| 快捷键         | 功能     |
| -------------- | -------- |
| `Ctrl+N`       | 新建连接 |
| `Ctrl+D`       | 断开连接 |
| `Ctrl+R`       | 刷新     |
| `Ctrl+F`       | 搜索     |
| `Ctrl+B`       | 开始事务 |
| `Ctrl+Shift+B` | 提交事务 |
| `Ctrl+Shift+R` | 回滚事务 |

### 2.3 存储过程和函数支持

- **新增方法**：
  - `loadProcedures(connectionId, dbName, schemaName)`
  - `loadFunctions(connectionId, dbName, schemaName)`
- **支持数据库**：MySQL、PostgreSQL

### 2.4 连接状态指示器

| 状态   | 样式     | 动画   |
| ------ | -------- | ------ |
| 已连接 | 绿色圆点 | 脉冲环 |
| 连接中 | 橙色圆点 | 闪烁   |
| 未连接 | 灰色圆点 | 无     |

### 2.5 自定义分组功能

**功能特性**：

- ✅ 创建分组（支持自定义名称、描述、颜色）
- ✅ 编辑分组
- ✅ 删除分组
- ✅ 分组展开/折叠
- ✅ 显示连接数量
- ✅ 数据持久化（localStorage）

### 2.6 节点拖拽排序

**功能特性**：

- ✅ 支持连接拖拽排序
- ✅ 支持分组拖拽排序
- ✅ 支持拖拽到分组内
- ✅ 拖拽状态显示（before/after/inside）

### 2.7 批量操作

**功能特性**：

- ✅ 批量删除连接
- ✅ 批量移动到分组
- ✅ 批量断开连接

### 2.8 记忆展开状态

**功能特性**：

- ✅ 记住用户展开的节点状态
- ✅ 状态持久化到 localStorage
- ✅ 刷新页面后恢复展开状态

### 2.9 操作反馈组件（Toast）

**功能特性**：

- ✅ 支持四种类型提示：success、error、info、warning
- ✅ 自动定时关闭（默认 3 秒）
- ✅ 鼠标悬停暂停倒计时
- ✅ 支持手动关闭
- ✅ 平滑动画过渡

### 2.10 高级过滤功能

**功能特性**：

- ✅ 按数据库类型过滤（MySQL/PostgreSQL/SQLite/DuckDB）
- ✅ 按连接状态过滤（已连接/连接中/未连接）
- ✅ 按节点类型过滤（表/视图/存储过程/函数/列）
- ✅ 显示/隐藏系统对象
- ✅ 支持重置和应用

### 2.11 错误边界组件

**功能特性**：

- ✅ 捕获子组件渲染错误
- ✅ 显示友好的错误提示
- ✅ 支持重试操作
- ✅ 记录错误日志

### 2.12 国际化支持

**功能特性**：

- ✅ 支持中文（zh-CN）和英文（en-US）
- ✅ 支持自定义消息加载
- ✅ 语言设置持久化到 localStorage
- ✅ 支持参数化消息

### 2.13 事件驱动架构

**功能特性**：

- ✅ 事件发布/订阅机制
- ✅ 支持单次订阅
- ✅ 自动清理订阅
- ✅ 支持多种导航栏事件

**支持的事件**：
| 事件名 | 说明 |
|--------|------|
| `connection-connected` | 连接成功 |
| `connection-disconnected` | 断开连接 |
| `connection-error` | 连接错误 |
| `node-expanded` | 节点展开 |
| `node-collapsed` | 节点收起 |
| `node-selected` | 节点选中 |
| `transaction-started` | 事务开始 |
| `transaction-committed` | 事务提交 |
| `transaction-rolled-back` | 事务回滚 |
| `group-created` | 分组创建 |
| `group-updated` | 分组更新 |
| `group-deleted` | 分组删除 |
| `search-query-change` | 搜索查询变更 |
| `filters-change` | 过滤器变更 |
| `refresh-requested` | 刷新请求 |

### 2.14 类型安全验证（Zod）

**功能特性**：

- ✅ 运行时类型验证
- ✅ 支持连接、分组、节点、过滤器等类型
- ✅ 安全解析，不抛出异常
- ✅ 自动类型推断

**支持的 Schema**：

- `ConnectionSchema` - 连接配置
- `ConnectionGroupSchema` - 分组配置
- `VirtualTreeNodeSchema` - 虚拟树节点
- `SearchIndexEntrySchema` - 搜索索引条目
- `ToastMessageSchema` - Toast 消息
- `FiltersSchema` - 过滤器配置

### 2.15 设置面板

**功能特性**：

- ✅ 连接池管理配置
- ✅ 操作历史设置
- ✅ 健康监控配置
- ✅ 性能设置
- ✅ 快捷键设置
- ✅ 外观设置（主题、字体大小）

**设置分类**：
| 分类 | 说明 |
|------|------|
| 连接池 | 最大连接数、最小空闲连接、超时时间、自动重连、健康检查 |
| 操作历史 | 保留数量、保留天数、启用历史、记录SQL、撤销/重做 |
| 健康监控 | 启用监控、更新间隔、告警通知、慢查询阈值 |
| 性能 | 虚拟滚动缓冲区、缓存大小、缓存过期、懒加载、预加载 |
| 快捷键 | 显示当前快捷键、支持修改、重置为默认 |
| 外观 | 主题选择、字体大小、紧凑模式 |

### 2.16 事务管理接口

**功能特性**：

- ✅ 开始事务（BEGIN TRANSACTION）
- ✅ 提交事务（COMMIT）
- ✅ 回滚事务（ROLLBACK）
- ✅ 获取事务状态

**后端接口**：

| 接口名称                 | 功能描述     | 参数               | 返回值                      |
| ------------------------ | ------------ | ------------------ | --------------------------- |
| `begin_transaction`      | 开始事务     | `conn_id?: string` | `TransactionStatusResponse` |
| `commit_transaction`     | 提交事务     | `conn_id?: string` | `TransactionStatusResponse` |
| `rollback_transaction`   | 回滚事务     | `conn_id?: string` | `TransactionStatusResponse` |
| `get_transaction_status` | 获取事务状态 | `conn_id?: string` | `TransactionStatusResponse` |

**响应类型**（TransactionStatusResponse）：

```typescript
interface TransactionStatusResponse {
  connId: string // 连接 ID
  isInTransaction: boolean // 是否在事务中
  transactionStartTimeMs?: number // 事务开始时间戳
  transactionDurationMs?: number // 事务持续时间（毫秒）
}
```

### 2.17 缓存预热管理

**功能特性**：

- ✅ 构建缓存索引（支持增量模式）
- ✅ 取消缓存预热
- ✅ 获取预热进度
- ✅ 缓存版本检查与迁移

**后端接口**：

| 接口名称                  | 功能描述     | 参数                                        | 返回值                    |
| ------------------------- | ------------ | ------------------------------------------- | ------------------------- |
| `build_cache_index`       | 构建缓存索引 | `connection_id`, `database`, `incremental?` | `IndexBuildResponse`      |
| `start_cache_warming`     | 启动缓存预热 | `connection_id`, `databases`                | `WarmingProgressResponse` |
| `cancel_cache_warming`    | 取消缓存预热 | `connection_id`                             | `void`                    |
| `get_warming_progress`    | 获取预热进度 | `connection_id`                             | `WarmingProgressResponse` |
| `check_cache_version`     | 检查缓存版本 | `connection_id`                             | `version`                 |
| `execute_cache_migration` | 执行缓存迁移 | `connection_id`                             | `MigrationResponse`       |

**响应类型**（WarmingProgressResponse）：

```typescript
interface WarmingProgressResponse {
  connectionId: string // 连接 ID
  isWarming: boolean // 是否正在预热
  currentStep: string // 当前步骤
  totalSteps: number // 总步骤数
  completedSteps: number // 已完成步骤数
  progressPercentage: number // 进度百分比
  currentDatabase?: string // 当前数据库
  currentSchema?: string // 当前 Schema
  currentTable?: string // 当前表
}
```

### 2.18 连接池状态查询

**功能特性**：

- ✅ 获取连接池状态

**后端接口**：

| 接口名称                     | 功能描述       | 参数      | 返回值                         |
| ---------------------------- | -------------- | --------- | ------------------------------ |
| `get_connection_pool_status` | 获取连接池状态 | `conn_id` | `ConnectionPoolStatusResponse` |

**响应类型**（ConnectionPoolStatusResponse）：

```typescript
interface ConnectionPoolStatusResponse {
  connId: string // 连接 ID
  activeConnections: number // 活跃连接数
  idleConnections: number // 空闲连接数
  maxConnections: number // 最大连接数
  minConnections: number // 最小连接数
  connectionTimeoutMs: number // 连接超时时间（毫秒）
  idleTimeoutMs: number // 空闲超时时间（毫秒）
  totalConnections: number // 总连接数
  waitQueueSize: number // 等待队列大小
}
```

### 2.19 搜索结果高亮与排序

**功能特性**：

- ✅ 搜索结果高亮显示匹配关键词
- ✅ 匹配度排序（精确匹配 > 前缀匹配 > 包含匹配）
- ✅ 支持中文和英文搜索
- ✅ 倒排索引加速搜索

**搜索索引类**（`SearchIndex`）：

| 方法                  | 功能描述           | 参数               | 返回值             |
| --------------------- | ------------------ | ------------------ | ------------------ |
| `add(entry)`          | 添加索引条目       | `SearchIndexEntry` | `void`             |
| `remove(nodeId)`      | 删除索引条目       | `nodeId: string`   | `void`             |
| `search(query)`       | 搜索并返回高亮信息 | `query: string`    | `SearchResult[]`   |
| `searchSimple(query)` | 简单搜索返回节点ID | `query: string`    | `string[]`         |
| `getEntry(nodeId)`    | 获取索引条目       | `nodeId: string`   | `SearchIndexEntry` |
| `clear()`             | 清空索引           | -                  | `void`             |

**类型定义**：

```typescript
interface SearchIndexEntry {
  nodeId: string
  nodeType: string
  connectionId: string
  labels: string[]
}

interface SearchResult {
  nodeId: string
  score: number
  highlights: HighlightInfo[]
}

interface HighlightInfo {
  label: string
  matchPositions: MatchPosition[]
}

interface MatchPosition {
  start: number
  length: number
}
```

**匹配度评分规则**：
| 匹配类型 | 分值 |
|---------|------|
| 精确匹配 | 100 |
| 前缀匹配 | 50 |
| 包含匹配 | 30 |
| 每个匹配词 | 10 |

### 2.20 连接池可视化面板

**功能特性**：

- ✅ 实时显示连接池状态
- ✅ 连接使用率进度条
- ✅ 连接池配置信息展示
- ✅ 连接池操作按钮（增加/减少连接数、清理空闲连接）
- ✅ 自动刷新（每5秒）

**组件**：`connection-pool-panel.vue`

**显示指标**：
| 指标 | 说明 |
|------|------|
| 活跃连接 | 当前正在使用的连接数 |
| 空闲连接 | 当前空闲的连接数 |
| 总连接数 | 已创建的连接总数 |
| 等待队列 | 等待获取连接的请求数 |
| 连接使用率 | 当前连接数占最大连接数的百分比 |

### 2.21 空状态优化组件

**功能特性**：

- ✅ 美观的空状态插画
- ✅ 引导用户创建第一个连接
- ✅ 快捷操作入口
- ✅ 装饰性背景元素

**组件**：`empty-state.vue`

**Props**：
| 属性 | 类型 | 说明 |
|------|------|------|
| title | string | 标题文本 |
| description | string | 描述文本 |
| actions | Action[] | 操作按钮列表 |

**Action 类型**：

```typescript
interface Action {
  id: string
  label: string
  icon: Component
  primary?: boolean
}
```

**使用场景**：

- 无连接时显示
- 搜索无结果时显示
- 分组为空时显示

### 2.22 操作确认对话框

**功能特性**：

- ✅ 支持多种类型（warning/error/info/success）
- ✅ 显示影响数量提示
- ✅ 支持详细信息展示
- ✅ 支持取消和次要操作按钮
- ✅ 平滑的动画效果

**组件**：`confirm-dialog.vue`

**Props**：
| 属性 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| visible | boolean | - | 是否显示对话框 |
| title | string | - | 标题文本 |
| message | string | - | 消息内容 |
| details | string | - | 详细信息（可选） |
| count | number | - | 影响数量（可选） |
| type | 'warning'\|'error'\|'info'\|'success' | 'warning' | 对话框类型 |
| confirmText | string | '确认' | 确认按钮文本 |
| cancelText | string | '取消' | 取消按钮文本 |
| secondaryText | string | - | 次要按钮文本 |
| showCancel | boolean | true | 是否显示取消按钮 |
| showSecondary | boolean | false | 是否显示次要按钮 |

**事件**：
| 事件 | 说明 |
|------|------|
| confirm | 用户点击确认按钮 |
| cancel | 用户点击取消按钮 |
| secondary | 用户点击次要按钮 |

**使用场景**：

- 批量删除确认
- 事务提交/回滚确认
- 分组删除确认
- 危险操作确认

### 2.23 通知中心组件

**功能特性**：

- ✅ 通知分类筛选（全部/未读/错误/警告）
- ✅ 通知标记已读
- ✅ 全部已读功能
- ✅ 单条通知删除
- ✅ 清空所有通知
- ✅ 通知时间格式化（刚刚/分钟前/小时前/日期）

**组件**：`notification-center.vue`

**通知类型**：
| 类型 | 说明 | 图标 |
|------|------|------|
| info | 信息通知 | Info |
| warning | 警告通知 | AlertTriangle |
| error | 错误通知 | AlertCircle |
| success | 成功通知 | CheckCircle |

**Props**：
| 属性 | 类型 | 说明 |
|------|------|------|
| notifications | Notification[] | 通知列表 |

**Notification 类型**：

```typescript
interface Notification {
  id: string
  type: 'info' | 'warning' | 'error' | 'success'
  message: string
  timestamp: number
  read: boolean
}
```

**事件**：
| 事件 | 说明 |
|------|------|
| markAsRead | 标记单条通知为已读 |
| markAllAsRead | 标记所有通知为已读 |
| dismiss | 删除单条通知 |
| clearAll | 清空所有通知 |

**标签页**：
| 标签 | 说明 |
|------|------|
| 全部 | 显示所有通知 |
| 未读 | 仅显示未读通知 |
| 错误 | 仅显示错误通知 |
| 警告 | 仅显示警告通知 |

### 2.24 单元测试覆盖

**测试框架**：Vitest

**测试文件**：

| 文件                              | 测试内容         |
| --------------------------------- | ---------------- |
| `tests/unit/search-index.spec.ts` | 搜索索引单元测试 |
| `tests/unit/event-bus.spec.ts`    | 事件总线单元测试 |

**搜索索引测试覆盖**：
| 测试项 | 说明 |
|--------|------|
| `add` | 添加索引条目 |
| `remove` | 删除索引条目 |
| `search` | 搜索功能（精确匹配、部分匹配、中文支持、排序、高亮） |
| `clear` | 清空索引 |

**事件总线测试覆盖**：
| 测试项 | 说明 |
|--------|------|
| `on/off` | 注册/取消事件监听 |
| `emit` | 触发事件 |
| `once` | 单次事件监听 |
| `offAll` | 移除所有监听 |
| `hasListeners` | 检查是否有监听 |

**运行测试**：

```bash
pnpm test
```

### 2.25 主题系统

**功能特性**：

- ✅ 支持浅色/深色/跟随系统三种模式
- ✅ 支持自定义主题色（6种预设颜色）
- ✅ 支持三种字体大小（小/中/大）
- ✅ 支持紧凑模式切换
- ✅ 主题设置持久化到 localStorage
- ✅ 跟随系统主题自动切换

**主题管理**（`use-theme-manager.ts`）：

**主题模式**：
| 模式 | 说明 |
|------|------|
| light | 浅色主题 |
| dark | 深色主题 |
| system | 跟随系统设置 |

**配置项**：

```typescript
interface ThemeConfig {
  mode: 'light' | 'dark' | 'system' // 主题模式
  accentColor: string // 主题色
  fontSize: 'small' | 'medium' | 'large' // 字体大小
  compactMode: boolean // 紧凑模式
}
```

**可用主题色**：
| 颜色 | 值 |
|------|------|
| 绿色（默认） | #00b464 |
| 蓝色 | #6464ff |
| 红色 | #ff6464 |
| 橙色 | #ffb400 |
| 紫色 | #9664ff |
| 亮绿色 | #00c853 |

**方法**：
| 方法 | 说明 |
|------|------|
| `setMode(mode)` | 设置主题模式 |
| `setAccentColor(color)` | 设置主题色 |
| `setFontSize(size)` | 设置字体大小 |
| `toggleCompactMode()` | 切换紧凑模式 |

### 2.26 插件化架构（概念说明）

**什么是插件化架构？**

插件化架构是一种软件设计模式，它允许在不修改核心代码的情况下扩展应用功能。通过定义标准的接口和扩展点，第三方开发者可以编写插件来增强应用的功能。

**插件化架构的核心概念**：

| 概念         | 说明                   |
| ------------ | ---------------------- |
| **核心框架** | 提供基础功能和扩展机制 |
| **扩展点**   | 定义插件可以插入的位置 |
| **插件接口** | 定义插件必须实现的方法 |
| **插件市场** | 集中管理和分发插件     |

**插件化架构的优势**：

| 优势         | 说明                           |
| ------------ | ------------------------------ |
| **可扩展性** | 无需修改核心代码即可添加新功能 |
| **模块化**   | 功能解耦，易于维护和测试       |
| **灵活性**   | 用户可以根据需求选择安装插件   |
| **生态系统** | 鼓励社区贡献，丰富功能         |

**数据库导航栏插件化设想**：

| 扩展点       | 说明                     |
| ------------ | ------------------------ |
| 节点类型扩展 | 添加自定义数据库对象类型 |
| 右键菜单扩展 | 添加自定义右键菜单项     |
| 工具栏扩展   | 添加自定义工具栏按钮     |
| 搜索扩展     | 添加自定义搜索过滤器     |
| 导出扩展     | 添加自定义数据导出格式   |

**插件接口示例**：

```typescript
interface DatabaseNavigatorPlugin {
  id: string
  name: string
  version: string
  activate(): void
  deactivate(): void
  contributes?: {
    nodeTypes?: CustomNodeType[]
    menuItems?: MenuItem[]
    toolbarButtons?: ToolbarButton[]
  }
}
```

**新增文件**：

- `types/group.ts` - 分组类型定义
- `composables/use-group-manager.ts` - 分组管理逻辑
- `components/group-node.vue` - 分组节点组件
- `components/group-dialog.vue` - 分组创建/编辑对话框
- `components/panels/SettingsPanel.vue` - 设置面板组件

---

## 三、性能优化

### 3.1 增量刷新机制

- **新增状态**：`lastSyncTimes`、`syncModes`
- **功能**：支持按时间戳进行差异更新，避免每次刷新都重新加载全部数据

### 3.2 虚拟滚动优化

- 使用整数计算避免浮点运算
- 缓存计算结果避免重复计算
- 优化缓冲区大小配置

### 3.3 缓存预热优化

| 参数           | 优化前 | 优化后  |
| -------------- | ------ | ------- |
| 并发数         | 5      | 2       |
| 预热深度       | tables | schemas |
| 延迟           | 30ms   | 100ms   |
| 最大数据库数   | 10     | 5       |
| 最大 Schema 数 | 20     | 10      |
| 最大表数       | 100    | 50      |

### 3.4 搜索性能优化（倒排索引）

**功能特性**：

- ✅ 使用倒排索引加速搜索
- ✅ 支持中文和英文分词
- ✅ 支持前缀匹配搜索
- ✅ 支持多词搜索（交集匹配）

**性能提升**：

- 搜索时间复杂度从 O(n) 降低到 O(log n)
- 支持模糊搜索、正则搜索
- 搜索结果高亮显示

### 3.5 缓存策略优化（LRU）

**功能特性**：

- ✅ 基于 LRU（最近最少使用）原则的缓存淘汰策略
- ✅ 可配置最大缓存大小
- ✅ 支持缓存驱逐回调
- ✅ 支持多级缓存（内存 + IndexedDB）

**性能提升**：

- 自动清理不常用的缓存数据
- 避免内存无限增长
- 提高缓存命中率

---

## 四、新增工具

### 4.1 可取消请求工具

**文件**：`utils/abortable-request.ts`

**功能**：在用户快速操作时取消之前的异步请求，避免不必要的网络开销和状态污染

**使用示例**：

```typescript
import { abortableRequest } from './utils/abortable-request'

abortableRequest.create('load-tables', async signal => {
  const result = await navigatorStore.loadTables(connectionId, dbName, schemaName)
  return result
})
```

### 4.2 防抖工具

**文件**：`utils/debounce.ts`

**功能**：支持同步和异步函数的防抖处理

**使用示例**：

```typescript
import { debounce } from './utils/debounce'

const debouncedSearch = debounce(onSearchQueryChange, 300)
```

### 4.3 性能监控工具

**文件**：`utils/performance-monitor.ts`

**功能**：监控性能指标，包括节点加载时间、渲染时间、请求数量、缓存命中率

**使用示例**：

```typescript
import { performanceMonitor } from './utils/performance-monitor'

// 在浏览器控制台查看性能报告
performanceMonitor.logReport()
```

### 4.4 LRU 缓存工具

**文件**：`utils/lru-cache.ts`

**功能**：基于最近最少使用原则的缓存策略，支持自动淘汰过期数据

**使用示例**：

```typescript
import { LRUCache } from './utils/lru-cache'

const cache = new LRUCache<string>({
  maxSize: 100,
  onEvict: (key, value) => {
    console.log(`Evicted: ${key}`)
  },
})

cache.set('key', 'value')
const value = cache.get('key')
```

### 4.5 搜索索引工具

**文件**：`utils/search-index.ts`

**功能**：使用倒排索引加速搜索，支持中文和英文分词

**使用示例**：

```typescript
import { SearchIndex } from './utils/search-index'

const index = new SearchIndex()

index.add({
  nodeId: 'node-1',
  nodeType: 'table',
  connectionId: 'conn-1',
  labels: ['users', '用户表'],
})

const results = index.search('user')
```

### 4.6 拖拽排序工具

**文件**：`composables/use-drag-sort.ts`

**功能**：实现节点的拖拽排序功能，支持连接和分组的拖拽

**使用示例**：

```typescript
import { useDragSort } from './composables/use-drag-sort'

const { dragState, startDrag, updateDrag, endDrag } = useDragSort()

startDrag('node-1', 'connection')
updateDrag('target-node', 'group', 'inside')
const result = endDrag()
```

### 4.7 国际化工具

**文件**：`composables/use-i18n.ts`

**功能**：提供多语言支持，支持中文和英文

**使用示例**：

```typescript
import { useI18n } from './composables/use-i18n'

const { t, setLocale, locale } = useI18n()

console.log(t('navigator.title')) // 输出：数据库导航
setLocale('en-US')
console.log(t('navigator.title')) // 输出：Database Navigator
```

### 4.8 事件总线工具

**文件**：`composables/use-event-bus.ts`

**功能**：提供组件间松耦合的通信机制

**使用示例**：

```typescript
import { eventBus } from './composables/use-event-bus'

// 订阅事件
eventBus.on('connection-connected', data => {
  console.log('Connection connected:', data)
})

// 发布事件
eventBus.emit('connection-connected', { connectionId: 'conn-1' })
```

### 4.9 Zod 类型验证工具

**文件**：`utils/zod-validation.ts`

**功能**：提供运行时类型验证功能

**使用示例**：

```typescript
import { validateConnection, ConnectionSchema } from './utils/zod-validation'

const connection = validateConnection(data)
if (connection) {
  console.log('Valid connection:', connection)
}
```

---

## 五、API 变更

### 5.1 database-navigator-store 新增方法

| 方法名            | 参数                                     | 返回值                    | 说明             |
| ----------------- | ---------------------------------------- | ------------------------- | ---------------- |
| `getLastSyncTime` | `connectionId`, `dbName?`, `schemaName?` | `number`                  | 获取最后同步时间 |
| `setLastSyncTime` | `connectionId`, `dbName?`, `schemaName?` | `void`                    | 设置同步时间     |
| `setSyncMode`     | `connectionId`, `mode`                   | `void`                    | 设置同步模式     |
| `getSyncMode`     | `connectionId`                           | `'full' \| 'incremental'` | 获取同步模式     |
| `loadProcedures`  | `connectionId`, `dbName`, `schemaName`   | `Promise<void>`           | 加载存储过程     |
| `loadFunctions`   | `connectionId`, `dbName`, `schemaName`   | `Promise<void>`           | 加载函数         |

### 5.2 节点类型新增

| 类型        | 图标           | 颜色    |
| ----------- | -------------- | ------- |
| `procedure` | Code           | #ef4444 |
| `function`  | FunctionSquare | #14b8a6 |

### 5.3 连接状态新增

```typescript
// 原状态
connectionStatus?: 'connected' | 'disconnected'

// 新增后
connectionStatus?: 'connected' | 'connecting' | 'disconnected'
```

---

## 六、文件变更清单

### 新增文件

| 文件路径                                | 说明             |
| --------------------------------------- | ---------------- |
| `types/group.ts`                        | 分组类型定义     |
| `composables/use-group-manager.ts`      | 分组管理逻辑     |
| `composables/use-keyboard-shortcuts.ts` | 键盘快捷键处理   |
| `composables/use-drag-sort.ts`          | 拖拽排序逻辑     |
| `composables/use-i18n.ts`               | 国际化支持       |
| `composables/use-event-bus.ts`          | 事件总线         |
| `components/group-node.vue`             | 分组节点组件     |
| `components/group-dialog.vue`           | 分组对话框       |
| `components/navigator-error.vue`        | 错误提示组件     |
| `components/navigator-toast.vue`        | 操作反馈组件     |
| `components/navigator-filter-panel.vue` | 高级过滤面板     |
| `components/error-boundary.vue`         | 错误边界组件     |
| `components/panels/SettingsPanel.vue`   | 设置面板组件     |
| `utils/abortable-request.ts`            | 可取消请求工具   |
| `utils/debounce.ts`                     | 防抖工具         |
| `utils/performance-monitor.ts`          | 性能监控工具     |
| `utils/lru-cache.ts`                    | LRU 缓存工具     |
| `utils/search-index.ts`                 | 搜索索引工具     |
| `utils/zod-validation.ts`               | Zod 类型验证工具 |

### 修改文件

| 文件路径                      | 修改内容                        |
| ----------------------------- | ------------------------------- |
| `navigator-toolbar.vue`       | 添加新建分组按钮、事务按钮      |
| `navigator-status.vue`        | 添加事务状态指示器              |
| `virtual-tree-node.vue`       | 增强连接状态显示、加载动画      |
| `database-navigator.vue`      | 集成分组管理、快捷键、错误处理  |
| `virtual-tree.ts`             | 添加连接中状态                  |
| `navigator.ts`                | 添加存储过程/函数节点类型       |
| `database-navigator-store.ts` | 添加存储过程/函数加载、增量刷新 |
| `use-database-tree-loader.ts` | 添加存储过程/函数节点创建       |
| `use-virtual-scroll.ts`       | 性能优化                        |
| `use-cache-warming.ts`        | 降低并发配置                    |
| `metadata_cache.rs`           | 修复 SQLite PRAGMA 错误         |

---

## 七、性能指标

在浏览器控制台输入以下命令查看性能报告：

```javascript
performanceMonitor.logReport()
```

输出示例：

```
=== Database Navigator Performance Report ===
Node Load Time: 12.34ms
Render Time: 4.56ms
Request Count: 12
Cache Hit Rate: 83.3%
============================================
```

---

## 版本历史

| 版本 | 日期       | 说明                       |
| ---- | ---------- | -------------------------- |
| v1.0 | 2026-05-07 | 初始版本，包含所有优化内容 |

---

## 相关文档

- [前端架构文档](docs/frontend/INDEX.md)
- [后端架构文档](docs/backend/README.md)
