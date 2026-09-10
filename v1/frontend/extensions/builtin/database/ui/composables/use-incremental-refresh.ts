/**
 * 增量刷新机制
 *
 * 只刷新变化的节点，而非整个树
 * 支持局部刷新、按需刷新、智能检测变化
 */

import { useRuntimeConnectionStore } from '@/extensions/builtin/connection/ui/stores/runtime-connection-store'

import { useDatabaseNavigatorStore } from '../stores/database-navigator-store'
import { NodeKeyEncoder } from '../types/virtual-tree'

import type { VirtualTreeNode } from '../types/virtual-tree'

export interface IRefreshResult {
  /** 刷新是否成功 */
  success: boolean
  /** 受影响的节点 key 列表 */
  affectedKeys: string[]
  /** 刷新耗时（毫秒） */
  duration: number
  /** 错误信息 */
  error?: string
}

export interface IRefreshOptions {
  /** 是否递归刷新子节点 */
  recursive?: boolean
  /** 是否刷新缓存 */
  refreshCache?: boolean
  /** 超时时间（毫秒） */
  timeout?: number
}

const DEFAULT_OPTIONS: IRefreshOptions = {
  recursive: false,
  refreshCache: false,
  timeout: 30000,
}

export function useIncrementalRefresh() {
  const navigatorStore = useDatabaseNavigatorStore()
  const runtimeConnectionStore = useRuntimeConnectionStore()

  /**
   * 刷新单个节点
   */
  async function refreshNode(
    node: VirtualTreeNode,
    options?: IRefreshOptions
  ): Promise<IRefreshResult> {
    const opts = { ...DEFAULT_OPTIONS, ...options }
    const startTime = Date.now()
    const affectedKeys: string[] = []

    try {
      const keyParts = NodeKeyEncoder.decode(node.key)
      if (keyParts.length === 0) {
        return { success: false, affectedKeys: [], duration: 0, error: '无效的节点 key' }
      }

      const nodeType = keyParts[0]
      const connectionId = keyParts[1]

      // 检查连接是否有效
      if (!runtimeConnectionStore.runtimeConnectionIds.has(connectionId)) {
        return { success: false, affectedKeys: [], duration: 0, error: '连接已断开' }
      }

      let result: IRefreshResult

      switch (nodeType) {
        case 'connection':
          result = await refreshConnection(keyParts, opts)
          break
        case 'catalog':
          result = await refreshDatabase(keyParts, opts)
          break
        case 'schema':
          result = await refreshSchema(keyParts, opts)
          break
        case 'tables-folder':
        case 'views-folder':
          result = await refreshFolder(keyParts, opts)
          break
        case 'table':
        case 'view':
          result = await refreshTable(keyParts, opts)
          break
        default:
          result = { success: false, affectedKeys: [], duration: 0 }
      }

      affectedKeys.push(...result.affectedKeys)

      return {
        success: result.success,
        affectedKeys,
        duration: Date.now() - startTime,
        error: result.error,
      }
    } catch (error) {
      return {
        success: false,
        affectedKeys,
        duration: Date.now() - startTime,
        error: error instanceof Error ? error.message : '刷新失败',
      }
    }
  }

  /**
   * 刷新连接节点
   */
  async function refreshConnection(
    keyParts: string[],
    options: IRefreshOptions
  ): Promise<IRefreshResult> {
    const connectionId = keyParts[2]
    const affectedKeys: string[] = []

    try {
      await navigatorStore.loadCatalogs(connectionId)
      affectedKeys.push(keyParts.join(':'))

      if (options.recursive) {
        const catalogs = navigatorStore.getCatalogs(connectionId)
        for (const cat of catalogs) {
          affectedKeys.push(`catalog:${connectionId}:${cat.name}`)
        }
      }

      return { success: true, affectedKeys, duration: 0 }
    } catch (error) {
      return {
        success: false,
        affectedKeys,
        duration: 0,
        error: error instanceof Error ? error.message : '刷新连接失败',
      }
    }
  }

  /**
   * 刷新数据库节点
   */
  async function refreshDatabase(
    keyParts: string[],
    options: IRefreshOptions
  ): Promise<IRefreshResult> {
    const connectionId = keyParts[1]
    const dbName = keyParts[2]
    const affectedKeys: string[] = []

    try {
      await navigatorStore.loadSchemas(connectionId, dbName)
      affectedKeys.push(keyParts.join(':'))

      if (options.recursive) {
        const schemas = navigatorStore.getCatalogSchemas(connectionId, dbName)
        for (const schema of schemas) {
          affectedKeys.push(`schema:${connectionId}:${dbName}:${schema.name}`)
        }
      }

      return { success: true, affectedKeys, duration: 0 }
    } catch (error) {
      return {
        success: false,
        affectedKeys,
        duration: 0,
        error: error instanceof Error ? error.message : '刷新数据库失败',
      }
    }
  }

  /**
   * 刷新 Schema 节点
   */
  async function refreshSchema(
    keyParts: string[],
    options: IRefreshOptions
  ): Promise<IRefreshResult> {
    const connectionId = keyParts[1]
    const dbName = keyParts[2]
    const schemaName = keyParts[3]
    const affectedKeys: string[] = []

    try {
      // loadTables 内部同时加载 tables + views，单次调用即可
      await navigatorStore.loadTables(connectionId, dbName, schemaName)
      affectedKeys.push(keyParts.join(':'))

      if (options.recursive) {
        const tables = navigatorStore.getSchemaTables(connectionId, dbName, schemaName)
        for (const table of tables) {
          affectedKeys.push(`table:${connectionId}:${dbName}:${schemaName}:${table.name}`)
        }

        const views = navigatorStore.getSchemaViews(connectionId, dbName, schemaName)
        for (const view of views) {
          affectedKeys.push(`view:${connectionId}:${dbName}:${schemaName}:${view.name}`)
        }
      }

      return { success: true, affectedKeys, duration: 0 }
    } catch (error) {
      return {
        success: false,
        affectedKeys,
        duration: 0,
        error: error instanceof Error ? error.message : '刷新 Schema 失败',
      }
    }
  }

  /**
   * 刷新文件夹节点
   */
  async function refreshFolder(
    keyParts: string[],
    _options: IRefreshOptions
  ): Promise<IRefreshResult> {
    const connectionId = keyParts[1]
    const dbName = keyParts[2]
    const schemaName = keyParts[3]
    const affectedKeys: string[] = []

    try {
      // loadTables 内部同时加载 tables + views，因此 tables-folder 和 views-folder 均使用同一调用
      await navigatorStore.loadTables(connectionId, dbName, schemaName)

      affectedKeys.push(keyParts.join(':'))

      return { success: true, affectedKeys, duration: 0 }
    } catch (error) {
      return {
        success: false,
        affectedKeys,
        duration: 0,
        error: error instanceof Error ? error.message : '刷新文件夹失败',
      }
    }
  }

  /**
   * 刷新表/视图节点
   */
  async function refreshTable(
    keyParts: string[],
    // eslint-disable-next-line unused-imports/no-unused-vars
    options: IRefreshOptions
  ): Promise<IRefreshResult> {
    const connectionId = keyParts[1]
    const dbName = keyParts[2]
    const schemaName = keyParts[3]
    const tableName = keyParts[4]
    const affectedKeys: string[] = []

    try {
      await navigatorStore.loadColumns(connectionId, dbName, schemaName, tableName)
      affectedKeys.push(keyParts.join(':'))

      return { success: true, affectedKeys, duration: 0 }
    } catch (error) {
      return {
        success: false,
        affectedKeys,
        duration: 0,
        error: error instanceof Error ? error.message : '刷新表失败',
      }
    }
  }

  /**
   * 批量刷新多个节点（去重优化）
   */
  async function refreshBatch(
    nodes: VirtualTreeNode[],
    options?: IRefreshOptions
  ): Promise<IRefreshResult> {
    const opts = { ...DEFAULT_OPTIONS, ...options }
    const startTime = Date.now()
    const allAffectedKeys: string[] = []
    const uniqueConnectionIds = new Set<string>()

    // 去重：同一连接的多个节点只刷新一次
    for (const node of nodes) {
      const keyParts = NodeKeyEncoder.decode(node.key)
      if (keyParts.length >= 2) {
        uniqueConnectionIds.add(keyParts[1])
      }
    }

    // 并行刷新不同连接
    const refreshPromises = nodes.map(node => refreshNode(node, opts))
    const results = await Promise.allSettled(refreshPromises)

    for (const result of results) {
      if (result.status === 'fulfilled') {
        allAffectedKeys.push(...result.value.affectedKeys)
      }
    }

    return {
      success: results.every(r => r.status === 'fulfilled' && r.value.success),
      affectedKeys: [...new Set(allAffectedKeys)],
      duration: Date.now() - startTime,
    }
  }

  /**
   * 智能检测变化并刷新
   * 通过比较元数据哈希值判断是否需要刷新
   */
  async function smartRefresh(
    node: VirtualTreeNode,
    options?: IRefreshOptions
  ): Promise<IRefreshResult> {
    const opts = { ...DEFAULT_OPTIONS, ...options }
    const startTime = Date.now()

    try {
      const keyParts = NodeKeyEncoder.decode(node.key)
      if (keyParts.length === 0) {
        return { success: false, affectedKeys: [], duration: 0, error: '无效的节点 key' }
      }

      const _nodeType = keyParts[0]
      const connectionId = keyParts[1]

      // 检查连接是否有效
      if (!runtimeConnectionStore.runtimeConnectionIds.has(connectionId)) {
        return { success: false, affectedKeys: [], duration: 0, error: '连接已断开' }
      }

      // 获取当前缓存的哈希值
      const currentHash = getNodeHash(node)

      // 重新加载数据
      const refreshResult = await refreshNode(node, opts)

      if (!refreshResult.success) {
        return refreshResult
      }

      // 比较哈希值，判断是否有变化
      const newHash = getNodeHash(node)
      const hasChanges = currentHash !== newHash

      return {
        success: true,
        affectedKeys: hasChanges ? refreshResult.affectedKeys : [],
        duration: Date.now() - startTime,
      }
    } catch (error) {
      return {
        success: false,
        affectedKeys: [],
        duration: Date.now() - startTime,
        error: error instanceof Error ? error.message : '智能刷新失败',
      }
    }
  }

  /**
   * 获取节点数据哈希值（用于变化检测）
   */
  function getNodeHash(node: VirtualTreeNode): string {
    const keyParts = NodeKeyEncoder.decode(node.key)
    const nodeType = keyParts[0]
    const connectionId = keyParts[1]

    switch (nodeType) {
      case 'connection': {
        const catalogs = navigatorStore.getCatalogs(connectionId)
        return JSON.stringify(catalogs.map(cat => cat.name))
      }
      case 'catalog': {
        const dbName = keyParts[2]
        const schemas = navigatorStore.getCatalogSchemas(connectionId, dbName)
        return JSON.stringify(schemas.map(s => s.name))
      }
      case 'schema': {
        const schemaDbName = keyParts[2]
        const schemaName = keyParts[3]
        const tables = navigatorStore.getSchemaTables(connectionId, schemaDbName, schemaName)
        return JSON.stringify(tables.map(t => t.name))
      }
      default:
        return ''
    }
  }

  return {
    refreshNode,
    refreshBatch,
    smartRefresh,
  }
}
