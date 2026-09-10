# IVM 导航栏优化策略

> 版本：v1.0
> 最后更新：2026-05-03
> 状态：✅ 持续更新

---

## 1. 性能优化

### 1.1 渲染优化

#### A. 虚拟滚动优化

```typescript
// 动态高度虚拟滚动
class DynamicVirtualViewport {
  private heightCache = new Map<string, number>()
  private totalHeight = 0

  // 预估高度 + 实际高度缓存
  getItemHeight(item: unknown, index: number): number {
    const key = this.getItemKey(item, index)

    // 优先使用缓存高度
    if (this.heightCache.has(key)) {
      return this.heightCache.get(key)!
    }

    // 使用预估高度
    return this.estimateHeight(item)
  }

  // 测量实际高度并缓存
  measureItem(element: HTMLElement, item: unknown, index: number): void {
    const key = this.getItemKey(item, index)
    const height = element.getBoundingClientRect().height

    this.heightCache.set(key, height)
    this.updateTotalHeight()
  }

  // 二分查找定位
  findItemAtOffset(offset: number): number {
    let low = 0
    let high = this.items.length - 1

    while (low < high) {
      const mid = Math.floor((low + high) / 2)
      const midOffset = this.getItemOffset(mid)

      if (midOffset < offset) {
        low = mid + 1
      } else {
        high = mid
      }
    }

    return low
  }
}
```

#### B. 细粒度更新

```typescript
// 组件级细粒度更新
class GranularUpdate {
  // 使用 requestIdleCallback 批量更新
  private updateQueue: Map<string, UpdateTask> = new Map()
  private scheduled = false

  scheduleUpdate(nodeId: string, changes: Partial<NavigatorNode>): void {
    this.updateQueue.set(nodeId, { nodeId, changes, timestamp: Date.now() })

    if (!this.scheduled) {
      this.scheduled = true

      if ('requestIdleCallback' in window) {
        requestIdleCallback(() => this.flushUpdates(), { timeout: 100 })
      } else {
        setTimeout(() => this.flushUpdates(), 0)
      }
    }
  }

  private flushUpdates(): void {
    const updates = Array.from(this.updateQueue.values())
    this.updateQueue.clear()
    this.scheduled = false

    // 合并同一节点的更新
    const merged = this.mergeUpdates(updates)

    // 批量应用
    merged.forEach(update => {
      this.applyUpdate(update)
    })
  }

  private mergeUpdates(updates: UpdateTask[]): UpdateTask[] {
    const grouped = groupBy(updates, 'nodeId')

    return Object.values(grouped).map(group => ({
      nodeId: group[0].nodeId,
      changes: group.reduce((acc, update) => ({ ...acc, ...update.changes }), {}),
      timestamp: Math.max(...group.map(u => u.timestamp)),
    }))
  }
}
```

#### C. 渲染节流

```typescript
// 滚动节流
class ScrollThrottler {
  private ticking = false
  private lastScrollTop = 0
  private scrollVelocity = 0

  onScroll(scrollTop: number, callback: (range: Range) => void): void {
    // 计算滚动速度
    this.scrollVelocity = Math.abs(scrollTop - this.lastScrollTop)
    this.lastScrollTop = scrollTop

    if (!this.ticking) {
      requestAnimationFrame(() => {
        // 根据速度调整渲染策略
        if (this.scrollVelocity > 1000) {
          // 高速滚动：使用占位符
          callback(this.computePlaceholderRange(scrollTop))
        } else {
          // 正常滚动：完整渲染
          callback(this.computeVisibleRange(scrollTop))
        }

        this.ticking = false
      })

      this.ticking = true
    }
  }

  // 高速滚动时使用简化渲染
  private computePlaceholderRange(scrollTop: number): Range {
    const range = this.computeVisibleRange(scrollTop)

    // 扩大视口范围，但使用占位符
    return {
      start: Math.max(0, range.start - 10),
      end: range.end + 10,
      usePlaceholder: true,
    }
  }
}
```

### 1.2 内存优化

#### A. 对象池

```typescript
// 渲染节点对象池
class RenderNodePool {
  private pool: RenderNode[] = []
  private active = new Set<RenderNode>()
  private maxSize = 100

  acquire(): RenderNode {
    let node = this.pool.pop()

    if (!node) {
      node = this.createNode()
    }

    this.active.add(node)
    return node
  }

  release(node: RenderNode): void {
    if (!this.active.has(node)) return

    this.active.delete(node)

    // 重置状态
    node.reset()

    // 限制池大小
    if (this.pool.length < this.maxSize) {
      this.pool.push(node)
    }
  }

  releaseAll(): void {
    this.active.forEach(node => {
      node.reset()
      if (this.pool.length < this.maxSize) {
        this.pool.push(node)
      }
    })
    this.active.clear()
  }
}

// 增量对象池
class DeltaPool {
  private pool: Delta<unknown>[] = []

  acquireAdd<T>(item: T, position: number, parentId: string): Delta<T> {
    const delta = this.pool.pop() || {}
    return {
      ...delta,
      type: 'ADD',
      item,
      position,
      parentId,
    } as Delta<T>
  }

  release<T>(delta: Delta<T>): void {
    // 清理引用
    ;(delta as any).item = null(delta as any).changes = null
    this.pool.push(delta as Delta<unknown>)
  }
}
```

#### B. 弱引用缓存

```typescript
// 使用 WeakMap 和 WeakRef 避免内存泄漏
class WeakCache<K extends object, V> {
  private cache = new WeakMap<K, V>()
  private refCache = new Map<string, WeakRef<K>>()
  private finalizer = new FinalizationRegistry<string>(key => {
    this.refCache.delete(key)
  })

  set(key: K, value: V, keyString: string): void {
    this.cache.set(key, value)

    const ref = new WeakRef(key)
    this.refCache.set(keyString, ref)
    this.finalizer.register(key, keyString)
  }

  get(keyString: string): V | undefined {
    const ref = this.refCache.get(keyString)
    if (!ref) return undefined

    const key = ref.deref()
    if (!key) {
      this.refCache.delete(keyString)
      return undefined
    }

    return this.cache.get(key)
  }

  has(keyString: string): boolean {
    const ref = this.refCache.get(keyString)
    if (!ref) return false

    const key = ref.deref()
    if (!key) {
      this.refCache.delete(keyString)
      return false
    }

    return this.cache.has(key)
  }
}
```

#### C. 内存监控

```typescript
// 内存监控器
class MemoryMonitor {
  private threshold = 150 * 1024 * 1024 // 150MB
  private warningThreshold = 0.8
  private checkInterval = 5000

  start(): void {
    setInterval(() => this.checkMemory(), this.checkInterval)
  }

  private checkMemory(): void {
    if (!performance.memory) return

    const used = performance.memory.usedJSHeapSize
    const total = performance.memory.totalJSHeapSize
    const limit = performance.memory.jsHeapSizeLimit

    const usageRatio = used / this.threshold

    if (usageRatio > 1) {
      // 超过阈值，紧急清理
      this.emergencyCleanup()
      this.emit('memory:exceeded', { used, threshold: this.threshold })
    } else if (usageRatio > this.warningThreshold) {
      // 警告阈值，主动清理
      this.proactiveCleanup()
      this.emit('memory:warning', { used, threshold: this.threshold * this.warningThreshold })
    }

    // 记录内存使用趋势
    this.recordMemoryUsage(used)
  }

  private emergencyCleanup(): void {
    // 1. 清空 L1 缓存
    l1Cache.clear()

    // 2. 释放对象池
    renderNodePool.releaseAll()
    deltaPool.clear()

    // 3. 强制垃圾回收（如果可用）
    if (globalThis.gc) {
      globalThis.gc()
    }
  }

  private proactiveCleanup(): void {
    // 清理过期缓存
    l1Cache.evictExpired()

    // 压缩 L2 缓存
    l2Cache.compress()
  }
}
```

### 1.3 计算优化

#### A. 增量索引

```typescript
// 增量倒排索引
class IncrementalInvertedIndex {
  private index = new Map<string, Set<string>>() // term -> nodeIds
  private nodeTerms = new Map<string, Set<string>>() // nodeId -> terms
  private termFrequency = new Map<string, number>() // term -> frequency

  // 增量添加
  addDocument(nodeId: string, text: string): void {
    const terms = this.tokenize(text)
    const uniqueTerms = new Set(terms)

    // 更新正向索引
    this.nodeTerms.set(nodeId, uniqueTerms)

    // 更新倒排索引
    for (const term of uniqueTerms) {
      if (!this.index.has(term)) {
        this.index.set(term, new Set())
        this.termFrequency.set(term, 0)
      }
      this.index.get(term)!.add(nodeId)
      this.termFrequency.set(term, this.termFrequency.get(term)! + 1)
    }
  }

  // 增量删除
  removeDocument(nodeId: string): void {
    const terms = this.nodeTerms.get(nodeId)
    if (!terms) return

    for (const term of terms) {
      const nodeIds = this.index.get(term)
      if (nodeIds) {
        nodeIds.delete(nodeId)

        // 清理空索引
        if (nodeIds.size === 0) {
          this.index.delete(term)
          this.termFrequency.delete(term)
        } else {
          this.termFrequency.set(term, this.termFrequency.get(term)! - 1)
        }
      }
    }

    this.nodeTerms.delete(nodeId)
  }

  // 增量更新
  updateDocument(nodeId: string, newText: string): void {
    const oldTerms = this.nodeTerms.get(nodeId) || new Set()
    const newTerms = new Set(this.tokenize(newText))

    // 计算差异
    const added = difference(newTerms, oldTerms)
    const removed = difference(oldTerms, newTerms)

    // 应用增量
    for (const term of added) {
      if (!this.index.has(term)) {
        this.index.set(term, new Set())
        this.termFrequency.set(term, 0)
      }
      this.index.get(term)!.add(nodeId)
      this.termFrequency.set(term, this.termFrequency.get(term)! + 1)
    }

    for (const term of removed) {
      const nodeIds = this.index.get(term)
      if (nodeIds) {
        nodeIds.delete(nodeId)
        if (nodeIds.size === 0) {
          this.index.delete(term)
          this.termFrequency.delete(term)
        } else {
          this.termFrequency.set(term, this.termFrequency.get(term)! - 1)
        }
      }
    }

    this.nodeTerms.set(nodeId, newTerms)
  }

  // 前缀搜索（自动完成）
  searchPrefix(prefix: string, limit = 10): string[] {
    const results: Array<{ term: string; frequency: number }> = []

    for (const [term, frequency] of this.termFrequency) {
      if (term.startsWith(prefix)) {
        results.push({ term, frequency })
      }
    }

    // 按频率排序
    results.sort((a, b) => b.frequency - a.frequency)

    return results.slice(0, limit).map(r => r.term)
  }

  // 模糊搜索
  searchFuzzy(query: string, maxDistance = 2): string[] {
    const results: Array<{ nodeId: string; score: number }> = []
    const queryTerms = this.tokenize(query)

    for (const [nodeId, terms] of this.nodeTerms) {
      let score = 0

      for (const queryTerm of queryTerms) {
        for (const term of terms) {
          const distance = levenshteinDistance(queryTerm, term)
          if (distance <= maxDistance) {
            score += 1 / (distance + 1)
          }
        }
      }

      if (score > 0) {
        results.push({ nodeId, score })
      }
    }

    results.sort((a, b) => b.score - a.score)
    return results.map(r => r.nodeId)
  }
}
```

#### B. Web Worker 计算

```typescript
// Web Worker 计算密集型任务
class WorkerPool {
  private workers: Worker[] = []
  private queue: Array<{ task: Task; resolve: Function; reject: Function }> = []
  private busy = new Set<Worker>()

  constructor(private poolSize = 4) {
    for (let i = 0; i < poolSize; i++) {
      const worker = new Worker('./navigator-worker.js')
      worker.onmessage = e => this.handleMessage(worker, e)
      this.workers.push(worker)
    }
  }

  execute<T>(task: Task): Promise<T> {
    return new Promise((resolve, reject) => {
      const availableWorker = this.workers.find(w => !this.busy.has(w))

      if (availableWorker) {
        this.runTask(availableWorker, task, resolve, reject)
      } else {
        this.queue.push({ task, resolve, reject })
      }
    })
  }

  private runTask(worker: Worker, task: Task, resolve: Function, reject: Function): void {
    this.busy.add(worker)

    const messageId = generateId()

    const handler = (e: MessageEvent) => {
      if (e.data.id === messageId) {
        worker.removeEventListener('message', handler)
        this.busy.delete(worker)

        if (e.data.error) {
          reject(new Error(e.data.error))
        } else {
          resolve(e.data.result)
        }

        // 处理队列中的下一个任务
        this.processQueue()
      }
    }

    worker.addEventListener('message', handler)
    worker.postMessage({ id: messageId, task })
  }

  private processQueue(): void {
    if (this.queue.length === 0) return

    const availableWorker = this.workers.find(w => !this.busy.has(w))
    if (!availableWorker) return

    const { task, resolve, reject } = this.queue.shift()!
    this.runTask(availableWorker, task, resolve, reject)
  }
}

// Worker 脚本
// navigator-worker.js
self.onmessage = function (e) {
  const { id, task } = e.data

  try {
    let result

    switch (task.type) {
      case 'COMPUTE_DIFF':
        result = computeDiff(task.oldSnapshot, task.newSnapshot)
        break
      case 'BUILD_INDEX':
        result = buildIndex(task.nodes)
        break
      case 'SEARCH':
        result = search(task.index, task.query)
        break
      case 'SORT':
        result = sort(task.items, task.compareFn)
        break
    }

    self.postMessage({ id, result })
  } catch (error) {
    self.postMessage({ id, error: error.message })
  }
}
```

## 2. 网络优化

### 2.1 请求合并

```typescript
// 请求合并器
class RequestBatcher {
  private batch = new Map<string, PendingRequest>()
  private timeout: NodeJS.Timeout | null = null
  private batchWindow = 10 // ms

  request<T>(key: string, fetcher: () => Promise<T>): Promise<T> {
    // 检查是否已有相同请求
    const pending = this.batch.get(key)
    if (pending) {
      return new Promise((resolve, reject) => {
        pending.callbacks.push({ resolve, reject })
      })
    }

    // 创建新请求
    return new Promise((resolve, reject) => {
      this.batch.set(key, {
        fetcher,
        callbacks: [{ resolve, reject }],
      })

      // 调度批量执行
      this.scheduleBatch()
    })
  }

  private scheduleBatch(): void {
    if (this.timeout) return

    this.timeout = setTimeout(() => {
      this.executeBatch()
      this.timeout = null
    }, this.batchWindow)
  }

  private async executeBatch(): Promise<void> {
    const batch = new Map(this.batch)
    this.batch.clear()

    // 批量请求
    const promises = Array.from(batch.entries()).map(async ([key, pending]) => {
      try {
        const result = await pending.fetcher()
        pending.callbacks.forEach(cb => cb.resolve(result))
      } catch (error) {
        pending.callbacks.forEach(cb => cb.reject(error))
      }
    })

    await Promise.all(promises)
  }
}
```

### 2.2 增量同步

```typescript
// 增量同步管理器
class IncrementalSync {
  private lastSyncVersion = 0
  private pendingChanges: LocalChange[] = []
  private syncInterval = 5000

  async sync(): Promise<void> {
    try {
      // 获取服务器增量
      const response = await api.syncMetadata({
        since: this.lastSyncVersion,
        limit: 1000,
      })

      // 应用服务器增量
      for (const delta of response.deltas) {
        viewEngine.applyDelta('navigator:main', delta)
      }

      // 发送本地变更
      if (this.pendingChanges.length > 0) {
        await api.pushChanges(this.pendingChanges)
        this.pendingChanges = []
      }

      this.lastSyncVersion = response.version
    } catch (error) {
      console.error('Sync failed:', error)
      // 重试逻辑
      setTimeout(() => this.sync(), this.syncInterval)
    }
  }

  queueChange(change: LocalChange): void {
    this.pendingChanges.push(change)

    // 防抖同步
    this.debouncedSync()
  }

  private debouncedSync = debounce(() => {
    this.sync()
  }, 1000)
}
```

### 2.3 预加载策略

```typescript
// 智能预加载
class SmartPreloader {
  private preloadQueue: string[] = []
  private isPreloading = false
  private preloadDelay = 100

  // 基于用户行为的预加载
  preloadBasedOnBehavior(nodeId: string, behavior: UserBehavior): void {
    const targets = this.predictNextTargets(nodeId, behavior)

    for (const target of targets) {
      if (!this.isLoaded(target) && !this.isInQueue(target)) {
        this.preloadQueue.push(target)
      }
    }

    this.schedulePreload()
  }

  // 预测下一个目标
  private predictNextTargets(nodeId: string, behavior: UserBehavior): string[] {
    const predictions: string[] = []

    // 基于历史行为
    const history = this.getUserHistory()
    const similarPatterns = history.filter(h => h.previousNode === nodeId)

    if (similarPatterns.length > 0) {
      // 统计最可能的下一个节点
      const frequency = countBy(similarPatterns, 'nextNode')
      const sorted = Object.entries(frequency)
        .sort((a, b) => b[1] - a[1])
        .slice(0, 3)

      predictions.push(...sorted.map(([node]) => node))
    }

    // 基于节点关系
    const siblings = this.getSiblings(nodeId)
    predictions.push(...siblings.slice(0, 2))

    return predictions
  }

  private async schedulePreload(): Promise<void> {
    if (this.isPreloading || this.preloadQueue.length === 0) return

    this.isPreloading = true

    // 使用 requestIdleCallback 在空闲时预加载
    const preload = () => {
      const nodeId = this.preloadQueue.shift()
      if (!nodeId) {
        this.isPreloading = false
        return
      }

      this.loadNode(nodeId).then(() => {
        if (this.preloadQueue.length > 0) {
          setTimeout(preload, this.preloadDelay)
        } else {
          this.isPreloading = false
        }
      })
    }

    if ('requestIdleCallback' in window) {
      requestIdleCallback(preload, { timeout: 2000 })
    } else {
      setTimeout(preload, this.preloadDelay)
    }
  }
}
```

## 3. 用户体验优化

### 3.1 骨架屏

```vue
<!-- SkeletonNode.vue -->
<template>
  <div class="skeleton-node" :style="{ paddingLeft: `${level * 16}px` }">
    <div class="skeleton-icon" />
    <div class="skeleton-text" :style="{ width: `${randomWidth}px` }" />
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
  level: number
}>()

const randomWidth = computed(() => {
  return 60 + Math.random() * 100
})
</script>

<style scoped>
.skeleton-node {
  display: flex;
  align-items: center;
  height: 28px;
  gap: 8px;
}

.skeleton-icon {
  width: 16px;
  height: 16px;
  border-radius: 4px;
  background: linear-gradient(
    90deg,
    var(--bg-hover) 25%,
    var(--bg-secondary) 50%,
    var(--bg-hover) 75%
  );
  background-size: 200% 100%;
  animation: shimmer 1.5s infinite;
}

.skeleton-text {
  height: 14px;
  border-radius: 2px;
  background: linear-gradient(
    90deg,
    var(--bg-hover) 25%,
    var(--bg-secondary) 50%,
    var(--bg-hover) 75%
  );
  background-size: 200% 100%;
  animation: shimmer 1.5s infinite;
}

@keyframes shimmer {
  0% {
    background-position: 200% 0;
  }
  100% {
    background-position: -200% 0;
  }
}
</style>
```

### 3.2 渐进式加载

```typescript
// 渐进式加载器
class ProgressiveLoader {
  private stages: LoadingStage[] = [
    { name: 'structure', priority: 1, weight: 0.3 },
    { name: 'metadata', priority: 2, weight: 0.3 },
    { name: 'statistics', priority: 3, weight: 0.2 },
    { name: 'relations', priority: 4, weight: 0.2 },
  ]

  async loadProgressively(
    nodeId: string,
    onProgress: (progress: LoadingProgress) => void
  ): Promise<void> {
    let completedWeight = 0

    for (const stage of this.stages) {
      onProgress({
        stage: stage.name,
        percent: completedWeight * 100,
        status: 'loading',
      })

      try {
        await this.loadStage(nodeId, stage)
        completedWeight += stage.weight

        onProgress({
          stage: stage.name,
          percent: completedWeight * 100,
          status: 'complete',
        })
      } catch (error) {
        onProgress({
          stage: stage.name,
          percent: completedWeight * 100,
          status: 'error',
          error: error as Error,
        })
      }
    }
  }

  private async loadStage(nodeId: string, stage: LoadingStage): Promise<void> {
    switch (stage.name) {
      case 'structure':
        await api.getNodeStructure(nodeId)
        break
      case 'metadata':
        await api.getNodeMetadata(nodeId)
        break
      case 'statistics':
        await api.getNodeStatistics(nodeId)
        break
      case 'relations':
        await api.getNodeRelations(nodeId)
        break
    }
  }
}
```

### 3.3 动画优化

```typescript
// 动画控制器
class AnimationController {
  private animations = new Map<string, Animation>()
  private prefersReducedMotion = false

  constructor() {
    // 检测用户偏好
    const mediaQuery = window.matchMedia('(prefers-reduced-motion: reduce)')
    this.prefersReducedMotion = mediaQuery.matches

    mediaQuery.addEventListener('change', e => {
      this.prefersReducedMotion = e.matches
      if (e.matches) {
        this.cancelAll()
      }
    })
  }

  animate(
    element: HTMLElement,
    keyframes: Keyframe[],
    options: KeyframeAnimationOptions
  ): Animation {
    if (this.prefersReducedMotion) {
      // 禁用动画，直接设置最终状态
      const lastFrame = keyframes[keyframes.length - 1]
      Object.assign(element.style, lastFrame)
      return null as any
    }

    const animation = element.animate(keyframes, {
      ...options,
      fill: 'forwards',
    })

    this.animations.set(animation.id, animation)

    animation.addEventListener('finish', () => {
      this.animations.delete(animation.id)
    })

    return animation
  }

  // 批量动画，使用 FLIP 技术
  animateBatch(elements: Array<{ element: HTMLElement; from: DOMRect; to: DOMRect }>): void {
    if (this.prefersReducedMotion) return

    // First: 记录初始状态
    // Last: 计算最终状态
    // Invert: 计算差异
    // Play: 执行动画

    elements.forEach(({ element, from, to }) => {
      const invertX = from.left - to.left
      const invertY = from.top - to.top
      const invertScaleX = from.width / to.width
      const invertScaleY = from.height / to.height

      // 应用反向变换
      element.style.transform = `
        translate(${invertX}px, ${invertY}px)
        scale(${invertScaleX}, ${invertScaleY})
      `

      // 强制重排
      element.getBoundingClientRect()

      // 执行动画
      this.animate(
        element,
        [{ transform: element.style.transform }, { transform: 'translate(0, 0) scale(1)' }],
        { duration: 300, easing: 'cubic-bezier(0.4, 0, 0.2, 1)' }
      )
    })
  }

  cancelAll(): void {
    this.animations.forEach(animation => animation.cancel())
    this.animations.clear()
  }
}
```

## 4. 监控与分析

### 4.1 性能指标收集

```typescript
// 性能监控器
class PerformanceMonitor {
  private metrics: PerformanceMetrics = {
    renderTime: [],
    loadTime: [],
    syncTime: [],
    memoryUsage: [],
    frameRate: [],
  }

  // 记录渲染时间
  measureRender(componentName: string, fn: () => void): void {
    const start = performance.now()
    fn()
    const end = performance.now()

    this.metrics.renderTime.push({
      component: componentName,
      duration: end - start,
      timestamp: Date.now(),
    })
  }

  // 记录加载时间
  measureLoad(operation: string, promise: Promise<unknown>): Promise<unknown> {
    const start = performance.now()

    return promise.finally(() => {
      const end = performance.now()
      this.metrics.loadTime.push({
        operation,
        duration: end - start,
        timestamp: Date.now(),
      })
    })
  }

  // 监控帧率
  monitorFrameRate(): void {
    let lastTime = performance.now()
    let frames = 0

    const measure = () => {
      frames++
      const currentTime = performance.now()

      if (currentTime - lastTime >= 1000) {
        const fps = Math.round((frames * 1000) / (currentTime - lastTime))
        this.metrics.frameRate.push({ fps, timestamp: Date.now() })

        frames = 0
        lastTime = currentTime
      }

      requestAnimationFrame(measure)
    }

    requestAnimationFrame(measure)
  }

  // 生成报告
  generateReport(): PerformanceReport {
    return {
      renderTime: this.calculateStats(this.metrics.renderTime.map(m => m.duration)),
      loadTime: this.calculateStats(this.metrics.loadTime.map(m => m.duration)),
      frameRate: this.calculateStats(this.metrics.frameRate.map(m => m.fps)),
      recommendations: this.generateRecommendations(),
    }
  }

  private calculateStats(values: number[]): Stats {
    const sorted = [...values].sort((a, b) => a - b)
    const sum = sorted.reduce((a, b) => a + b, 0)

    return {
      min: sorted[0],
      max: sorted[sorted.length - 1],
      avg: sum / sorted.length,
      p50: sorted[Math.floor(sorted.length * 0.5)],
      p95: sorted[Math.floor(sorted.length * 0.95)],
      p99: sorted[Math.floor(sorted.length * 0.99)],
    }
  }
}
```

### 4.2 错误追踪

```typescript
// 错误追踪器
class ErrorTracker {
  private errors: TrackedError[] = []
  private maxErrors = 100

  track(error: Error, context?: Record<string, unknown>): void {
    const trackedError: TrackedError = {
      message: error.message,
      stack: error.stack,
      context,
      timestamp: Date.now(),
      userAgent: navigator.userAgent,
      url: window.location.href,
    }

    this.errors.push(trackedError)

    // 限制错误数量
    if (this.errors.length > this.maxErrors) {
      this.errors.shift()
    }

    // 发送到分析服务
    this.sendToAnalytics(trackedError)
  }

  // 分类统计
  getErrorStats(): ErrorStats {
    const categorized = groupBy(this.errors, e => this.categorizeError(e))

    return {
      total: this.errors.length,
      byCategory: mapValues(categorized, errors => errors.length),
      byTime: this.groupByTime(this.errors),
      recent: this.errors.slice(-10),
    }
  }

  private categorizeError(error: TrackedError): string {
    if (error.message.includes('network')) return 'network'
    if (error.message.includes('timeout')) return 'timeout'
    if (error.message.includes('memory')) return 'memory'
    return 'unknown'
  }
}
```

## 5. 持续优化策略

### 5.1 A/B 测试

```typescript
// A/B 测试框架
class ABTestFramework {
  private experiments = new Map<string, Experiment>()

  registerExperiment(experiment: Experiment): void {
    this.experiments.set(experiment.id, experiment)
  }

  getVariant(experimentId: string): string {
    const experiment = this.experiments.get(experimentId)
    if (!experiment) return 'control'

    // 根据用户 ID 分配变体
    const userId = this.getUserId()
    const hash = this.hashString(`${experimentId}:${userId}`)
    const variantIndex = hash % experiment.variants.length

    return experiment.variants[variantIndex]
  }

  trackEvent(experimentId: string, event: string, data?: unknown): void {
    const variant = this.getVariant(experimentId)

    analytics.track({
      experiment: experimentId,
      variant,
      event,
      data,
      timestamp: Date.now(),
    })
  }
}
```

### 5.2 性能预算

```typescript
// 性能预算检查
const performanceBudget = {
  // 加载性能
  initialLoad: 100, // 100ms
  subsequentLoad: 50, // 50ms

  // 渲染性能
  renderTime: 16, // 16ms (60fps)
  frameRate: 60, // 60fps

  // 资源使用
  memoryUsage: 150 * 1024 * 1024, // 150MB
  cacheSize: 50 * 1024 * 1024, // 50MB

  // 网络
  requestCount: 10, // 同时请求数
  requestSize: 100 * 1024, // 100KB
}

// 预算检查
function checkPerformanceBudget(metrics: PerformanceMetrics): BudgetViolation[] {
  const violations: BudgetViolation[] = []

  if (metrics.loadTime > performanceBudget.initialLoad) {
    violations.push({
      metric: 'loadTime',
      budget: performanceBudget.initialLoad,
      actual: metrics.loadTime,
      severity: 'warning',
    })
  }

  if (metrics.renderTime > performanceBudget.renderTime) {
    violations.push({
      metric: 'renderTime',
      budget: performanceBudget.renderTime,
      actual: metrics.renderTime,
      severity: 'error',
    })
  }

  return violations
}
```
