/**
 * Column / Index / Constraint 加载器
 *
 * 从 database-navigator-store 抽取，负责：
 * - 列信息加载（DB / 缓存）
 * - 索引 / 约束列表加载
 * - Table 节点字段更新（columns / indexes / constraints）
 */

import { type Ref } from 'vue'

import * as databaseApi from '../../api/database-api'
import {
  getMetadataCacheStatus,
  getColumnsFromCache,
  saveColumnsBatchToCache,
} from '../../services/metadata-cache-service'
import { mutateTreeNode } from '../../utils/tree-mutation'

import type { ColumnInput } from '../../services/metadata-cache-service'
import type {
  CatalogNode,
  TableNode,
  ColumnNode,
  IndexNode,
  ConstraintNode,
} from '../../types/nav-types'

// ========== Composable ==========

export function useColumnLoader(
  connectionCatalogs: Ref<Map<string, CatalogNode[]>>,
  connectionTypes: Ref<Map<string, 'global' | 'project'>>,
  connectionProjectPaths: Ref<Map<string, string | undefined>>,
  loadingColumns: Ref<Set<string>>,
  nodeErrors: Ref<Map<string, string>>
) {
  // ========== 内部：更新 Table 的 columns ==========

  function updateTableColumns(
    connectionId: string,
    catalogName: string,
    schemaName: string,
    tableName: string,
    columns: ColumnNode[]
  ) {
    const found = mutateTreeNode(
      connectionCatalogs.value,
      connectionId,
      { catalogName, schemaName, tableName },
      table => {
        ;(table as TableNode).columns = columns
      }
    )

    if (!found) {
      // 尝试 Views 回退
      const catalogs = connectionCatalogs.value.get(connectionId)
      if (catalogs) {
        const cat = catalogs.find(c => c.name === catalogName)
        if (cat) {
          const schema = cat.schemas.find(s => s.name === schemaName)
          if (schema) {
            const view = schema.views.find(v => v.name === tableName)
            if (view) {
              view.columns = columns
            }
          }
        }
      }
    }
  }

  // ========== 内部：从 DB 加载 Columns ==========

  async function loadColumnsFromDb(
    connectionId: string,
    catalogName: string,
    schemaName: string,
    tableName: string
  ) {
    const connType = connectionTypes.value.get(connectionId) || 'global'
    const projectPath = connectionProjectPaths.value.get(connectionId)

    const columnMetas = await databaseApi.loadColumns(
      connectionId,
      catalogName,
      schemaName,
      tableName,
      connType,
      projectPath
    )

    const columns = columnMetas.map(col => ({
      name: col.name,
      data_type: col.dataType,
      nullable: col.isNullable,
      default_value: col.defaultValue || undefined,
      is_primary_key: col.isPrimaryKey || false,
    }))

    const columnInputs: ColumnInput[] = columns.map(
      (col: {
        name: string
        data_type: string
        nullable: boolean
        default_value: string | undefined
        is_primary_key: boolean
      }) => ({
        id: `${connectionId}:${catalogName}:${schemaName}:${tableName}:${col.name}`,
        name: col.name,
        data_type: col.data_type,
        is_nullable: col.nullable,
        is_primary: col.is_primary_key,
        is_unique: false,
      })
    )

    if (columnInputs.length > 0) {
      try {
        await saveColumnsBatchToCache(
          connectionId,
          connType,
          projectPath,
          catalogName,
          schemaName,
          tableName,
          columnInputs
        )
      } catch (err) {
        console.warn('保存列缓存失败（非致命）:', err)
      }
    }

    updateTableColumns(
      connectionId,
      catalogName,
      schemaName,
      tableName,
      columns.map(
        (col: {
          name: string
          data_type: string
          nullable: boolean
          default_value: string | undefined
          is_primary_key: boolean
        }) => ({
          name: col.name,
          dataType: col.data_type,
          nullable: col.nullable,
          defaultValue: col.default_value ?? undefined,
          isPrimaryKey: col.is_primary_key,
        })
      )
    )
  }

  // ========== 公开：加载 Columns ==========

  async function loadColumns(
    connectionId: string,
    catalogName: string,
    schemaName: string,
    tableName: string
  ) {
    const key = `${connectionId}:${catalogName}:${schemaName}:${tableName}`
    if (loadingColumns.value.has(key)) return

    loadingColumns.value.add(key)

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

      if (cacheStatus.is_valid) {
        const columns = await getColumnsFromCache(
          connectionId,
          connType,
          catalogName,
          schemaName,
          tableName,
          projectPath
        ).catch(() => [])

        if (columns.length > 0) {
          updateTableColumns(
            connectionId,
            catalogName,
            schemaName,
            tableName,
            columns.map(c => ({
              name: c.name,
              dataType: c.data_type,
              nullable: c.is_nullable,
              defaultValue: undefined,
              isPrimaryKey: c.is_primary,
            }))
          )
          return
        }
      }

      await loadColumnsFromDb(connectionId, catalogName, schemaName, tableName)
    } catch (e) {
      const msg = e instanceof Error ? e.message : '加载列信息失败'
      console.error('[column-loader] 加载列信息失败:', key, e)
      nodeErrors.value.set(key, msg)
      throw e
    } finally {
      loadingColumns.value.delete(key)
    }
  }

  // ========== 公开：加载 Indexes ==========

  async function loadIndexes(
    connectionId: string,
    catalogName: string,
    schemaName: string,
    tableName: string
  ) {
    const key = `${connectionId}:${catalogName}:${schemaName}:${tableName}:indexes`
    if (loadingColumns.value.has(key)) return

    loadingColumns.value.add(key)

    try {
      const connType = connectionTypes.value.get(connectionId) || 'global'
      const projectPath = connectionProjectPaths.value.get(connectionId)

      const indexMetas = await databaseApi.loadIndexes(
        connectionId,
        catalogName,
        schemaName,
        tableName,
        connType,
        projectPath
      )
      const indexes: IndexNode[] = indexMetas.map(
        (idx: { name: string; columnNames: string[]; isUnique: boolean; isPrimary: boolean }) => ({
          name: idx.name,
          columns: idx.columnNames || [],
          isUnique: idx.isUnique || false,
          isPrimary: idx.isPrimary || false,
        })
      )

      mutateTreeNode(
        connectionCatalogs.value,
        connectionId,
        { catalogName, schemaName, tableName },
        table => {
          ;(table as TableNode).indexes = indexes
        }
      )
    } catch (e) {
      const msg = e instanceof Error ? e.message : '加载索引列表失败'
      console.error('[column-loader] 加载索引列表失败:', key, e)
      nodeErrors.value.set(key, msg)
      throw e
    } finally {
      loadingColumns.value.delete(key)
    }
  }

  // ========== 公开：加载 Constraints ==========

  async function loadConstraints(
    connectionId: string,
    catalogName: string,
    schemaName: string,
    tableName: string
  ) {
    const key = `${connectionId}:${catalogName}:${schemaName}:${tableName}:constraints`
    if (loadingColumns.value.has(key)) return

    loadingColumns.value.add(key)

    try {
      const connType = connectionTypes.value.get(connectionId) || 'global'
      const projectPath = connectionProjectPaths.value.get(connectionId)

      const constraintMetas = await databaseApi.loadConstraints(
        connectionId,
        catalogName,
        schemaName,
        tableName,
        connType,
        projectPath
      )
      const constraints: ConstraintNode[] = constraintMetas.map(
        (c: { name: string; constraintType: string; columnNames: string[] }) => ({
          name: c.name,
          type: c.constraintType as ConstraintNode['type'],
          columns: c.columnNames || [],
        })
      )

      mutateTreeNode(
        connectionCatalogs.value,
        connectionId,
        { catalogName, schemaName, tableName },
        table => {
          ;(table as TableNode).constraints = constraints
        }
      )
    } catch (e) {
      const msg = e instanceof Error ? e.message : '加载约束列表失败'
      console.error('[column-loader] 加载约束列表失败:', key, e)
      nodeErrors.value.set(key, msg)
      throw e
    } finally {
      loadingColumns.value.delete(key)
    }
  }

  return {
    loadColumns,
    loadIndexes,
    loadConstraints,
  }
}
