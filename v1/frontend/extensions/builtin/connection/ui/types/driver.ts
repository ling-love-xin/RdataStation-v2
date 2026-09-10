/**
 * 驱动类型定义
 */

import type { DriverField } from '@/shared/types/databaseMeta'

// DriverConfig 和 DriverField 从 connection.ts 重新导出

// 驱动类型
export type DriverType = 'mysql' | 'postgresql' | 'sqlite' | 'duckdb' | string

// 驱动选项类型
export interface DriverOption {
  name: string
  label: string
  type: 'string' | 'number' | 'boolean' | 'select'
  optionType?: 'string' | 'number' | 'boolean' | 'select'
  option_type?: 'string' | 'number' | 'boolean' | 'select'
  default?: string | number | boolean
  description?: string
  required?: boolean
  options?: { label: string; value: string }[]
}

// 驱动字段值
export type DriverFieldValue = string | number | boolean | undefined

// 驱动配置表单数据
export interface DriverFormData {
  [key: string]: DriverFieldValue
}

// 驱动描述符
export interface DriverDescriptor {
  id: string
  name: string
  icon: string
  features: string[]
  category?: string
  defaultPort?: number
  default_port?: number
  description?: string
  fields?: DriverField[]
  extraOptions?: DriverOption[]
  extra_options?: DriverOption[]
  require_file?: boolean
  requireFile?: boolean
  supportsSsl?: boolean
  supports_ssh_tunnel?: boolean
  supports_http_proxy?: boolean
  supports_socks_proxy?: boolean
  supportsSshTunnel?: boolean
  supportsHttpProxy?: boolean
  supportsSocksProxy?: boolean
}

// 连接配置
export interface ConnectionConfig {
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
  connectionMethod?: string
}
