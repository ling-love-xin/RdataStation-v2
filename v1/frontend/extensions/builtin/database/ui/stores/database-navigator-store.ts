import { defineStore } from 'pinia'
import { ref, computed } from 'vue'

import {
  closeConnection,
  executeSql as executeSqlService,
} from '@/extensions/builtin/connection/ui/services/connection'

import * as databaseApi from '../api/database-api'
import { clearMetadataCache } from '../services/metadata-cache-service'
import { useCatalogLoader } from './nav-loaders/use-catalog-loader'
import { useColumnLoader } from './nav-loaders/use-column-loader'
import { useObjectLoader } from './nav-loaders/use-object-loader'
import { useTableLoader } from './nav-loaders/use-table-loader'
import { NodeKeyEncoder } from '../types/virtual-tree'

import type { IntrospectionLevel } from '../api/database-api'
import type {
  CatalogNode,
  SchemaNode,
  TableNode,
  ViewNode,
  ColumnNode,
  IndexNode,
  ConstraintNode,
  ProcedureNode,
  FunctionNode,
  SequenceNode,
  TriggerNode,
  SelectedObject,
  SearchResult,
} from '../types/nav-types'

export const useDatabaseNavigatorStore = defineStore('databaseNavigator', () => {
  // ====================================================================
  //  State — 全部保留，与旧版 store 完全一致
  // ====================================================================

  const connectionCatalogs = ref<Map<string, CatalogNode[]>>(new Map())
  const selectedObject = ref<SelectedObject | null>(null)
  const loadingCatalogs = ref<Set<string>>(new Set())
  const loadingSchemas = ref<Set<string>>(new Set())
  const loadingTables = ref<Set<string>>(new Set())
  const loadingColumns = ref<Set<string>>(new Set())
  const error = ref<string | null>(null)
  const connectionTypes = ref<Map<string, 'global' | 'project'>>(new Map())
  const connectionProjectPaths = ref<Map<string, string | undefined>>(new Map())
  const introspectionLevels = ref<Map<string, IntrospectionLevel>>(new Map())
  const connectionDbTypes = ref<Map<string, string>>(new Map())
  const lastSyncTimes = ref<Map<string, number>>(new Map())
  const syncModes = ref<Map<string, 'full' | 'incremental'>>(new Map())
  const nodeErrors = ref<Map<string, string>>(new Map())
  const warmingInProgress = ref<Set<string>>(new Set())

  // ====================================================================
  //  Sub-loader 实例化 — 传入共享 Ref，由 loader 直接操作
  // ====================================================================

  const catalogLoader = useCatalogLoader(
    connectionCatalogs,
    connectionTypes,
    connectionProjectPaths,
    connectionDbTypes,
    loadingCatalogs,
    loadingSchemas,
    lastSyncTimes,
    nodeErrors
  )

  const tableLoader = useTableLoader(
    connectionCatalogs,
    connectionTypes,
    connectionProjectPaths,
    loadingTables,
    lastSyncTimes,
    nodeErrors
  )

  const objectLoader = useObjectLoader(
    connectionCatalogs,
    connectionTypes,
    connectionProjectPaths,
    nodeErrors
  )

  const columnLoader = useColumnLoader(
    connectionCatalogs,
    connectionTypes,
    connectionProjectPaths,
    loadingColumns,
    nodeErrors
  )

  // ====================================================================
  //  Tree 读取器（纯 getter，不修改数据）
  // ====================================================================

  function getCatalogs(connectionId: string): CatalogNode[] {
    return connectionCatalogs.value.get(connectionId) || []
  }

  function getCatalogSchemas(connectionId: string, catalogName: string): SchemaNode[] {
    const catalogs = connectionCatalogs.value.get(connectionId)
    if (!catalogs) return []
    const cat = catalogs.find(c => c.name === catalogName)
    if (!cat) return []
    return cat.schemas || []
  }

  function getSchemaTables(
    connectionId: string,
    catalogName: string,
    schemaName: string
  ): TableNode[] {
    const catalogs = connectionCatalogs.value.get(connectionId)
    if (!catalogs) return []
    const cat = catalogs.find(c => c.name === catalogName)
    if (!cat) return []
    if (cat.schemas.length === 0) return cat.tables || []
    const schema = cat.schemas.find(s => s.name === schemaName)
    if (!schema) return cat.tables || []
    return schema.tables || []
  }

  function getSchemaViews(
    connectionId: string,
    catalogName: string,
    schemaName: string
  ): ViewNode[] {
    const catalogs = connectionCatalogs.value.get(connectionId)
    if (!catalogs) return []
    const cat = catalogs.find(c => c.name === catalogName)
    if (!cat) return []
    const schema = cat.schemas.find(s => s.name === schemaName)
    if (!schema) return []
    return schema.views || []
  }

  // ====================================================================
  //  同步与缓存元数据
  // ====================================================================

  function getLastSyncTime(
    connectionId: string,
    catalogName?: string,
    schemaName?: string
  ): number {
    const key = catalogName
      ? schemaName
        ? `${connectionId}:${catalogName}:${schemaName}`
        : `${connectionId}:${catalogName}`
      : connectionId
    return lastSyncTimes.value.get(key) || 0
  }

  function setLastSyncTime(connectionId: string, catalogName?: string, schemaName?: string) {
    const key = catalogName
      ? schemaName
        ? `${connectionId}:${catalogName}:${schemaName}`
        : `${connectionId}:${catalogName}`
      : connectionId
    lastSyncTimes.value.set(key, Date.now())
  }

  function setSyncMode(connectionId: string, mode: 'full' | 'incremental') {
    syncModes.value.set(connectionId, mode)
  }

  function getSyncMode(connectionId: string): 'full' | 'incremental' {
    return syncModes.value.get(connectionId) || 'incremental'
  }

  function getNodeError(nodeKey: string): string | null {
    return nodeErrors.value.get(nodeKey) || null
  }

  function setNodeError(nodeKey: string, msg: string) {
    nodeErrors.value.set(nodeKey, msg)
  }

  function clearNodeError(nodeKey: string) {
    nodeErrors.value.delete(nodeKey)
  }

  function clearAllNodeErrors() {
    nodeErrors.value.clear()
  }

  // ====================================================================
  //  Computed 加载状态
  // ====================================================================

  const isLoadingCatalogs = computed(() => {
    return (connectionId: string): boolean => {
      return loadingCatalogs.value.has(connectionId)
    }
  })

  const isLoadingSchemas = computed(() => {
    return (connectionId: string, catalogName: string): boolean => {
      return loadingSchemas.value.has(`${connectionId}:${catalogName}`)
    }
  })

  const isLoadingTables = computed(() => {
    return (connectionId: string, catalogName: string, schemaName: string): boolean => {
      return loadingTables.value.has(`${connectionId}:${catalogName}:${schemaName}`)
    }
  })

  const isLoadingProcedures = computed(() => {
    return (connectionId: string, catalogName: string, schemaName: string): boolean => {
      return objectLoader.loadingProcedures.value.has(
        `${connectionId}:${catalogName}:${schemaName}:procedures`
      )
    }
  })

  const isLoadingFunctions = computed(() => {
    return (connectionId: string, catalogName: string, schemaName: string): boolean => {
      return objectLoader.loadingFunctions.value.has(
        `${connectionId}:${catalogName}:${schemaName}:functions`
      )
    }
  })

  const isLoadingSequences = computed(() => {
    return (connectionId: string, catalogName: string, schemaName: string): boolean => {
      return objectLoader.loadingSequences.value.has(
        `${connectionId}:${catalogName}:${schemaName}:sequences`
      )
    }
  })

  const isLoadingTriggers = computed(() => {
    return (connectionId: string, catalogName: string, schemaName: string): boolean => {
      return objectLoader.loadingTriggers.value.has(
        `${connectionId}:${catalogName}:${schemaName}:triggers`
      )
    }
  })

  // ====================================================================
  //  连接信息
  // ====================================================================

  function setConnectionInfo(
    connectionId: string,
    type: 'global' | 'project',
    projectPath?: string,
    dbType?: string
  ) {
    connectionTypes.value.set(connectionId, type)
    connectionProjectPaths.value.set(connectionId, projectPath)
    if (dbType) {
      connectionDbTypes.value.set(connectionId, dbType)
    }
  }

  function getDbType(connectionId: string): string {
    return connectionDbTypes.value.get(connectionId)?.toLowerCase() || ''
  }

  function getConnectionType(connectionId: string): 'global' | 'project' | undefined {
    return connectionTypes.value.get(connectionId)
  }

  function getProjectPath(connectionId: string): string | undefined {
    return connectionProjectPaths.value.get(connectionId)
  }

  // ====================================================================
  //  Delegated Loading — 全部委派到子 loader
  // ====================================================================

  function loadCatalogs(connectionId: string) {
    return catalogLoader.loadCatalogs(connectionId)
  }

  function loadCatalogsFromCacheSilent(connectionId: string): Promise<boolean> {
    return catalogLoader.loadCatalogsFromCacheSilent(connectionId)
  }

  function loadSchemas(connectionId: string, catalogName: string) {
    return catalogLoader.loadSchemas(connectionId, catalogName)
  }

  function loadTables(connectionId: string, catalogName: string, schemaName: string) {
    return tableLoader.loadTables(connectionId, catalogName, schemaName)
  }

  function loadProcedures(connectionId: string, catalogName: string, schemaName: string) {
    return objectLoader.loadProcedures(connectionId, catalogName, schemaName)
  }

  function loadFunctions(connectionId: string, catalogName: string, schemaName: string) {
    return objectLoader.loadFunctions(connectionId, catalogName, schemaName)
  }

  function loadSequences(connectionId: string, catalogName: string, schemaName: string) {
    return objectLoader.loadSequences(connectionId, catalogName, schemaName)
  }

  function loadTriggers(connectionId: string, catalogName: string, schemaName: string) {
    return objectLoader.loadTriggers(connectionId, catalogName, schemaName)
  }

  function loadColumns(
    connectionId: string,
    catalogName: string,
    schemaName: string,
    tableName: string
  ) {
    return columnLoader.loadColumns(connectionId, catalogName, schemaName, tableName)
  }

  function loadIndexes(
    connectionId: string,
    catalogName: string,
    schemaName: string,
    tableName: string
  ) {
    return columnLoader.loadIndexes(connectionId, catalogName, schemaName, tableName)
  }

  function loadConstraints(
    connectionId: string,
    catalogName: string,
    schemaName: string,
    tableName: string
  ) {
    return columnLoader.loadConstraints(connectionId, catalogName, schemaName, tableName)
  }

  // ====================================================================
  //  Operations — 编排与工具函数
  // ====================================================================

  async function setIntrospectionLevel(
    connectionId: string,
    level: IntrospectionLevel
  ): Promise<void> {
    await databaseApi.setIntrospectionLevel(connectionId, level)
    introspectionLevels.value.set(connectionId, level)
  }

  function getIntrospectionLevel(connectionId: string): IntrospectionLevel {
    return introspectionLevels.value.get(connectionId) || 'level3'
  }

  async function refreshMetadata(connectionId: string, catalogName?: string, schemaName?: string) {
    const connType = connectionTypes.value.get(connectionId) || 'global'
    const projectPath = connectionProjectPaths.value.get(connectionId)

    await clearMetadataCache({
      connection_id: connectionId,
      connection_type: connType,
      database_name: catalogName || 'all',
      schema_name: schemaName,
      project_path: projectPath,
    }).catch(() => {})

    clearCache(connectionId)

    if (catalogName) {
      await loadCatalogs(connectionId)
      await loadSchemas(connectionId, catalogName)
      if (schemaName) {
        await loadTables(connectionId, catalogName, schemaName)
      }
    } else {
      await loadCatalogs(connectionId)
      startCacheWarming(connectionId)
    }
  }

  function searchObjects(query: string, connectionId?: string): SearchResult[] {
    if (!query || query.trim().length === 0) return []

    const results: SearchResult[] = []
    const lowerQuery = query.toLowerCase()
    const MAX_RESULTS = 50

    const catalogsToSearch = connectionId
      ? [[connectionId, connectionCatalogs.value.get(connectionId)] as const]
      : Array.from(connectionCatalogs.value.entries())

    for (const [connId, catalogs] of catalogsToSearch) {
      if (!catalogs) continue

      for (const cat of catalogs) {
        if (results.length >= MAX_RESULTS) return results

        if (cat.name.toLowerCase().includes(lowerQuery)) {
          results.push({
            connectionId: connId,
            type: 'catalog',
            name: cat.name,
            path: cat.name,
            matchType: 'name',
          })
        }

        for (const schema of cat.schemas) {
          if (results.length >= MAX_RESULTS) return results

          if (schema.name.toLowerCase().includes(lowerQuery)) {
            results.push({
              connectionId: connId,
              type: 'schema',
              name: schema.name,
              path: `${cat.name}.${schema.name}`,
              matchType: 'name',
            })
          }

          for (const table of schema.tables) {
            if (results.length >= MAX_RESULTS) return results

            if (table.name.toLowerCase().includes(lowerQuery)) {
              results.push({
                connectionId: connId,
                type: 'table',
                name: table.name,
                path: `${cat.name}.${schema.name}.${table.name}`,
                matchType: 'name',
              })
            }

            // 列搜索仅在列已加载时有效（level1 内省时 columns 为空数组）
            for (const col of table.columns) {
              if (results.length >= MAX_RESULTS) return results
              if (col.name.toLowerCase().includes(lowerQuery)) {
                results.push({
                  connectionId: connId,
                  type: 'column',
                  name: `${table.name}.${col.name}`,
                  path: `${cat.name}.${schema.name}.${table.name}.${col.name}`,
                  matchType: 'name',
                  parentTable: table.name,
                })
              }
              if (col.dataType.toLowerCase().includes(lowerQuery)) {
                results.push({
                  connectionId: connId,
                  type: 'column',
                  name: `${table.name}.${col.name}`,
                  path: `${cat.name}.${schema.name}.${table.name}.${col.name}`,
                  matchType: 'type',
                  parentTable: table.name,
                })
              }
            }
          }

          for (const view of schema.views) {
            if (results.length >= MAX_RESULTS) return results

            if (view.name.toLowerCase().includes(lowerQuery)) {
              results.push({
                connectionId: connId,
                type: 'view',
                name: view.name,
                path: `${cat.name}.${schema.name}.${view.name}`,
                matchType: 'name',
              })
            }

            for (const col of view.columns) {
              if (results.length >= MAX_RESULTS) return results
              if (col.name.toLowerCase().includes(lowerQuery)) {
                results.push({
                  connectionId: connId,
                  type: 'column',
                  name: `${view.name}.${col.name}`,
                  path: `${cat.name}.${schema.name}.${view.name}.${col.name}`,
                  matchType: 'name',
                  parentTable: view.name,
                })
              }
            }
          }
        }
      }
    }

    return results
  }

  function setSelectedObject(object: SelectedObject | null) {
    selectedObject.value = object
  }

  function clearCache(connectionId?: string) {
    if (connectionId) {
      connectionCatalogs.value.delete(connectionId)
    } else {
      connectionCatalogs.value.clear()
    }
  }

  function clearError() {
    error.value = null
  }

  async function disconnectConnection(connectionId: string) {
    await closeConnection(connectionId).catch(() => {})
    clearCache(connectionId)
  }

  async function executeSql(
    connectionId: string,
    _catalogName: string,
    sql: string
  ): Promise<unknown> {
    return await executeSqlService(connectionId, sql)
  }

  function expandToNode(_nodeKey: string): void {
    // V2: 树展开由 database-navigator 组件管理，store 无法直接操作虚拟树。
    // 当前调用方（use-workbench-integration）期望展开到指定节点。
    // 计划通过 ref-based event（expandRequest）让组件 watch 并展开。
    // 参见: use-workbench-integration.ts L70-71
  }

  function selectNode(nodeKey: string): void {
    const parts = NodeKeyEncoder.decode(nodeKey)
    if (parts.length < 2) return

    const nodeType = parts[0]
    const connectionId = parts[1]
    const name = parts[parts.length - 1]

    setSelectedObject({
      name,
      kind: nodeType === 'view' ? 'view' : 'table',
      connectionId,
    } as SelectedObject)
  }

  async function startCacheWarming(connectionId: string): Promise<void> {
    // 防止并发预热同一连接
    if (warmingInProgress.value.has(connectionId)) return

    const catalogs = connectionCatalogs.value.get(connectionId)
    if (!catalogs || catalogs.length === 0) return

    warmingInProgress.value.add(connectionId)

    try {
      const t0 = performance.now()

      const targetCatalogs = catalogs.filter(cat => cat.name !== 'default')
      if (targetCatalogs.length === 0) return

      // Phase 1: 并行加载所有 Catalog 的 Schema
      const schemaResults = await Promise.allSettled(
        targetCatalogs.map(cat => loadSchemas(connectionId, cat.name))
      )

      // Phase 2: 收集所有 Schema，并行加载表
      const tablePromises: Promise<void>[] = []
      for (const cat of targetCatalogs) {
        const schemas = getCatalogSchemas(connectionId, cat.name)
        for (const schema of schemas) {
          if (schema.name === 'default') continue
          tablePromises.push(loadTables(connectionId, cat.name, schema.name).catch(() => {}))
        }
      }
      await Promise.allSettled(tablePromises)

      let colTaskCount = 0
      // Phase 3: 根据内省级别决定是否加载列
      const colLevel = introspectionLevels.value.get(connectionId) || 'level3'
      if (colLevel !== 'level1') {
        const columnPromises: Promise<void>[] = []
        for (const cat of targetCatalogs) {
          const schemas = getCatalogSchemas(connectionId, cat.name)
          for (const schema of schemas) {
            const tables = getSchemaTables(connectionId, cat.name, schema.name)
            for (const table of tables.slice(0, 10)) {
              columnPromises.push(
                loadColumns(connectionId, cat.name, schema.name, table.name).catch(() => {})
              )
            }
          }
        }

        const BATCH_SIZE = 20
        for (let i = 0; i < columnPromises.length; i += BATCH_SIZE) {
          const batch = columnPromises.slice(i, i + BATCH_SIZE)
          await Promise.allSettled(batch)
        }
        colTaskCount = columnPromises.length
      }

      const elapsed = (performance.now() - t0).toFixed(0)
      // eslint-disable-next-line no-console
      console.debug(
        `[CacheWarming] 连接 ${connectionId} 缓存预热完成 ` +
          `(耗时 ${elapsed}ms, Catalog=${targetCatalogs.length}, schema结果=${schemaResults.length}, ` +
          `table任务=${tablePromises.length}, column任务=${colTaskCount})`
      )
    } catch (err) {
      const msg = err instanceof Error ? err.message : '缓存预热失败'
      console.warn(`[CacheWarming] ${connectionId}:`, msg)
    } finally {
      warmingInProgress.value.delete(connectionId)
    }
  }

  // ====================================================================
  //  公共 API
  // ====================================================================

  return {
    connectionCatalogs,
    selectedObject,
    error,
    getCatalogs,
    getCatalogSchemas,
    getSchemaTables,
    getSchemaViews,
    isLoadingCatalogs,
    isLoadingSchemas,
    isLoadingTables,
    isLoadingProcedures,
    isLoadingFunctions,
    isLoadingSequences,
    isLoadingTriggers,
    setConnectionInfo,
    getDbType,
    getConnectionType,
    getProjectPath,
    setIntrospectionLevel,
    getIntrospectionLevel,
    loadCatalogs,
    loadCatalogsFromCacheSilent,
    loadSchemas,
    loadTables,
    loadProcedures,
    loadFunctions,
    loadIndexes,
    loadConstraints,
    loadSequences,
    loadTriggers,
    loadColumns,
    refreshMetadata,
    searchObjects,
    setSelectedObject,
    clearCache,
    clearError,
    disconnectConnection,
    executeSql,
    expandToNode,
    selectNode,
    startCacheWarming,
    getLastSyncTime,
    setLastSyncTime,
    setSyncMode,
    getSyncMode,
    getNodeError,
    setNodeError,
    clearNodeError,
    clearAllNodeErrors,
  }
})

// ====================================================================
//  Type Re-exports（统一从 nav-types 导出，消除类型冲突）
// ====================================================================

export type {
  CatalogNode,
  SchemaNode,
  TableNode,
  ViewNode,
  ColumnNode,
  IndexNode,
  ConstraintNode,
  ProcedureNode,
  FunctionNode,
  SequenceNode,
  TriggerNode,
  SelectedObject,
  SearchResult,
}
