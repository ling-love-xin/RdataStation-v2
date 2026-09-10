/**
 * 查询服务类型定义
 */

import type { QueryResult } from '@/shared/types'

export type { QueryResult }

// 查询服务接口
export interface QueryService {
  execute(connectionId: string, sql: string): Promise<QueryResult>
  cancel(queryId: string): Promise<void>
  getHistory(): Promise<QueryHistoryItem[]>
}

// 查询历史项
export interface QueryHistoryItem {
  id: string
  sql: string
  connectionId: string
  executedAt: string
  executionTime: number
  rowCount: number
  success: boolean
}

// 执行查询输入
export interface ExecuteQueryInput {
  connectionId: string
  sql: string
}

// 执行查询响应
export interface ExecuteQueryResponse {
  queryId: string
  columns: string[]
  rows: unknown[][]
  rowCount: number
  executionTime: number
  affectedRows?: number
}

// 执行 SQL 响应
export interface ExecuteSqlResponse {
  queryId: string
  columns: string[]
  rows: unknown[][]
  rowCount: number
  executionTime: number
  affectedRows?: number
  total_rows?: number
  elapsed_ms?: number
  affected_rows?: number
  truncated?: boolean
  result?: {
    columns: string[]
    rows: unknown[][]
    total_rows?: number
    affected_rows?: number
    is_read_only?: boolean
  }
}

// 执行事务响应
export interface ExecuteTransactionResponse {
  transactionId: string
  results: ExecuteSqlResponse[]
  success: boolean
  total_elapsed_ms?: number
}

// SQL 历史响应
export interface SqlHistoryResponse {
  id: string
  sql: string
  connectionId: string
  executedAt: string
  executionTime: number
  rowCount: number
  success: boolean
}
