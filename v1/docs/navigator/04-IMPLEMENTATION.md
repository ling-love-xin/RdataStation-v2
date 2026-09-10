# IVM 导航栏实施步骤

> 版本：v1.0
> 最后更新：2026-05-03
> 状态：✅ 持续更新

---

## 1. 项目准备

### 1.1 目录结构创建

```
src/
├── extensions/builtin/navigator/
│   ├── core/                    # 核心引擎
│   │   ├── engine/
│   │   │   ├── ViewEngine.ts
│   │   │   ├── DeltaProcessor.ts
│   │   │   └── ChangePropagator.ts
│   │   ├── cache/
│   │   │   ├── L1Cache.ts
│   │   │   ├── L2Cache.ts
│   │   │   └── L3Cache.ts
│   │   ├── viewport/
│   │   │   └── VirtualViewport.ts
│   │   └── index.ts
│   ├── services/                # 业务服务
│   │   ├── NavigatorService.ts
│   │   ├── SearchService.ts
│   │   └── SyncService.ts
│   ├── composables/             # Vue Composables
│   │   ├── useNavigator.ts
│   │   ├── useVirtualScroll.ts
│   │   ├── useSearch.ts
│   │   └── useCache.ts
│   ├── ui/                      # UI 组件
│   │   ├── components/
│   │   │   ├── NavigatorTree.vue
│   │   │   ├── NavigatorNode.vue
│   │   │   ├── ConnectionCard.vue
│   │   │   ├── SearchPanel.vue
│   │   │   └── VirtualList.vue
│   │   └── views/
│   │       └── NavigatorPanel.vue
│   ├── types/                   # 类型定义
│   │   ├── navigator.ts
│   │   ├── delta.ts
│   │   └── view.ts
│   └── index.ts
```

### 1.2 依赖安装

```bash
# 核心依赖
pnpm add rxjs zod

# 虚拟滚动
pnpm add @tanstack/vue-virtual

# 索引数据库
pnpm add idb

# 工具库
pnpm add fast-diff lodash-es

# 开发依赖
pnpm add -D @types/lodash-es
```

## 2. 第一阶段：核心引擎 (Week 1-2)

### 2.1 增量处理器实现

```typescript
// src/extensions/builtin/navigator/core/engine/DeltaProcessor.ts

import { diff } from 'fast-diff'

export class DeltaProcessor {
  /**
   * 计算两个快照之间的差异
   */
  computeDiff<T>(
    oldSnapshot: T[],
    newSnapshot: T[],
    keyExtractor: (item: T) => string
  ): Delta<T>[] {
    const oldMap = new Map(oldSnapshot.map(item => [keyExtractor(item), item]))
    const newMap = new Map(newSnapshot.map(item => [keyExtractor(item), item]))

    const deltas: Delta<T>[] = []
    const processed = new Set<string>()

    // 检测新增和更新
    for (const [key, newItem] of newMap) {
      const oldItem = oldMap.get(key)

      if (!oldItem) {
        // 新增
        deltas.push({
          type: 'ADD',
          item: newItem,
          position: newSnapshot.indexOf(newItem),
          parentId: this.extractParentId(newItem),
        })
      } else if (!this.isEqual(oldItem, newItem)) {
        // 更新
        deltas.push({
          type: 'UPDATE',
          id: key,
          changes: this.extractChanges(oldItem, newItem),
          oldValues: this.extractChanges(newItem, oldItem),
        })
      }

      processed.add(key)
    }

    // 检测删除
    for (const [key, oldItem] of oldMap) {
      if (!processed.has(key)) {
        deltas.push({
          type: 'REMOVE',
          id: key,
          position: oldSnapshot.indexOf(oldItem),
          parentId: this.extractParentId(oldItem),
        })
      }
    }

    // 检测移动
    const moveDeltas = this.detectMoves(oldSnapshot, newSnapshot, keyExtractor)
    deltas.push(...moveDeltas)

    return this.sortDeltas(deltas)
  }

  /**
   * 合并多个增量
   */
  mergeDeltas<T>(deltas: Delta<T>[]): Delta<T>[] {
    const merged = new Map<string, Delta<T>>()

    for (const delta of deltas) {
      const key = this.getDeltaKey(delta)
      const existing = merged.get(key)

      if (existing) {
        merged.set(key, this.mergeTwoDeltas(existing, delta))
      } else {
        merged.set(key, delta)
      }
    }

    return Array.from(merged.values())
  }

  /**
   * 压缩增量
   */
  compressDeltas<T>(deltas: Delta<T>[]): Delta<T>[] {
    // 移除冗余的更新
    const compressed: Delta<T>[] = []
    const updates = new Map<string, UpdateDelta<T>>()

    for (const delta of deltas) {
      if (delta.type === 'UPDATE') {
        const existing = updates.get(delta.id)
        if (existing) {
          // 合并变更
          existing.changes = { ...existing.changes, ...delta.changes }
        } else {
          updates.set(delta.id, delta)
          compressed.push(delta)
        }
      } else {
        compressed.push(delta)
      }
    }

    return compressed
  }

  /**
   * 应用增量到快照
   */
  applyToSnapshot<T>(snapshot: T[], deltas: Delta<T>[]): T[] {
    const result = [...snapshot]
    const indexMap = new Map(snapshot.map((item, idx) => [this.extractId(item), idx]))

    // 按类型分组处理
    const removes = deltas.filter(d => d.type === 'REMOVE') as RemoveDelta[]
    const adds = deltas.filter(d => d.type === 'ADD') as AddDelta<T>[]
    const updates = deltas.filter(d => d.type === 'UPDATE') as UpdateDelta<T>[]
    const moves = deltas.filter(d => d.type === 'MOVE') as MoveDelta[]

    // 1. 先处理删除（从后向前）
    removes
      .sort((a, b) => b.position - a.position)
      .forEach(delta => {
        result.splice(delta.position, 1)
      })

    // 2. 处理更新
    updates.forEach(delta => {
      const idx = indexMap.get(delta.id)
      if (idx !== undefined) {
        result[idx] = { ...result[idx], ...delta.changes }
      }
    })

    // 3. 处理新增
    adds.forEach(delta => {
      result.splice(delta.position, 0, delta.item)
    })

    // 4. 处理移动
    moves.forEach(delta => {
      const [item] = result.splice(delta.from, 1)
      result.splice(delta.to, 0, item)
    })

    return result
  }

  // 辅助方法
  private isEqual<T>(a: T, b: T): boolean {
    return JSON.stringify(a) === JSON.stringify(b)
  }

  private extractChanges<T>(oldItem: T, newItem: T): Partial<T> {
    const changes: Partial<T> = {}
    for (const key in newItem) {
      if (oldItem[key] !== newItem[key]) {
        changes[key] = newItem[key]
      }
    }
    return changes
  }

  private sortDeltas<T>(deltas: Delta<T>[]): Delta<T>[] {
    const order = { REMOVE: 0, MOVE: 1, UPDATE: 2, ADD: 3 }
    return deltas.sort((a, b) => order[a.type] - order[b.type])
  }
}
```

### 2.2 视图引擎实现

```typescript
// src/extensions/builtin/navigator/core/engine/ViewEngine.ts

import { reactive, computed, ref, type Ref } from 'vue'
import type { Delta, MaterializedView, ViewConfig } from '../../types'
import { DeltaProcessor } from './DeltaProcessor'

export class ViewEngine {
  private views = new Map<string, MaterializedView<unknown>>()
  private dependencies = new Map<string, Set<string>>()
  private dependents = new Map<string, Set<string>>()
  private subscribers = new Map<string, Set<(delta: Delta<unknown>) => void>>()
  private deltaProcessor = new DeltaProcessor()

  /**
   * 创建物化视图
   */
  createView<T>(config: ViewConfig<T>): MaterializedView<T> {
    const view: MaterializedView<T> = {
      name: config.name,
      version: 0,
      snapshot: reactive(config.initialData || []) as T[],

      // 查询方法
      getById: (id: string) => {
        return view.snapshot.find(item => this.extractId(item) === id)
      },

      getByIndex: (index: number) => {
        return view.snapshot[index]
      },

      find: (predicate: (item: T) => boolean) => {
        return view.snapshot.find(predicate)
      },

      filter: (predicate: (item: T) => boolean) => {
        return view.snapshot.filter(predicate)
      },

      // 订阅
      onChange: (callback: (delta: Delta<T>) => void) => {
        return this.subscribe(config.name, callback as (delta: Delta<unknown>) => void)
      },
    }

    this.views.set(config.name, view as MaterializedView<unknown>)

    // 如果有数据源，开始监听
    if (config.source) {
      config.source.subscribe(data => {
        const deltas = this.deltaProcessor.computeDiff(view.snapshot, data, item =>
          this.extractId(item)
        )
        this.applyBatch(config.name, deltas)
      })
    }

    return view
  }

  /**
   * 应用单个增量
   */
  applyDelta<T>(viewName: string, delta: Delta<T>): void {
    const view = this.views.get(viewName) as MaterializedView<T> | undefined
    if (!view) {
      throw new Error(`View not found: ${viewName}`)
    }

    // 应用到快照
    const newSnapshot = this.deltaProcessor.applyToSnapshot(view.snapshot, [delta])
    view.snapshot.length = 0
    view.snapshot.push(...newSnapshot)
    view.version++

    // 通知订阅者
    this.notifySubscribers(viewName, delta as Delta<unknown>)

    // 传播到依赖视图
    this.propagateChange(viewName, delta as Delta<unknown>)
  }

  /**
   * 批量应用增量
   */
  applyBatch<T>(viewName: string, deltas: Delta<T>[]): void {
    if (deltas.length === 0) return

    const view = this.views.get(viewName) as MaterializedView<T> | undefined
    if (!view) {
      throw new Error(`View not found: ${viewName}`)
    }

    // 合并和压缩增量
    const merged = this.deltaProcessor.mergeDeltas(deltas)
    const compressed = this.deltaProcessor.compressDeltas(merged)

    // 批量应用
    const newSnapshot = this.deltaProcessor.applyToSnapshot(view.snapshot, compressed)
    view.snapshot.length = 0
    view.snapshot.push(...newSnapshot)
    view.version += compressed.length

    // 通知订阅者
    compressed.forEach(delta => {
      this.notifySubscribers(viewName, delta as Delta<unknown>)
    })

    // 传播到依赖视图
    compressed.forEach(delta => {
      this.propagateChange(viewName, delta as Delta<unknown>)
    })
  }

  /**
   * 添加视图依赖
   */
  addDependency(view: string, dependsOn: string): void {
    if (!this.dependencies.has(view)) {
      this.dependencies.set(view, new Set())
    }
    this.dependencies.get(view)!.add(dependsOn)

    if (!this.dependents.has(dependsOn)) {
      this.dependents.set(dependsOn, new Set())
    }
    this.dependents.get(dependsOn)!.add(view)
  }

  /**
   * 订阅视图变更
   */
  subscribe(viewName: string, callback: (delta: Delta<unknown>) => void): () => void {
    if (!this.subscribers.has(viewName)) {
      this.subscribers.set(viewName, new Set())
    }

    this.subscribers.get(viewName)!.add(callback)

    return () => {
      this.subscribers.get(viewName)?.delete(callback)
    }
  }

  /**
   * 获取视图
   */
  getView<T>(name: string): MaterializedView<T> | undefined {
    return this.views.get(name) as MaterializedView<T> | undefined
  }

  // 私有方法
  private notifySubscribers(viewName: string, delta: Delta<unknown>): void {
    const callbacks = this.subscribers.get(viewName)
    if (callbacks) {
      callbacks.forEach(callback => {
        try {
          callback(delta)
        } catch (error) {
          console.error('Error in view subscriber:', error)
        }
      })
    }
  }

  private propagateChange(sourceView: string, delta: Delta<unknown>): void {
    const dependentViews = this.dependents.get(sourceView)
    if (!dependentViews) return

    dependentViews.forEach(dependentView => {
      // 转换增量并应用
      const transformedDelta = this.transformDelta(delta, sourceView, dependentView)
      this.applyDelta(dependentView, transformedDelta)
    })
  }

  private transformDelta(
    delta: Delta<unknown>,
    sourceView: string,
    targetView: string
  ): Delta<unknown> {
    // 根据视图转换逻辑转换增量
    // 这里简化处理，实际应根据 transform 配置转换
    return delta
  }

  private extractId<T>(item: T): string {
    return (item as any).id || String(item)
  }
}

// 创建单例
export const viewEngine = new ViewEngine()
```

### 2.3 虚拟视口实现

```typescript
// src/extensions/builtin/navigator/core/viewport/VirtualViewport.ts

import { ref, computed, type Ref, type ComputedRef } from 'vue'

export interface VirtualViewportConfig {
  itemHeight: number
  overscan?: number
  estimateItemHeight?: (item: unknown, index: number) => number
}

export interface VirtualViewportState {
  scrollTop: Ref<number>
  containerHeight: Ref<number>
  visibleRange: ComputedRef<{ start: number; end: number }>
  totalHeight: ComputedRef<number>
}

export class VirtualViewport {
  private config: Required<VirtualViewportConfig>
  private scrollTop = ref(0)
  private containerHeight = ref(0)
  private items: Ref<unknown[]> = ref([])

  constructor(config: VirtualViewportConfig) {
    this.config = {
      overscan: 5,
      ...config,
    }
  }

  /**
   * 设置数据项
   */
  setItems(items: unknown[]): void {
    this.items.value = items
  }

  /**
   * 更新滚动位置
   */
  onScroll(scrollTop: number): void {
    this.scrollTop.value = scrollTop
  }

  /**
   * 更新容器高度
   */
  onResize(height: number): void {
    this.containerHeight.value = height
  }

  /**
   * 计算可见范围
   */
  visibleRange = computed(() => {
    const itemHeight = this.config.itemHeight
    const overscan = this.config.overscan

    const start = Math.floor(this.scrollTop.value / itemHeight)
    const visibleCount = Math.ceil(this.containerHeight.value / itemHeight)

    return {
      start: Math.max(0, start - overscan),
      end: Math.min(this.items.value.length, start + visibleCount + overscan),
    }
  })

  /**
   * 获取可见项
   */
  visibleItems = computed(() => {
    const { start, end } = this.visibleRange.value
    return this.items.value.slice(start, end).map((item, index) => ({
      item,
      index: start + index,
      style: {
        position: 'absolute',
        top: `${(start + index) * this.config.itemHeight}px`,
        height: `${this.config.itemHeight}px`,
        left: 0,
        right: 0,
      },
    }))
  })

  /**
   * 计算总高度
   */
  totalHeight = computed(() => {
    return this.items.value.length * this.config.itemHeight
  })

  /**
   * 滚动到指定索引
   */
  scrollToIndex(index: number, behavior: ScrollBehavior = 'smooth'): void {
    const top = index * this.config.itemHeight
    // 触发滚动事件
    this.onScroll(top)
  }

  /**
   * 获取项的偏移位置
   */
  getItemOffset(index: number): number {
    return index * this.config.itemHeight
  }
}
```

## 3. 第二阶段：缓存层 (Week 2-3)

### 3.1 L1 内存缓存

```typescript
// src/extensions/builtin/navigator/core/cache/L1Cache.ts

export class L1Cache<T> {
  private cache = new Map<string, CacheEntry<T>>()
  private maxSize: number
  private accessOrder: string[] = []

  constructor(maxSize: number = 1000) {
    this.maxSize = maxSize
  }

  /**
   * 获取缓存项
   */
  get(key: string): T | undefined {
    const entry = this.cache.get(key)
    if (!entry) return undefined

    // 更新访问顺序
    this.updateAccessOrder(key)

    return entry.value
  }

  /**
   * 设置缓存项
   */
  set(key: string, value: T, options?: { ttl?: number }): void {
    // 检查容量
    if (this.cache.size >= this.maxSize && !this.cache.has(key)) {
      this.evictLRU()
    }

    const entry: CacheEntry<T> = {
      value,
      timestamp: Date.now(),
      ttl: options?.ttl,
    }

    this.cache.set(key, entry)
    this.updateAccessOrder(key)
  }

  /**
   * 删除缓存项
   */
  delete(key: string): boolean {
    this.removeFromAccessOrder(key)
    return this.cache.delete(key)
  }

  /**
   * 清空缓存
   */
  clear(): void {
    this.cache.clear()
    this.accessOrder = []
  }

  /**
   * 批量获取
   */
  batchGet(keys: string[]): Map<string, T> {
    const result = new Map<string, T>()

    for (const key of keys) {
      const value = this.get(key)
      if (value !== undefined) {
        result.set(key, value)
      }
    }

    return result
  }

  /**
   * 批量设置
   */
  batchSet(entries: Array<[string, T]>, options?: { ttl?: number }): void {
    for (const [key, value] of entries) {
      this.set(key, value, options)
    }
  }

  // 私有方法
  private updateAccessOrder(key: string): void {
    const index = this.accessOrder.indexOf(key)
    if (index > -1) {
      this.accessOrder.splice(index, 1)
    }
    this.accessOrder.push(key)
  }

  private removeFromAccessOrder(key: string): void {
    const index = this.accessOrder.indexOf(key)
    if (index > -1) {
      this.accessOrder.splice(index, 1)
    }
  }

  private evictLRU(): void {
    if (this.accessOrder.length === 0) return

    const lruKey = this.accessOrder[0]
    this.delete(lruKey)
  }
}

interface CacheEntry<T> {
  value: T
  timestamp: number
  ttl?: number
}
```

### 3.2 L2 IndexedDB 缓存

```typescript
// src/extensions/builtin/navigator/core/cache/L2Cache.ts

import { openDB, type DBSchema, type IDBPDatabase } from 'idb'

interface NavigatorCacheSchema extends DBSchema {
  nodes: {
    key: string
    value: {
      id: string
      data: unknown
      version: number
      timestamp: number
    }
    indexes: {
      'by-parent': string
      'by-timestamp': number
    }
  }
}

export class L2Cache {
  private db: IDBPDatabase<NavigatorCacheSchema> | null = null
  private dbName = 'navigator-cache'
  private version = 1

  /**
   * 初始化数据库
   */
  async initialize(): Promise<void> {
    this.db = await openDB<NavigatorCacheSchema>(this.dbName, this.version, {
      upgrade(db) {
        // 创建对象存储
        const store = db.createObjectStore('nodes', { keyPath: 'id' })

        // 创建索引
        store.createIndex('by-parent', 'parentId')
        store.createIndex('by-timestamp', 'timestamp')
      },
    })
  }

  /**
   * 获取缓存项
   */
  async get<T>(key: string): Promise<T | undefined> {
    if (!this.db) await this.initialize()

    const entry = await this.db!.get('nodes', key)
    if (!entry) return undefined

    return entry.data as T
  }

  /**
   * 设置缓存项
   */
  async set<T>(key: string, value: T, options?: { version?: number }): Promise<void> {
    if (!this.db) await this.initialize()

    await this.db!.put('nodes', {
      id: key,
      data: value,
      version: options?.version || 1,
      timestamp: Date.now(),
    })
  }

  /**
   * 批量获取
   */
  async batchGet<T>(keys: string[]): Promise<Map<string, T>> {
    if (!this.db) await this.initialize()

    const result = new Map<string, T>()

    const tx = this.db!.transaction('nodes', 'readonly')
    const store = tx.objectStore('nodes')

    await Promise.all(
      keys.map(async key => {
        const entry = await store.get(key)
        if (entry) {
          result.set(key, entry.data as T)
        }
      })
    )

    await tx.done

    return result
  }

  /**
   * 批量设置
   */
  async batchSet<T>(entries: Array<[string, T]>): Promise<void> {
    if (!this.db) await this.initialize()

    const tx = this.db!.transaction('nodes', 'readwrite')
    const store = tx.objectStore('nodes')

    for (const [key, value] of entries) {
      await store.put({
        id: key,
        data: value,
        version: 1,
        timestamp: Date.now(),
      })
    }

    await tx.done
  }

  /**
   * 查询子节点
   */
  async getChildren<T>(parentId: string): Promise<T[]> {
    if (!this.db) await this.initialize()

    const entries = await this.db!.getAllFromIndex('nodes', 'by-parent', parentId)

    return entries.map(entry => entry.data as T)
  }

  /**
   * 清理过期缓存
   */
  async cleanup(maxAge: number): Promise<number> {
    if (!this.db) await this.initialize()

    const cutoff = Date.now() - maxAge
    const tx = this.db!.transaction('nodes', 'readwrite')
    const store = tx.objectStore('nodes')
    const index = store.index('by-timestamp')

    let deleted = 0
    const range = IDBKeyRange.upperBound(cutoff)

    let cursor = await index.openCursor(range)
    while (cursor) {
      await cursor.delete()
      deleted++
      cursor = await cursor.continue()
    }

    await tx.done

    return deleted
  }
}
```

## 4. 第三阶段：UI 组件 (Week 3-4)

### 4.1 导航树组件

```vue
<!-- src/extensions/builtin/navigator/ui/components/NavigatorTree.vue -->

<template>
  <div class="navigator-tree" ref="containerRef">
    <!-- 工具栏 -->
    <div class="tree-toolbar">
      <slot name="toolbar" />
    </div>

    <!-- 虚拟列表 -->
    <VirtualList :items="visibleNodes" :item-height="28" :overscan="5" @scroll="handleScroll">
      <template #item="{ item, index, style }">
        <NavigatorNode
          :node="item"
          :level="item.level"
          :style="style"
          :expanded="isExpanded(item.id)"
          :selected="isSelected(item.id)"
          @expand="handleExpand(item)"
          @collapse="handleCollapse(item)"
          @select="handleSelect(item)"
          @click="handleClick(item)"
          @dblclick="handleDoubleClick(item)"
        />
      </template>
    </VirtualList>

    <!-- 空状态 -->
    <div v-if="nodes.length === 0" class="empty-state">
      <slot name="empty">
        <span>暂无数据</span>
      </slot>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { useNavigator } from '../../composables/useNavigator'
import VirtualList from './VirtualList.vue'
import NavigatorNode from './NavigatorNode.vue'

interface Props {
  viewName: string
  connectionId?: string
}

const props = defineProps<Props>()

const emit = defineEmits<{
  'node-click': [node: NavigatorNode]
  'node-expand': [node: NavigatorNode]
  'node-collapse': [node: NavigatorNode]
  'node-select': [node: NavigatorNode]
}>()

// 使用导航器
const { nodes, expandedKeys, selectedKeys, expand, collapse, select, loading } = useNavigator({
  viewName: props.viewName,
  connectionId: props.connectionId,
})

// 可见节点（扁平化）
const visibleNodes = computed(() => {
  const result: Array<NavigatorNode & { level: number }> = []

  const traverse = (nodeList: NavigatorNode[], level: number) => {
    for (const node of nodeList) {
      result.push({ ...node, level })

      if (isExpanded(node.id) && node.children) {
        traverse(node.children, level + 1)
      }
    }
  }

  traverse(nodes.value, 0)
  return result
})

// 方法
const isExpanded = (id: string) => expandedKeys.value.has(id)
const isSelected = (id: string) => selectedKeys.value.has(id)

const handleExpand = (node: NavigatorNode) => {
  expand(node.id)
  emit('node-expand', node)
}

const handleCollapse = (node: NavigatorNode) => {
  collapse(node.id)
  emit('node-collapse', node)
}

const handleSelect = (node: NavigatorNode) => {
  select(node.id)
  emit('node-select', node)
}

const handleClick = (node: NavigatorNode) => {
  emit('node-click', node)
}

const handleDoubleClick = (node: NavigatorNode) => {
  // 双击展开/折叠
  if (isExpanded(node.id)) {
    handleCollapse(node)
  } else {
    handleExpand(node)
  }
}

const handleScroll = (scrollTop: number) => {
  // 处理滚动
}
</script>

<style scoped>
.navigator-tree {
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
}

.tree-toolbar {
  flex-shrink: 0;
  padding: 8px;
  border-bottom: 1px solid var(--border-color);
}

.empty-state {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 200px;
  color: var(--text-secondary);
}
</style>
```

### 4.2 useNavigator Composable

```typescript
// src/extensions/builtin/navigator/composables/useNavigator.ts

import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { viewEngine } from '../core/engine/ViewEngine'
import type { NavigatorNode, Delta } from '../types'

export interface UseNavigatorOptions {
  viewName: string
  connectionId?: string
  lazyLoad?: boolean
}

export function useNavigator(options: UseNavigatorOptions) {
  // 状态
  const nodes = ref<NavigatorNode[]>([])
  const expandedKeys = ref<Set<string>>(new Set())
  const selectedKeys = ref<Set<string>>(new Set())
  const loading = ref(false)
  const error = ref<Error | null>(null)

  // 获取视图
  const view = computed(() => viewEngine.getView<NavigatorNode>(options.viewName))

  // 订阅视图变更
  let unsubscribe: (() => void) | null = null

  onMounted(() => {
    if (view.value) {
      // 初始化数据
      nodes.value = [...view.value.snapshot]

      // 订阅增量更新
      unsubscribe = view.value.onChange(delta => {
        applyDeltaToNodes(delta)
      })
    }
  })

  onUnmounted(() => {
    unsubscribe?.()
  })

  // 应用增量到节点列表
  const applyDeltaToNodes = (delta: Delta<NavigatorNode>) => {
    switch (delta.type) {
      case 'ADD':
        // 找到父节点并添加子节点
        addNode(delta.item, delta.parentId)
        break
      case 'REMOVE':
        removeNode(delta.id)
        break
      case 'UPDATE':
        updateNode(delta.id, delta.changes)
        break
    }
  }

  // 展开节点
  const expand = async (nodeId: string) => {
    expandedKeys.value.add(nodeId)

    // 懒加载子节点
    if (options.lazyLoad) {
      await loadChildren(nodeId)
    }
  }

  // 折叠节点
  const collapse = (nodeId: string) => {
    expandedKeys.value.delete(nodeId)
  }

  // 选择节点
  const select = (nodeId: string) => {
    selectedKeys.value.clear()
    selectedKeys.value.add(nodeId)
  }

  // 加载子节点
  const loadChildren = async (nodeId: string) => {
    loading.value = true
    error.value = null

    try {
      // 调用 API 加载子节点
      const children = await fetchChildren(nodeId)

      // 添加到视图
      children.forEach(child => {
        viewEngine.applyDelta(options.viewName, {
          type: 'ADD',
          item: child,
          position: -1,
          parentId: nodeId,
        })
      })
    } catch (err) {
      error.value = err as Error
    } finally {
      loading.value = false
    }
  }

  // 辅助方法
  const addNode = (node: NavigatorNode, parentId: string | null) => {
    if (!parentId) {
      nodes.value.push(node)
      return
    }

    const parent = findNode(nodes.value, parentId)
    if (parent) {
      if (!parent.children) {
        parent.children = []
      }
      parent.children.push(node)
    }
  }

  const removeNode = (nodeId: string) => {
    const removeFromList = (list: NavigatorNode[]): boolean => {
      const index = list.findIndex(n => n.id === nodeId)
      if (index > -1) {
        list.splice(index, 1)
        return true
      }

      for (const node of list) {
        if (node.children && removeFromList(node.children)) {
          return true
        }
      }

      return false
    }

    removeFromList(nodes.value)
  }

  const updateNode = (nodeId: string, changes: Partial<NavigatorNode>) => {
    const node = findNode(nodes.value, nodeId)
    if (node) {
      Object.assign(node, changes)
    }
  }

  const findNode = (list: NavigatorNode[], id: string): NavigatorNode | null => {
    for (const node of list) {
      if (node.id === id) return node
      if (node.children) {
        const found = findNode(node.children, id)
        if (found) return found
      }
    }
    return null
  }

  return {
    nodes,
    expandedKeys,
    selectedKeys,
    loading,
    error,
    expand,
    collapse,
    select,
  }
}

// 模拟 API 调用
async function fetchChildren(nodeId: string): Promise<NavigatorNode[]> {
  // 实际实现中调用后端 API
  return []
}
```

## 5. 第四阶段：集成与优化 (Week 4-5)

### 5.1 与现有系统集成

```typescript
// src/extensions/builtin/navigator/index.ts

import { defineExtension } from '@/core/extension/defineExtension'
import { viewEngine } from './core/engine/ViewEngine'
import { NavigatorPanel } from './ui/views/NavigatorPanel.vue'

export default defineExtension({
  id: 'builtin.navigator',
  name: 'Database Navigator',
  version: '1.0.0',

  activate(context) {
    // 注册视图
    context.viewRegistry.register({
      id: 'navigator',
      name: 'Database Navigator',
      component: NavigatorPanel,
      location: 'left',
    })

    // 创建默认视图
    viewEngine.createView({
      name: 'navigator:main',
      source: context.dataSource.create('metadata'),
      initialData: [],
    })

    // 注册命令
    context.commands.register({
      id: 'navigator.refresh',
      name: 'Refresh Navigator',
      handler: () => {
        viewEngine.refreshView('navigator:main')
      },
    })

    // 注册快捷键
    context.keybindings.register({
      command: 'navigator.refresh',
      keybinding: 'Ctrl+Shift+R',
    })
  },

  deactivate() {
    // 清理资源
  },
})
```

### 5.2 性能优化清单

```typescript
// 性能优化配置
export const performanceConfig = {
  // 虚拟滚动
  virtualScroll: {
    enabled: true,
    itemHeight: 28,
    overscan: 5,
  },

  // 增量更新
  incrementalUpdate: {
    enabled: true,
    batchSize: 100,
    debounceMs: 16, // 一帧
  },

  // 缓存
  cache: {
    l1Size: 1000,
    l2Enabled: true,
    l3Enabled: true,
    ttl: 24 * 60 * 60 * 1000, // 24小时
  },

  // 懒加载
  lazyLoad: {
    enabled: true,
    preloadDepth: 1,
    batchSize: 50,
  },

  // 搜索
  search: {
    debounceMs: 150,
    minLength: 2,
    maxResults: 100,
  },
}
```

## 6. 测试策略

### 6.1 单元测试

```typescript
// __tests__/DeltaProcessor.test.ts

import { describe, it, expect } from 'vitest'
import { DeltaProcessor } from '../core/engine/DeltaProcessor'

describe('DeltaProcessor', () => {
  const processor = new DeltaProcessor()

  it('should compute ADD deltas', () => {
    const oldSnapshot = [{ id: '1', name: 'A' }]
    const newSnapshot = [
      { id: '1', name: 'A' },
      { id: '2', name: 'B' },
    ]

    const deltas = processor.computeDiff(oldSnapshot, newSnapshot, item => item.id)

    expect(deltas).toHaveLength(1)
    expect(deltas[0].type).toBe('ADD')
    expect(deltas[0].item).toEqual({ id: '2', name: 'B' })
  })

  it('should compute REMOVE deltas', () => {
    const oldSnapshot = [
      { id: '1', name: 'A' },
      { id: '2', name: 'B' },
    ]
    const newSnapshot = [{ id: '1', name: 'A' }]

    const deltas = processor.computeDiff(oldSnapshot, newSnapshot, item => item.id)

    expect(deltas).toHaveLength(1)
    expect(deltas[0].type).toBe('REMOVE')
    expect(deltas[0].id).toBe('2')
  })

  it('should compute UPDATE deltas', () => {
    const oldSnapshot = [{ id: '1', name: 'A' }]
    const newSnapshot = [{ id: '1', name: 'B' }]

    const deltas = processor.computeDiff(oldSnapshot, newSnapshot, item => item.id)

    expect(deltas).toHaveLength(1)
    expect(deltas[0].type).toBe('UPDATE')
    expect(deltas[0].changes).toEqual({ name: 'B' })
  })
})
```

### 6.2 性能测试

```typescript
// __tests__/performance/ViewEngine.perf.ts

import { describe, it, expect } from 'vitest'
import { ViewEngine } from '../core/engine/ViewEngine'

describe('ViewEngine Performance', () => {
  it('should handle 10k items update within 16ms', () => {
    const engine = new ViewEngine()
    const items = Array.from({ length: 10000 }, (_, i) => ({
      id: String(i),
      name: `Item ${i}`,
    }))

    engine.createView({
      name: 'test',
      initialData: items,
    })

    const newItems = items.map(item => ({
      ...item,
      name: `${item.name} updated`,
    }))

    const start = performance.now()
    engine.applyBatch('test', [
      {
        type: 'UPDATE',
        id: '5000',
        changes: { name: 'Updated' },
      },
    ])
    const end = performance.now()

    expect(end - start).toBeLessThan(16)
  })
})
```

## 7. 部署检查清单

- [ ] 核心引擎实现完成
- [ ] 缓存层实现完成
- [ ] UI 组件实现完成
- [ ] 单元测试覆盖率 > 80%
- [ ] 性能测试通过
- [ ] 内存泄漏检查通过
- [ ] TypeScript 类型检查通过
- [ ] ESLint 检查通过
- [ ] 文档编写完成
- [ ] 示例代码运行正常
