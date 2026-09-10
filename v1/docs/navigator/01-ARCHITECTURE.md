# IVM 导航栏架构设计

> 版本：v1.0
> 最后更新：2026-05-03
> 状态：✅ 持续更新

---

## 1. 架构概述

### 1.1 设计目标

基于 IVM（增量视图维护）理念，构建响应式、高性能、资源友好的数据库导航架构。

### 1.2 核心原则

- **增量优先**：所有变更以增量形式传播和处理
- **懒加载**：按需加载，避免一次性加载大量数据
- **响应式**：数据变更自动触发视图更新
- **离线可用**：本地缓存支持断网使用

## 2. 分层架构

### 2.1 四层视图模型

```
┌─────────────────────────────────────────────────────────────────┐
│  Layer 4: Presentation View (呈现视图)                          │
│  ├── ViewportView          视口渲染视图                         │
│  ├── SelectionView         选中状态视图                         │
│  ├── HighlightView         高亮状态视图                         │
│  └── AnimationView         动画效果视图                         │
├─────────────────────────────────────────────────────────────────┤
│  Layer 3: Aggregated View (聚合视图)                            │
│  ├── ConnectionView        连接聚合视图                         │
│  ├── StatisticsView        统计信息视图                         │
│  ├── SearchIndexView       搜索索引视图                         │
│  └── DependencyView        依赖关系视图                         │
├─────────────────────────────────────────────────────────────────┤
│  Layer 2: Object View (对象视图)                                │
│  ├── SchemaView            结构视图                             │
│  ├── TableView             表视图                               │
│  ├── ColumnView            列视图                               │
│  ├── IndexView             索引视图                             │
│  └── RelationView          关系视图                             │
├─────────────────────────────────────────────────────────────────┤
│  Layer 1: Raw View (原始视图)                                   │
│  ├── ConnectionSource      连接数据源                           │
│  ├── CatalogSource         目录数据源                           │
│  ├── MetadataSource        元数据源                             │
│  └── EventSource           事件数据源                           │
└─────────────────────────────────────────────────────────────────┘
```

### 2.2 各层职责

| 层级            | 职责         | 更新频率 | 持久化   |
| --------------- | ------------ | -------- | -------- |
| L1 Raw          | 原始数据获取 | 实时     | 否       |
| L2 Object       | 对象建模     | 按需     | L2 Cache |
| L3 Aggregated   | 聚合计算     | 增量     | L3 Cache |
| L4 Presentation | UI 渲染      | 60fps    | 否       |

## 3. 核心组件

### 3.1 视图引擎 (ViewEngine)

```typescript
class ViewEngine {
  // 视图注册表
  private views = new Map<string, MaterializedView>()

  // 变更日志
  private changeLog: ChangeLog[] = []

  // 创建物化视图
  createView<T>(
    name: string,
    source: DataSource<T>,
    transform: ViewTransform<T>
  ): MaterializedView<T>

  // 应用增量变更
  applyDelta(viewName: string, delta: Delta<T>): void

  // 批量应用
  applyBatch(viewName: string, deltas: Delta<T>[]): void

  // 变更传播
  propagateChange(sourceView: string, delta: Delta<T>): void
}
```

### 3.2 增量处理器 (DeltaProcessor)

```typescript
class DeltaProcessor {
  // 计算差异
  computeDiff<T>(oldSnapshot: T[], newSnapshot: T[], keyExtractor: (item: T) => string): Delta<T>[]

  // 合并变更
  mergeDeltas<T>(deltas: Delta<T>[]): Delta<T>[]

  // 压缩变更
  compressDeltas<T>(deltas: Delta<T>[]): Delta<T>[]

  // 应用变更到快照
  applyToSnapshot<T>(snapshot: T[], deltas: Delta<T>[]): T[]
}
```

### 3.3 变更传播器 (ChangePropagator)

```typescript
class ChangePropagator {
  // 依赖图
  private dependencyGraph = new Graph<string>()

  // 注册依赖
  addDependency(view: string, dependsOn: string): void

  // 传播变更
  propagate(sourceView: string, delta: Delta<unknown>): void

  // 拓扑排序
  getPropagationOrder(sourceView: string): string[]
}
```

### 3.4 虚拟视口 (VirtualViewport)

```typescript
class VirtualViewport {
  // 视口配置
  config = {
    itemHeight: 28, // 每项高度
    overscan: 5, // 预渲染数量
    containerHeight: 0, // 容器高度
  }

  // 可见范围
  visibleRange = {
    start: 0,
    end: 0,
  }

  // 计算可见项
  computeVisibleRange(scrollTop: number): void

  // 获取可见项
  getVisibleItems<T>(items: T[]): T[]

  // 计算总高度
  computeTotalHeight(itemCount: number): number
}
```

## 4. 数据模型

### 4.1 导航节点模型

```typescript
interface NavigatorNode {
  // 基础标识
  id: string
  type: NodeType
  name: string

  // 层级关系
  parentId: string | null
  path: string
  depth: number

  // 状态
  state: NodeState

  // 派生数据
  derived: NodeDerived

  // 元数据
  metadata: NodeMetadata

  // 子节点（懒加载）
  children?: IncrementalCollection<NavigatorNode>
}

type NodeType =
  | 'project'
  | 'connection'
  | 'database'
  | 'schema'
  | 'table'
  | 'view'
  | 'procedure'
  | 'function'
  | 'column'
  | 'index'
  | 'trigger'
  | 'folder'

interface NodeState {
  expanded: boolean
  selected: boolean
  loading: boolean
  error: Error | null
  highlighted: boolean
  visible: boolean
}

interface NodeDerived {
  fullPath: string
  displayName: string
  iconType: string
  badgeCount: number
  hasChildren: boolean
  isLeaf: boolean
}

interface NodeMetadata {
  rowCount?: number
  size?: string
  engine?: string
  charset?: string
  createdAt?: Date
  updatedAt?: Date
  comment?: string
}
```

### 4.2 增量变更模型

```typescript
type Delta<T> = AddDelta<T> | RemoveDelta | UpdateDelta<T> | MoveDelta | ReorderDelta

interface AddDelta<T> {
  type: 'ADD'
  item: T
  position: number
  parentId: string
}

interface RemoveDelta {
  type: 'REMOVE'
  id: string
  position: number
  parentId: string
}

interface UpdateDelta<T> {
  type: 'UPDATE'
  id: string
  changes: Partial<T>
  oldValues: Partial<T>
}

interface MoveDelta {
  type: 'MOVE'
  id: string
  from: number
  to: number
  oldParentId: string
  newParentId: string
}

interface ReorderDelta {
  type: 'REORDER'
  parentId: string
  newOrder: string[]
}
```

### 4.3 物化视图模型

```typescript
interface MaterializedView<T> {
  // 视图标识
  name: string
  version: number

  // 数据快照
  snapshot: T[]

  // 索引
  index: Map<string, number> // id -> position

  // 变更处理
  applyDelta(delta: Delta<T>): void
  applyBatch(deltas: Delta<T>[]): void

  // 查询
  getById(id: string): T | undefined
  getByIndex(index: number): T | undefined
  find(predicate: (item: T) => boolean): T | undefined
  filter(predicate: (item: T) => boolean): T[]

  // 订阅
  subscribe(callback: (delta: Delta<T>) => void): () => void
}
```

## 5. 状态管理

### 5.1 响应式状态流

```
User Action → State Change → Delta Generation → View Update → Render
     ↑                                                      |
     └────────────────── Feedback Loop ←────────────────────┘
```

### 5.2 状态分层

| 状态类型     | 存储位置         | 持久化      | 同步方式  |
| ------------ | ---------------- | ----------- | --------- |
| UI State     | Pinia Store      | 否          | 本地      |
| View State   | ViewEngine       | L1 Cache    | 响应式    |
| Data State   | MaterializedView | L2/L3 Cache | 增量同步  |
| Source State | DataSource       | SQLite      | 实时/轮询 |

### 5.3 状态变更流程

```typescript
// 状态变更示例：展开节点
async function expandNode(nodeId: string) {
  // 1. 更新 UI 状态
  uiState.setExpanding(nodeId, true)

  // 2. 检查缓存
  const cached = await l2Cache.getChildren(nodeId)
  if (cached) {
    // 3a. 从缓存加载
    viewEngine.applyDelta({
      type: 'ADD',
      parentId: nodeId,
      items: cached,
    })
  } else {
    // 3b. 从服务器加载
    const children = await api.fetchChildren(nodeId)

    // 4. 更新视图
    viewEngine.applyDelta({
      type: 'ADD',
      parentId: nodeId,
      items: children,
    })

    // 5. 更新缓存
    await l2Cache.setChildren(nodeId, children)
  }

  // 6. 更新 UI 状态
  uiState.setExpanded(nodeId, true)
  uiState.setExpanding(nodeId, false)
}
```

## 6. 扩展点设计

### 6.1 节点提供者扩展

```typescript
interface NodeProvider {
  // 支持的节点类型
  nodeTypes: NodeType[]

  // 获取子节点
  getChildren(parentId: string): Promise<NavigatorNode[]>

  // 获取节点详情
  getNodeDetails(nodeId: string): Promise<NodeMetadata>

  // 搜索节点
  searchNodes(query: string): Promise<NavigatorNode[]>

  // 监听变更
  onChanges(callback: (delta: Delta<NavigatorNode>) => void): () => void
}
```

### 6.2 视图转换扩展

```typescript
interface ViewTransform<T, R> {
  // 转换名称
  name: string

  // 输入视图
  sourceView: string

  // 转换函数
  transform(snapshot: T[]): R[]

  // 增量转换
  transformDelta(delta: Delta<T>, currentSnapshot: R[]): Delta<R>

  // 依赖声明
  dependencies: string[]
}
```

### 6.3 渲染器扩展

```typescript
interface NodeRenderer {
  // 支持的节点类型
  nodeTypes: NodeType[]

  // 渲染节点
  render(node: NavigatorNode, props: RenderProps): VNode

  // 渲染前缀图标
  renderPrefix?(node: NavigatorNode): VNode

  // 渲染后缀信息
  renderSuffix?(node: NavigatorNode): VNode

  // 渲染上下文菜单
  renderContextMenu?(node: NavigatorNode): MenuItem[]
}
```

## 7. 错误处理

### 7.1 错误分类

| 错误类型 | 处理策略          | 用户反馈       |
| -------- | ----------------- | -------------- |
| 网络错误 | 重试 + 降级到缓存 | 显示离线指示器 |
| 超时错误 | 取消 + 提示       | 显示超时提示   |
| 数据错误 | 跳过 + 记录       | 静默处理       |
| 渲染错误 | 降级 + 恢复       | 显示降级视图   |

### 7.2 错误恢复

```typescript
class ErrorRecovery {
  // 重试策略
  retry<T>(operation: () => Promise<T>, options: RetryOptions): Promise<T>

  // 降级策略
  fallback<T>(primary: () => Promise<T>, fallback: () => Promise<T>): Promise<T>

  // 断路器
  circuitBreaker(operation: () => Promise<void>, threshold: number): Promise<void>
}
```

## 8. 监控与调试

### 8.1 性能监控

```typescript
interface PerformanceMetrics {
  // 渲染性能
  renderTime: number
  frameRate: number

  // 数据性能
  loadTime: number
  syncTime: number
  cacheHitRate: number

  // 资源使用
  memoryUsage: number
  nodeCount: number
  viewCount: number
}
```

### 8.2 调试工具

```typescript
// 开发工具集成
class NavigatorDevTools {
  // 查看视图状态
  inspectView(viewName: string): ViewSnapshot

  // 查看变更历史
  getChangeHistory(): ChangeLog[]

  // 模拟变更
  simulateDelta(delta: Delta<unknown>): void

  // 性能分析
  profile(): PerformanceReport
}
```
