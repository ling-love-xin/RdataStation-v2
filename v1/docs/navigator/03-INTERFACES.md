# IVM 导航栏接口规范

> 版本：v1.0
> 最后更新：2026-05-03
> 状态：✅ 持续更新

---

## 1. 接口概述

### 1.1 设计原则

- **类型安全**：所有接口使用 TypeScript 强类型
- **向后兼容**：接口版本化，支持渐进升级
- **错误处理**：统一的错误码和错误处理机制
- **文档完备**：每个接口都有详细的文档说明

### 1.2 接口分层

| 层级 | 接口类型     | 说明                  |
| ---- | ------------ | --------------------- |
| L1   | 数据源接口   | 与后端通信的接口      |
| L2   | 视图引擎接口 | 视图管理和操作接口    |
| L3   | 组件接口     | Vue 组件 Props/Events |
| L4   | 扩展接口     | 插件扩展点接口        |

## 2. 数据源接口 (L1)

### 2.1 WebSocket 接口

```typescript
// WebSocket 连接配置
interface WebSocketConfig {
  url: string
  connectionId: string
  reconnectInterval?: number
  maxReconnectAttempts?: number
  heartbeatInterval?: number
}

// WebSocket 数据源接口
interface IWebSocketDataSource {
  // 连接管理
  connect(config: WebSocketConfig): Promise<void>
  disconnect(): void
  reconnect(): Promise<void>

  // 状态查询
  isConnected(): boolean
  getConnectionState(): ConnectionState

  // 事件订阅
  onMessage(handler: (event: MetadataEvent) => void): () => void
  onConnect(handler: () => void): () => void
  onDisconnect(handler: (reason: string) => void): () => void
  onError(handler: (error: Error) => void): () => void
}

// 元数据事件
interface MetadataEvent {
  id: string
  type: EventType
  timestamp: number
  connectionId: string
  data: EventData
}

type EventType =
  | 'TABLE_CREATED'
  | 'TABLE_DROPPED'
  | 'TABLE_ALTERED'
  | 'COLUMN_ADDED'
  | 'COLUMN_DROPPED'
  | 'COLUMN_ALTERED'
  | 'INDEX_CREATED'
  | 'INDEX_DROPPED'
  | 'CONNECTION_STATUS'
  | 'SYNC_COMPLETE'

type EventData =
  | TableEventData
  | ColumnEventData
  | IndexEventData
  | ConnectionStatusData
  | SyncCompleteData
```

### 2.2 HTTP API 接口

```typescript
// HTTP 数据源接口
interface IHTTPDataSource {
  // 元数据获取
  fetchMetadata(
    connectionId: string,
    type: MetadataType,
    parentId?: string,
    options?: FetchOptions
  ): Promise<MetadataResponse>

  // 增量同步
  syncMetadata(connectionId: string, since: Timestamp, options?: SyncOptions): Promise<SyncResponse>

  // 搜索
  searchMetadata(
    connectionId: string,
    query: string,
    options?: SearchOptions
  ): Promise<SearchResponse>

  // 批量获取
  batchFetch(connectionId: string, requests: BatchRequest[]): Promise<BatchResponse>
}

// 请求选项
interface FetchOptions {
  limit?: number
  offset?: number
  sortBy?: string
  sortOrder?: 'asc' | 'desc'
  filter?: FilterCondition
}

interface SyncOptions {
  batchSize?: number
  timeout?: number
  includeDeleted?: boolean
}

interface SearchOptions {
  types?: MetadataType[]
  fuzzy?: boolean
  limit?: number
  highlight?: boolean
}

// 响应类型
interface MetadataResponse {
  data: Metadata[]
  total: number
  hasMore: boolean
  version: number
  timestamp: Timestamp
}

interface SyncResponse {
  deltas: Delta<Metadata>[]
  version: number
  timestamp: Timestamp
  hasMore: boolean
  nextCursor?: string
}

interface SearchResponse {
  results: SearchResult[]
  total: number
  highlights?: Map<string, string[]>
  suggestions?: string[]
}
```

### 2.3 本地缓存接口

```typescript
// 本地缓存接口
interface ILocalCache {
  // 基础操作
  get<T>(key: string): Promise<T | undefined>
  set<T>(key: string, value: T, options?: CacheOptions): Promise<void>
  delete(key: string): Promise<void>
  clear(): Promise<void>

  // 批量操作
  batchGet<T>(keys: string[]): Promise<Map<string, T>>
  batchSet<T>(entries: Array<[string, T]>, options?: CacheOptions): Promise<void>
  batchDelete(keys: string[]): Promise<void>

  // 查询操作
  query<T>(predicate: (value: T) => boolean): Promise<T[]>
  find<T>(predicate: (value: T) => boolean): Promise<T | undefined>

  // 索引操作
  createIndex(name: string, keyExtractor: (value: unknown) => string): Promise<void>
  queryByIndex(indexName: string, key: string): Promise<unknown[]>

  // 元数据
  getMetadata(key: string): Promise<CacheMetadata | undefined>
  setMetadata(key: string, metadata: CacheMetadata): Promise<void>

  // 统计
  getStats(): Promise<CacheStats>
}

interface CacheOptions {
  ttl?: number
  priority?: 'high' | 'normal' | 'low'
  compress?: boolean
}

interface CacheMetadata {
  createdAt: number
  updatedAt: number
  version: number
  size: number
  checksum: string
}

interface CacheStats {
  totalEntries: number
  totalSize: number
  hitCount: number
  missCount: number
  hitRate: number
}
```

## 3. 视图引擎接口 (L2)

### 3.1 视图引擎核心接口

```typescript
// 视图引擎接口
interface IViewEngine {
  // 视图管理
  createView<T>(config: ViewConfig<T>): MaterializedView<T>
  getView<T>(name: string): MaterializedView<T> | undefined
  deleteView(name: string): void
  listViews(): string[]

  // 视图操作
  applyDelta<T>(viewName: string, delta: Delta<T>): void
  applyBatch<T>(viewName: string, deltas: Delta<T>[]): void
  refreshView(viewName: string): Promise<void>

  // 依赖管理
  addDependency(view: string, dependsOn: string): void
  removeDependency(view: string, dependsOn: string): void
  getDependencies(view: string): string[]
  getDependents(view: string): string[]

  // 订阅管理
  subscribe<T>(viewName: string, callback: ViewChangeCallback<T>): () => void
  subscribeToDelta<T>(viewName: string, callback: DeltaCallback<T>): () => void

  // 事务支持
  beginTransaction(): ViewTransaction
  commitTransaction(transaction: ViewTransaction): void
  rollbackTransaction(transaction: ViewTransaction): void
}

// 视图配置
interface ViewConfig<T> {
  name: string
  source: DataSource<T>
  transform?: ViewTransform<T, unknown>
  indexBy?: keyof T | ((item: T) => string)
  initialData?: T[]
}

// 物化视图接口
interface MaterializedView<T> {
  readonly name: string
  readonly version: number
  readonly snapshot: readonly T[]
  readonly size: number

  // 查询
  getById(id: string): T | undefined
  getByIndex(index: number): T | undefined
  find(predicate: (item: T) => boolean): T | undefined
  filter(predicate: (item: T) => boolean): T[]
  map<R>(mapper: (item: T) => R): R[]
  reduce<R>(reducer: (acc: R, item: T) => R, initial: R): R

  // 排序和分页
  sort(compareFn: (a: T, b: T) => number): T[]
  slice(start: number, end: number): T[]

  // 订阅
  onChange(callback: (delta: Delta<T>) => void): () => void
  onBatch(callback: (deltas: Delta<T>[]) => void): () => void
}

// 视图变更回调
type ViewChangeCallback<T> = (snapshot: T[], delta: Delta<T>) => void
type DeltaCallback<T> = (delta: Delta<T>) => void

// 视图事务
interface ViewTransaction {
  id: string
  views: Map<string, TransactionView>

  apply<T>(viewName: string, delta: Delta<T>): void
  commit(): void
  rollback(): void
}
```

### 3.2 增量处理器接口

```typescript
// 增量处理器接口
interface IDeltaProcessor {
  // 差异计算
  computeDiff<T>(oldSnapshot: T[], newSnapshot: T[], keyExtractor: (item: T) => string): Delta<T>[]

  // 增量合并
  mergeDeltas<T>(deltas: Delta<T>[]): Delta<T>[]

  // 增量压缩
  compressDeltas<T>(deltas: Delta<T>[]): Delta<T>[]

  // 增量应用
  applyToSnapshot<T>(snapshot: T[], deltas: Delta<T>[]): T[]

  // 增量验证
  validateDelta<T>(delta: Delta<T>, schema: ZodSchema<T>): ValidationResult

  // 增量转换
  transformDelta<T, R>(delta: Delta<T>, transformer: (item: T) => R): Delta<R>
}

// 差异算法选项
interface DiffOptions {
  algorithm: 'myers' | 'patience' | 'histogram'
  ignoreWhitespace?: boolean
  ignoreCase?: boolean
  contextLines?: number
}

// 验证结果
interface ValidationResult {
  valid: boolean
  errors?: ValidationError[]
}

interface ValidationError {
  path: string
  message: string
  code: string
}
```

### 3.3 虚拟视口接口

```typescript
// 虚拟视口接口
interface IVirtualViewport {
  // 配置
  config: ViewportConfig

  // 状态
  scrollTop: number
  containerHeight: number
  visibleRange: Range

  // 计算
  computeVisibleRange(scrollTop: number): Range
  getVisibleItems<T>(items: T[]): T[]
  computeTotalHeight(itemCount: number): number
  computeScrollPosition(index: number): number

  // 事件
  onScroll(callback: (scrollTop: number) => void): () => void
  onResize(callback: (height: number) => void): () => void

  // 滚动到指定位置
  scrollToIndex(index: number, behavior?: ScrollBehavior): void
  scrollToItem(itemId: string, behavior?: ScrollBehavior): void
}

interface ViewportConfig {
  itemHeight: number
  overscan: number
  estimateItemHeight?: (item: unknown) => number
  getItemKey?: (item: unknown, index: number) => string
}

interface Range {
  start: number
  end: number
}
```

## 4. 组件接口 (L3)

### 4.1 导航树组件接口

```typescript
// 导航树组件 Props
interface NavigatorTreeProps {
  // 数据源
  viewName: string
  connectionId?: string

  // 配置
  config?: NavigatorConfig

  // 初始状态
  defaultExpandedKeys?: string[]
  defaultSelectedKeys?: string[]

  // 自定义渲染
  renderNode?: (node: NavigatorNode, props: NodeRenderProps) => VNode
  renderPrefix?: (node: NavigatorNode) => VNode
  renderSuffix?: (node: NavigatorNode) => VNode
  renderEmpty?: () => VNode
  renderLoading?: () => VNode
  renderError?: (error: Error) => VNode
}

// 导航树组件 Events
interface NavigatorTreeEvents {
  // 节点事件
  onNodeClick(node: NavigatorNode, event: MouseEvent): void
  onNodeDoubleClick(node: NavigatorNode, event: MouseEvent): void
  onNodeExpand(node: NavigatorNode): void
  onNodeCollapse(node: NavigatorNode): void
  onNodeSelect(node: NavigatorNode, selected: boolean): void

  // 加载事件
  onLoadStart(nodeId: string): void
  onLoadComplete(nodeId: string): void
  onLoadError(nodeId: string, error: Error): void

  // 拖拽事件
  onNodeDragStart(node: NavigatorNode, event: DragEvent): void
  onNodeDragEnd(node: NavigatorNode, event: DragEvent): void
  onNodeDrop(targetNode: NavigatorNode, draggedNode: NavigatorNode, event: DragEvent): void
}

// 导航树配置
interface NavigatorConfig {
  // 虚拟滚动
  virtualScroll?: boolean
  itemHeight?: number
  overscan?: number

  // 懒加载
  lazyLoad?: boolean
  loadOnExpand?: boolean

  // 选择
  selectable?: boolean
  multiSelect?: boolean
  checkable?: boolean

  // 拖拽
  draggable?: boolean
  droppable?: boolean
  dragPreview?: (node: NavigatorNode) => VNode

  // 动画
  animate?: boolean
  animationDuration?: number

  // 搜索
  searchable?: boolean
  searchDebounce?: number
  highlightSearch?: boolean
}
```

### 4.2 导航节点组件接口

```typescript
// 导航节点组件 Props
interface NavigatorNodeProps {
  node: NavigatorNode
  level: number
  index: number

  // 状态
  expanded: boolean
  selected: boolean
  loading: boolean
  highlighted: boolean

  // 配置
  config?: NodeConfig

  // 自定义渲染
  renderIcon?: (node: NavigatorNode) => VNode
  renderLabel?: (node: NavigatorNode) => VNode
  renderActions?: (node: NavigatorNode) => VNode
}

// 导航节点配置
interface NodeConfig {
  // 缩进
  indentSize: number
  showIndentGuide: boolean

  // 图标
  iconSize: number
  showExpandIcon: boolean
  expandIconPosition: 'left' | 'right'

  // 标签
  labelMaxLength: number
  showTooltip: boolean
  tooltipDelay: number

  // 交互
  clickToExpand: boolean
  doubleClickToOpen: boolean
  hoverDelay: number
}
```

### 4.3 搜索面板组件接口

```typescript
// 搜索面板组件 Props
interface SearchPanelProps {
  // 数据源
  viewName: string

  // 配置
  config?: SearchConfig

  // 初始值
  defaultQuery?: string
  defaultFilters?: SearchFilter[]
}

// 搜索面板组件 Events
interface SearchPanelEvents {
  onSearch(query: string, filters: SearchFilter[]): void
  onResultClick(result: SearchResult): void
  onFilterChange(filters: SearchFilter[]): void
}

// 搜索配置
interface SearchConfig {
  // 输入
  placeholder?: string
  debounce?: number
  minLength?: number
  maxLength?: number

  // 筛选
  filters?: FilterOption[]
  defaultFilter?: string

  // 结果
  pageSize?: number
  highlightMatches?: boolean
  showRecentSearches?: boolean
  maxRecentSearches?: number

  // 快捷键
  shortcut?: string
}

interface FilterOption {
  key: string
  label: string
  type: 'select' | 'checkbox' | 'radio' | 'date'
  options?: Array<{ label: string; value: unknown }>
}
```

## 5. 扩展接口 (L4)

### 5.1 节点提供者接口

```typescript
// 节点提供者接口
interface INodeProvider {
  // 标识
  readonly id: string
  readonly name: string
  readonly version: string

  // 支持的节点类型
  readonly supportedTypes: NodeType[]

  // 获取子节点
  getChildren(parentId: string, context: ProviderContext): Promise<NavigatorNode[]>

  // 获取节点详情
  getNodeDetails(nodeId: string): Promise<NodeMetadata>

  // 搜索节点
  searchNodes(query: string, options: SearchOptions): Promise<NavigatorNode[]>

  // 监听变更
  onChanges(callback: (delta: Delta<NavigatorNode>) => void): () => void

  // 生命周期
  activate(context: ExtensionContext): Promise<void>
  deactivate(): Promise<void>
}

// 提供者上下文
interface ProviderContext {
  connectionId: string
  database?: string
  schema?: string
  signal?: AbortSignal
}

// 扩展上下文
interface ExtensionContext {
  // 服务访问
  viewEngine: IViewEngine
  cache: ILocalCache
  api: IHTTPDataSource

  // 工具
  logger: ILogger
  telemetry: ITelemetry

  // 存储
  globalState: IStorage
  workspaceState: IStorage

  // 事件
  onDidChangeConnection: Event<ConnectionChangeEvent>
  onDidChangeConfiguration: Event<ConfigurationChangeEvent>
}
```

### 5.2 视图转换器接口

```typescript
// 视图转换器接口
interface IViewTransform<T, R> {
  // 标识
  readonly name: string
  readonly sourceView: string
  readonly targetView: string

  // 依赖声明
  readonly dependencies: string[]

  // 转换函数
  transform(snapshot: T[]): R[]

  // 增量转换
  transformDelta(delta: Delta<T>, currentSnapshot: R[]): Delta<R>

  // 批量转换优化
  transformBatch?(deltas: Delta<T>[], currentSnapshot: R[]): Delta<R>[]

  // 验证
  validate?(data: R[]): ValidationResult
}

// 内置转换器
interface BuiltInTransforms {
  // 筛选转换器
  createFilterTransform<T>(predicate: (item: T) => boolean): IViewTransform<T, T>

  // 排序转换器
  createSortTransform<T>(compareFn: (a: T, b: T) => number): IViewTransform<T, T>

  // 映射转换器
  createMapTransform<T, R>(mapper: (item: T) => R): IViewTransform<T, R>

  // 聚合转换器
  createAggregateTransform<T, R>(aggregator: (items: T[]) => R): IViewTransform<T, R>

  // 分组转换器
  createGroupTransform<T>(keyExtractor: (item: T) => string): IViewTransform<T, GroupedItems<T>>
}
```

### 5.3 节点渲染器接口

```typescript
// 节点渲染器接口
interface INodeRenderer {
  // 标识
  readonly id: string
  readonly name: string

  // 支持的节点类型
  readonly supportedTypes: NodeType[]
  readonly priority: number

  // 渲染方法
  render(node: NavigatorNode, props: RenderProps): VNode
  renderIcon?(node: NavigatorNode): VNode
  renderLabel?(node: NavigatorNode): VNode
  renderSuffix?(node: NavigatorNode): VNode
  renderContextMenu?(node: NavigatorNode): MenuItem[]
  renderTooltip?(node: NavigatorNode): VNode
  renderPreview?(node: NavigatorNode): VNode

  // 交互处理
  handleClick?(node: NavigatorNode, event: MouseEvent): boolean
  handleDoubleClick?(node: NavigatorNode, event: MouseEvent): boolean
  handleExpand?(node: NavigatorNode): void
  handleCollapse?(node: NavigatorNode): void
}

// 渲染属性
interface RenderProps {
  level: number
  expanded: boolean
  selected: boolean
  loading: boolean
  highlighted: boolean
  hasChildren: boolean
}

// 菜单项
interface MenuItem {
  id: string
  label: string
  icon?: string
  shortcut?: string
  disabled?: boolean
  danger?: boolean
  children?: MenuItem[]
  action?: () => void
}
```

### 5.4 拖拽处理器接口

```typescript
// 拖拽处理器接口
interface IDragDropHandler {
  // 拖拽源
  canDrag(node: NavigatorNode): boolean
  getDragData(node: NavigatorNode): DragData
  getDragPreview(node: NavigatorNode): VNode

  // 拖放目标
  canDrop(target: NavigatorNode, data: DragData): boolean
  getDropEffect(target: NavigatorNode, data: DragData): 'move' | 'copy' | 'link'
  handleDrop(target: NavigatorNode, data: DragData, event: DragEvent): void

  // 拖拽反馈
  onDragEnter?(target: NavigatorNode, data: DragData): void
  onDragLeave?(target: NavigatorNode, data: DragData): void
  onDragOver?(target: NavigatorNode, data: DragData): void
}

// 拖拽数据
interface DragData {
  type: 'table' | 'column' | 'view' | 'custom'
  sourceId: string
  sourceView: string
  items: DragItem[]
  sqlSnippet?: string
}

interface DragItem {
  id: string
  type: string
  name: string
  path: string
  metadata?: Record<string, unknown>
}
```

## 6. 错误处理接口

### 6.1 错误类型定义

```typescript
// 导航栏错误类型
enum NavigatorErrorCode {
  // 连接错误
  CONNECTION_FAILED = 'CONN_001',
  CONNECTION_LOST = 'CONN_002',
  CONNECTION_TIMEOUT = 'CONN_003',

  // 数据错误
  DATA_LOAD_FAILED = 'DATA_001',
  DATA_PARSE_ERROR = 'DATA_002',
  DATA_VALIDATION_ERROR = 'DATA_003',
  DATA_SYNC_ERROR = 'DATA_004',

  // 视图错误
  VIEW_NOT_FOUND = 'VIEW_001',
  VIEW_UPDATE_ERROR = 'VIEW_002',
  VIEW_TRANSFORM_ERROR = 'VIEW_003',

  // 缓存错误
  CACHE_READ_ERROR = 'CACHE_001',
  CACHE_WRITE_ERROR = 'CACHE_002',
  CACHE_CORRUPTED = 'CACHE_003',

  // 扩展错误
  EXTENSION_LOAD_ERROR = 'EXT_001',
  EXTENSION_ACTIVATE_ERROR = 'EXT_002',
  EXTENSION_INCOMPATIBLE = 'EXT_003',
}

// 错误接口
interface INavigatorError extends Error {
  code: NavigatorErrorCode
  context?: Record<string, unknown>
  recoverable: boolean
  retry?: () => Promise<void>
}

// 错误处理器接口
interface IErrorHandler {
  // 错误处理
  handle(error: INavigatorError): ErrorAction

  // 错误恢复
  recover(error: INavigatorError): Promise<boolean>

  // 错误报告
  report(error: INavigatorError): void
}

type ErrorAction =
  | { type: 'retry'; delay?: number }
  | { type: 'fallback'; fallback: () => void }
  | { type: 'ignore' }
  | { type: 'notify'; message: string }
  | { type: 'throw' }
```

## 7. 工具类型

### 7.1 通用类型定义

```typescript
// 标识符类型
type NodeId = string
type ViewName = string
type ConnectionId = string
type Timestamp = number

// 集合类型
type NodeMap = Map<NodeId, NavigatorNode>
type NodeSet = Set<NodeId>

// 事件类型
interface Event<T> {
  type: string
  payload: T
  timestamp: Timestamp
  source: string
}

type EventHandler<T> = (event: Event<T>) => void
type Unsubscribe = () => void

// 异步类型
type AsyncResult<T> = Promise<Result<T, Error>>

interface Result<T, E> {
  success: boolean
  data?: T
  error?: E
}

// 选项类型
type Optional<T> = T | undefined
type Nullable<T> = T | null
```

### 7.2 类型守卫

```typescript
// 类型守卫函数
function isNavigatorNode(value: unknown): value is NavigatorNode {
  return (
    typeof value === 'object' &&
    value !== null &&
    'id' in value &&
    'type' in value &&
    'name' in value
  )
}

function isDelta<T>(value: unknown): value is Delta<T> {
  return (
    typeof value === 'object' &&
    value !== null &&
    'type' in value &&
    ['ADD', 'REMOVE', 'UPDATE', 'MOVE', 'REORDER'].includes((value as any).type)
  )
}

function isContainerNode(node: NavigatorNode): boolean {
  return !node.isLeaf && (node.childrenCount ?? 0) > 0
}
```
