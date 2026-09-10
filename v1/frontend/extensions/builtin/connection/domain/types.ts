/**
 * 连接相关类型定义
 *
 * 集中管理所有与数据库连接相关的 TypeScript 类型
 * 与后端 API 模型保持同步
 */

/** 数据源元数据 */
export interface DataSourceMeta {
  supports_transaction: boolean
  supports_streaming: boolean
  supports_arrow: boolean
  supports_federated: boolean
  supports_concurrent_write: boolean
  is_in_memory: boolean
}

/** 创建数据库连接请求参数
 *
 * ⚠️ 与 Rust specta 自动生成的 @/generated/specta/bindings.ts 中的
 *    ConnectDatabaseInput 保持同步。此为手写副本，用于减少编译时依赖。
 *    修改此类型前，请先对照 bindings.ts 中的 specta 定义。
 */
export interface ConnectDatabaseInput {
  conn_id?: string | null
  db_type: string
  url: string
  name?: string
  connection_type?: string
  project_id?: string
  description?: string | null
  driver_id?: string | null
  environment_id?: string | null
  auth_config_id?: string | null
  auth_method?: string
  network_config_id?: string | null
  driver_properties?: string | null
  advanced_options?: string | null
  options?: string | null
  tags?: string | null
  metadata_path?: string | null
  schema_name?: string | null
  use_duckdb_fed?: boolean | null
  password?: string | null
}

/** 连接响应 */
export interface ConnectDatabaseResponse {
  conn_id: string
  name: string
  db_type: string
  url: string
  status: string
  meta: DataSourceMeta
}

/** 连接信息响应 */
export interface ConnectionInfoResponse {
  conn_id: string
  name: string
  db_type: string
  url: string
  created_at: string
  is_active: boolean
  meta: DataSourceMeta
}

/** 最近连接记录 */
export interface RecentConnectionRecord {
  name: string
  db_type: string
  url: string
  last_used_at: string
}

/** 前端使用的连接对象 */
export interface Connection {
  connId: string
  name: string
  dbType: string
  url: string
  meta: {
    supportsTransaction: boolean
    supportsStreaming: boolean
    supportsArrow: boolean
    supportsFederated: boolean
    supportsConcurrentWrite: boolean
    isInMemory: boolean
  }
}

/** 前端使用的最近连接 */
export interface RecentConnection {
  name: string
  dbType: string
  url: string
  lastUsedAt: string
}

// ========== 数据源类型 & 驱动定义（v2.0: SQLite global.db 动态注册） ==========

/** 数据源大类（来自 SQLite data_source_types 表） */
export interface DataSourceType {
  id: string
  name: string
  category: DataSourceCategory
  icon?: string
  enabled: boolean
  created_at: string
}

/** 数据源分类，预留 string 兜底以兼容未来新增类型 */
export type DataSourceCategory =
  | 'relational'
  | 'file-based'
  | 'nosql'
  | 'analytics'
  | 'cloud'
  | 'mq'
  | 'http'
  | (string & {})

/** 驱动定义（来自 SQLite drivers 表，config_schema 为 JSON 字符串） */
export interface Driver {
  id: string
  type_id: string
  name: string
  driver_kind: DriverKind
  is_file: boolean
  default_port?: number
  url_template?: string
  download_url?: string
  download_checksum?: string
  version?: string
  config_schema: string // JSON Schema 字符串 → 前端 parseConfigSchema() 解析
  supported_auth_types?: string // JSON 数组字符串
  capabilities?: string // JSON 数组字符串
  driver_properties?: string // JSON 键值对字符串，驱动默认连接属性
  enabled: boolean
}

/** 驱动类型，预留 string 兜底以兼容未来新类型 */
export type DriverKind =
  | 'native'
  | 'jdbc'
  | 'odbc'
  | 'wasm'
  | 'adbc'
  | 'http'
  | 'python'
  | 'js'
  | (string & {})

/** 驱动列表响应 */
export interface DriverListResponse {
  drivers: Driver[]
  missing: MissingDriver[]
}

export interface MissingDriver {
  driver_id: string
  driver_name: string
  download_url: string
}

// ========== 旧版驱动描述符（保留向前兼容） ==========

/** 驱动描述符 */
export interface DriverDescriptor {
  id: string
  name: string
  description: string
  defaultPort?: number
  requiresDatabase: boolean
  requiresFile: boolean
  supportsSsl: boolean
  supportsSshTunnel: boolean
  supportsHttpProxy: boolean
  supportsSocksProxy: boolean
  /** 驱动字段（未加载时为 undefined） */
  fields?: DriverField[]
  /** 驱动扩展选项（未加载时为 undefined） */
  extraOptions?: DriverOption[]
}

/** 驱动字段定义 */
export interface DriverField {
  name: string
  label: string
  fieldType: 'text' | 'number' | 'password' | 'file' | 'select'
  required: boolean
  defaultValue?: string
  placeholder?: string
  options?: { label: string; value: string }[]
}

/** 驱动选项 */
export interface DriverOption {
  name: string
  label: string
  optionType: 'string' | 'number' | 'boolean' | 'select'
  defaultValue?: string | number | boolean
  description?: string
  options?: { label: string; value: string }[]
}

/** TLS 版本 */
export type TlsVersion = 'tls1_0' | 'tls1_1' | 'tls1_2' | 'tls1_3'

/** SSL 配置 */
export interface SslConfig {
  verifyServerCert: boolean
  caCertPath?: string
  clientCertPath?: string
  clientKeyPath?: string
  minTlsVersion: TlsVersion
}

/** SSH 认证方式 */
export type SshAuthType = 'password' | 'private_key' | 'agent'

/** SSH 认证配置 */
export interface SshAuth {
  type: SshAuthType
  password?: string
  keyPath?: string
  passphrase?: string
}

/** SSH 配置 */
export interface SshConfig {
  host: string
  port: number
  username: string
  auth: SshAuth
  remoteHost: string
  remotePort: number
  localPort?: number
  timeoutSecs: number
}

/** 代理认证 */
export interface ProxyAuth {
  username: string
  password: string
}

/** 代理配置 */
export interface ProxyConfig {
  host: string
  port: number
  auth?: ProxyAuth
  noProxy?: string[]
  timeoutSecs: number
}

/** 连接方式类型 */
export type ConnectionMethodType = 'direct' | 'ssl' | 'ssh' | 'http_proxy' | 'socks_proxy'

/** 连接方式配置 */
export interface ConnectionMethodConfig {
  type: ConnectionMethodType
  sslConfig?: SslConfig
  sshConfig?: SshConfig
  proxyConfig?: ProxyConfig
}

/** 连接配置 */
export interface ConnectionConfig {
  driver: string
  name?: string
  host?: string
  port?: number
  database?: string
  username?: string
  password?: string
  filePath?: string
  connectionMethod: ConnectionMethodConfig
  options: Record<string, string>
}

/** Schema 对象类型 */
export type SchemaObjectKind = 'database' | 'schema' | 'table' | 'view' | 'column'

/** Schema 对象 */
export interface SchemaObject {
  name: string
  kind: SchemaObjectKind
  children?: SchemaObject[]
}

// ==================== 项目级连接类型 ====================
// ⚠️ 已迁移到 types/connection.ts — 统一使用该文件中的 ProjectConnection
// domain 保留此注释供追溯，新代码请 import from '@/extensions/builtin/connection/types/connection'
