import { defineStore } from 'pinia'
import { ref, computed } from 'vue'

import { copyToClipboard } from '@/shared/utils/clipboard'

import {
  reExecuteWithFilter,
  executeDuckdbAnalysis as executeDuckdbApi,
  createDuckdbTempTable,
  saveCellUpdate as apiSaveCellUpdate,
} from '../services/result-analysis'

import type {
  FilterMode,
  QueryResult,
  ResultTab,
  ExportFormat,
  ExportOptions,
} from '../types/result'

let tabCounter = 0

export const useResultStore = defineStore('result', () => {
  // ========== 按 editorId 隔离的状态 ==========
  const tabsMap = ref<Map<string, ResultTab[]>>(new Map())
  const activeTabIdMap = ref<Map<string, string | null>>(new Map())
  const showPanelMap = ref<Map<string, boolean>>(new Map())
  const currentEditorId = ref<string | null>(null)

  // ========== 兼容旧的全局访问（基于 currentEditorId） ==========
  const tabs = computed(() => {
    const eid = currentEditorId.value
    if (!eid) return []
    return tabsMap.value.get(eid) ?? []
  })

  const activeTabId = computed(() => {
    const eid = currentEditorId.value
    if (!eid) return null
    return activeTabIdMap.value.get(eid) ?? null
  })

  const showPanel = computed(() => {
    const eid = currentEditorId.value
    if (!eid) return false
    return showPanelMap.value.get(eid) ?? false
  })

  const activeTab = computed(() => {
    const eid = currentEditorId.value
    if (!eid) return null
    const ts = tabsMap.value.get(eid) ?? []
    const aid = activeTabIdMap.value.get(eid) ?? null
    return ts.find(t => t.id === aid) ?? null
  })

  const isAnyLoading = computed(() => {
    const ts = tabs.value
    return ts.some(t => t.isSqlFilterLoading || t.isDuckdbLoading)
  })

  const activeTabPagedRows = computed(() => {
    const tab = activeTab.value
    if (!tab) return []
    const start = tab.page * tab.pageSize
    const end = start + tab.pageSize
    return tab.objectRows.slice(start, end)
  })

  const activeTabTotalPages = computed(() => {
    const tab = activeTab.value
    if (!tab || tab.displayedRowCount === 0) return 0
    return Math.ceil(tab.displayedRowCount / tab.pageSize)
  })

  // ========== 辅助方法 ==========
  function getOrCreateTabs(editorId: string): ResultTab[] {
    if (!tabsMap.value.has(editorId)) {
      tabsMap.value.set(editorId, [])
    }
    return tabsMap.value.get(editorId) ?? []
  }

  function setCurrentEditor(editorId: string | null): void {
    currentEditorId.value = editorId
  }

  function getTabs(editorId: string): ResultTab[] {
    return tabsMap.value.get(editorId) ?? []
  }

  function getActiveTab(editorId: string): ResultTab | null {
    const ts = tabsMap.value.get(editorId) ?? []
    const aid = activeTabIdMap.value.get(editorId) ?? null
    return ts.find(t => t.id === aid) ?? null
  }

  function getShowPanel(editorId: string): boolean {
    return showPanelMap.value.get(editorId) ?? false
  }

  // ========== Tab 操作（带 editorId 参数） ==========
  function createTab(sql: string, connectionId: string): ResultTab {
    tabCounter++
    const id = `result_${Date.now()}_${tabCounter}`
    return {
      id,
      title: `结果 #${tabCounter}`,
      originalSql: sql || '',
      tableName: '',
      connectionId: connectionId || '',
      duckdbTempTable: '',
      isLoading: false,
      columns: [],
      rows: [],
      objectRows: [],
      page: 0,
      pageSize: 100,
      originalRowCount: 0,
      displayedRowCount: 0,
      filterMode: 'quick' as FilterMode,
      quickFilterExpression: '',
      filteredRowCount: 0,
      sqlFilterExpression: '',
      isSqlFilterLoading: false,
      duckdbSql: '',
      isDuckdbLoading: false,
      isAnalysisActive: false,
      executionTime: 0,
      timestamp: '',
      dirtyRows: new Set(),
    }
  }

  function addTab(editorId: string, sql: string, connectionId: string): ResultTab {
    const tab = createTab(sql, connectionId)
    const ts = getOrCreateTabs(editorId)
    ts.push(tab)
    activeTabIdMap.value.set(editorId, tab.id)
    showPanelMap.value.set(editorId, true)
    return tab
  }

  function setTabResult(editorId: string, id: string, result: QueryResult): void {
    const ts = tabsMap.value.get(editorId)
    if (!ts) return
    const tab = ts.find(t => t.id === id)
    if (!tab) return

    tab.columns = result.columns
    tab.rows = result.rows
    tab.objectRows = convertRowsToObjects(result.columns, result.rows)
    tab.tableName = extractTableName(tab.originalSql, tab.columns[0] ?? '')
    tab.page = 0
    tab.originalRowCount = result.rowCount
    tab.displayedRowCount = result.rowCount
    tab.executionTime = result.elapsedMs
    tab.timestamp = new Date().toLocaleString()
    tab.filterMode = 'quick'
    tab.quickFilterExpression = ''
    tab.isAnalysisActive = false

    if (result.tempTable) {
      tab.duckdbTempTable = result.tempTable
    }

    showPanelMap.value.set(editorId, true)
  }

  function convertRowsToObjects(columns: string[], rows: unknown[][]): Record<string, unknown>[] {
    // 检测列名冲突：如果 replace 后存在重复键，使用原始列名
    const safeKeys = columns.map((col, i) => {
      const replaced = col.replace(/\./g, '_')
      // 检查是否与其他列冲突
      const conflictIdx = columns.findIndex((c, j) => j !== i && c.replace(/\./g, '_') === replaced)
      if (conflictIdx >= 0) {
        return `_col${i}_${replaced}`
      }
      return replaced
    })
    return rows.map(row => {
      const obj: Record<string, unknown> = {}
      columns.forEach((col, i) => {
        obj[safeKeys[i]] = row[i]
      })
      return obj
    })
  }

  function closeTab(editorId: string, id: string): void {
    const ts = tabsMap.value.get(editorId)
    if (!ts) return
    const idx = ts.findIndex(t => t.id === id)
    if (idx === -1) return
    const tab = ts[idx]
    // 释放 DuckDB 临时表引用（后端会在连接关闭时自动清理）
    if (tab.duckdbTempTable) {
      tab.duckdbTempTable = ''
    }
    ts.splice(idx, 1)
    const currentAid = activeTabIdMap.value.get(editorId)
    if (currentAid === id) {
      activeTabIdMap.value.set(editorId, ts[idx]?.id || ts[idx - 1]?.id || null)
    }
    if (ts.length === 0) {
      showPanelMap.value.set(editorId, false)
    }
  }

  function switchTab(editorId: string, id: string): void {
    activeTabIdMap.value.set(editorId, id)
  }

  function closeAllTabs(editorId: string): void {
    tabsMap.value.set(editorId, [])
    activeTabIdMap.value.set(editorId, null)
    showPanelMap.value.set(editorId, false)
  }

  function removeTabResult(editorId: string, id: string): void {
    const ts = tabsMap.value.get(editorId)
    if (!ts) return
    const tab = ts.find(t => t.id === id)
    if (!tab) return
    tab.columns = []
    tab.rows = []
    tab.originalRowCount = 0
    tab.displayedRowCount = 0
    tab.duckdbTempTable = ''
  }

  function setFilterMode(editorId: string, id: string, mode: FilterMode): void {
    const ts = tabsMap.value.get(editorId)
    if (!ts) return
    const tab = ts.find(t => t.id === id)
    if (!tab) return
    tab.filterMode = mode
  }

  function applyQuickFilter(editorId: string, id: string, expression: string): void {
    const ts = tabsMap.value.get(editorId)
    if (!ts) return
    const tab = ts.find(t => t.id === id)
    if (!tab) return
    tab.quickFilterExpression = expression
  }

  function clearFilter(editorId: string, id: string): void {
    const ts = tabsMap.value.get(editorId)
    if (!ts) return
    const tab = ts.find(t => t.id === id)
    if (!tab) return
    tab.filterMode = 'quick'
    tab.quickFilterExpression = ''
    tab.sqlFilterExpression = ''
    tab.duckdbSql = ''
    tab.isAnalysisActive = false
    tab.displayedRowCount = tab.originalRowCount
    tab.filteredRowCount = tab.originalRowCount
  }

  async function executeSqlFilter(
    editorId: string,
    id: string,
    whereClause: string
  ): Promise<void> {
    const ts = tabsMap.value.get(editorId)
    if (!ts) return
    const tab = ts.find(t => t.id === id)
    if (!tab || !whereClause.trim()) return

    tab.isSqlFilterLoading = true
    try {
      const result = await reExecuteWithFilter(tab.connectionId, tab.originalSql, whereClause)
      tab.columns = result.columns
      tab.rows = result.rows
      tab.objectRows = convertRowsToObjects(result.columns, result.rows)
      // 保留原始行数，只更新过滤后的显示行数
      tab.displayedRowCount = result.rows.length
      tab.filteredRowCount = result.rows.length
      tab.executionTime = result.elapsed_ms
      if (result.temp_table) tab.duckdbTempTable = result.temp_table
    } finally {
      tab.isSqlFilterLoading = false
    }
  }

  async function executeDuckdbAnalysis(
    editorId: string,
    id: string,
    duckSql: string
  ): Promise<void> {
    const ts = tabsMap.value.get(editorId)
    if (!ts) return
    const tab = ts.find(t => t.id === id)
    if (!tab || !duckSql.trim()) return

    tab.isDuckdbLoading = true
    try {
      const hasTempTable = !!tab.duckdbTempTable
      const result = await executeDuckdbApi(
        tab.duckdbTempTable,
        duckSql,
        hasTempTable ? undefined : tab.columns,
        hasTempTable ? undefined : (tab.rows as unknown[][])
      )
      tab.columns = result.columns
      tab.rows = result.rows
      tab.objectRows = convertRowsToObjects(result.columns, result.rows)
      tab.displayedRowCount = result.rows.length
      tab.executionTime = result.elapsed_ms
      tab.isAnalysisActive = true
      tab.timestamp = new Date().toLocaleString()
    } finally {
      tab.isDuckdbLoading = false
    }
  }

  async function bridgeFilterFromDuckdb(
    editorId: string,
    id: string,
    visibleRows: Record<string, unknown>[]
  ): Promise<void> {
    const ts = tabsMap.value.get(editorId)
    if (!ts) return
    const tab = ts.find(t => t.id === id)
    if (!tab || visibleRows.length === 0) return

    tab.isDuckdbLoading = true
    try {
      const rowsData: unknown[][] = visibleRows.map(row =>
        tab.columns.map(col => (row[col] ?? null) as unknown)
      )
      const tableName = await createDuckdbTempTable(tab.columns, rowsData)
      tab.duckdbTempTable = tableName
      tab.duckdbSql = `SELECT * FROM ${tableName} LIMIT 100`
    } finally {
      tab.isDuckdbLoading = false
    }
  }

  async function ensureDuckdbTable(editorId: string, id: string): Promise<void> {
    const ts = tabsMap.value.get(editorId)
    if (!ts) return
    const tab = ts.find(t => t.id === id)
    if (!tab || tab.duckdbTempTable || tab.columns.length === 0 || tab.rows.length === 0) return

    try {
      const tableName = await createDuckdbTempTable(tab.columns, tab.rows)
      if (tableName) tab.duckdbTempTable = tableName
    } catch {
      /* silent */
    }
  }

  async function exportTab(
    editorId: string,
    id: string,
    format: ExportFormat,
    _options?: ExportOptions
  ): Promise<void> {
    const ts = tabsMap.value.get(editorId)
    if (!ts) return
    const tab = ts.find(t => t.id === id)
    if (!tab) return

    if (format === 'csv') {
      const escapeCsv = (v: unknown): string => {
        if (v === null || v === undefined) return ''
        const s = String(v)
        if (s.includes(',') || s.includes('"') || s.includes('\n') || s.includes('\r')) {
          return `"${s.replace(/"/g, '""')}"`
        }
        return s
      }
      const header = tab.columns.map(c => escapeCsv(c)).join(',')
      const body = tab.rows.map(r => r.map(v => escapeCsv(v)).join(','))
      const csv = [header, ...body].join('\n')
      await copyToClipboard(csv)
    } else if (format === 'json') {
      const data = tab.rows.map(r => {
        const obj: Record<string, unknown> = {}
        tab.columns.forEach((c, i) => {
          obj[c] = r[i]
        })
        return obj
      })
      await copyToClipboard(JSON.stringify(data, null, 2))
    } else if (format === 'insert') {
      const tableName = tab.tableName || 'table_name'
      const colList = tab.columns.map(c => `\`${c}\``).join(', ')
      const values = tab.objectRows.map(row => {
        const vals = tab.columns.map(col => {
          const key = col.replace(/\./g, '_')
          const val = (row as Record<string, unknown>)[key]
          if (val === null || val === undefined) return 'NULL'
          if (typeof val === 'boolean') return val ? 'TRUE' : 'FALSE'
          if (typeof val === 'number') return String(val)
          return `'${String(val).replace(/'/g, "''")}'`
        })
        return `(${vals.join(', ')})`
      })
      const sql = `INSERT INTO \`${tableName}\` (${colList}) VALUES\n${values.join(',\n')};`
      await copyToClipboard(sql)
    }
  }

  function markCellDirty(editorId: string, tabId: string, rowIndex: number): void {
    const ts = tabsMap.value.get(editorId)
    if (!ts) return
    const tab = ts.find(t => t.id === tabId)
    if (!tab) return
    tab.dirtyRows.add(rowIndex)
  }

  function resetDirtyCells(editorId: string, tabId: string): void {
    const ts = tabsMap.value.get(editorId)
    if (!ts) return
    const tab = ts.find(t => t.id === tabId)
    if (tab) tab.dirtyRows.clear()
  }

  function setPage(editorId: string, id: string, page: number): void {
    const ts = tabsMap.value.get(editorId)
    if (!ts) return
    const tab = ts.find(t => t.id === id)
    if (!tab) return
    const maxPage = Math.max(0, Math.ceil(tab.displayedRowCount / tab.pageSize) - 1)
    tab.page = Math.max(0, Math.min(page, maxPage))
  }

  function setPageSize(editorId: string, id: string, size: number): void {
    const ts = tabsMap.value.get(editorId)
    if (!ts) return
    const tab = ts.find(t => t.id === id)
    if (!tab || size < 10) return
    tab.pageSize = size
    tab.page = 0
  }

  function nextPage(editorId: string, id: string): void {
    const ts = tabsMap.value.get(editorId)
    if (!ts) return
    const tab = ts.find(t => t.id === id)
    if (!tab) return
    const maxPage = Math.max(0, Math.ceil(tab.displayedRowCount / tab.pageSize) - 1)
    if (tab.page < maxPage) {
      tab.page++
    }
  }

  function prevPage(editorId: string, id: string): void {
    const ts = tabsMap.value.get(editorId)
    if (!ts) return
    const tab = ts.find(t => t.id === id)
    if (!tab) return
    if (tab.page > 0) {
      tab.page--
    }
  }

  function extractTableName(sql: string, fallbackColumn: string): string {
    try {
      const fromMatch = sql.match(/\bFROM\s+`?(\w+)`?\s*(?:AS\s+\w+)?/i)
      if (fromMatch) return fromMatch[1]
      const joinMatch = sql.match(/\bJOIN\s+`?(\w+)`?\s/i)
      if (joinMatch) return joinMatch[1]
    } catch {
      // 正则失败则回退
    }
    return fallbackColumn ? `_result_${fallbackColumn}` : '_unknown'
  }

  async function saveCellUpdate(
    editorId: string,
    tabId: string,
    columnName: string,
    newValue: unknown,
    rowIndex: number
  ): Promise<boolean> {
    const ts = tabsMap.value.get(editorId)
    if (!ts) return false
    const tab = ts.find(t => t.id === tabId)
    if (!tab || !tab.tableName || !tab.connectionId) return false

    const oldRow = tab.objectRows[rowIndex]
    if (!oldRow) return false

    const rowIdentity: Record<string, unknown> = {}
    for (const col of tab.columns) {
      const key = col.replace(/\./g, '_')
      if (key !== columnName.replace(/\./g, '_')) {
        rowIdentity[col] = (oldRow as Record<string, unknown>)[key] ?? null
      }
    }

    try {
      const result = await apiSaveCellUpdate({
        conn_id: tab.connectionId,
        table_name: tab.tableName,
        column_name: columnName,
        new_value: newValue,
        row_identity: rowIdentity,
      })

      if (result.success) {
        const fieldKey = columnName.replace(/\./g, '_')
        const newObjRow = { ...tab.objectRows[rowIndex], [fieldKey]: newValue }
        tab.objectRows[rowIndex] = newObjRow
        tab.dirtyRows.add(rowIndex)
        return true
      }
      return false
    } catch {
      return false
    }
  }

  // ========== 编辑器生命周期管理 ==========
  function clearEditorResults(editorId: string): void {
    const ts = tabsMap.value.get(editorId)
    if (ts) {
      for (const tab of ts) {
        if (tab.duckdbTempTable) {
          // DuckDB 临时表为内存表，连接关闭时后端自动释放，前端仅清理引用
          tab.duckdbTempTable = ''
        }
      }
    }
    tabsMap.value.delete(editorId)
    activeTabIdMap.value.delete(editorId)
    showPanelMap.value.delete(editorId)
  }

  function hasResults(editorId: string): boolean {
    const ts = tabsMap.value.get(editorId)
    return ts !== undefined && ts.length > 0
  }

  return {
    // 兼容旧 API（基于 currentEditorId）
    tabs,
    activeTabId,
    activeTab,
    showPanel,
    isAnyLoading,
    activeTabPagedRows,
    activeTabTotalPages,

    // 新 API（按 editorId）
    currentEditorId,
    setCurrentEditor,
    getTabs,
    getActiveTab,
    getShowPanel,
    hasResults,

    // 操作（带 editorId）
    addTab,
    setTabResult,
    closeTab,
    switchTab,
    closeAllTabs,
    removeTabResult,
    setFilterMode,
    applyQuickFilter,
    clearFilter,
    executeSqlFilter,
    executeDuckdbAnalysis,
    bridgeFilterFromDuckdb,
    ensureDuckdbTable,
    exportTab,
    markCellDirty,
    resetDirtyCells,
    setPage,
    setPageSize,
    nextPage,
    prevPage,
    saveCellUpdate,
    clearEditorResults,
  }
})
