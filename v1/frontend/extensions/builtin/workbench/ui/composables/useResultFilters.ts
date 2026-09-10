import {
  reExecuteWithFilter as apiExecuteWithFilter,
  executeDuckdbAnalysis as apiDuckdbAnalysis,
  createDuckdbTempTable as apiCreateTempTable,
} from '../services/result-analysis'

import type { ResultTab } from '../types/result'
import type { GridApi, IRowNode } from 'ag-grid-community'
import type { Ref } from 'vue'

type FilterGridApi = GridApi & { setQuickFilter: (value: string) => void }

interface MessageApi {
  success: (msg: string) => void
  error: (msg: string) => void
  loading: (msg: string, opts?: Record<string, unknown>) => { destroy: () => void }
  info: (msg: string) => void
}

function buildObjectRows(columns: string[], rows: unknown[][]): Record<string, unknown>[] {
  return rows.map(row => {
    const obj: Record<string, unknown> = {}
    columns.forEach((col, i) => {
      obj[col.replace(/\./g, '_')] = row[i]
    })
    return obj
  })
}

/**
 * 安全引用 SQL 标识符（列名/表名），用双引号包裹并转义内部双引号。
 * 防止 SQL 注入：标识符可能包含特殊字符或恶意内容。
 */
function quoteIdentifier(ident: string): string {
  return `"${String(ident).replace(/"/g, '""')}"`
}

export function useResultFilters(
  gridApi: Ref<GridApi | null>,
  message: MessageApi,
  t: (key: string, opts?: Record<string, unknown>) => string
) {
  function applyQuickFilter(tab: ResultTab, expr: string) {
    if (!gridApi.value) return
    ;(gridApi.value as FilterGridApi).setQuickFilter(expr)
    tab.filteredRowCount = gridApi.value.getDisplayedRowCount()
    tab.displayedRowCount = tab.filteredRowCount
  }

  function clearQuickFilter(tab: ResultTab) {
    tab.quickFilterExpression = ''
    if (gridApi.value) {
      ;(gridApi.value as FilterGridApi).setQuickFilter('')
      tab.filteredRowCount = tab.originalRowCount
      tab.displayedRowCount = tab.originalRowCount
    }
  }

  async function executeSqlFilter(tab: ResultTab) {
    const whereClause = tab.sqlFilterExpression.trim()
    if (!whereClause) return
    tab.isSqlFilterLoading = true
    try {
      const result = await apiExecuteWithFilter(tab.connectionId, tab.originalSql, whereClause)
      tab.columns = result.columns
      tab.rows = result.rows
      tab.objectRows = buildObjectRows(result.columns, result.rows)
      tab.originalRowCount = result.rows.length
      tab.displayedRowCount = result.rows.length
      tab.executionTime = result.elapsed_ms
      if (result.temp_table) tab.duckdbTempTable = result.temp_table
    } catch (e: unknown) {
      message.error(String(e))
    } finally {
      tab.isSqlFilterLoading = false
    }
  }

  async function executeDuckdbAnalysis(tab: ResultTab) {
    const sql = tab.duckdbSql.trim()
    if (!sql) return
    tab.isDuckdbLoading = true
    try {
      const hasTempTable = !!tab.duckdbTempTable
      const result = await apiDuckdbAnalysis(
        tab.duckdbTempTable,
        sql,
        hasTempTable ? undefined : tab.columns,
        hasTempTable ? undefined : (tab.rows as unknown[][])
      )
      tab.columns = result.columns
      tab.rows = result.rows
      tab.objectRows = buildObjectRows(result.columns, result.rows)
      tab.displayedRowCount = result.rows.length
      tab.executionTime = result.elapsed_ms
      tab.isAnalysisActive = true
      tab.timestamp = new Date().toLocaleString()
    } catch (e: unknown) {
      message.error(String(e))
    } finally {
      tab.isDuckdbLoading = false
    }
  }

  function clearDuckdbAnalysis(tab: ResultTab) {
    tab.isAnalysisActive = false
    tab.filterMode = 'quick'
    tab.quickFilterExpression = ''
    tab.sqlFilterExpression = ''
    tab.duckdbSql = ''
  }

  function quickDuckdbAction(tab: ResultTab, type: string) {
    const table = quoteIdentifier(tab.duckdbTempTable || 'result_temp')
    if (type === 'count') tab.duckdbSql = `SELECT COUNT(*) FROM ${table}`
    else if (type === 'distinct') tab.duckdbSql = `SELECT DISTINCT * FROM ${table} LIMIT 100`
    else if (type === 'group') {
      const firstCol = tab.columns[0] || 'col1'
      tab.duckdbSql = `SELECT ${quoteIdentifier(firstCol)}, COUNT(*) FROM ${table} GROUP BY ${quoteIdentifier(firstCol)} ORDER BY 2 DESC`
    }
  }

  async function handleBridgeFilter(tab: ResultTab) {
    if (!gridApi.value) return
    const visibleRows: unknown[] = []
    gridApi.value.forEachNodeAfterFilter((node: IRowNode) => visibleRows.push(node.data))
    if (visibleRows.length === 0) return
    tab.isDuckdbLoading = true
    try {
      const rowsData: unknown[][] = (visibleRows as Record<string, unknown>[]).map(row =>
        tab.columns.map(col => row[col] ?? null)
      )
      const tableName = await apiCreateTempTable(tab.columns, rowsData)
      tab.duckdbTempTable = tableName
      tab.duckdbSql = `SELECT * FROM ${quoteIdentifier(tableName)} LIMIT 100`
      message.success(`${t('resultPanel.rows')}: ${visibleRows.length} → DuckDB`)
    } catch (e: unknown) {
      message.error(String(e))
    } finally {
      tab.isDuckdbLoading = false
    }
  }

  const modeLabel: Record<string, string> = {
    quick: t('resultPanel.instantFilter'),
    sql: t('resultPanel.sqlFilter'),
    duckdb: t('resultPanel.duckdbAnalysis'),
  }

  return {
    applyQuickFilter,
    clearQuickFilter,
    executeSqlFilter,
    executeDuckdbAnalysis,
    clearDuckdbAnalysis,
    quickDuckdbAction,
    handleBridgeFilter,
    modeLabel,
    buildObjectRows,
  }
}
