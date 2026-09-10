/**
 * 查询服务
 *
 * 提供 SQL 查询执行的 API 调用
 * 使用 tauri-specta 生成的 typed commands
 */

import { commands } from '@/generated/specta/bindings'
import type {
  ExecuteSqlInput,
  ExecuteTransactionInput,
  ExecuteSqlResponse,
  ExecuteTransactionResponse,
  TransactionStatusResponse,
} from '@/generated/specta/bindings'
import { typed, tauriInvoke } from '@/shared/api'

/**
 * 执行 SQL（主入口）
 */
export async function executeSql(
  sql: string,
  connectionId?: string,
  timeoutMs?: number
): Promise<ExecuteSqlResponse> {
  const input: ExecuteSqlInput = {
    conn_id: connectionId ?? null,
    sql,
    timeout_ms: timeoutMs ?? null,
  }
  return typed(commands.executeSql(input))
}

/**
 * 执行事务
 */
export async function executeTransaction(
  sqls: string[],
  connectionId?: string
): Promise<ExecuteTransactionResponse> {
  const input: ExecuteTransactionInput = {
    conn_id: connectionId ?? null,
    sqls,
  }
  return typed(commands.executeTransaction(input))
}

/**
 * 取消查询
 */
export async function cancelQuery(connId: string): Promise<boolean> {
  return typed(commands.cancelSqlQuery(connId))
}

/**
 * 事务控制
 */
export async function beginTransaction(connId?: string): Promise<TransactionStatusResponse> {
  return typed(commands.beginTransaction(connId ?? null))
}

export async function commitTransaction(connId?: string): Promise<TransactionStatusResponse> {
  return typed(commands.commitTransaction(connId ?? null))
}

export async function rollbackTransaction(connId?: string): Promise<TransactionStatusResponse> {
  return typed(commands.rollbackTransaction(connId ?? null))
}

export async function getTransactionStatus(connId?: string): Promise<TransactionStatusResponse> {
  return typed(commands.getTransactionStatus(connId ?? null))
}

// ==================== DuckDB 加速查询 ====================

export interface DuckDBAcceleratedParams {
  sql: string
  connId: string
  dbType?: string
  dataDir?: string
}

export interface DuckDBAcceleratedResult {
  success: boolean
  columns: string[]
  rows: unknown[][]
  elapsedMs: number
  error: string | null
}

/**
 * DuckDB 加速查询
 *
 * 注意：execute_duckdb_accelerated 已通过 #[specta::specta] + collect_commands! 注册，
 * 下一次 cargo build 将重新生成 specta bindings，届时可改用 typed(commands.executeDuckdbAccelerated(...))
 */
export async function executeDuckDBAccelerated(
  params: DuckDBAcceleratedParams
): Promise<DuckDBAcceleratedResult> {
  const raw = await tauriInvoke<{
    success: boolean
    columns: string[] | null
    rows: unknown[][] | null
    elapsed_ms: number
    error: string | null
  }>('execute_duckdb_accelerated', {
    sql: params.sql,
    connId: params.connId,
    dbType: params.dbType || null,
    dataDir: params.dataDir || null,
  })

  return {
    success: raw.success,
    columns: raw.columns ?? [],
    rows: raw.rows ?? [],
    elapsedMs: raw.elapsed_ms,
    error: raw.error,
  }
}

// ==================== DuckDB 外部数据库管理 ====================

export interface RegisterExternalDatabaseParams {
  connId: string
  name: string
  driver: string
  connectionString: string
}

export async function registerExternalDatabase(
  params: RegisterExternalDatabaseParams
): Promise<void> {
  await typed(
    commands.registerExternalDatabase({
      conn_id: params.connId ?? null,
      name: params.name,
      driver: params.driver,
      connection_string: params.connectionString,
    })
  )
}

export interface CreateExternalTableParams {
  connId: string
  schemaName: string
  tableName: string
  externalDbName: string
}

export async function createExternalTable(params: CreateExternalTableParams): Promise<void> {
  await typed(
    commands.createExternalTable({
      conn_id: params.connId ?? null,
      external_db_name: params.externalDbName,
      schema_name: params.schemaName,
      table_name: params.tableName,
      external_table_name: `${params.externalDbName}_${params.tableName}`,
    })
  )
}
