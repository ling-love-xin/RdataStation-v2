/**
 * 项目连接服务
 *
 * 提供项目级连接配置的 API 调用（v2.5 单轨化：*_store_* → project_*）
 * 映射到后端 project_store_commands.rs 中的 create_project_connection / get_project_connections 等命令
 */

import { invoke } from '@tauri-apps/api/core'

import type {
  ProjectConnection,
  CreateProjectConnectionInput,
  ConnectionStatus,
} from '../../types/connection'

/** 后端 ProjectConnectionResponse shape（Tauri 自动 camelCase） */
interface ProjectConnectionResponse {
  id: string
  name: string
  driver: string
  host: string | null
  port: number | null
  database: string | null
  schema_name: string | null
  username: string | null
  password: string | null
  options: string | null
  tags: string | null
  use_duckdb_fed: boolean
  metadata_path: string | null
  description: string | null
  server_version: string | null
  driver_id: string | null
  environment_id: string | null
  auth_config_id: string | null
  auth_method: string | null
  network_config_id: string | null
  driver_properties: string | null
  advanced_options: string | null
  connection_type: string | null
  is_active: boolean
  created_at: string
  updated_at: string
}

/** 后端响应 → 前端 ProjectConnection */
function mapResponse(r: ProjectConnectionResponse): ProjectConnection {
  return {
    id: r.id,
    name: r.name,
    driver: r.driver,
    host: r.host ?? undefined,
    port: r.port ?? undefined,
    database: r.database ?? undefined,
    schema_name: r.schema_name ?? undefined,
    username: r.username ?? undefined,
    password: r.password ?? undefined,
    options: r.options ?? undefined,
    tags: r.tags ?? undefined,
    use_duckdb_fed: r.use_duckdb_fed,
    metadata_path: r.metadata_path ?? undefined,
    is_active: r.is_active,
    server_version: r.server_version ?? undefined,
    description: r.description ?? undefined,
    driver_id: r.driver_id ?? undefined,
    environment_id: r.environment_id ?? undefined,
    auth_config_id: r.auth_config_id ?? undefined,
    auth_method: r.auth_method ?? undefined,
    network_config_id: r.network_config_id ?? undefined,
    driver_properties: r.driver_properties ?? undefined,
    advanced_options: r.advanced_options ?? undefined,
    connection_type: (r.connection_type as 'global' | 'project') ?? 'project',
    status: r.is_active ? 'connected' : 'disconnected',
    created_at: r.created_at,
    updated_at: r.updated_at,
  }
}

/**
 * 获取项目所有连接
 * 后端命令: get_project_connections
 */
export async function getProjectConnections(projectPath: string): Promise<ProjectConnection[]> {
  const list = await invoke<ProjectConnectionResponse[]>('get_project_connections', { projectPath })
  return list.map(mapResponse)
}

/**
 * 创建项目连接
 * 后端命令: create_project_connection
 *
 * 注意：这仅将连接配置保存到 project.db，不会创建运行时连接。
 * 需要单独调用 connect_database 来建立实际连接。
 */
export async function createProjectConnection(
  input: CreateProjectConnectionInput
): Promise<ProjectConnection> {
  const r = await invoke<ProjectConnectionResponse>('create_project_connection', {
    input: {
      project_path: input.project_path,
      name: input.name,
      driver: input.driver,
      host: input.host ?? null,
      port: input.port ?? null,
      database: input.database ?? null,
      schema_name: input.schema_name ?? null,
      username: input.username ?? null,
      password: input.password ?? null,
      options: input.options ?? null,
      tags: input.tags ?? null,
      use_duckdb_fed: input.use_duckdb_fed ?? false,
      metadata_path: input.metadata_path ?? null,
      description: input.description ?? null,
      driver_id: input.driver_id ?? null,
      environment_id: input.environment_id ?? null,
      auth_config_id: input.auth_config_id ?? null,
      auth_method: input.auth_method ?? null,
      network_config_id: input.network_config_id ?? null,
      driver_properties: input.driver_properties ?? null,
      advanced_options: input.advanced_options ?? null,
    },
  })
  return mapResponse(r)
}

/**
 * 更新项目连接
 * 后端命令: update_project_connection
 */
export async function updateProjectConnection(
  connection: ProjectConnection,
  projectPath: string
): Promise<void> {
  await invoke('update_project_connection', {
    projectPath,
    connection: {
      id: connection.id,
      name: connection.name,
      driver: connection.driver,
      host: connection.host ?? null,
      port: connection.port ?? null,
      database: connection.database ?? null,
      schema_name: connection.schema_name ?? null,
      username: connection.username ?? null,
      password: connection.password ?? null,
      options: connection.options ?? null,
      tags: connection.tags ?? null,
      use_duckdb_fed: connection.use_duckdb_fed ?? false,
      metadata_path: connection.metadata_path ?? null,
      is_active: connection.is_active ?? false,
      server_version: connection.server_version ?? null,
      description: connection.description ?? null,
      driver_id: connection.driver_id ?? null,
      environment_id: connection.environment_id ?? null,
      auth_config_id: connection.auth_config_id ?? null,
      auth_method: connection.auth_method ?? null,
      network_config_id: connection.network_config_id ?? null,
      driver_properties: connection.driver_properties ?? null,
      advanced_options: connection.advanced_options ?? null,
      connection_type: connection.connection_type ?? null,
      created_at: connection.created_at,
      updated_at: new Date().toISOString(),
    },
  })
}

/**
 * 更新项目连接状态
 * 后端命令: update_project_connection_status
 */
export async function updateProjectConnectionStatus(
  projectPath: string,
  connectionId: string,
  status: ConnectionStatus,
  _errorMessage?: string
): Promise<void> {
  await invoke('update_project_connection_status', {
    projectPath,
    connectionId,
    isActive: status === 'connected',
  })
}

/**
 * 删除项目连接
 * 后端命令: delete_project_connection
 */
export async function deleteProjectConnection(
  projectPath: string,
  connectionId: string
): Promise<void> {
  await invoke('delete_project_connection', { projectPath, connectionId })
}

/**
 * 搜索项目连接（后端侧）
 * 后端命令: search_project_connections
 */
export async function searchProjectConnections(
  projectPath: string,
  query: string,
  _limit?: number,
  _offset?: number
): Promise<ProjectConnection[]> {
  const list = await invoke<ProjectConnectionResponse[]>('search_project_connections', {
    projectPath,
    query,
  })
  let results = list.map(mapResponse)
  // 后端不支持分页，前端裁剪
  if (_offset) results = results.slice(_offset)
  if (_limit != null) results = results.slice(0, _limit)
  return results
}

/**
 * 构建连接 URL
 */
export function buildConnectionUrl(connection: ProjectConnection): string {
  const { driver, host, port, database, username, password } = connection

  switch (driver?.toLowerCase()) {
    case 'mysql':
      if (username && password) {
        return `mysql://${username}:${password}@${host || 'localhost'}:${port || 3306}/${database || ''}`
      }
      return `mysql://${host || 'localhost'}:${port || 3306}/${database || ''}`

    case 'postgresql':
    case 'postgres':
      if (username && password) {
        return `postgresql://${username}:${password}@${host || 'localhost'}:${port || 5432}/${database || ''}`
      }
      return `postgresql://${host || 'localhost'}:${port || 5432}/${database || ''}`

    case 'sqlite':
      return `sqlite://${host || ''}`

    case 'duckdb':
      return `duckdb://${host || ''}`

    default:
      throw new Error(`不支持的数据库类型: ${driver || 'unknown'}`)
  }
}

/**
 * 获取连接显示名称
 */
export function getConnectionDisplayName(connection: ProjectConnection): string {
  if (connection.name) {
    return connection.name
  }

  const { driver, host, port, database } = connection

  if (database) {
    return `${driver} - ${database}@${host || 'localhost'}`
  }

  if (port) {
    return `${driver} - ${host || 'localhost'}:${port}`
  }

  return `${driver} - ${host || 'localhost'}`
}
