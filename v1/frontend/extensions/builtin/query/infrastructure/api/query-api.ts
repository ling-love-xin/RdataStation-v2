/**
 * 查询服务
 *
 * 负责与后端 SQL 查询相关的所有 API 调用
 * 使用 tauri-specta 生成的 typed commands
 */

import { commands } from '@/generated/specta/bindings'
import type { ExecuteSqlInput, ExecuteTransactionInput } from '@/generated/specta/bindings'
import { typed } from '@/shared/api'

import type {
  ExecuteSqlResponse,
  ExecuteTransactionResponse,
  SqlHistoryResponse,
} from '../types/query-service'

/**
 * 执行 SQL 查询
 */
export async function executeSql(
  sql: string,
  connId?: string,
  timeoutMs?: number
): Promise<ExecuteSqlResponse> {
  const input: ExecuteSqlInput = {
    conn_id: connId ?? null,
    sql,
    timeout_ms: timeoutMs ?? null,
  }
  return typed(commands.executeSql(input)) as unknown as ExecuteSqlResponse
}

/**
 * 执行事务
 */
export async function executeTransaction(
  sqls: string[],
  connId?: string
): Promise<ExecuteTransactionResponse> {
  const input: ExecuteTransactionInput = {
    conn_id: connId ?? null,
    sqls,
  }
  return typed(commands.executeTransaction(input)) as unknown as ExecuteTransactionResponse
}

/**
 * 获取 SQL 历史记录
 */
export async function getSqlHistory(limit?: number): Promise<SqlHistoryResponse[]> {
  return typed(commands.getSqlHistory(limit ?? null)) as unknown as SqlHistoryResponse[]
}

/**
 * 搜索 SQL 历史记录
 */
export async function searchSqlHistory(
  keyword: string,
  limit?: number
): Promise<SqlHistoryResponse[]> {
  return typed(commands.searchSqlHistory(keyword, limit ?? null)) as unknown as SqlHistoryResponse[]
}

/**
 * 清空 SQL 历史记录
 */
export async function clearSqlHistory(): Promise<void> {
  await typed(commands.clearSqlHistory())
}

/**
 * 删除指定 SQL 历史记录
 */
export async function removeSqlHistory(id: string): Promise<void> {
  await typed(commands.removeSqlHistory(id))
}
