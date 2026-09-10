/**
 * 共享类型定义
 */

// 重新导出 databaseMeta 类型
export * from './databaseMeta'

// 重新导出 SQL 相关类型
export * from './sql'

import type { DatabaseType } from './sql'

// ============================================================================
// 连接分类类型
// ============================================================================

/** 连接类型枚举 */
export type ConnectionType = 'global' | 'project'

/**
 * 多主机配置（故障转移 / 读写分离）
 *
 * 若提供 `hosts` 数组，则优先使用；否则回退到 `url` 字段。
 * `priority` 越小越优先（故障转移按优先级顺序尝试）。
 */
export interface HostConfig {
  host: string
  port?: number
  priority?: number
  role?: 'primary' | 'replica'
}

/** 连接配置 */
export interface ConnectionConfig {
  id?: string
  name: string
  dbType: DatabaseType
  url: string
  hosts?: HostConfig[]
  connectionType?: ConnectionType
  projectId?: string
}

/** 连接信息响应 */
export interface ConnectionInfoResponse {
  id: string
  name: string
  dbType: DatabaseType
  url: string
  connectionType: ConnectionType
  projectId: string | null
  status: 'connected' | 'disconnected' | 'error'
  isActive: boolean
  createdAtMs: number
}

/** 连接类型转换请求 */
export interface ConvertConnectionRequest {
  connId: string
  targetType: ConnectionType
  projectId?: string
}

/** 连接类型转换响应 */
export interface ConvertConnectionResponse {
  connId: string
  connectionType: ConnectionType
  projectId: string | null
  message: string
}

// ============================================================================
// 连接相关类型
// ============================================================================

export interface Connection {
  connId: string
  name: string
  dbType: DatabaseType
  url: string
  connectionType: ConnectionType
  projectId?: string | null
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
  dbType: DatabaseType
  url: string
  connectionType: ConnectionType
  connectedAt: string
}

// ============================================================================
// 项目相关类型
// ============================================================================

export interface Project {
  id: string
  name: string
  localPath: string
  createdAt: string
  updatedAt: string
}

// ============================================================================
// 查询相关类型
// ============================================================================

export interface QueryColumn {
  name: string
  dataType: string
}

export interface QueryResult {
  columns: string[]
  rows: unknown[]
  rowCount: number
  executionTime: number
  affectedRows?: number
}

export interface QueryTab {
  id: string
  title: string
  name?: string
  sql: string
  connectionId?: string
  result?: QueryResult | null
  isExecuting: boolean
  loading?: boolean
  error?: string | null
  elapsedMs?: number
  affectedRows?: number
  status?: 'idle' | 'executing' | 'success' | 'error'
}

export interface SqlHistory {
  id: string
  sql: string
  connectionId?: string
  connId?: string
  executedAt: string
  executionTime: number
  rowCount: number
  success: boolean
  error?: string
}

// ============================================================================
// 数据库导航器类型
// ============================================================================

// NavigatorNode 类型已从 databaseMeta.ts 导出

// ============================================================================
// 事务相关类型
// ============================================================================

/** 事务状态响应 */
export interface TransactionStatusResponse {
  connId: string
  isInTransaction: boolean
  transactionStartTimeMs?: number
  transactionDurationMs?: number
}
