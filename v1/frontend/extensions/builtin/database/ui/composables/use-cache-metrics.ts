/**
 * 缓存命中率统计与监控
 *
 * 提供缓存操作的统计指标：
 * - 命中率（hits / total）
 * - 平均延迟
 * - 缓存大小
 * - 预热进度
 *
 * 用于数据驱动优化缓存策略
 */

import { ref } from 'vue'

/**
 * 缓存操作类型
 */
export type CacheOperationType = 'read' | 'write' | 'refresh' | 'invalidate'

/**
 * 单次缓存操作记录
 */
export interface CacheOperationRecord {
  /** 操作类型 */
  type: CacheOperationType
  /** 操作时间戳 */
  timestamp: number
  /** 是否命中 */
  hit: boolean
  /** 操作延迟（毫秒） */
  latency: number
  /** 连接 ID */
  connectionId: string
  /** 数据库名 */
  databaseName: string
  /** Schema 名（可选） */
  schemaName?: string
  /** 表名（可选） */
  tableName?: string
}

/**
 * 缓存统计指标
 */
export interface CacheMetrics {
  /** 总操作次数 */
  totalOperations: number
  /** 命中次数 */
  hits: number
  /** 未命中次数 */
  misses: number
  /** 命中率（0-1） */
  hitRate: number
  /** 平均延迟（毫秒） */
  avgLatency: number
  /** 最近 100 次操作的延迟列表 */
  recentLatencies: number[]
  /** 按连接 ID 统计的指标 */
  byConnection: Map<string, ConnectionMetrics>
  /** 按操作类型统计的指标 */
  byOperationType: Map<CacheOperationType, OperationTypeMetrics>
}

/**
 * 单个连接的缓存指标
 */
export interface ConnectionMetrics {
  /** 连接 ID */
  connectionId: string
  /** 总操作次数 */
  totalOperations: number
  /** 命中次数 */
  hits: number
  /** 命中率 */
  hitRate: number
  /** 平均延迟 */
  avgLatency: number
  /** 最后操作时间 */
  lastOperation: number
}

/**
 * 按操作类型统计的指标
 */
export interface OperationTypeMetrics {
  /** 操作类型 */
  type: CacheOperationType
  /** 总操作次数 */
  totalOperations: number
  /** 命中次数 */
  hits: number
  /** 命中率 */
  hitRate: number
  /** 平均延迟 */
  avgLatency: number
}

/**
 * 缓存指标管理器
 */
class CacheMetricsManager {
  /** 操作记录列表 */
  private records = ref<CacheOperationRecord[]>([])

  /** 最大记录数 */
  private maxRecords = 1000

  /**
   * 记录一次缓存操作
   */
  recordOperation(
    type: CacheOperationType,
    hit: boolean,
    latency: number,
    connectionId: string,
    databaseName: string,
    schemaName?: string,
    tableName?: string
  ): void {
    const record: CacheOperationRecord = {
      type,
      timestamp: Date.now(),
      hit,
      latency,
      connectionId,
      databaseName,
      schemaName,
      tableName,
    }

    this.records.value.push(record)

    if (this.records.value.length > this.maxRecords) {
      this.records.value = this.records.value.slice(-this.maxRecords)
    }
  }

  /**
   * 获取全局缓存指标
   */
  getMetrics(): CacheMetrics {
    const records = this.records.value
    const total = records.length
    const hits = records.filter(r => r.hit).length
    const misses = total - hits
    const hitRate = total > 0 ? hits / total : 0
    const avgLatency = total > 0 ? records.reduce((sum, r) => sum + r.latency, 0) / total : 0

    const recentLatencies = records.slice(-100).map(r => r.latency)

    const byConnection = this.aggregateByConnection(records)
    const byOperationType = this.aggregateByOperationType(records)

    return {
      totalOperations: total,
      hits,
      misses,
      hitRate,
      avgLatency,
      recentLatencies,
      byConnection,
      byOperationType,
    }
  }

  /**
   * 获取单个连接的指标
   */
  getConnectionMetrics(connectionId: string): ConnectionMetrics | null {
    const records = this.records.value.filter(r => r.connectionId === connectionId)
    const total = records.length
    if (total === 0) return null

    const hits = records.filter(r => r.hit).length
    const avgLatency = records.reduce((sum, r) => sum + r.latency, 0) / total
    const lastOperation = records[records.length - 1].timestamp

    return {
      connectionId,
      totalOperations: total,
      hits,
      hitRate: hits / total,
      avgLatency,
      lastOperation,
    }
  }

  /**
   * 获取指定时间范围内的指标
   */
  getMetricsInTimeRange(startTime: number, endTime: number): CacheMetrics {
    const records = this.records.value.filter(
      r => r.timestamp >= startTime && r.timestamp <= endTime
    )

    const total = records.length
    const hits = records.filter(r => r.hit).length
    const avgLatency = total > 0 ? records.reduce((sum, r) => sum + r.latency, 0) / total : 0

    return {
      totalOperations: total,
      hits,
      misses: total - hits,
      hitRate: total > 0 ? hits / total : 0,
      avgLatency,
      recentLatencies: records.map(r => r.latency),
      byConnection: this.aggregateByConnection(records),
      byOperationType: this.aggregateByOperationType(records),
    }
  }

  /**
   * 清除所有记录
   */
  clear(): void {
    this.records.value = []
  }

  /**
   * 清除指定连接的记录
   */
  clearConnection(connectionId: string): void {
    this.records.value = this.records.value.filter(r => r.connectionId !== connectionId)
  }

  /**
   * 按连接 ID 聚合指标
   */
  private aggregateByConnection(records: CacheOperationRecord[]): Map<string, ConnectionMetrics> {
    const map = new Map<string, ConnectionMetrics>()

    for (const record of records) {
      let metrics = map.get(record.connectionId)
      if (!metrics) {
        metrics = {
          connectionId: record.connectionId,
          totalOperations: 0,
          hits: 0,
          hitRate: 0,
          avgLatency: 0,
          lastOperation: 0,
        }
        map.set(record.connectionId, metrics)
      }

      metrics.totalOperations++
      if (record.hit) metrics.hits++
      metrics.lastOperation = Math.max(metrics.lastOperation, record.timestamp)
    }

    for (const metrics of map.values()) {
      metrics.hitRate = metrics.totalOperations > 0 ? metrics.hits / metrics.totalOperations : 0
    }

    return map
  }

  /**
   * 按操作类型聚合指标
   */
  private aggregateByOperationType(
    records: CacheOperationRecord[]
  ): Map<CacheOperationType, OperationTypeMetrics> {
    const map = new Map<CacheOperationType, OperationTypeMetrics>()

    for (const record of records) {
      let metrics = map.get(record.type)
      if (!metrics) {
        metrics = {
          type: record.type,
          totalOperations: 0,
          hits: 0,
          hitRate: 0,
          avgLatency: 0,
        }
        map.set(record.type, metrics)
      }

      metrics.totalOperations++
      if (record.hit) metrics.hits++
    }

    for (const metrics of map.values()) {
      metrics.hitRate = metrics.totalOperations > 0 ? metrics.hits / metrics.totalOperations : 0
    }

    return map
  }
}

/**
 * 缓存操作性能包装器
 *
 * 自动记录操作的命中率和延迟
 */
export async function wrapCacheOperation<T>(
  type: CacheOperationType,
  connectionId: string,
  databaseName: string,
  operation: () => Promise<{ data: T; hit: boolean }>,
  schemaName?: string,
  tableName?: string
): Promise<T> {
  const startTime = performance.now()

  try {
    const result = await operation()
    const latency = performance.now() - startTime

    cacheMetricsManager.recordOperation(
      type,
      result.hit,
      latency,
      connectionId,
      databaseName,
      schemaName,
      tableName
    )

    return result.data
  } catch (error) {
    const latency = performance.now() - startTime

    cacheMetricsManager.recordOperation(
      type,
      false,
      latency,
      connectionId,
      databaseName,
      schemaName,
      tableName
    )

    throw error
  }
}

/**
 * 单例管理器
 */
export const cacheMetricsManager = new CacheMetricsManager()
