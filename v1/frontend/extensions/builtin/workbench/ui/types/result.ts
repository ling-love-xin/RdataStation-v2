export type FilterMode = 'quick' | 'sql' | 'duckdb'

export type ViewMode = 'grid' | 'text' | 'record' | 'chart'

export type ExportFormat = 'csv' | 'json' | 'insert' | 'xlsx' | 'parquet' | 'sql'

export interface QueryResult {
  columns: string[]
  rows: unknown[][]
  rowCount: number
  elapsedMs: number
  tempTable?: string
  affectedRows?: number
}

export interface ResultTab {
  readonly id: string
  title: string
  originalSql: string
  tableName: string
  connectionId: string
  duckdbTempTable: string
  isLoading: boolean
  columns: string[]
  rows: unknown[][]
  objectRows: Record<string, unknown>[]
  page: number
  pageSize: number
  originalRowCount: number
  displayedRowCount: number
  filterMode: FilterMode

  quickFilterExpression: string
  filteredRowCount: number

  sqlFilterExpression: string
  isSqlFilterLoading: boolean

  duckdbSql: string
  isDuckdbLoading: boolean
  isAnalysisActive: boolean

  executionTime: number
  timestamp: string

  dirtyRows: Set<number>
}

export interface ExecutionMeta {
  dbDuration: number
  duckdbDuration: number
  timestamp: string
}

export interface ResultData {
  columns: string[]
  rows: unknown[][]
}

export type MenuActionCategory = 'edit' | 'filter' | 'sort' | 'view' | 'export' | 'meta'

export interface MenuContext {
  value: unknown
  column: string
  columnIndex: number
  rowIndex: number
  tab: ResultTab
}

export interface ExportOptions {
  format: ExportFormat
  includeHeaders: boolean
  nullAs: string
  delimiter?: string
  quoteFields?: boolean
  prettyPrint?: boolean
  includeSchema?: boolean
}

export interface FilterPreset {
  id: string
  name: string
  filterMode: FilterMode
  expression: string
  createdAt: string
}

export interface CellUpdateInput {
  connId: string
  tableName: string
  columnName: string
  newValue: unknown
  rowIdentity: Record<string, unknown>
}

export interface CellUpdateResult {
  success: boolean
  affectedRows: number
  message: string
}

export interface MultiRuleResult {
  data: unknown
  quality?: {
    checks: Array<{
      rule: string
      passed: boolean
      message?: string
    }>
  } | null
}
