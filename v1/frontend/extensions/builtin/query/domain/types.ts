/**
 * 查询相关类型定义
 *
 * 集中管理所有与 SQL 查询相关的 TypeScript 类型
 */

/** 查询结果 */
export interface QueryResult {
  columns: string[]
  rows: unknown[][]
}

/** 执行 SQL 请求参数 */
export interface ExecuteSqlInput {
  sql: string
  conn_id?: string
  timeout_ms?: number
}

/** 执行 SQL 响应 */
export interface ExecuteSqlResponse {
  result: QueryResult
  elapsed_ms: number
  affected_rows?: number
}

/** 执行事务请求参数 */
export interface ExecuteTransactionInput {
  sqls: string[]
  conn_id?: string
}

/** 执行事务响应 */
export interface ExecuteTransactionResponse {
  results: QueryResult[]
  total_elapsed_ms: number
}

/** SQL 历史记录响应 */
export interface SqlHistoryResponse {
  id: string
  sql: string
  conn_id?: string
  executed_at: string
}

/** 前端使用的 SQL 历史记录 */
export interface SqlHistory {
  id: string
  sql: string
  connId?: string
  executedAt: string
}

/** 查询状态 */
export type QueryStatus = 'idle' | 'running' | 'success' | 'error'

/** 查询错误 */
export interface QueryError {
  message: string
  code?: string
  position?: { line: number; column: number }
}

/** 查询执行选项 */
export interface QueryOptions {
  timeoutMs?: number
  maxRows?: number
  cancelToken?: AbortSignal
}

/** 查询标签页 */
export interface QueryTab {
  id: string
  name: string
  sql: string
  result: QueryResult | null
  loading: boolean
  error: string | null
  elapsedMs: number
  affectedRows?: number
}
