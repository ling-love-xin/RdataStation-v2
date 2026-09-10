/**
 * 缓存刷新流程
 *
 * 实现完整的缓存刷新流程：
 * 1. 清除旧缓存（后端）
 * 2. 从数据库获取元数据（后端数据库驱动）
 * 3. 批量保存新缓存（后端）
 *
 * 遵循架构规范：前端只负责调度，不实现业务逻辑
 */

import { cacheStateManager } from './use-cache-state'
import {
  refreshMetadataCache,
  saveTablesBatchToCache,
  saveColumnsBatchToCache,
} from '../services/metadata-cache-service'

import type { TableInput, ColumnInput } from '../services/metadata-cache-service'

/**
 * 缓存刷新结果
 */
export interface CacheRefreshResult {
  tablesCount: number
  columnsCount: number
  success: boolean
  error?: string
}

/**
 * 完整的缓存刷新流程
 *
 * @param connectionId 连接 ID
 * @param connectionType 连接类型
 * @param dbName 数据库名
 * @param schemaName Schema 名
 * @param projectPath 项目路径（项目连接需要）
 * @param fetchTablesFn 获取表列表的函数（由 store 提供）
 * @param fetchColumnsFn 获取列列表的函数（由 store 提供）
 */
export async function refreshCacheComplete(
  connectionId: string,
  connectionType: 'global' | 'project',
  dbName: string,
  schemaName: string,
  projectPath: string | undefined,
  fetchTablesFn: () => Promise<Array<{ name: string }>>,
  fetchColumnsFn: (tableName: string) => Promise<
    Array<{
      name: string
      data_type: string
      nullable: boolean
      is_primary_key: boolean
    }>
  >
): Promise<CacheRefreshResult> {
  try {
    await refreshMetadataCache({
      connection_id: connectionId,
      connection_type: connectionType,
      database_name: dbName,
      schema_name: schemaName,
      project_path: projectPath,
    })

    const tables = await fetchTablesFn()
    const tableInputs: TableInput[] = tables.map(t => ({
      id: `${connectionId}:${dbName}:${schemaName}:${t.name}`,
      name: t.name,
      comment: undefined,
    }))

    let tablesSaved = 0
    let columnsSaved = 0

    if (tableInputs.length > 0) {
      tablesSaved = await saveTablesBatchToCache(
        connectionId,
        connectionType,
        projectPath,
        dbName,
        schemaName,
        tableInputs
      )

      for (const table of tables) {
        const columns = await fetchColumnsFn(table.name)
        const columnInputs: ColumnInput[] = columns.map(c => ({
          id: `${connectionId}:${dbName}:${schemaName}:${table.name}:${c.name}`,
          name: c.name,
          data_type: c.data_type,
          is_nullable: c.nullable,
          is_primary: c.is_primary_key,
          is_unique: false,
        }))

        if (columnInputs.length > 0) {
          const saved = await saveColumnsBatchToCache(
            connectionId,
            connectionType,
            projectPath,
            dbName,
            schemaName,
            table.name,
            columnInputs
          )
          columnsSaved += saved
        }
      }
    }

    cacheStateManager.markValid(
      { connectionId, databaseName: dbName, schemaName },
      tablesSaved,
      columnsSaved
    )

    return {
      tablesCount: tablesSaved,
      columnsCount: columnsSaved,
      success: true,
    }
  } catch (error) {
    cacheStateManager.markInvalid({
      connectionId,
      databaseName: dbName,
      schemaName,
    })

    return {
      tablesCount: 0,
      columnsCount: 0,
      success: false,
      error: error instanceof Error ? error.message : String(error),
    }
  }
}

/**
 * 刷新单个数据库的缓存
 */
export async function refreshDatabaseCache(
  connectionId: string,
  connectionType: 'global' | 'project',
  dbName: string,
  projectPath: string | undefined,
  fetchSchemasFn: () => Promise<Array<{ name: string }>>,
  fetchTablesFn: (schemaName: string) => Promise<Array<{ name: string }>>,
  fetchColumnsFn: (
    schemaName: string,
    tableName: string
  ) => Promise<
    Array<{
      name: string
      data_type: string
      nullable: boolean
      is_primary_key: boolean
    }>
  >
): Promise<{ success: boolean; errors: string[] }> {
  const errors: string[] = []
  const schemas = await fetchSchemasFn()

  for (const schema of schemas) {
    try {
      await refreshCacheComplete(
        connectionId,
        connectionType,
        dbName,
        schema.name,
        projectPath,
        () => fetchTablesFn(schema.name),
        tableName => fetchColumnsFn(schema.name, tableName)
      )
    } catch (error) {
      errors.push(`刷新 ${dbName}.${schema.name} 失败: ${error}`)
    }
  }

  return {
    success: errors.length === 0,
    errors,
  }
}

/**
 * 刷新单个表的缓存
 */
export async function refreshTableCache(
  connectionId: string,
  connectionType: 'global' | 'project',
  dbName: string,
  schemaName: string,
  tableName: string,
  projectPath: string | undefined,
  fetchColumnsFn: () => Promise<
    Array<{
      name: string
      data_type: string
      nullable: boolean
      is_primary_key: boolean
    }>
  >
): Promise<CacheRefreshResult> {
  try {
    const columns = await fetchColumnsFn()
    const columnInputs: ColumnInput[] = columns.map(c => ({
      id: `${Date.now()}-${Math.random().toString(36).slice(2, 9)}`,
      name: c.name,
      data_type: c.data_type,
      is_nullable: c.nullable,
      is_primary: c.is_primary_key,
      is_unique: false,
    }))

    let columnsSaved = 0
    if (columnInputs.length > 0) {
      columnsSaved = await saveColumnsBatchToCache(
        connectionId,
        connectionType,
        projectPath,
        dbName,
        schemaName,
        tableName,
        columnInputs
      )
    }

    return {
      tablesCount: 1,
      columnsCount: columnsSaved,
      success: true,
    }
  } catch (error) {
    return {
      tablesCount: 0,
      columnsCount: 0,
      success: false,
      error: error instanceof Error ? error.message : String(error),
    }
  }
}
