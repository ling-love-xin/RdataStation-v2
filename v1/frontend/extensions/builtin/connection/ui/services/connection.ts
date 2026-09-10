/**
 * 连接服务
 *
 * 提供运行时连接的 API 调用
 * 使用 tauri-specta 生成的 typed commands
 */

import type {
  ConnectDatabaseInput,
  ConnectionInfoResponse,
  CreateDatabaseFileInput,
  ExecuteSqlInput,
  GlobalConnectionInfoResponse,
  SaveNavigatorStateInput,
  TestConnectionResponse,
} from '@/generated/specta/bindings'
import { commands } from '@/generated/specta/bindings'
import { typed, tauriInvoke } from '@/shared/api'

import type { ConnectionResponse, RecentConnectionResponse } from '../../types/connection'

/**
 * 获取所有活动连接
 */
export async function getConnections(): Promise<ConnectionResponse[]> {
  return (await typed(commands.getConnections())) as unknown as ConnectionResponse[]
}

/**
 * 连接数据库
 */
export async function connectDatabase(
  dbType: string,
  url: string,
  name?: string,
  connectionType?: 'global' | 'project',
  projectId?: string,
  opts?: {
    connId?: string
    driverId?: string
    networkConfigId?: string | null
    environmentId?: string
    authConfigId?: string
    authMethod?: string
    driverProperties?: string
    advancedOptions?: string
    description?: string
    options?: string
    tags?: string
    metadataPath?: string
    schemaName?: string
    useDuckdbFed?: boolean
    password?: string
  }
): Promise<ConnectionResponse> {
  const input: ConnectDatabaseInput = {
    conn_id: opts?.connId ?? null,
    db_type: dbType,
    url,
    name: name ?? null,
    connection_type: connectionType || 'global',
    project_id: projectId ?? null,
    driver_id: opts?.driverId ?? null,
    network_config_id: opts?.networkConfigId ?? null,
    environment_id: opts?.environmentId ?? null,
    auth_config_id: opts?.authConfigId ?? null,
    auth_method: opts?.authMethod ?? null,
    driver_properties: opts?.driverProperties ?? null,
    advanced_options: opts?.advancedOptions ?? null,
    description: opts?.description ?? null,
    options: opts?.options ?? null,
    tags: opts?.tags ?? null,
    metadata_path: opts?.metadataPath ?? null,
    schema_name: opts?.schemaName ?? null,
    use_duckdb_fed: opts?.useDuckdbFed ?? false,
    password: opts?.password ?? null,
  }
  const result = await typed(commands.connectDatabase(input))
  return result as unknown as ConnectionResponse
}

/**
 * 切换活动连接
 */
export async function switchConnection(connId: string): Promise<void> {
  await typed(commands.switchConnection(connId))
}

/**
 * 关闭指定连接
 */
export async function closeConnection(connId: string): Promise<void> {
  await typed(commands.closeConnection(connId))
}

/**
 * 关闭所有连接
 */
export async function closeAllConnections(): Promise<void> {
  await typed(commands.closeAllConnections())
}

/**
 * 获取最近连接列表
 */
export async function getRecentConnections(): Promise<RecentConnectionResponse[]> {
  return (await typed(commands.getRecentConnections())) as unknown as RecentConnectionResponse[]
}

/**
 * 删除最近连接记录
 */
export async function removeRecentConnection(name: string): Promise<void> {
  await typed(commands.removeRecentConnection(name))
}

/**
 * 测试连接响应 — 由 specta 导出
 */
export type { TestConnectionResponse }

/**
 * 测试连接
 */
export async function testConnection(
  dbType: string,
  url: string,
  networkConfigId?: string | null,
  authConfigId?: string | null,
  authMethod?: string | null,
  projectPath?: string | null,
  password?: string | null
): Promise<TestConnectionResponse> {
  return typed(
    commands.testConnection(
      dbType,
      url,
      networkConfigId ?? null,
      authConfigId ?? null,
      authMethod ?? null,
      projectPath ?? null,
      password ?? null
    )
  )
}

/**
 * 创建数据库文件（SQLite/DuckDB）
 */
export async function createDatabaseFile(
  dbType: string,
  filePath: string
): Promise<{ file_path: string; success: boolean; message: string }> {
  const input: CreateDatabaseFileInput = {
    db_type: dbType,
    file_path: filePath,
  }
  return typed(commands.createDatabaseFile(input))
}

/**
 * 执行 SQL
 */
export async function executeSql(connId: string, sql: string): Promise<unknown> {
  const input: ExecuteSqlInput = {
    conn_id: connId,
    sql,
    timeout_ms: null,
  }
  return typed(commands.executeSql(input))
}

/**
 * 获取项目级连接
 * 后端命令: get_project_connections（尚未在 specta collect_commands! 中注册）
 */
export async function getProjectConnections(projectPath: string): Promise<unknown[]> {
  return tauriInvoke<unknown[]>('get_project_connections', { projectPath })
}

/**
 * 检测项目中的全局连接
 */
export async function detectGlobalConnectionsInProject(
  projectId: string
): Promise<ConnectionInfoResponse[]> {
  return typed(commands.detectGlobalConnectionsInProject(projectId))
}

/**
 * 转换连接类型（全局 ↔ 项目）
 * 后端命令: convert_connection_type（specta 类型与旧 API 不兼容，保留 tauriInvoke）
 */
export async function convertConnectionType(
  connectionId: string,
  fromType: 'global' | 'project',
  toType: 'global' | 'project',
  projectPath?: string,
  globalConnectionId?: string
): Promise<{ success: boolean; message: string }> {
  return tauriInvoke('convert_connection_type', {
    input: {
      fromType,
      toType,
      connectionId,
      projectPath: projectPath ?? null,
      globalConnectionId: globalConnectionId ?? null,
    },
  })
}

/**
 * 全局连接信息 — 由 specta 导出
 */
export type { GlobalConnectionInfoResponse as GlobalConnectionInfo }

/**
 * 获取所有全局连接
 */
export async function getGlobalConnections(): Promise<GlobalConnectionInfoResponse[]> {
  return typed(commands.getGlobalConnections())
}

/**
 * 获取单个全局连接（通过 ID 过滤）
 */
export async function getGlobalConnection(
  connId: string
): Promise<GlobalConnectionInfoResponse | null> {
  const all = await typed(commands.getGlobalConnections())
  return all.find(c => c.id === connId) ?? null
}

/**
 * 更新全局连接
 */
export async function updateGlobalConnection(input: {
  conn_id: string
  name?: string
  driver?: string
  host?: string
  port?: number
  database?: string
  schema_name?: string
  username?: string
  password?: string
  options?: string
  tags?: string[]
  use_duckdb_fed?: boolean
  metadata_path?: string
  driver_id?: string
  environment_id?: string
  auth_config_id?: string
  auth_method?: string
  network_config_id?: string
  driver_properties?: string
  advanced_options?: string
  description?: string
  server_version?: string
}): Promise<void> {
  const payload: Record<string, unknown> = {
    conn_id: input.conn_id,
    name: input.name ?? null,
    driver: input.driver ?? null,
    host: input.host ?? null,
    port: input.port ?? null,
    database: input.database ?? null,
    schema_name: input.schema_name ?? null,
    username: input.username ?? null,
    password: input.password ?? null,
    options: input.options ?? null,
    tags: input.tags ?? null,
    use_duckdb_fed: input.use_duckdb_fed ?? null,
    metadata_path: input.metadata_path ?? null,
    driver_id: input.driver_id ?? null,
    environment_id: input.environment_id ?? null,
    auth_config_id: input.auth_config_id ?? null,
    auth_method: input.auth_method ?? null,
    network_config_id: input.network_config_id ?? null,
    driver_properties: input.driver_properties ?? null,
    advanced_options: input.advanced_options ?? null,
    description: input.description ?? null,
    server_version: input.server_version ?? null,
  }
  await tauriInvoke('update_global_connection', payload)
}

/**
 * 保存导航器状态
 * 注意：filter_config 被 #[specta(skip)]，不通过 specta 通道传输
 */
export async function saveNavigatorState(
  connectionId: string,
  expandedKeys: string[],
  selectedKeys: string[],
  _filterConfig?: Record<string, unknown>
): Promise<void> {
  const input: SaveNavigatorStateInput = {
    connection_id: connectionId,
    expanded_keys: expandedKeys,
    selected_keys: selectedKeys,
  }
  await typed(commands.saveNavigatorState(input))
}

/**
 * 加载导航器状态
 */
export interface NavigatorState {
  expanded_keys: string[]
  selected_keys: string[]
  filter_config: Record<string, unknown> | null
}

export async function loadNavigatorState(connectionId: string): Promise<NavigatorState> {
  const result = await typed(commands.loadNavigatorState(connectionId))
  return result as unknown as NavigatorState
}
