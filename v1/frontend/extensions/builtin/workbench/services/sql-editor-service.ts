/**
 * SQL 编辑器增强服务
 * 提供智能代码补全、语法验证等功能
 * 基于 sqlglot-rust 后端实现 SQL 解析、验证、格式化和转译
 */

import { EditorView } from '@codemirror/view'
import { invoke } from '@tauri-apps/api/core'

import {
  createErrorDiagnostic,
  setEditorDiagnostics,
  clearEditorDiagnostics,
} from '@/extensions/builtin/workbench/services/cm-sql-extensions'
import type { SqlDialect } from '@/shared/types/sql'

interface SqlMarkerData {
  severity: 'error' | 'warning' | 'info'
  message: string
  startLineNumber: number
  startColumn: number
  endLineNumber: number
  endColumn: number
}

interface SqlExecutionResult {
  result?: {
    columns?: string[]
    rows?: unknown[][]
    total_rows?: number
    affected_rows?: number
  }
  elapsed_ms?: number
  truncated?: boolean
}

interface ValidateSqlResult {
  valid: boolean
  errors?: string[]
}

interface FormatSqlResult {
  success: boolean
  formatted_sql?: string
}

interface TranspileSqlResult {
  success: boolean
  transpiled_sql?: string
}

interface ParseSqlResult {
  success: boolean
  statements_count: number
  error?: string
}

/**
 * SQL 语法验证（基于 sqlglot-rust）
 */
export async function validateSql(sql: string, dialect?: SqlDialect): Promise<SqlMarkerData[]> {
  const markers: SqlMarkerData[] = []

  try {
    const result = await invoke<ValidateSqlResult>('validate_sql', {
      input: {
        sql,
        dialect: dialect || 'generic',
      },
    })

    if (!result.valid && result.errors && result.errors.length > 0) {
      result.errors.forEach((error: string) => {
        markers.push({
          severity: 'error' as const,
          message: error,
          startLineNumber: 1,
          startColumn: 1,
          endLineNumber: 1,
          endColumn: sql.length + 1,
        })
      })
    }
  } catch {
    // 降级到基础检查
    const basicMarkers = basicValidateSql(sql)
    markers.push(...basicMarkers)
  }

  return markers
}

/**
 * 基础 SQL 验证（降级方案）
 */
function basicValidateSql(sql: string): SqlMarkerData[] {
  const markers: SqlMarkerData[] = []
  const lines = sql.split('\n')

  lines.forEach((line, lineIndex) => {
    const lineNum = lineIndex + 1

    const openParens = (line.match(/\(/g) || []).length
    const closeParens = (line.match(/\)/g) || []).length
    if (openParens !== closeParens) {
      markers.push({
        severity: 'warning',
        message: '括号可能未闭合',
        startLineNumber: lineNum,
        startColumn: 1,
        endLineNumber: lineNum,
        endColumn: line.length + 1,
      })
    }

    const singleQuotes = (line.match(/'/g) || []).length
    if (singleQuotes % 2 !== 0) {
      markers.push({
        severity: 'error',
        message: '字符串引号未闭合',
        startLineNumber: lineNum,
        startColumn: 1,
        endLineNumber: lineNum,
        endColumn: line.length + 1,
      })
    }
  })

  return markers
}

/**
 * 格式化 SQL（基于 sqlglot-rust）
 */
export async function formatSql(sql: string, dialect?: SqlDialect): Promise<string> {
  try {
    const result = await invoke<FormatSqlResult>('format_sql', {
      input: {
        sql,
        dialect: dialect || 'generic',
      },
    })

    if (result.success) {
      return result.formatted_sql ?? ''
    } else {
      // 静默返回原始 SQL，不打印警告
      return sql
    }
  } catch {
    // 静默返回原始 SQL
    return sql
  }
}

/**
 * 转译 SQL（基于 sqlglot-rust）
 */
export async function transpileSql(
  sql: string,
  sourceDialect: SqlDialect,
  targetDialect: SqlDialect
): Promise<string> {
  try {
    const result = await invoke<TranspileSqlResult>('transpile_sql', {
      input: {
        sql,
        source_dialect: sourceDialect,
        target_dialect: targetDialect,
      },
    })

    if (result.success) {
      return result.transpiled_sql ?? sql
    }
    return sql
  } catch {
    return sql
  }
}

/**
 * 解析 SQL（基于 sqlglot-rust）
 */
export async function parseSql(
  sql: string,
  dialect?: SqlDialect
): Promise<{ success: boolean; statementsCount: number; error?: string }> {
  try {
    const result = await invoke<ParseSqlResult>('parse_sql', {
      sql,
      dialect: dialect || 'generic',
    })

    return {
      success: result.success,
      statementsCount: result.statements_count,
      error: result.error,
    }
  } catch (error) {
    return {
      success: false,
      statementsCount: 0,
      error: String(error),
    }
  }
}

/**
 * 分割多语句 SQL（基于 sqlglot-rust）
 */
export async function splitSql(sql: string, dialect?: SqlDialect): Promise<string[]> {
  try {
    return await invoke<string[]>('split_sql', {
      sql,
      dialect: dialect ? String(dialect) : null,
    })
  } catch {
    return [sql]
  }
}

/**
 * 生成 DuckDB ATTACH 名称（与后端 `ext_{sanitized_name}` 保持一致）
 */
export function generateAttachName(connectionName: string): string {
  return `ext_${connectionName.replace(/[^a-zA-Z0-9_]/g, '_')}`
}

interface DuckDBExecuteParams {
  sql: string
  connId: string
  dataDir: string
}

/**
 * DuckDB 加速执行 SQL
 */
export async function executeDuckDBAccelerated(
  params: DuckDBExecuteParams
): Promise<SqlExecutionResult> {
  return invoke<SqlExecutionResult>('execute_sql', {
    input: {
      conn_id: params.connId,
      sql: params.sql,
      use_duckdb: true,
      data_dir: params.dataDir,
    },
  })
}

const DUCKDB_SQL_KEYWORDS = new Set([
  'WHERE',
  'SET',
  'VALUES',
  'SELECT',
  'ON',
  'AS',
  'AND',
  'OR',
  'NOT',
  'IN',
  'IS',
  'NULL',
  'TRUE',
  'FALSE',
  'LIKE',
  'BETWEEN',
  'EXISTS',
  'CASE',
  'WHEN',
  'THEN',
  'ELSE',
  'END',
  'GROUP',
  'BY',
  'ORDER',
  'HAVING',
  'LIMIT',
  'OFFSET',
  'UNION',
  'ALL',
  'DISTINCT',
])

const DUCKDB_TABLE_KEYWORDS = [
  'FROM',
  'JOIN',
  'INNER JOIN',
  'LEFT JOIN',
  'RIGHT JOIN',
  'FULL JOIN',
  'CROSS JOIN',
  'NATURAL JOIN',
  'LEFT OUTER JOIN',
  'RIGHT OUTER JOIN',
  'FULL OUTER JOIN',
  'INSERT INTO',
  'UPDATE',
  'INTO',
]

/**
 * 为 DuckDB 联邦查询重写 SQL：给无前缀表名加 ATTACH 前缀
 *
 * 例如：SELECT * FROM users WHERE id = 1
 *    → SELECT * FROM ext_MyConn.users WHERE id = 1
 *
 * 识别范围：FROM | JOIN | INSERT INTO 后的表名
 * 安全过滤：跳过 SQL 关键字、已有 '.' 前缀的表名
 */
export function rewriteDuckDBSQL(sql: string, attachName: string): string {
  if (!attachName) return sql

  let result = sql

  for (const keyword of DUCKDB_TABLE_KEYWORDS) {
    const pattern = new RegExp(
      `(\\b${keyword}\\b)\\s+(?!${attachName.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}\\.)([a-zA-Z_][a-zA-Z0-9_]*)\\b(?!\\s*\\.)`,
      'gi'
    )
    result = result.replace(pattern, (_match, kw, tableName) => {
      if (DUCKDB_SQL_KEYWORDS.has(tableName.toUpperCase())) {
        return _match
      }
      return `${kw} ${attachName}.${tableName}`
    })
  }

  return result
}

export interface ErrorPosition {
  line: number
  column: number
  message: string
}

export function parseErrorPosition(errorMessage: string): ErrorPosition | null {
  const patterns: { regex: RegExp; extract: (m: RegExpExecArray) => ErrorPosition }[] = [
    {
      regex: /at line (\d+)(?:,| at) column (\d+)/i,
      extract: m => ({
        line: parseInt(m[1], 10),
        column: parseInt(m[2], 10),
        message: errorMessage,
      }),
    },
    {
      regex: /line\s*(\d+).*?(?:column|col|char|position)\s*(\d+)/i,
      extract: m => ({
        line: parseInt(m[1], 10),
        column: parseInt(m[2], 10),
        message: errorMessage,
      }),
    },
    {
      regex: /near\s+["'`](.+?)["'`]\s+at line\s+(\d+)/i,
      extract: m => ({
        line: parseInt(m[2], 10),
        column: 1,
        message: errorMessage,
      }),
    },
    {
      regex: /at\s+position:\s*(\d+)/i,
      extract: m => ({
        line: 0,
        column: parseInt(m[1], 10),
        message: errorMessage,
      }),
    },
  ]

  for (const pattern of patterns) {
    const match = pattern.regex.exec(errorMessage)
    if (match) {
      return pattern.extract(match)
    }
  }

  return null
}

export function clearErrorMarkers(view: EditorView): void {
  clearEditorDiagnostics(view)
}

const MAX_SQL_LENGTH = 2000

export function setErrorMarker(view: EditorView, errorMessage: string, sql: string): void {
  const doc = view.state.doc
  const position = parseErrorPosition(errorMessage)

  if (position) {
    let { line, column } = position

    if (line === 0 && column > 0 && sql) {
      const textBefore = sql.slice(0, Math.min(column, sql.length))
      line = (textBefore.match(/\n/g)?.length ?? 0) + 1
      const lastNewline = textBefore.lastIndexOf('\n')
      column = lastNewline >= 0 ? column - lastNewline : column
    }

    line = Math.max(1, Math.min(line, doc.lines))
    try {
      const lineObj = doc.line(line)
      column = Math.max(1, Math.min(column, lineObj.length + 1))
    } catch {
      column = 1
    }

    const diagnostic = createErrorDiagnostic(
      view,
      errorMessage.slice(0, MAX_SQL_LENGTH),
      line,
      column
    )

    setEditorDiagnostics(view, [diagnostic])

    try {
      const pos = doc.line(line).from
      view.dispatch({
        effects: EditorView.scrollIntoView(pos, { y: 'center' }),
        selection: { anchor: pos + column - 1 },
      })
    } catch {
      console.warn('[sql-editor-service] scrollIntoView failed')
    }
    view.focus()
  } else {
    const diagnostic = createErrorDiagnostic(view, errorMessage.slice(0, MAX_SQL_LENGTH), 1, 1)
    setEditorDiagnostics(view, [diagnostic])
  }
}

export interface ParamInfo {
  name: string
  occurrences: number
}

export function detectParams(sql: string): ParamInfo[] {
  const paramMap = new Map<string, number>()
  const regex = /:([a-zA-Z_][a-zA-Z0-9_]*)/g
  let match: RegExpExecArray | null

  while ((match = regex.exec(sql)) !== null) {
    const name = match[1]
    paramMap.set(name, (paramMap.get(name) || 0) + 1)
  }

  const result: ParamInfo[] = []
  paramMap.forEach((occurrences, name) => {
    result.push({ name, occurrences })
  })

  return result
}

export function bindParams(sql: string, values: Record<string, string>): string {
  let result = sql
  for (const [name, value] of Object.entries(values)) {
    const escapedValue = value.replace(/'/g, "''")
    const regex = new RegExp(`:${name}\\b`, 'g')
    result = result.replace(regex, `'${escapedValue}'`)
  }
  return result
}
