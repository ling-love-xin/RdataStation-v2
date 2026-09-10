/**
 * Table & View 加载器
 *
 * 从 database-navigator-store 抽取，负责：
 * - 表/视图列表加载（DB / 缓存）
 * - Schema→Table 树节点更新
 * - 统计信息计算（computeSchemaStats）
 *
 * 注意：loadViews 仅为 loadTables 的包装器，不单独导出；
 *       视图已在 loadTables → loadTablesFromDb 中与表合并加载。
 */

import { type Ref } from 'vue'

import * as databaseApi from '../../api/database-api'
import {
  getMetadataCacheStatus,
  getTablesFromCache,
  saveTablesBatchToCache,
} from '../../services/metadata-cache-service'
import { mutateTreeNode, mutateCatalogNode } from '../../utils/tree-mutation'

import type { TableInput } from '../../services/metadata-cache-service'
import type { CatalogNode, SchemaNode, TableNode } from '../../types/nav-types'

// ========== Composable ==========

export function useTableLoader(
  connectionCatalogs: Ref<Map<string, CatalogNode[]>>,
  connectionTypes: Ref<Map<string, 'global' | 'project'>>,
  connectionProjectPaths: Ref<Map<string, string | undefined>>,
  loadingTables: Ref<Set<string>>,
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

  // ========== Schema 统计 ==========

  function computeSchemaStats(schema: SchemaNode) {
    schema.totalTables = schema.tables.filter(t => t.type === 'table' || !t.type).length
    schema.totalViews = schema.views.length

    let totalSize = 0
    let totalRows = 0
    for (const t of schema.tables) {
      if (t.dataLength) totalSize += t.dataLength
      if (t.indexLength) totalSize += t.indexLength
      if (t.rowCount) totalRows += t.rowCount
    }
    if (totalSize > 0) schema.totalSizeBytes = totalSize
    if (totalRows > 0) schema.rowCountTotal = totalRows
  }

  // ========== 内部：更新 Schema 下的 Table ==========

  function updateSchemaTables(
    connectionId: string,
    catalogName: string,
    schemaName: string,
    tables: TableNode[]
  ) {
    const catalogs = connectionCatalogs.value.get(connectionId)
    if (!catalogs) {
      console.warn(`未找到 connection: ${connectionId}`)
      return
    }

    const cat = catalogs.find(c => c.name === catalogName)
    if (!cat) {
      console.warn(`未找到 catalog: ${catalogName}`)
      return
    }

    // 无 Schema 的数据库（MySQL 等）：表直接存储在 CatalogNode.tables 上
    if (cat.schemas.length === 0) {
      mutateCatalogNode(connectionCatalogs.value, connectionId, catalogName, c => {
        c.tables = tables
      })
    } else {
      const found = mutateTreeNode(
        connectionCatalogs.value,
        connectionId,
        { catalogName, schemaName },
        schema => {
          ;(schema as SchemaNode).tables = tables
          computeSchemaStats(schema as SchemaNode)
        }
      )
      if (!found) {
        console.warn(`未找到 schema: ${schemaName}，回退到 catalog.tables`)
        mutateCatalogNode(connectionCatalogs.value, connectionId, catalogName, c => {
          c.tables = tables
        })
      }
    }

    triggerReactivity()
  }

  // ========== 内部：从 DB 加载 Table ==========

  async function loadTablesFromDb(connectionId: string, catalogName: string, schemaName: string) {
    const connType = connectionTypes.value.get(connectionId) || 'global'
    const projectPath = connectionProjectPaths.value.get(connectionId)

    try {
      const [tableMetas, viewMetas] = await Promise.all([
        databaseApi.loadTables(connectionId, catalogName, schemaName, connType, projectPath),
        databaseApi.loadViews(connectionId, catalogName, schemaName, connType, projectPath),
      ])

      const allTables = tableMetas.map(t => ({ name: t.name, type: t.type || 'table' }))
      const allViews = viewMetas.map(v => ({ name: v.name, type: 'view' }))

      const merged = [...allTables, ...allViews]

      updateSchemaTables(
        connectionId,
        catalogName,
        schemaName,
        merged.map(t => ({ name: t.name, type: t.type, columns: [] }))
      )

      const tableInputs: TableInput[] = merged.map(t => ({
        id: `${connectionId}:${catalogName}:${schemaName}:${t.name}`,
        name: t.name,
        comment: undefined,
      }))

      if (tableInputs.length > 0) {
        try {
          await saveTablesBatchToCache(
            connectionId,
            connType,
            projectPath,
            catalogName,
            schemaName,
            tableInputs
          )
        } catch (err) {
          console.warn('保存表缓存失败（非致命）:', err)
        }
      }

      setLastSyncTime(connectionId, catalogName, schemaName)
    } catch (err) {
      const msg = err instanceof Error ? err.message : '从数据库加载表失败'
      const key = `${connectionId}:${catalogName}:${schemaName}`
      console.error('[table-loader] loadTablesFromDb 失败:', key, err)
      nodeErrors.value.set(key, msg)
      throw err
    }
  }

  // ========== 公开：加载 Table（含 Table + View） ==========

  async function loadTables(connectionId: string, catalogName: string, schemaName: string) {
    const key = `${connectionId}:${catalogName}:${schemaName}`
    if (loadingTables.value.has(key)) return

    loadingTables.value.add(key)

    try {
      const connType = connectionTypes.value.get(connectionId) || 'global'
      const projectPath = connectionProjectPaths.value.get(connectionId)

      const cacheStatus = await getMetadataCacheStatus(
        connectionId,
        connType,
        catalogName,
        schemaName,
        projectPath
      ).catch(() => ({ is_valid: false, last_sync: null, stats: null }))

      if (cacheStatus.is_valid && cacheStatus.stats && cacheStatus.stats.table_count > 0) {
        const tables = await getTablesFromCache(
          connectionId,
          connType,
          catalogName,
          schemaName,
          projectPath
        )
        if (tables.length > 0) {
          updateSchemaTables(
            connectionId,
            catalogName,
            schemaName,
            tables.map(t => ({
              name: t.name,
              type: 'table',
              columns: [],
              rowCount: t.rowCountEstimate ?? null,
              dataLength: t.dataLength ?? null,
              indexLength: t.indexLength ?? null,
            }))
          )
          return
        }
      }

      await loadTablesFromDb(connectionId, catalogName, schemaName)
    } catch (e) {
      const msg = e instanceof Error ? e.message : '加载表列表失败'
      console.error('[table-loader] 加载表列表失败:', key, e)
      nodeErrors.value.set(key, msg)
      throw e
    } finally {
      loadingTables.value.delete(key)
    }
  }

  return {
    loadTables,
    computeSchemaStats,
  }
}
