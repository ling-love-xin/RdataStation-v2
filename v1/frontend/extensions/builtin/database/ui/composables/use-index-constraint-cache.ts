/**
 * 索引与约束缓存管理
 *
 * 实现表展开时缓存索引和约束信息：
 * - 表展开时自动加载并缓存索引和约束
 * - 支持缓存读取和刷新
 * - 提供统一的索引/约束数据访问接口
 *
 * 遵循架构规范：前端只负责调度，不实现业务逻辑
 */

import { cacheStateManager } from './use-cache-state'
import { useDatabaseNavigatorStore } from '../stores/database-navigator-store'

/**
 * 索引元数据
 */
export interface IndexMeta {
  id: string
  name: string
  tableName: string
  columns: string[]
  isUnique: boolean
  isPrimary: boolean
  type: string
  comment: string | null
}

/**
 * 约束元数据
 */
export interface ConstraintMeta {
  id: string
  name: string
  tableName: string
  type: 'PRIMARY KEY' | 'FOREIGN KEY' | 'UNIQUE' | 'CHECK'
  columns: string[]
  referencedTable?: string
  referencedColumns?: string[]
  comment: string | null
}

/**
 * 表索引与约束缓存
 */
export interface TableIndexConstraintCache {
  indexes: IndexMeta[]
  constraints: ConstraintMeta[]
  lastSync: number
  isValid: boolean
}

/**
 * 索引与约束缓存管理器
 */
class IndexConstraintCacheManager {
  /** 缓存存储 */
  private cache = new Map<string, TableIndexConstraintCache>()

  /** 缓存有效期（毫秒）- 默认 24 小时 */
  private maxAge = 24 * 60 * 60 * 1000

  /**
   * 生成缓存键
   */
  private generateCacheKey(
    connectionId: string,
    dbName: string,
    schemaName: string | undefined,
    tableName: string
  ): string {
    return `${connectionId}:${dbName}:${schemaName || ''}:${tableName}`
  }

  /**
   * 获取缓存
   */
  getCache(
    connectionId: string,
    dbName: string,
    schemaName: string | undefined,
    tableName: string
  ): TableIndexConstraintCache | null {
    const key = this.generateCacheKey(connectionId, dbName, schemaName, tableName)
    const cached = this.cache.get(key)

    if (!cached) return null

    const isExpired = Date.now() - cached.lastSync > this.maxAge
    if (isExpired) {
      this.cache.delete(key)
      return null
    }

    return cached
  }

  /**
   * 设置缓存
   */
  setCache(
    connectionId: string,
    dbName: string,
    schemaName: string | undefined,
    tableName: string,
    indexes: IndexMeta[],
    constraints: ConstraintMeta[]
  ): void {
    const key = this.generateCacheKey(connectionId, dbName, schemaName, tableName)

    this.cache.set(key, {
      indexes,
      constraints,
      lastSync: Date.now(),
      isValid: true,
    })

    cacheStateManager.markValid(
      { connectionId, databaseName: dbName, schemaName, tableName },
      indexes.length,
      constraints.length
    )
  }

  /**
   * 失效缓存
   */
  invalidateCache(
    connectionId: string,
    dbName: string,
    schemaName: string | undefined,
    tableName: string
  ): void {
    const key = this.generateCacheKey(connectionId, dbName, schemaName, tableName)
    const cached = this.cache.get(key)

    if (cached) {
      cached.isValid = false
      this.cache.delete(key)
    }

    cacheStateManager.markInvalid({
      connectionId,
      databaseName: dbName,
      schemaName,
      tableName,
    })
  }

  /**
   * 清除连接的所有缓存
   */
  clearConnection(connectionId: string): void {
    const keysToRemove: string[] = []

    this.cache.forEach((_, key) => {
      if (key.startsWith(`${connectionId}:`)) {
        keysToRemove.push(key)
      }
    })

    keysToRemove.forEach(key => this.cache.delete(key))
  }

  /**
   * 清除所有缓存
   */
  clearAll(): void {
    this.cache.clear()
  }

  /**
   * 获取缓存统计信息
   */
  getStats(): { total: number; valid: number; invalid: number } {
    const total = this.cache.size
    let valid = 0
    let invalid = 0

    this.cache.forEach(cached => {
      if (cached.isValid) valid++
      else invalid++
    })

    return { total, valid, invalid }
  }
}

export const indexConstraintCacheManager = new IndexConstraintCacheManager()

/**
 * 索引与约束缓存 Composable
 */
export function useIndexConstraintCache() {
  const _navigatorStore = useDatabaseNavigatorStore()

  /**
   * 加载并缓存表的索引和约束
   */
  async function loadAndCacheIndexConstraint(
    connectionId: string,
    connectionType: 'global' | 'project',
    dbName: string,
    schemaName: string | undefined,
    tableName: string,
    _projectPath?: string
  ): Promise<{ indexes: IndexMeta[]; constraints: ConstraintMeta[] }> {
    const cached = indexConstraintCacheManager.getCache(connectionId, dbName, schemaName, tableName)

    if (cached) {
      return { indexes: cached.indexes, constraints: cached.constraints }
    }

    try {
      const { loadIndexes, loadConstraints } = await import('../api/database-api')

      const [indexes, constraints] = await Promise.all([
        loadIndexes(connectionId, dbName, schemaName || '', tableName),
        loadConstraints(connectionId, dbName, schemaName || '', tableName),
      ])

      const indexMetas: IndexMeta[] = (indexes as unknown[]).map((idx: unknown) => {
        const i = idx as Record<string, unknown>
        return {
          id: `${connectionId}:${dbName}:${schemaName || ''}:${tableName}:${i.name}`,
          name: i.name as string,
          tableName,
          columns: (i.columns as string[]) || [],
          isUnique: (i.isUnique as boolean) || false,
          isPrimary: (i.isPrimary as boolean) || false,
          type: (i.type as string) || 'BTREE',
          comment: (i.comment as string) || null,
        }
      })

      const constraintMetas: ConstraintMeta[] = (constraints as unknown[]).map((con: unknown) => {
        const c = con as Record<string, unknown>
        return {
          id: `${connectionId}:${dbName}:${schemaName || ''}:${tableName}:${c.name}`,
          name: c.name as string,
          tableName,
          type: c.type as ConstraintMeta['type'],
          columns: (c.columns as string[]) || [],
          referencedTable: c.referenced_table as string | undefined,
          referencedColumns: c.referenced_columns as string[] | undefined,
          comment: (c.comment as string) || null,
        }
      })

      indexConstraintCacheManager.setCache(
        connectionId,
        dbName,
        schemaName,
        tableName,
        indexMetas,
        constraintMetas
      )

      return { indexes: indexMetas, constraints: constraintMetas }
    } catch (error) {
      console.error('加载索引和约束失败:', error)
      return { indexes: [], constraints: [] }
    }
  }

  /**
   * 刷新表的索引和约束缓存
   */
  async function refreshIndexConstraint(
    connectionId: string,
    connectionType: 'global' | 'project',
    dbName: string,
    schemaName: string | undefined,
    tableName: string,
    projectPath?: string
  ): Promise<{ indexes: IndexMeta[]; constraints: ConstraintMeta[] }> {
    indexConstraintCacheManager.invalidateCache(connectionId, dbName, schemaName, tableName)

    return loadAndCacheIndexConstraint(
      connectionId,
      connectionType,
      dbName,
      schemaName,
      tableName,
      projectPath
    )
  }

  /**
   * 获取表的索引
   */
  function getTableIndexes(
    connectionId: string,
    dbName: string,
    schemaName: string | undefined,
    tableName: string
  ): IndexMeta[] {
    const cached = indexConstraintCacheManager.getCache(connectionId, dbName, schemaName, tableName)

    return cached?.indexes || []
  }

  /**
   * 获取表的约束
   */
  function getTableConstraints(
    connectionId: string,
    dbName: string,
    schemaName: string | undefined,
    tableName: string
  ): ConstraintMeta[] {
    const cached = indexConstraintCacheManager.getCache(connectionId, dbName, schemaName, tableName)

    return cached?.constraints || []
  }

  return {
    loadAndCacheIndexConstraint,
    refreshIndexConstraint,
    getTableIndexes,
    getTableConstraints,
    cacheManager: indexConstraintCacheManager,
  }
}
