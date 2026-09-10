# IVM 导航栏数据流设计

> 版本：v1.0
> 最后更新：2026-05-03
> 状态：✅ 持续更新

---

## 1. 数据流概述

### 1.1 核心原则

- **单向数据流**：数据从 Source → View → UI 单向流动
- **增量传播**：变更以增量形式在各层间传播
- **响应式更新**：数据变化自动触发视图更新
- **不可变数据**：每次变更产生新数据，便于追踪和回滚

### 1.2 数据流架构图

```
┌─────────────────────────────────────────────────────────────────────┐
│                          Data Sources                               │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐              │
│  │   WebSocket  │  │    HTTP API  │  │   SQLite     │              │
│  │  (Real-time) │  │   (Request)  │  │   (Local)    │              │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘              │
└─────────┼─────────────────┼─────────────────┼──────────────────────┘
          │                 │                 │
          ▼                 ▼                 ▼
┌─────────────────────────────────────────────────────────────────────┐
│                        Source Layer (L1)                            │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │                    Raw Data Stream                           │  │
│  │  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐         │  │
│  │  │Connection│  │ Database│  │  Table  │  │  Column │         │  │
│  │  │  Meta   │  │  Meta   │  │  Meta   │  │  Meta   │         │  │
│  │  └────┬────┘  └────┬────┘  └────┬────┘  └────┬────┘         │  │
│  └───────┼────────────┼────────────┼────────────┼────────────────┘  │
└──────────┼────────────┼────────────┼────────────┼───────────────────┘
           │            │            │            │
           ▼            ▼            ▼            ▼
┌─────────────────────────────────────────────────────────────────────┐
│                     Transformation Layer                            │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │                    Delta Processing                          │  │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐       │  │
│  │  │   Normalize  │→ │   Validate   │→ │   Enrich     │       │  │
│  │  └──────────────┘  └──────────────┘  └──────────────┘       │  │
│  └──────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────┘
           │
           ▼
┌─────────────────────────────────────────────────────────────────────┐
│                      View Layer (L2-L4)                             │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐              │
│  │  Object View │→ │Aggregate View│→ │Present View  │              │
│  │   (L2)       │  │   (L3)       │  │   (L4)       │              │
│  └──────────────┘  └──────────────┘  └──────────────┘              │
└─────────────────────────────────────────────────────────────────────┘
           │
           ▼
┌─────────────────────────────────────────────────────────────────────┐
│                         UI Layer                                    │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │                     Vue Components                           │  │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐     │  │
│  │  │  Tree    │  │  Node    │  │  Search  │  │  Toolbar │     │  │
│  │  └──────────┘  └──────────┘  └──────────┘  └──────────┘     │  │
│  └──────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────┘
```

## 2. 数据源层 (Source Layer)

### 2.1 WebSocket 实时流

```typescript
// WebSocket 数据源
class WebSocketDataSource implements DataSource<MetadataEvent> {
  private ws: WebSocket
  private eventStream = new Subject<MetadataEvent>()

  connect(connectionId: string) {
    this.ws = new WebSocket(`ws://localhost:${port}/metadata/${connectionId}`)

    this.ws.onmessage = event => {
      const message: MetadataEvent = JSON.parse(event.data)
      this.eventStream.next(message)
    }
  }

  // 订阅事件流
  subscribe(callback: (event: MetadataEvent) => void): Subscription {
    return this.eventStream.subscribe(callback)
  }
}

// 事件类型
type MetadataEvent =
  | { type: 'TABLE_CREATED'; data: TableMetadata }
  | { type: 'TABLE_DROPPED'; data: { tableId: string } }
  | { type: 'TABLE_ALTERED'; data: TableMetadata }
  | { type: 'COLUMN_ADDED'; data: ColumnMetadata }
  | { type: 'INDEX_CREATED'; data: IndexMetadata }
  | { type: 'CONNECTION_STATUS'; data: { status: ConnectionStatus } }
```

### 2.2 HTTP API 请求

```typescript
// HTTP 数据源
class HTTPDataSource implements DataSource<MetadataResponse> {
  // 批量获取元数据
  async fetchMetadata(
    connectionId: string,
    type: MetadataType,
    parentId?: string
  ): Promise<MetadataResponse>

  // 增量同步
  async syncMetadata(connectionId: string, since: Timestamp): Promise<Delta<Metadata>[]>

  // 搜索
  async searchMetadata(
    connectionId: string,
    query: string,
    options: SearchOptions
  ): Promise<SearchResult>
}
```

### 2.3 SQLite 本地缓存

```typescript
// SQLite 数据源
class SQLiteDataSource implements DataSource<LocalMetadata> {
  private db: Database

  // 初始化表结构
  async initialize() {
    await this.db.exec(`
      CREATE TABLE IF NOT EXISTS navigator_cache (
        id TEXT PRIMARY KEY,
        type TEXT NOT NULL,
        parent_id TEXT,
        data JSON NOT NULL,
        version INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
      );
      
      CREATE INDEX IF NOT EXISTS idx_parent ON navigator_cache(parent_id);
      CREATE INDEX IF NOT EXISTS idx_type ON navigator_cache(type);
      CREATE INDEX IF NOT EXISTS idx_updated ON navigator_cache(updated_at);
    `)
  }

  // 获取子节点
  async getChildren(parentId: string): Promise<LocalMetadata[]> {
    return this.db.all('SELECT * FROM navigator_cache WHERE parent_id = ? ORDER BY name', [
      parentId,
    ])
  }

  // 批量写入
  async batchWrite(items: LocalMetadata[]): Promise<void> {
    const stmt = await this.db.prepare(`
      INSERT OR REPLACE INTO navigator_cache 
      (id, type, parent_id, data, version, updated_at)
      VALUES (?, ?, ?, ?, ?, ?)
    `)

    for (const item of items) {
      await stmt.run(
        item.id,
        item.type,
        item.parentId,
        JSON.stringify(item.data),
        item.version,
        Date.now()
      )
    }

    await stmt.finalize()
  }
}
```

## 3. 数据转换层 (Transformation Layer)

### 3.1 标准化处理

```typescript
// 数据标准化
class DataNormalizer {
  // 将各种数据源格式统一为标准格式
  normalize<T>(source: DataSourceType, data: unknown): NormalizedData<T> {
    switch (source) {
      case 'websocket':
        return this.normalizeWebSocketEvent(data as MetadataEvent)
      case 'http':
        return this.normalizeHTTPResponse(data as MetadataResponse)
      case 'sqlite':
        return this.normalizeLocalData(data as LocalMetadata)
      default:
        throw new Error(`Unknown source: ${source}`)
    }
  }

  private normalizeWebSocketEvent(event: MetadataEvent): NormalizedData<unknown> {
    return {
      id: this.generateId(event),
      type: this.mapEventType(event.type),
      data: event.data,
      timestamp: Date.now(),
      source: 'websocket',
    }
  }
}
```

### 3.2 数据验证

```typescript
// 数据验证
class DataValidator {
  // 验证节点数据完整性
  validateNode(node: unknown): ValidationResult<NavigatorNode> {
    const schema = z.object({
      id: z.string(),
      type: z.enum(['table', 'view', 'column', 'index', ...]),
      name: z.string(),
      parentId: z.string().nullable(),
      metadata: z.object({}).optional()
    })

    const result = schema.safeParse(node)

    if (result.success) {
      return { valid: true, data: result.data }
    } else {
      return { valid: false, errors: result.error.errors }
    }
  }
}
```

### 3.3 数据增强

```typescript
// 数据增强
class DataEnricher {
  // 为节点添加派生数据
  enrich(node: NavigatorNode): EnrichedNode {
    return {
      ...node,
      derived: {
        fullPath: this.computePath(node),
        displayName: this.formatName(node),
        iconType: this.determineIcon(node),
        hasChildren: this.checkChildren(node),
        statistics: this.computeStats(node),
      },
    }
  }

  private computePath(node: NavigatorNode): string {
    // 计算完整路径
    const parts: string[] = []
    let current: NavigatorNode | undefined = node

    while (current) {
      parts.unshift(current.name)
      current = this.getParent(current)
    }

    return parts.join('.')
  }
}
```

## 4. 视图层数据流 (View Layer)

### 4.1 物化视图更新流程

```
Raw Data → Normalize → Validate → Enrich → Apply Delta → Notify Subscribers
                                              ↓
                                      ┌───────────────┐
                                      │  Compute Diff  │
                                      │  Update Index  │
                                      │  Update Stats  │
                                      └───────────────┘
```

### 4.2 增量应用流程

```typescript
// 视图更新流程
class ViewUpdateFlow {
  // 主流程
  async updateView(viewName: string, newData: unknown[]) {
    // 1. 获取当前快照
    const currentSnapshot = this.viewEngine.getSnapshot(viewName)

    // 2. 计算差异
    const deltas = this.deltaProcessor.computeDiff(currentSnapshot, newData, item => item.id)

    // 3. 应用增量
    for (const delta of deltas) {
      await this.applyDelta(viewName, delta)
    }

    // 4. 通知订阅者
    this.notifySubscribers(viewName, deltas)
  }

  // 应用单个增量
  private async applyDelta(viewName: string, delta: Delta<unknown>) {
    const view = this.viewEngine.getView(viewName)

    switch (delta.type) {
      case 'ADD':
        await this.handleAdd(view, delta)
        break
      case 'REMOVE':
        await this.handleRemove(view, delta)
        break
      case 'UPDATE':
        await this.handleUpdate(view, delta)
        break
      case 'MOVE':
        await this.handleMove(view, delta)
        break
    }

    // 更新依赖视图
    await this.propagateToDependents(viewName, delta)
  }
}
```

### 4.3 多视图联动

```typescript
// 视图依赖关系
const viewDependencies = {
  raw: [],
  object: ['raw'],
  filtered: ['object'],
  sorted: ['filtered'],
  aggregated: ['sorted'],
  viewport: ['aggregated'],
}

// 变更传播
class ViewPropagation {
  // 传播变更到依赖视图
  propagate(sourceView: string, delta: Delta<unknown>) {
    const dependents = this.getDependents(sourceView)

    for (const dependent of dependents) {
      // 转换增量
      const transformedDelta = this.transformDelta(delta, sourceView, dependent)

      // 应用到依赖视图
      this.viewEngine.applyDelta(dependent, transformedDelta)
    }
  }

  // 获取依赖视图（拓扑排序）
  private getDependents(sourceView: string): string[] {
    const dependents: string[] = []

    for (const [view, deps] of Object.entries(viewDependencies)) {
      if (deps.includes(sourceView)) {
        dependents.push(view)
      }
    }

    // 按依赖深度排序
    return this.topologicalSort(dependents)
  }
}
```

## 5. UI 层数据流 (UI Layer)

### 5.1 Vue 响应式集成

```typescript
// Vue Composable
export function useNavigatorView(options: ViewOptions) {
  // 创建响应式状态
  const state = reactive({
    nodes: [],
    loading: false,
    error: null,
    expandedKeys: new Set<string>(),
    selectedKeys: new Set<string>(),
  })

  // 订阅视图变更
  const unsubscribe = viewEngine.subscribe(options.viewName, delta => {
    // 应用增量到响应式状态
    applyDeltaToState(state, delta)
  })

  // 计算属性
  const visibleNodes = computed(() => {
    return state.nodes.filter(node => isNodeVisible(node, state.expandedKeys))
  })

  // 方法
  const expandNode = async (nodeId: string) => {
    state.expandedKeys.add(nodeId)

    // 加载子节点
    if (!hasChildrenLoaded(nodeId)) {
      await loadChildren(nodeId)
    }
  }

  // 清理
  onUnmounted(() => {
    unsubscribe()
  })

  return {
    state,
    visibleNodes,
    expandNode,
    collapseNode,
    selectNode,
  }
}
```

### 5.2 虚拟滚动数据流

```typescript
// 虚拟滚动数据流
class VirtualScrollDataFlow {
  // 视口状态
  viewport = reactive({
    scrollTop: 0,
    containerHeight: 0,
    itemHeight: 28,
    overscan: 5,
  })

  // 计算可见范围
  visibleRange = computed(() => {
    const start = Math.floor(viewport.scrollTop / viewport.itemHeight)
    const count = Math.ceil(viewport.containerHeight / viewport.itemHeight)

    return {
      start: Math.max(0, start - viewport.overscan),
      end: start + count + viewport.overscan,
    }
  })

  // 可见项
  visibleItems = computed(() => {
    const { start, end } = visibleRange.value
    return allNodes.value.slice(start, end)
  })

  // 总高度
  totalHeight = computed(() => {
    return allNodes.value.length * viewport.itemHeight
  })

  // 处理滚动
  onScroll(scrollTop: number) {
    viewport.scrollTop = scrollTop
  }
}
```

## 6. 缓存数据流 (Cache Layer)

### 6.1 三级缓存策略

```
User Request
     │
     ▼
┌─────────┐  Miss   ┌─────────┐  Miss   ┌─────────┐  Miss   ┌─────────┐
│  L1     │ ──────→ │  L2     │ ──────→ │  L3     │ ──────→ │ Source  │
│ Memory  │         │IndexedDB│         │ SQLite  │         │ Server  │
└────┬────┘         └────┬────┘         └────┬────┘         └────┬────┘
     │ Hit               │ Hit               │ Hit               │
     ▼                   ▼                   ▼                   ▼
  Return              Return              Return              Return
  (0.1ms)             (5ms)               (20ms)              (200ms)
```

### 6.2 缓存同步流程

```typescript
// 缓存同步
class CacheSynchronizer {
  // 写入时同步到各级缓存
  async write(key: string, value: unknown, options: WriteOptions) {
    // 1. 写入 L1
    this.l1Cache.set(key, value)

    // 2. 异步写入 L2
    if (options.persist !== false) {
      this.l2Cache.set(key, value).catch(err => {
        console.warn('L2 cache write failed:', err)
      })
    }

    // 3. 批量写入 L3
    if (options.batch) {
      this.batchBuffer.push({ key, value })
      this.scheduleFlush()
    } else {
      await this.l3Cache.set(key, value)
    }
  }

  // 读取时逐级查找
  async read<T>(key: string): Promise<T | undefined> {
    // 1. 尝试 L1
    const l1Value = this.l1Cache.get(key)
    if (l1Value !== undefined) {
      return l1Value as T
    }

    // 2. 尝试 L2
    const l2Value = await this.l2Cache.get(key)
    if (l2Value !== undefined) {
      // 提升到 L1
      this.l1Cache.set(key, l2Value)
      return l2Value as T
    }

    // 3. 尝试 L3
    const l3Value = await this.l3Cache.get(key)
    if (l3Value !== undefined) {
      // 提升到 L1 和 L2
      this.l1Cache.set(key, l3Value)
      this.l2Cache.set(key, l3Value).catch(console.warn)
      return l3Value as T
    }

    return undefined
  }
}
```

## 7. 实时同步数据流

### 7.1 WebSocket 事件处理

```typescript
// WebSocket 事件处理器
class WebSocketEventHandler {
  // 事件处理映射
  private handlers: Map<string, EventHandler> = new Map([
    ['TABLE_CREATED', this.handleTableCreated],
    ['TABLE_DROPPED', this.handleTableDropped],
    ['TABLE_ALTERED', this.handleTableAltered],
    ['COLUMN_ADDED', this.handleColumnAdded],
    ['INDEX_CREATED', this.handleIndexCreated],
  ])

  // 处理事件
  async handleEvent(event: MetadataEvent) {
    const handler = this.handlers.get(event.type)
    if (!handler) {
      console.warn(`No handler for event type: ${event.type}`)
      return
    }

    // 转换为增量
    const delta = await handler(event.data)

    // 应用到视图
    await this.viewEngine.applyDelta('raw', delta)

    // 更新缓存
    await this.updateCache(delta)
  }

  // 处理表创建
  private async handleTableCreated(data: TableMetadata): Promise<Delta<unknown>> {
    return {
      type: 'ADD',
      item: {
        id: data.id,
        type: 'table',
        name: data.name,
        parentId: data.schemaId,
        metadata: data,
      },
      position: -1, // 追加到末尾
      parentId: data.schemaId,
    }
  }
}
```

### 7.2 离线同步

```typescript
// 离线同步管理器
class OfflineSyncManager {
  private pendingChanges: LocalChange[] = []
  private isOnline = true

  // 监听网络状态
  constructor() {
    window.addEventListener('online', () => this.onOnline())
    window.addEventListener('offline', () => this.onOffline())
  }

  // 离线时缓存变更
  async queueChange(change: LocalChange) {
    if (!this.isOnline) {
      this.pendingChanges.push(change)
      await this.persistPendingChanges()
      return
    }

    // 在线时直接发送
    await this.sendChange(change)
  }

  // 恢复在线时同步
  private async onOnline() {
    this.isOnline = true

    // 加载待同步的变更
    const changes = await this.loadPendingChanges()

    // 批量同步
    for (const change of changes) {
      try {
        await this.sendChange(change)
        await this.removePendingChange(change.id)
      } catch (err) {
        console.error('Sync failed for change:', change.id, err)
        break
      }
    }
  }
}
```

## 8. 数据流监控

### 8.1 性能指标采集

```typescript
// 数据流监控
class DataFlowMonitor {
  private metrics: DataFlowMetrics = {
    sourceLatency: new Histogram(),
    transformLatency: new Histogram(),
    viewUpdateLatency: new Histogram(),
    renderLatency: new Histogram(),
    cacheHitRate: new Gauge(),
    deltaCount: new Counter(),
  }

  // 记录数据源延迟
  recordSourceLatency(source: string, latency: number) {
    this.metrics.sourceLatency.observe(latency, { source })
  }

  // 记录缓存命中率
  recordCacheAccess(level: CacheLevel, hit: boolean) {
    this.metrics.cacheHitRate.set(hit ? 1 : 0, { level })
  }

  // 记录增量数量
  recordDeltaCount(viewName: string, count: number) {
    this.metrics.deltaCount.inc(count, { view: viewName })
  }
}
```

### 8.2 数据流追踪

```typescript
// 数据流追踪
class DataFlowTracer {
  private traces: Map<string, DataTrace> = new Map()

  // 开始追踪
  startTrace(dataId: string): TraceContext {
    const trace: DataTrace = {
      id: generateTraceId(),
      dataId,
      startTime: performance.now(),
      stages: [],
    }

    this.traces.set(trace.id, trace)

    return {
      id: trace.id,
      stage: (name: string, data: unknown) => {
        this.recordStage(trace.id, name, data)
      },
      end: () => {
        this.endTrace(trace.id)
      },
    }
  }

  // 记录阶段
  private recordStage(traceId: string, stageName: string, data: unknown) {
    const trace = this.traces.get(traceId)
    if (!trace) return

    trace.stages.push({
      name: stageName,
      timestamp: performance.now(),
      dataSize: JSON.stringify(data).length,
    })
  }
}
```
