/**
 * 查询服务相关类型定义
 */

/**
 * 查询结果
 */
export interface QueryResult {
  columns: string[]
  rows: Record<string, unknown>[]
  rowCount?: number
  executionTime?: number
  error?: string
}

/**
 * 执行 SQL 输入
 */
export interface ExecuteSqlInput {
  sql: string
  conn_id?: string
  timeout_ms?: number
}

/**
 * 执行 SQL 响应（后端返回格式）
 */
export interface ExecuteSqlResponse {
  success: boolean
  result?: {
    columns: string[]
    rows: Record<string, unknown>[]
  }
  error?: string
  execution_time: number
  elapsed_ms: number
  affected_rows?: number
}

/**
 * 执行事务输入
 */
export interface ExecuteTransactionInput {
  sqls: string[]
  conn_id?: string
}

/**
 * 执行事务响应（后端返回格式）
 */
export interface ExecuteTransactionResponse {
  success: boolean
  results?: {
    columns: string[]
    rows: Record<string, unknown>[]
  }[]
  error?: string
  execution_time: number
  total_elapsed_ms: number
}

/**
 * SQL 历史记录响应（后端返回格式）
 */
export interface SqlHistoryResponse {
  id: string
  sql: string
  conn_id?: string
  executed_at: string
  execution_time: number
  row_count?: number
  success: boolean
  error?: string
}

/**
 * SQL 历史记录（前端使用）
 */
export interface SqlHistory {
  id: string
  sql: string
  connId?: string
  executedAt: string
  executionTime: number
  rowCount?: number
  success: boolean
  error?: string
}

/**
 * 查询状态
 */
export type QueryStatus = 'idle' | 'running' | 'success' | 'error'

/**
 * 查询错误
 */
export interface QueryError {
  message: string
  code?: string
  position?: { line: number; column: number }
}

/**
 * 查询选项
 */
export interface QueryOptions {
  timeout?: number
  maxRows?: number
  fetchSize?: number
}

/**
 * 查询标签页
 */
export interface QueryTab {
  id: string
  name: string
  sql: string
  result?: QueryResult | null
  status: QueryStatus
  executionTime?: number
  connId?: string
  // 前端状态字段
  loading?: boolean
  error?: string | null
  elapsedMs?: number
  affectedRows?: number
}
