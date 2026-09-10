/**
 * Catalog & Schema 加载器
 *
 * 从 database-navigator-store 抽取，负责：
 * - Catalog 列表加载（DB / 缓存 / 离线静默）
 * - Schema 列表加载（DB / 缓存）
 * - Catalog→Schema 树节点更新
 */

import { type Ref } from 'vue'

import * as databaseApi from '../../api/database-api'
import { getMetadataCacheStatus, getTablesFromCache } from '../../services/metadata-cache-service'
import { mutateCatalogNode } from '../../utils/tree-mutation'

import type { CatalogNode, SchemaNode } from '../../types/nav-types'

// ========== Composable ==========

export function useCatalogLoader(
  connectionCatalogs: Ref<Map<string, CatalogNode[]>>,
  connectionTypes: Ref<Map<string, 'global' | 'project'>>,
  connectionProjectPaths: Ref<Map<string, string | undefined>>,
  connectionDbTypes: Ref<Map<string, string>>,
  loadingCatalogs: Ref<Set<string>>,
  loadingSchemas: Ref<Set<string>>,
  lastSyncTimes: Ref<Map<string, number>>,
  nodeErrors: Ref<Map<string, string>>
) {
  // ========== 辅助 ==========

  function setLastSyncTime(connectionId: string, catalogName?: string, schemaName?: string) {
    const key = catalogName
      ? schemaName
        ? `${connectionId}:${catalogName}:${schemaName}`
        : `${connectionId}:${catalogName}`
      : connectionId
    lastSyncTimes.value.set(key, Date.now())
  }

  function triggerReactivity() {
    const currentMap = connectionCatalogs.value
    const newMap = new Map(currentMap)
    connectionCatalogs.value = newMap
  }

  // ========== Catalog ==========

  async function loadCatalogsFromDb(connectionId: string) {
    const connType = connectionTypes.value.get(connectionId) || 'global'
    const projectPath = connectionProjectPaths.value.get(connectionId)

    const catalogMetas = await databaseApi.loadCatalogs(connectionId, connType, projectPath)

    let catalogs: { name: string }[] = catalogMetas.map(d => ({ name: d.name }))

    if (catalogs.length === 0) {
      catalogs = [{ name: 'default' }]
    }

    const newCatalogs: CatalogNode[] = catalogs.map(cat => ({
      name: cat.name,
      schemas: [],
    }))

    const currentMap = connectionCatalogs.value
    const newMap = new Map(currentMap)
    newMap.set(connectionId, newCatalogs)
    connectionCatalogs.value = newMap

    setLastSyncTime(connectionId)
  }

  async function loadCatalogsFromCache(
    connectionId: string,
    connType: 'global' | 'project',
    projectPath?: string
  ): Promise<CatalogNode[]> {
    const tables = await getTablesFromCache(
      connectionId,
      connType,
      'all',
      undefined,
      projectPath
    ).catch(() => [])

    const dbMap = new Map<string, Set<string>>()
    tables.forEach(table => {
      const parts = table.name.split('.')
      if (parts.length >= 2) {
        const catalogName = parts[0]
        const schemaName = parts[1]
        if (!dbMap.has(catalogName)) {
          dbMap.set(catalogName, new Set())
        }
        const schemas = dbMap.get(catalogName)
        if (schemas) {
          schemas.add(schemaName)
        }
      }
    })

    return Array.from(dbMap.entries()).map(([name, schemas]) => ({
      name,
      schemas: Array.from(schemas).map(s => ({ name: s, tables: [], views: [] })),
    }))
  }

  async function loadCatalogs(connectionId: string) {
    if (loadingCatalogs.value.has(connectionId)) return

    loadingCatalogs.value.add(connectionId)

    try {
      const connType = connectionTypes.value.get(connectionId) || 'global'
      const projectPath = connectionProjectPaths.value.get(connectionId)

      const cacheStatus = await getMetadataCacheStatus(
        connectionId,
        connType,
        'all',
        undefined,
        projectPath
      ).catch(() => ({ is_valid: false, last_sync: null, stats: null }))

      if (cacheStatus.is_valid && cacheStatus.stats && cacheStatus.stats.table_count > 0) {
        const catalogs = await loadCatalogsFromCache(connectionId, connType, projectPath)
        if (catalogs.length > 0) {
          const currentMap = connectionCatalogs.value
          const newMap = new Map(currentMap)
          newMap.set(connectionId, catalogs)
          connectionCatalogs.value = newMap
          return
        }
      }

      await loadCatalogsFromDb(connectionId)
    } catch (e) {
      const msg = e instanceof Error ? e.message : '加载 Catalog 列表失败'
      console.error('[catalog-loader] 加载 Catalog 列表失败:', connectionId, e)
      nodeErrors.value.set(connectionId, msg)
      throw e
    } finally {
      loadingCatalogs.value.delete(connectionId)
    }
  }

  /**
   * 静默从 L2 缓存加载 Catalogs（离线模式专用）
   * 不访问数据库，只从本地 SQLite 元数据缓存读取，失败时静默处理
   */
  async function loadCatalogsFromCacheSilent(connectionId: string): Promise<boolean> {
    // 如果已经有缓存数据，不重复加载
    const existing = connectionCatalogs.value.get(connectionId)
    if (existing && existing.length > 0) return true

    try {
      const connType = connectionTypes.value.get(connectionId) || 'global'
      const projectPath = connectionProjectPaths.value.get(connectionId)
      const catalogs = await loadCatalogsFromCache(connectionId, connType, projectPath)

      if (catalogs.length > 0) {
        const currentMap = connectionCatalogs.value
        const newMap = new Map(currentMap)
        newMap.set(connectionId, catalogs)
        connectionCatalogs.value = newMap
        return true
      }
    } catch {
      // 离线模式下静默处理缓存加载失败
    }
    return false
  }

  // ========== Schema ==========

  function updateCatalogSchemas(connectionId: string, catalogName: string, schemas: SchemaNode[]) {
    mutateCatalogNode(connectionCatalogs.value, connectionId, catalogName, cat => {
      cat.schemas = schemas
    })
    triggerReactivity()
  }

  async function loadSchemasFromCache(
    connectionId: string,
    connType: 'global' | 'project',
    catalogName: string,
    projectPath?: string
  ): Promise<SchemaNode[]> {
    const tables = await getTablesFromCache(
      connectionId,
      connType,
      catalogName,
      undefined,
      projectPath
    ).catch(() => [])

    const schemaSet = new Set<string>()
    tables.forEach(table => {
      if (table.schema_name) {
        schemaSet.add(table.schema_name)
      }
    })

    return Array.from(schemaSet).map(name => ({ name, tables: [], views: [] }))
  }

  async function loadSchemasFromDb(connectionId: string, catalogName: string) {
    const connType = connectionTypes.value.get(connectionId) || 'global'
    const projectPath = connectionProjectPaths.value.get(connectionId)

    const schemaMetas = await databaseApi.loadSchemas(
      connectionId,
      catalogName,
      connType,
      projectPath
    )

    let schemas: { name: string }[] = schemaMetas.map(s => ({ name: s.name }))

    if (schemas.length === 0) {
      schemas = [{ name: catalogName }]
    }

    updateCatalogSchemas(
      connectionId,
      catalogName,
      schemas.map(s => ({
        name: s.name,
        tables: [],
        views: [],
      }))
    )

    setLastSyncTime(connectionId, catalogName)
  }

  async function loadSchemas(connectionId: string, catalogName: string) {
    const key = `${connectionId}:${catalogName}`
    if (loadingSchemas.value.has(key)) return

    loadingSchemas.value.add(key)

    try {
      const connType = connectionTypes.value.get(connectionId) || 'global'
      const projectPath = connectionProjectPaths.value.get(connectionId)

      const cacheStatus = await getMetadataCacheStatus(
        connectionId,
        connType,
        catalogName,
        undefined,
        projectPath
      ).catch(() => ({ is_valid: false, last_sync: null, stats: null }))

      if (cacheStatus.is_valid) {
        const schemas = await loadSchemasFromCache(connectionId, connType, catalogName, projectPath)
        if (schemas.length > 0) {
          updateCatalogSchemas(connectionId, catalogName, schemas)
          return
        }
      }

      await loadSchemasFromDb(connectionId, catalogName)
    } catch (e) {
      const msg = e instanceof Error ? e.message : '加载 Schema 列表失败'
      console.error('[catalog-loader] 加载 Schema 列表失败:', key, e)
      nodeErrors.value.set(key, msg)
      // 回退：至少显示一个默认 schema
      mutateCatalogNode(connectionCatalogs.value, connectionId, catalogName, cat => {
        cat.schemas = [{ name: catalogName, tables: [], views: [] }]
      })
      triggerReactivity()
      throw e
    } finally {
      loadingSchemas.value.delete(key)
    }
  }

  return {
    loadCatalogs,
    loadCatalogsFromCache,
    loadCatalogsFromCacheSilent,
    loadSchemas,
  }
}
