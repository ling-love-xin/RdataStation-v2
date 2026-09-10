/**
 * 前端缓存状态管理器
 *
 * 维护缓存有效性状态，减少不必要的 IPC 调用
 * 实现智能缓存策略：
 * - 前端维护缓存状态，避免频繁调用后端检查
 * - 支持缓存版本控制
 * - 支持缓存失效策略
 * - 集成缓存命中率统计
 */

import { ref, computed } from 'vue'

import { cacheMetricsManager } from './use-cache-metrics'

/**
 * 缓存状态
 */
export interface CacheState {
  /** 缓存是否有效 */
  isValid: boolean
  /** 最后同步时间 */
  lastSync: number | null
  /** 缓存版本号 */
  version: number
  /** 表数量 */
  tableCount: number
  /** 列数量 */
  columnCount: number
  /** 缓存创建时间 */
  createdAt: number
  /** 最后访问时间（用于 LRU） */
  lastAccessed: number
  /** 访问次数（用于 LRU） */
  accessCount: number
}

/**
 * 缓存键
 */
export interface CacheKey {
  connectionId: string
  databaseName: string
  schemaName?: string
  tableName?: string
  columnName?: string
}

/**
 * 缓存状态管理器
 */
class CacheStateManager {
  private cacheStates = ref<Map<string, CacheState>>(new Map())
  private cacheVersion = ref<Map<string, number>>(new Map())

  /**
   * 每个连接的最大缓存表数量（LRU 限制）
   */
  private maxTablesPerConnection = 1000

  /**
   * 生成缓存键
   */
  private generateKey(key: CacheKey): string {
    return `${key.connectionId}:${key.databaseName}:${key.schemaName || ''}:${key.tableName || ''}`
  }

  /**
   * 获取缓存状态（带指标统计和 LRU 更新）
   */
  getState(key: CacheKey): CacheState | null {
    const cacheKey = this.generateKey(key)
    const state = this.cacheStates.value.get(cacheKey) || null

    if (state) {
      state.lastAccessed = Date.now()
      state.accessCount++
      this.cacheStates.value.set(cacheKey, state)
    }

    cacheMetricsManager.recordOperation(
      'read',
      state !== null,
      0,
      key.connectionId,
      key.databaseName,
      key.schemaName,
      key.tableName
    )

    return state
  }

  /**
   * 设置缓存状态（带指标统计和 LRU 检查）
   */
  setState(key: CacheKey, state: Partial<CacheState>): void {
    const startTime = performance.now()
    const cacheKey = this.generateKey(key)
    const existing = this.cacheStates.value.get(cacheKey)

    const newState: CacheState = {
      isValid: state.isValid ?? existing?.isValid ?? false,
      lastSync: state.lastSync ?? existing?.lastSync ?? null,
      version: state.version ?? existing?.version ?? 1,
      tableCount: state.tableCount ?? existing?.tableCount ?? 0,
      columnCount: state.columnCount ?? existing?.columnCount ?? 0,
      createdAt: existing?.createdAt ?? Date.now(),
      lastAccessed: Date.now(),
      accessCount: existing?.accessCount ?? 0,
    }

    this.cacheStates.value.set(cacheKey, newState)

    const latency = performance.now() - startTime
    cacheMetricsManager.recordOperation(
      'write',
      true,
      latency,
      key.connectionId,
      key.databaseName,
      key.schemaName,
      key.tableName
    )

    this.enforceLRULimit(key.connectionId)
  }

  /**
   * 执行 LRU 淘汰策略
   */
  private enforceLRULimit(connectionId: string): void {
    const connectionEntries: Array<[string, CacheState]> = []

    this.cacheStates.value.forEach((state, cacheKey) => {
      if (cacheKey.startsWith(`${connectionId}:`)) {
        connectionEntries.push([cacheKey, state])
      }
    })

    if (connectionEntries.length <= this.maxTablesPerConnection) {
      return
    }

    connectionEntries.sort((a, b) => {
      const scoreA = a[1].lastAccessed + a[1].accessCount * 1000
      const scoreB = b[1].lastAccessed + b[1].accessCount * 1000
      return scoreA - scoreB
    })

    const entriesToRemove = connectionEntries.length - this.maxTablesPerConnection
    for (let i = 0; i < entriesToRemove; i++) {
      const [cacheKey] = connectionEntries[i]
      this.cacheStates.value.delete(cacheKey)
    }
  }

  /**
   * 标记缓存为有效
   */
  markValid(key: CacheKey, tableCount: number, columnCount: number): void {
    this.setState(key, {
      isValid: true,
      lastSync: Date.now(),
      tableCount,
      columnCount,
    })
  }

  /**
   * 标记缓存为无效
   */
  markInvalid(key: CacheKey): void {
    this.setState(key, {
      isValid: false,
      lastSync: null,
      tableCount: 0,
      columnCount: 0,
    })
  }

  /**
   * 清除指定连接的缓存状态
   */
  clearConnection(connectionId: string): void {
    const keysToRemove: string[] = []
    this.cacheStates.value.forEach((_, key) => {
      if (key.startsWith(`${connectionId}:`)) {
        keysToRemove.push(key)
      }
    })
    keysToRemove.forEach(key => this.cacheStates.value.delete(key))
  }

  /**
   * 清除所有缓存状态
   */
  clearAll(): void {
    this.cacheStates.value.clear()
  }

  /**
   * 获取缓存版本号
   */
  getVersion(connectionId: string): number {
    return this.cacheVersion.value.get(connectionId) || 1
  }

  /**
   * 更新缓存版本号
   */
  incrementVersion(connectionId: string): number {
    const current = this.getVersion(connectionId)
    const newVersion = current + 1
    this.cacheVersion.value.set(connectionId, newVersion)
    return newVersion
  }

  /**
   * 检查缓存是否过期（默认 24 小时）
   */
  isExpired(key: CacheKey, maxAgeMs: number = 24 * 60 * 60 * 1000): boolean {
    const state = this.getState(key)
    if (!state || !state.lastSync) return true
    return Date.now() - state.lastSync > maxAgeMs
  }

  /**
   * 获取所有缓存状态的统计信息
   */
  getStats(): { total: number; valid: number; invalid: number } {
    let total = 0
    let valid = 0
    let invalid = 0

    this.cacheStates.value.forEach(state => {
      total++
      if (state.isValid) valid++
      else invalid++
    })

    return { total, valid, invalid }
  }

  /**
   * 获取指定连接的缓存统计
   */
  getConnectionStats(_connectionId: string): {
    total: number
    valid: number
    invalid: number
    totalTables: number
    totalColumns: number
  } {
    let total = 0
    let valid = 0
    let invalid = 0
    let totalTables = 0
    let totalColumns = 0

    this.cacheStates.value.forEach(state => {
      if (state.tableCount > 0) {
        total++
        if (state.isValid) valid++
        else invalid++
        totalTables += state.tableCount
        totalColumns += state.columnCount
      }
    })

    return { total, valid, invalid, totalTables, totalColumns }
  }

  /**
   * 设置每个连接的最大缓存表数量
   */
  setMaxTablesPerConnection(max: number): void {
    this.maxTablesPerConnection = max
  }
}

/**
 * 单例实例
 */
export const cacheStateManager = new CacheStateManager()

/**
 * Composable 函数
 */
export function useCacheState() {
  const manager = cacheStateManager

  const stats = computed(() => manager.getStats())

  function getState(key: CacheKey) {
    return manager.getState(key)
  }

  function setState(key: CacheKey, state: Partial<CacheState>) {
    manager.setState(key, state)
  }

  function markValid(key: CacheKey, tableCount: number, columnCount: number) {
    manager.markValid(key, tableCount, columnCount)
  }

  function markInvalid(key: CacheKey) {
    manager.markInvalid(key)
  }

  function clearConnection(connectionId: string) {
    manager.clearConnection(connectionId)
  }

  function clearAll() {
    manager.clearAll()
  }

  function isExpired(key: CacheKey, maxAgeMs?: number) {
    return manager.isExpired(key, maxAgeMs)
  }

  function getVersion(connectionId: string) {
    return manager.getVersion(connectionId)
  }

  function incrementVersion(connectionId: string) {
    return manager.incrementVersion(connectionId)
  }

  function getConnectionStats(connectionId: string) {
    return manager.getConnectionStats(connectionId)
  }

  function setMaxTablesPerConnection(max: number) {
    manager.setMaxTablesPerConnection(max)
  }

  return {
    stats,
    getState,
    setState,
    markValid,
    markInvalid,
    clearConnection,
    clearAll,
    isExpired,
    getVersion,
    incrementVersion,
    getConnectionStats,
    setMaxTablesPerConnection,
  }
}
