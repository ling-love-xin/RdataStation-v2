/**
 * 性能监控工具
 *
 * 用于监控数据库导航栏的性能指标
 */

export interface PerformanceMetrics {
  /** 节点加载时间 */
  nodeLoadTime: number
  /** 渲染时间 */
  renderTime: number
  /** 请求数量 */
  requestCount: number
  /** 缓存命中率 */
  cacheHitRate: number
}

export class PerformanceMonitor {
  private metrics: PerformanceMetrics = {
    nodeLoadTime: 0,
    renderTime: 0,
    requestCount: 0,
    cacheHitRate: 0,
  }

  private cacheHits = 0
  private cacheMisses = 0
  private timings = new Map<string, number>()

  /**
   * 开始计时
   */
  startTimer(name: string): void {
    this.timings.set(name, performance.now())
  }

  /**
   * 结束计时并返回耗时（毫秒）
   */
  endTimer(name: string): number {
    const startTime = this.timings.get(name)
    if (startTime) {
      const duration = performance.now() - startTime
      this.timings.delete(name)
      return duration
    }
    return 0
  }

  /**
   * 记录缓存命中
   */
  recordCacheHit(): void {
    this.cacheHits++
    this.updateCacheHitRate()
  }

  /**
   * 记录缓存未命中
   */
  recordCacheMiss(): void {
    this.cacheMisses++
    this.updateCacheHitRate()
  }

  /**
   * 更新缓存命中率
   */
  private updateCacheHitRate(): void {
    const total = this.cacheHits + this.cacheMisses
    this.metrics.cacheHitRate = total > 0 ? (this.cacheHits / total) * 100 : 0
  }

  /**
   * 记录请求
   */
  recordRequest(): void {
    this.metrics.requestCount++
  }

  /**
   * 更新节点加载时间
   */
  updateNodeLoadTime(time: number): void {
    this.metrics.nodeLoadTime = time
  }

  /**
   * 更新渲染时间
   */
  updateRenderTime(time: number): void {
    this.metrics.renderTime = time
  }

  /**
   * 获取当前指标
   */
  getMetrics(): PerformanceMetrics {
    return { ...this.metrics }
  }

  /**
   * 重置所有指标
   */
  reset(): void {
    this.metrics = {
      nodeLoadTime: 0,
      renderTime: 0,
      requestCount: 0,
      cacheHitRate: 0,
    }
    this.cacheHits = 0
    this.cacheMisses = 0
    this.timings.clear()
  }

  /**
   * 打印性能报告
   */
  logReport(): void {
    const metrics = this.getMetrics()
    // eslint-disable-next-line no-console
    console.debug('=== Database Navigator Performance Report ===')
    // eslint-disable-next-line no-console
    console.debug(`Node Load Time: ${metrics.nodeLoadTime.toFixed(2)}ms`)
    // eslint-disable-next-line no-console
    console.debug(`Render Time: ${metrics.renderTime.toFixed(2)}ms`)
    // eslint-disable-next-line no-console
    console.debug(`Request Count: ${metrics.requestCount}`)
    // eslint-disable-next-line no-console
    console.debug(`Cache Hit Rate: ${metrics.cacheHitRate.toFixed(1)}%`)
    // eslint-disable-next-line no-console
    console.debug('============================================')
  }
}

export const performanceMonitor = new PerformanceMonitor()
