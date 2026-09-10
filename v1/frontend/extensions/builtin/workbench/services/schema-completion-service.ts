/**
 * Schema 感知 SQL 自动补全服务
 *
 * 为 CodeMirror 6 提供基于数据库 schema 的智能补全：
 * - 表名补全（FROM/JOIN/INTO 后）
 * - 列名补全（SELECT/WHERE/ON/SET/ORDER BY 后）
 * - 关键字补全（无连接时降级）
 * - 缓存 TTL 30s
 */

import { type Completion, type CompletionContext, type CompletionResult, type CompletionSource } from '@codemirror/autocomplete'
import { syntaxTree } from '@codemirror/language'

import {
  getTablesFromCache,
  getColumnsFromCache,
} from '@/extensions/builtin/database/ui/services/metadata-cache-service'

// ==================== 类型定义 ====================

interface SchemaCache {
  tables: string[]
  /** tableName → columnNames */
  columns: Map<string, string[]>
  views: string[]
  timestamp: number
}

interface SchemaCompletionOptions {
  /** 连接 ID */
  connId: string
  /** 数据库名称 */
  dbName: string
  /** Schema 名称 */
  schemaName: string
  /** 连接类型 */
  connectionType?: 'global' | 'project'
  /** 项目路径（project 连接需要） */
  projectPath?: string
}

// ==================== 常量 ====================

const TTL_MS = 30_000
const MAX_TABLES = 50

const SQL_KEYWORDS = [
  'SELECT', 'FROM', 'WHERE', 'JOIN', 'LEFT JOIN', 'RIGHT JOIN', 'INNER JOIN',
  'OUTER JOIN', 'CROSS JOIN', 'ON', 'GROUP BY', 'ORDER BY', 'HAVING',
  'LIMIT', 'OFFSET', 'FETCH', 'INSERT INTO', 'VALUES', 'UPDATE', 'SET',
  'DELETE FROM', 'CREATE TABLE', 'ALTER TABLE', 'DROP TABLE', 'CREATE INDEX',
  'UNIQUE', 'PRIMARY KEY', 'FOREIGN KEY', 'REFERENCES', 'AS', 'DISTINCT',
  'AND', 'OR', 'NOT', 'IN', 'EXISTS', 'BETWEEN', 'LIKE', 'IS NULL', 'IS NOT NULL',
  'COUNT', 'SUM', 'AVG', 'MAX', 'MIN', 'COALESCE', 'NULLIF', 'CAST',
  'CASE', 'WHEN', 'THEN', 'ELSE', 'END', 'UNION', 'ALL', 'ANY', 'SOME',
  'ASC', 'DESC', 'NULLS FIRST', 'NULLS LAST', 'DEFAULT', 'CHECK',
  'TRUNCATE', 'WITH', 'RECURSIVE', 'RETURNING', 'IF', 'ELSE', 'THEN',
  'BEGIN', 'COMMIT', 'ROLLBACK', 'SAVEPOINT', 'GRANT', 'REVOKE',
  'EXPLAIN', 'ANALYZE', 'VACUUM', 'REINDEX',
]

const SQL_FUNCTIONS = [
  { label: 'COUNT', detail: 'function', info: 'COUNT(expr) — 返回行数' },
  { label: 'SUM', detail: 'function', info: 'SUM(expr) — 返回总和' },
  { label: 'AVG', detail: 'function', info: 'AVG(expr) — 返回平均值' },
  { label: 'MAX', detail: 'function', info: 'MAX(expr) — 返回最大值' },
  { label: 'MIN', detail: 'function', info: 'MIN(expr) — 返回最小值' },
  { label: 'COALESCE', detail: 'function', info: 'COALESCE(val1, val2, ...) — 返回第一个非 NULL 值' },
  { label: 'NULLIF', detail: 'function', info: 'NULLIF(expr1, expr2) — 相等时返回 NULL' },
  { label: 'CAST', detail: 'function', info: 'CAST(expr AS type) — 类型转换' },
  { label: 'CONCAT', detail: 'function', info: 'CONCAT(str1, str2, ...) — 字符串连接' },
  { label: 'SUBSTRING', detail: 'function', info: 'SUBSTRING(str, start, length) — 子字符串' },
  { label: 'UPPER', detail: 'function', info: 'UPPER(str) — 转为大写' },
  { label: 'LOWER', detail: 'function', info: 'LOWER(str) — 转为小写' },
  { label: 'TRIM', detail: 'function', info: 'TRIM(str) — 去除首尾空格' },
  { label: 'LENGTH', detail: 'function', info: 'LENGTH(str) — 字符串长度' },
  { label: 'REPLACE', detail: 'function', info: 'REPLACE(str, from, to) — 字符串替换' },
  { label: 'NOW', detail: 'function', info: 'NOW() — 当前日期时间' },
  { label: 'CURRENT_DATE', detail: 'function', info: 'CURRENT_DATE — 当前日期' },
  { label: 'CURRENT_TIMESTAMP', detail: 'function', info: 'CURRENT_TIMESTAMP — 当前时间戳' },
  { label: 'DATEADD', detail: 'function', info: 'DATEADD(part, n, date) — 日期加法' },
  { label: 'DATEDIFF', detail: 'function', info: 'DATEDIFF(part, d1, d2) — 日期差' },
  { label: 'EXTRACT', detail: 'function', info: 'EXTRACT(part FROM date) — 提取日期部分' },
  { label: 'ROW_NUMBER', detail: 'function', info: 'ROW_NUMBER() OVER(...) — 行号窗口函数' },
  { label: 'RANK', detail: 'function', info: 'RANK() OVER(...) — 排名窗口函数' },
  { label: 'DENSE_RANK', detail: 'function', info: 'DENSE_RANK() OVER(...) — 密集排名窗口函数' },
  { label: 'LAG', detail: 'function', info: 'LAG(expr) OVER(...) — 前一行值' },
  { label: 'LEAD', detail: 'function', info: 'LEAD(expr) OVER(...) — 后一行值' },
  { label: 'OVER', detail: 'keyword', info: 'OVER(PARTITION BY ... ORDER BY ...) — 窗口函数子句' },
  { label: 'PARTITION BY', detail: 'keyword', info: 'PARTITION BY col — 窗口分区' },
  { label: 'IFNULL', detail: 'function', info: 'IFNULL(expr, default) — MySQL: NULL 替换' },
  { label: 'NVL', detail: 'function', info: 'NVL(expr, default) — Oracle: NULL 替换' },
  { label: 'IIF', detail: 'function', info: 'IIF(condition, t, f) — 条件表达式' },
  { label: 'GREATEST', detail: 'function', info: 'GREATEST(a, b, ...) — 返回最大值' },
  { label: 'LEAST', detail: 'function', info: 'LEAST(a, b, ...) — 返回最小值' },
  { label: 'ROUND', detail: 'function', info: 'ROUND(n, d) — 四舍五入' },
  { label: 'FLOOR', detail: 'function', info: 'FLOOR(n) — 向下取整' },
  { label: 'CEIL', detail: 'function', info: 'CEIL(n) — 向上取整' },
  { label: 'ABS', detail: 'function', info: 'ABS(n) — 绝对值' },
  { label: 'MOD', detail: 'function', info: 'MOD(a, b) — 取模' },
  { label: 'POWER', detail: 'function', info: 'POWER(base, exp) — 幂运算' },
  { label: 'SQRT', detail: 'function', info: 'SQRT(n) — 平方根' },
  { label: 'RAND', detail: 'function', info: 'RAND() — 随机数' },
]

// ==================== 缓存管理 ====================

const schemaCacheMap = new Map<string, SchemaCache>()

function getCacheKey(connId: string, dbName: string, schemaName: string): string {
  return `${connId}:${dbName}:${schemaName}`
}

async function loadSchemaCache(opts: SchemaCompletionOptions): Promise<SchemaCache> {
  const { connId, dbName, schemaName, connectionType, projectPath } = opts
  const cacheKey = getCacheKey(connId, dbName, schemaName)

  const cached = schemaCacheMap.get(cacheKey)
  if (cached && Date.now() - cached.timestamp < TTL_MS) {
    return cached
  }

  try {
    const type = connectionType ?? 'global'
    const tables = await getTablesFromCache(connId, type, dbName, schemaName, projectPath)

    const tableNames = tables.map(t => t.name).slice(0, MAX_TABLES)
    const columnsMap = new Map<string, string[]>()

    // 批量加载列（并发限制）
    const batchSize = 10
    for (let i = 0; i < tableNames.length; i += batchSize) {
      const batch = tableNames.slice(i, i + batchSize)
      const results = await Promise.allSettled(
        batch.map(async (tableName) => {
          const cols = await getColumnsFromCache(connId, type, dbName, schemaName, tableName, projectPath)
          return { tableName, columns: cols.map(c => c.name) }
        })
      )
      for (const result of results) {
        if (result.status === 'fulfilled') {
          columnsMap.set(result.value.tableName, result.value.columns)
        }
      }
    }

    const schema: SchemaCache = {
      tables: tableNames,
      columns: columnsMap,
      views: [],
      timestamp: Date.now(),
    }
    schemaCacheMap.set(cacheKey, schema)
    return schema
  } catch {
    return { tables: [], columns: new Map(), views: [], timestamp: Date.now() }
  }
}

/** 清除指定连接的 schema 缓存 */
export function invalidateSchemaCache(connId: string, dbName?: string, schemaName?: string): void {
  if (dbName && schemaName) {
    schemaCacheMap.delete(getCacheKey(connId, dbName, schemaName))
  } else {
    // 清除该连接的所有缓存
    const prefix = `${connId}:`
    for (const key of schemaCacheMap.keys()) {
      if (key.startsWith(prefix)) {
        schemaCacheMap.delete(key)
      }
    }
  }
}

// ==================== SQL 上下文分析 ====================

/** 判断节点是否在 FROM 子句后（需要表名） */
function isAfterFrom(node: ReturnType<typeof syntaxTree>['topNode'] | null, text: string): boolean {
  if (!node) return false
  const parent = node.parent
  if (!parent) return false

  // 检查父节点类型
  if (parent.name === 'FromClause' || parent.name === 'JoinClause') return true
  if (parent.name === 'UpdateStmt') return true
  if (parent.name === 'InsertStmt') return true

  // 检查关键字：当前光标在 FROM / JOIN / INTO / UPDATE 后
  const before = text.slice(Math.max(0, node.from - 20), node.from).toUpperCase()
  return /\b(FROM|JOIN|INTO|UPDATE)\s*$/i.test(before)
}

/** 判断节点是否在 SELECT 后（需要列名或函数） */
function isAfterSelect(node: ReturnType<typeof syntaxTree>['topNode'] | null, text: string): boolean {
  if (!node) return false
  const parent = node.parent
  if (parent?.name === 'Selection') return true

  const before = text.slice(Math.max(0, node.from - 20), node.from).toUpperCase()
  return /\bSELECT\s*$/i.test(before)
}

/** 判断节点是否在 WHERE/ON/AND/OR 后（需要列名） */
function isAfterWhere(node: ReturnType<typeof syntaxTree>['topNode'] | null, text: string): boolean {
  if (!node) return false
  const parent = node.parent
  if (parent?.name === 'WhereClause') return true
  if (parent?.name === 'OnClause') return true

  const before = text.slice(Math.max(0, node.from - 20), node.from).toUpperCase()
  return /\b(WHERE|AND|OR|ON|HAVING)\s*$/i.test(before)
}

/** 判断是否在 SET 子句后（UPDATE SET / INSERT SET） */
function isAfterSet(node: ReturnType<typeof syntaxTree>['topNode'] | null, text: string): boolean {
  if (!node) return false
  const before = text.slice(Math.max(0, node.from - 20), node.from).toUpperCase()
  return /\bSET\s*$/i.test(before)
}

/** 判断是否在 GROUP BY / ORDER BY 后 */
function isAfterGroupOrder(node: ReturnType<typeof syntaxTree>['topNode'] | null, text: string): boolean {
  if (!node) return false
  const before = text.slice(Math.max(0, node.from - 30), node.from).toUpperCase()
  return /\b(GROUP\s+BY|ORDER\s+BY)\s*$/i.test(before)
}

/** 判断是否在表别名后（需要列名，如 `t.`） */
function isAfterTableAlias(node: ReturnType<typeof syntaxTree>['topNode'] | null, text: string): boolean {
  if (!node) return false
  const before = node.from > 0 ? text[node.from - 1] : ''
  return before === '.'
}

// ==================== Completion 构建 ====================

function buildTableCompletions(tables: string[]): Completion[] {
  return tables.map(name => ({
    label: name,
    type: 'type' as const,
    detail: 'table',
    boost: 3,
  }))
}

function buildColumnCompletions(
  columns: Map<string, string[]>,
  allTables: string[]
): Completion[] {
  const result: Completion[] = []

  for (const tableName of allTables) {
    const cols = columns.get(tableName)
    if (cols) {
      for (const col of cols) {
        result.push({
          label: col,
          type: 'property' as const,
          detail: tableName,
          boost: 2,
        })
      }
    }
  }

  return result
}

function buildTableAliasColumnCompletions(
  columns: Map<string, string[]>,
  allTables: string[]
): Completion[] {
  const result: Completion[] = []

  for (const tableName of allTables) {
    const cols = columns.get(tableName)
    if (cols) {
      for (const col of cols) {
        result.push({
          label: col,
          type: 'property' as const,
          detail: `${tableName}.${col}`,
          boost: 4,
        })
      }
    }
  }

  return result
}

function buildKeywordCompletions(): Completion[] {
  return SQL_KEYWORDS.map(kw => ({
    label: kw,
    type: 'keyword' as const,
    boost: 1,
  }))
}

function buildFunctionCompletions(): Completion[] {
  return SQL_FUNCTIONS.map(fn => ({
    label: fn.label,
    type: 'function' as const,
    detail: fn.detail,
    info: fn.info,
    boost: 2,
  }))
}

// ==================== CompletionSource 工厂 ====================

/**
 * 创建 Schema 感知的 CompletionSource
 *
 * @param optionsFactory - 返回当前连接信息的工厂函数（每次补全触发时调用）
 * @returns CodeMirror CompletionSource
 */
export function createSchemaCompletionSource(
  optionsFactory: () => SchemaCompletionOptions | null
): CompletionSource {
  return async (context: CompletionContext): Promise<CompletionResult | null> => {
    const opts = optionsFactory()
    const word = context.matchBefore(/\w*/)
    const node = syntaxTree(context.state).resolveInner(context.pos, -1)
    const text = context.state.doc.toString()
    const from = word ? word.from : context.pos

    /** 将 Completion[] 包装为 CompletionResult */
    function wrap(options: Completion[]): CompletionResult {
      return { from, options, filter: true }
    }

    // 情况 1：无连接 → 仅关键字 + 函数
    if (!opts) {
      const all = [...buildKeywordCompletions(), ...buildFunctionCompletions()]
      if (!word || word.text === '') return wrap(all)
      const prefix = word.text.toLowerCase()
      return wrap(all.filter(c => c.label.toLowerCase().startsWith(prefix)))
    }

    // 加载 schema 缓存
    const schema = await loadSchemaCache(opts)
    const hasSchema = schema.tables.length > 0

    // 情况 2：表别名后 → 仅列名
    if (isAfterTableAlias(node, text)) {
      const cols = buildTableAliasColumnCompletions(schema.columns, schema.tables)
      if (!word || word.text === '') return wrap(cols)
      const prefix = word.text.toLowerCase()
      return wrap(cols.filter(c => c.label.toLowerCase().startsWith(prefix)))
    }

    // 情况 3：FROM/JOIN/INTO 后 → 表名优先
    if (isAfterFrom(node, text)) {
      const tableCompletions = buildTableCompletions(schema.tables)
      if (!word || word.text === '') return wrap(tableCompletions)
      const prefix = word.text.toLowerCase()
      return wrap(tableCompletions.filter(c => c.label.toLowerCase().startsWith(prefix)))
    }

    // 情况 4：SELECT 后 → 列名 + 函数 + 关键字
    if (isAfterSelect(node, text)) {
      const results: Completion[] = []
      if (hasSchema) {
        results.push(...buildColumnCompletions(schema.columns, schema.tables))
      }
      results.push(...buildFunctionCompletions())
      results.push(...buildKeywordCompletions())

      if (!word || word.text === '') return wrap(results)
      const prefix = word.text.toLowerCase()
      return wrap(results.filter(c => c.label.toLowerCase().startsWith(prefix)))
    }

    // 情况 5：WHERE/ON/AND/OR/HAVING 后 → 列名 + 函数 + 关键字
    if (isAfterWhere(node, text)) {
      const results: Completion[] = []
      if (hasSchema) {
        results.push(...buildColumnCompletions(schema.columns, schema.tables))
      }
      results.push(...buildFunctionCompletions())
      results.push(...buildKeywordCompletions())

      if (!word || word.text === '') return wrap(results)
      const prefix = word.text.toLowerCase()
      return wrap(results.filter(c => c.label.toLowerCase().startsWith(prefix)))
    }

    // 情况 6：SET 后 → 列名
    if (isAfterSet(node, text)) {
      const cols = buildTableAliasColumnCompletions(schema.columns, schema.tables)
      if (!word || word.text === '') return wrap(cols)
      const prefix = word.text.toLowerCase()
      return wrap(cols.filter(c => c.label.toLowerCase().startsWith(prefix)))
    }

    // 情况 7：GROUP BY / ORDER BY 后 → 列名
    if (isAfterGroupOrder(node, text)) {
      const cols = buildColumnCompletions(schema.columns, schema.tables)
      if (!word || word.text === '') return wrap(cols)
      const prefix = word.text.toLowerCase()
      return wrap(cols.filter(c => c.label.toLowerCase().startsWith(prefix)))
    }

    // 情况 8：默认 → 所有补全
    const results: Completion[] = []
    if (hasSchema) {
      results.push(...buildTableCompletions(schema.tables))
      results.push(...buildColumnCompletions(schema.columns, schema.tables))
    }
    results.push(...buildKeywordCompletions())
    results.push(...buildFunctionCompletions())

    if (!word || word.text === '') return wrap(results)
    const prefix = word.text.toLowerCase()
    return wrap(results.filter(c => c.label.toLowerCase().startsWith(prefix)))
  }
}