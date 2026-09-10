/**
 * Tauri API 封装 — 使用 tauri-specta 生成的 typed commands
 *
 * 所有类型从 bindings.ts 自动生成，命令调用具有完整类型安全。
 * 兼容层提供与旧 API 相同的调用接口。
 *
 * 迁移策略：
 * - typed commands 直接从 bindings.ts 导出，新代码优先使用
 * - 旧 API 对象保留向后兼容，内部逐步迁移到 typed commands
 * - tauriInvoke 保留用于尚未迁移的命令
 */

import type {
  ConnectDatabaseInput,
  ConnectDatabaseResponse,
  ConnectionInfoResponse,
  SqlHistoryResponse,
  ProjectInfoResponse,
  CreateProjectInput,
  UpdateProjectInput,
  RegisterExternalDatabaseInput,
  CreateExternalTableInput,
} from '@/generated/specta/bindings'
import { commands } from '@/generated/specta/bindings'
import type { SchemaObject } from '@/shared/types'

/** 重新导出 typed commands 供新代码直接使用 */
export { commands }

/** 重新导出 specta 生成的类型 */
export type {
  ConnectDatabaseInput,
  ConnectDatabaseResponse,
  ConnectionInfoResponse,
  SqlHistoryResponse,
  ProjectInfoResponse as ProjectInfo,
  CreateProjectInput,
  UpdateProjectInput,
}

/** specta typedError 的 unwrap helper */
export async function typed<T>(
  promise: Promise<{ status: 'ok'; data: T } | { status: 'error'; error: unknown }>
): Promise<T> {
  const result = await promise
  if (result.status === 'ok') return result.data
  throw result.error
}

/** 保留向后兼容的 tauriInvoke 用于尚未迁移的调用 */
export async function tauriInvoke<T = unknown>(
  command: string,
  args: Record<string, unknown> = {}
): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core')
  try {
    return await invoke<T>(command, args)
  } catch (error) {
    console.error(`[TauriInvoke] Command "${command}" failed:`, error)
    throw error
  }
}

// ==================== 连接相关 API ====================

export const connectionApi = {
  /** typed — 创建数据库连接 */
  connectDatabase(input: ConnectDatabaseInput) {
    return typed(commands.connectDatabase(input))
  },

  /** legacy — 断开连接 */
  disconnectDatabase(connId: string) {
    return tauriInvoke<void>('disconnect_database', { conn_id: connId })
  },

  /** typed — 获取所有连接 */
  getConnections() {
    return typed(commands.getConnections())
  },

  /** legacy — 获取单个连接信息 */
  getConnectionInfo(connId: string) {
    return tauriInvoke<ConnectionInfoResponse>('get_connection_info', { conn_id: connId })
  },

  /** typed — 测试连接 */
  testConnection(
    dbType: string,
    url: string,
    networkConfigId?: string | null,
    authConfigId?: string | null,
    authMethod?: string | null
  ) {
    return typed(
      commands.testConnection(
        dbType,
        url,
        networkConfigId ?? null,
        authConfigId ?? null,
        authMethod ?? null
      )
    )
  },
}

// ==================== SQL 执行类型定义 ====================

/** @deprecated QueryResult.columns 已改为 string[]，此类型保留向后兼容 */
export interface QueryColumn {
  name: string
  data_type: string
}

export interface QueryResult {
  columns: string[]
  rows: unknown[]
  total_rows: number
  affected_rows?: number
  is_read_only?: boolean
}

export interface ExecuteSqlInput {
  conn_id?: string
  sql: string
  timeout_ms?: number
}

export interface ExecuteSqlResponse {
  result: QueryResult
  elapsed_ms: number
  affected_rows?: number
  truncated: boolean
  error?: string
}

// ==================== SQL 执行相关 API ====================

export const sqlApi = {
  executeSql(input: ExecuteSqlInput) {
    return tauriInvoke<ExecuteSqlResponse>('execute_sql', { input })
  },

  executeTransaction(connId: string | null, sqls: string[]) {
    return tauriInvoke<{ results: ExecuteSqlResponse[] }>('execute_transaction', {
      input: { conn_id: connId, sqls },
    })
  },

  getSqlHistory(limit?: number) {
    return tauriInvoke<SqlHistoryResponse[]>('get_sql_history', { limit })
  },

  searchSqlHistory(keyword: string, limit?: number) {
    return tauriInvoke<SqlHistoryResponse[]>('search_sql_history', { keyword, limit })
  },

  clearSqlHistory() {
    return tauriInvoke<void>('clear_sql_history')
  },

  removeSqlHistory(id: string) {
    return tauriInvoke<void>('remove_sql_history', { id })
  },
}

// ==================== 导航器相关 API ====================

export interface NavigatorNodeResponse {
  id: string
  node_type: string
  name: string
  parent_id: string | null
  path: string
  depth: number
  is_leaf: boolean
  metadata?: Record<string, unknown>
}

export const navigatorApi = {
  getCatalogs(connId: string) {
    return tauriInvoke<NavigatorNodeResponse[]>('get_catalogs', { conn_id: connId })
  },

  getSchemas(connId: string, catalog: string) {
    return tauriInvoke<NavigatorNodeResponse[]>('get_schemas', {
      conn_id: connId,
      database: catalog,
    })
  },

  getTables(connId: string, catalog: string, schema: string) {
    return tauriInvoke<NavigatorNodeResponse[]>('get_tables', {
      conn_id: connId,
      database: catalog,
      schema,
    })
  },

  getViews(connId: string, catalog: string, schema: string) {
    return tauriInvoke<NavigatorNodeResponse[]>('get_views', {
      conn_id: connId,
      database: catalog,
      schema,
    })
  },

  getColumns(connId: string, catalog: string, schema: string, table: string) {
    return tauriInvoke<NavigatorNodeResponse[]>('get_columns', {
      conn_id: connId,
      database: catalog,
      schema,
      table,
    })
  },

  // 旧版兼容 API
  listCatalogs(connId: string) {
    return tauriInvoke<string[]>('list_catalogs', { conn_id: connId })
  },

  listSchemas(connId: string, catalog: string) {
    return tauriInvoke<string[]>('list_schemas', { conn_id: connId, database: catalog })
  },

  listTables(connId: string, catalog: string, schema?: string) {
    return tauriInvoke<SchemaObject[]>('list_tables', {
      conn_id: connId,
      database: catalog,
      schema,
    })
  },

  listColumns(connId: string, catalog: string, schema: string | null, table: string) {
    return tauriInvoke<SchemaObject[]>('list_columns', {
      conn_id: connId,
      database: catalog,
      schema,
      table,
    })
  },
}

// ==================== 联邦查询相关 API ====================

export const federatedApi = {
  /** typed — 注册外部数据库 */
  registerExternalDatabase(
    connId: string | null,
    name: string,
    driver: string,
    connectionString: string
  ) {
    const input: RegisterExternalDatabaseInput = {
      conn_id: connId,
      name,
      driver,
      connection_string: connectionString,
    }
    return typed(commands.registerExternalDatabase(input))
  },

  /** typed — 创建外部表 */
  createExternalTable(
    connId: string | null,
    externalDbName: string,
    schemaName: string,
    tableName: string,
    externalTableName: string
  ) {
    const input: CreateExternalTableInput = {
      conn_id: connId,
      external_db_name: externalDbName,
      schema_name: schemaName,
      table_name: tableName,
      external_table_name: externalTableName,
    }
    return typed(commands.createExternalTable(input))
  },
}

// ==================== 项目相关 API ====================

/** @deprecated 使用 ProjectInfo 替代 */
export type ProjectResponse = ProjectInfoResponse

export const projectApi = {
  /** typed — 创建并保存项目 */
  createProject(input: CreateProjectInput) {
    return typed(commands.createAndSaveProject(input))
  },

  /** typed — 通过路径打开 */
  openProjectByPath(path: string) {
    return typed(commands.openProjectByPath(path))
  },

  /** typed — 通过 ID 打开 */
  openProjectById(id: string) {
    return typed(commands.openProjectById(id))
  },

  /** typed — 获取最近项目 */
  getRecentProjects() {
    return typed(commands.getRecentProjects(null))
  },

  /** typed — 从最近移除 */
  removeFromRecent(projectId: string) {
    return typed(commands.removeFromRecent(projectId))
  },

  /** typed — 删除项目 */
  deleteProject(projectId: string) {
    return typed(commands.deleteProject(projectId))
  },

  /** typed — 从磁盘删除 */
  deleteProjectDisk(projectId: string) {
    return typed(commands.deleteProjectDisk(projectId))
  },

  /** typed — 更新项目 */
  updateProject(input: UpdateProjectInput) {
    return typed(commands.updateProject(input))
  },
}
