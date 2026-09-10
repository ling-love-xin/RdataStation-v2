/**
 * 连接类型定义
 */

import type { DriverOption } from './driver'
import type { ConnectionMethodConfig } from '../../domain/types'
import type {
  Connection,
  RecentConnection,
  ProjectConnection,
  CreateProjectConnectionInput,
  DriverConfig,
  DriverField,
  ConnectionFormData,
} from '../../types/connection'

export type {
  Connection,
  RecentConnection,
  ProjectConnection,
  CreateProjectConnectionInput,
  DriverConfig,
  DriverField,
  DriverOption,
  ConnectionFormData,
}

// 连接状态
export type ConnectionStatus = 'connected' | 'disconnected' | 'error' | 'connecting'

// 连接配置
export interface ConnectionConfiguration {
  id?: string
  name: string
  driver: string
  host: string
  port?: number
  database?: string
  username?: string
  password?: string
  properties?: Record<string, unknown>
  options?: Record<string, unknown>
  url?: string
  connectionMethod?: ConnectionMethodConfig
  connectionType?: 'global' | 'project'
  useDuckdbFed?: boolean

  // 认证方式
  authMethod?: 'password' | 'trust' | 'ssh' | 'ssl'

  // SSH 配置
  sshHost?: string
  sshPort?: number
  sshUsername?: string
  sshPassword?: string
  sshKeyPath?: string

  // SSL 配置
  sslMode?: 'disable' | 'require' | 'verify-ca' | 'verify-full'
  sslCa?: string
  sslCert?: string
  sslKey?: string
}

// 连接配置（别名）
export type ConnectionConfig = ConnectionConfiguration

/**
 * UI 层驱动描述符（Rust→TS 传输 DTO）
 *
 * 与 domain/types.ts 的 DriverDescriptor 不同：
 * - domain 版本是精简的领域模型（10字段）
 * - 此版本是 Rust 后端序列化直出（24字段，含 snake_case 别名供 Rust 反序列化兼容）
 * - 两者都叫 DriverDescriptor 但不可互换
 */
export interface DriverDescriptor {
  id: string
  name: string
  icon: string
  version?: string
  features: string[]
  category?: string
  defaultPort?: number
  default_port?: number
  description?: string
  driverKind?: string
  urlTemplate?: string
  fields?: DriverField[]
  extraOptions?: DriverOption[]
  extra_options?: DriverOption[]
  requireFile?: boolean
  require_file?: boolean
  requireDatabase?: boolean
  require_database?: boolean
  supportsSsl?: boolean
  supportsSshTunnel?: boolean
  supports_ssh_tunnel?: boolean
  supports_http_proxy?: boolean
  supports_socks_proxy?: boolean
  supportsHttpProxy?: boolean
  supportsSocksProxy?: boolean
  navigation?: Record<string, unknown>
}

// 连接方法类型
export type ConnectionMethodType = 'direct' | 'ssl' | 'ssh' | 'http_proxy' | 'socks_proxy'
