/**
 * 连接模块类型定义
 */

// ============================================================================
// 基础连接类型
// ============================================================================

export interface Connection {
  connId: string
  name: string
  dbType: string
  url: string
  status: 'connected' | 'disconnected' | 'error'
  isActive: boolean
  meta: ConnectionMeta
}

export interface ConnectionMeta {
  supportsTransaction: boolean
  supportsStreaming: boolean
  supportsArrow: boolean
  supportsFederated: boolean
  supportsConcurrentWrite: boolean
  isInMemory: boolean
}

export interface RecentConnection {
  id: string
  name: string
  dbType: string
  url: string
  connectedAt: string
}

// ============================================================================
// 连接状态枚举
// ============================================================================

/**
 * 连接状态
 * - disconnected: 未连接（配置已保存，但未建立实际连接）
 * - connected: 已连接（已建立实际数据库连接）
 * - connecting: 连接中（正在建立连接）
 * - error: 连接错误（连接失败）
 */
export type ConnectionStatus = 'disconnected' | 'connected' | 'connecting' | 'error'

// ============================================================================
// 项目连接类型
// ============================================================================

export interface ProjectConnection {
  id: string
  name: string
  driver: string
  host?: string
  port?: number
  database?: string
  schema_name?: string
  username?: string
  password?: string
  options?: string
  tags?: string
  use_duckdb_fed?: boolean
  metadata_path?: string
  is_active?: boolean
  server_version?: string
  description?: string
  driver_id?: string
  environment_id?: string
  auth_config_id?: string
  auth_method?: string
  network_config_id?: string
  driver_properties?: string
  advanced_options?: string
  /** 连接状态 */
  status?: ConnectionStatus
  /** 连接错误信息 */
  error_message?: string
  /** 最后连接时间 */
  last_connected_at?: string
  properties?: Record<string, unknown>
  created_at: string
  updated_at: string
  connection_type?: 'global' | 'project'
  project_path?: string
}

export interface CreateProjectConnectionInput {
  id?: string
  project_path: string
  name: string
  driver: string
  host?: string
  port?: number
  database?: string
  schema_name?: string
  username?: string
  password?: string
  options?: string
  tags?: string
  is_active?: boolean
  use_duckdb_fed?: boolean
  metadata_path?: string
  server_version?: string
  description?: string
  driver_id?: string
  environment_id?: string
  auth_config_id?: string
  auth_method?: string
  network_config_id?: string
  driver_properties?: string
  advanced_options?: string
  properties?: Record<string, unknown>
  connection_type?: 'global' | 'project'
  created_at?: string
  updated_at?: string
}

// ============================================================================
// 驱动配置类型
// ============================================================================

export interface DriverConfig {
  id: string
  name: string
  icon: string
  features: DriverFeature[]
  defaultPort?: number
  fields: DriverField[]
}

export type DriverFeature =
  | 'schemas'
  | 'tables'
  | 'views'
  | 'procedures'
  | 'functions'
  | 'triggers'
  | 'indexes'
  | 'foreignKeys'
  | 'ssl'
  | 'sshTunnel'
  | 'httpProxy'

export interface DriverField {
  name: string
  label: string
  type: 'text' | 'number' | 'password' | 'file' | 'select' | 'checkbox'
  fieldType?: 'text' | 'number' | 'password' | 'file' | 'select' | 'checkbox'
  required?: boolean
  default?: unknown
  placeholder?: string
  description?: string
  options?: { label: string; value: unknown }[]
}

// ============================================================================
// 连接表单类型
// ============================================================================

export interface ConnectionFormData {
  name: string
  driver: string
  host: string
  port?: number
  database?: string
  username?: string
  password?: string
  ssl?: boolean
  sshTunnel?: boolean
  properties?: Record<string, unknown>
}

// ============================================================================
// 后端存储类型（匹配 Rust StoredConnection）
// ============================================================================

/**
 * 与后端 Rust StoredConnection 结构体字段对齐的类型
 * 用于 save_project_store_connection / get_project_store_connections 的数据传输
 */
export interface StoredConnection {
  id: string
  name: string
  driver: string
  host: string | null
  port: number | null
  database: string | null
  schema_name: string | null
  username: string | null
  password_encrypted: string | null
  options: string | null
  tags: string | null
  use_duckdb_fed: boolean
  metadata_path: string | null
  is_active: boolean
  server_version: string | null
  description: string | null
  driver_id: string | null
  environment_id: string | null
  auth_config_id: string | null
  auth_method: string | null
  network_config_id: string | null
  driver_properties: string | null
  advanced_options: string | null
  created_at: string
  updated_at: string
}

// ============================================================================
// API 响应类型
// ============================================================================

export interface ConnectionResponse {
  conn_id: string
  name: string
  db_type: string
  url: string
  connection_type: string
  project_id: string | null
  status: 'connected' | 'disconnected' | 'error'
  is_active: boolean
  description?: string | null
  driver_id?: string | null
  environment_id?: string | null
  auth_config_id?: string | null
  auth_method?: string | null
  network_config_id?: string | null
  driver_properties?: string | null
  advanced_options?: string | null
  meta?: {
    supports_transaction?: boolean
    supports_streaming?: boolean
    supports_arrow?: boolean
    supports_federated?: boolean
    supports_concurrent_write?: boolean
    is_in_memory?: boolean
  }
}

export interface RecentConnectionResponse {
  id: string
  name: string
  db_type: string
  url: string
  connection_type?: string
  connected_at: string
}
