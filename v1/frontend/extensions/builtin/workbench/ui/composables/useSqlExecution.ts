import { createDiscreteApi } from 'naive-ui'
import { ref, type Ref } from 'vue'

import * as queryService from '@/extensions/builtin/query/ui/services/query'
import {
  parseSql,
  splitSql,
  rewriteDuckDBSQL,
  generateAttachName,
  detectParams,
  bindParams,
} from '@/extensions/builtin/workbench/services/sql-editor-service'
import { useResultStore } from '@/extensions/builtin/workbench/ui/stores/result-store'
import { useSqlExecutionStore } from '@/extensions/builtin/workbench/ui/stores/sql-execution-store'

import { useTransaction } from './useTransaction'

const DEFAULT_QUERY_TIMEOUT_MS = 30_000
const SMART_MODE_THRESHOLD = 1000

interface SqlExecutionOptions {
  panelId: string
  editorId: Ref<string> | string
  getEditorValue: () => string
  getSelectedText: () => string
  runtimeConnId: Ref<string>
  currentDatabaseType?: Ref<string | null>
  currentConnectionName?: Ref<string>
}

export function useSqlExecution(options: SqlExecutionOptions) {
  const {
    panelId,
    editorId,
    getEditorValue,
    getSelectedText,
    runtimeConnId,
    currentDatabaseType,
    currentConnectionName,
  } = options

  const { message } = createDiscreteApi(['message'])
  const store = useSqlExecutionStore()
  const executing = ref(false)
  const lastExecutionTime = ref<number | null>(null)
  const hasResults = ref(false)
  const executionMode = ref<'normal' | 'analysis' | 'smart'>('normal')
  let abortController: AbortController | undefined
  const currentResultData = ref<{
    columns: string[]
    rows: unknown[][]
    totalRows: number
    elapsedMs: number
    affectedRows: number
    error: string | null
  } | null>(null)

  const { inTransaction, beginTransaction, commitTransaction, rollbackTransaction } =
    useTransaction(runtimeConnId)

  const statementCount = ref(0)
  let parseTimer: ReturnType<typeof setTimeout> | null = null

  function scheduleParse(): void {
    if (parseTimer) clearTimeout(parseTimer)
    parseTimer = setTimeout(async () => {
      const sql = getEditorValue()
      if (!sql.trim()) {
        statementCount.value = 0
        return
      }
      try {
        const result = await parseSql(sql)
        if (result.success) {
          statementCount.value = result.statementsCount
        }
      } catch {
        statementCount.value = 0
      }
    }, 500)
  }

  function storeResult(data: {
    columns: string[]
    rows: unknown[][]
    totalRows: number
    elapsedMs: number
    affectedRows: number
    error: string | null
  }): void {
    currentResultData.value = data
    hasResults.value = true

    const executionResult = {
      panelId,
      result: {
        columns: data.columns,
        rows: data.rows,
        rowCount: data.totalRows,
        executionTime: data.elapsedMs,
        affectedRows: data.affectedRows,
      },
      error: data.error,
      timestamp: Date.now(),
    }
    store.executionResults.set(panelId, executionResult)
    store.executionResults = new Map(store.executionResults)
    store.setActiveEditorPanelId(panelId)
    store.incrementResultVersion()

    // Also write to resultStore with editorId
    const resultStore = useResultStore()
    const eid = typeof editorId === 'string' ? editorId : editorId.value
    const sql = getEditorValue()
    const connId = runtimeConnId.value
    const tab = resultStore.addTab(eid, sql, connId)
    resultStore.setTabResult(eid, tab.id, {
      columns: data.columns,
      rows: data.rows,
      rowCount: data.totalRows,
      elapsedMs: data.elapsedMs,
    })
  }

  async function executeSql(
    sql: string,
    connId: string
  ): Promise<{
    columns: string[]
    rows: unknown[][]
    totalRows: number
    elapsedMs: number
    affectedRows: number
    truncated: boolean
    error: string | null
  }> {
    const response = await queryService.executeSql(sql, connId, DEFAULT_QUERY_TIMEOUT_MS)
    const data = response.result

    return {
      columns: data.columns ?? [],
      rows: data.rows ?? [],
      totalRows: data.total_rows ?? 0,
      elapsedMs: response.elapsed_ms ?? 0,
      affectedRows: response.affected_rows ?? 0,
      truncated: response.truncated ?? false,
      error: null,
    }
  }

  async function executeSingleStatement(): Promise<void> {
    if (executing.value) {
      message.warning('查询正在执行中，请稍后重试')
      return
    }

    const sql = getSelectedText() || getEditorValue()
    if (!sql.trim()) {
      message.warning('No SQL to execute')
      return
    }

    const connId = runtimeConnId.value
    if (!connId) {
      message.warning('No active connection')
      return
    }

    abortController = new AbortController()
    executing.value = true

    try {
      if (abortController.signal.aborted) return

      const result = await executeSql(sql, connId)
      lastExecutionTime.value = result.elapsedMs

      storeResult(result)

      if (abortController.signal.aborted) return

      if (result.error) {
        message.error(result.error)
      } else if (result.totalRows > 0) {
        if (result.truncated) {
          message.warning(
            `${result.totalRows} rows returned (truncated from larger result set) in ${result.elapsedMs}ms`
          )
        } else {
          message.success(`${result.totalRows} rows returned in ${result.elapsedMs}ms`)
        }
      } else {
        message.success(`Done in ${result.elapsedMs}ms, ${result.affectedRows} rows affected`)
      }
    } catch (error) {
      const ac = abortController
      if (ac?.signal.aborted) {
        message.info('Query cancelled')
        return
      }
      const elapsed = lastExecutionTime.value ?? 0
      lastExecutionTime.value = elapsed
      storeResult({
        columns: [],
        rows: [],
        totalRows: 0,
        elapsedMs: elapsed,
        affectedRows: 0,
        error: error instanceof Error ? error.message : String(error),
      })
      message.error(error instanceof Error ? error.message : String(error))
    } finally {
      abortController = undefined
      executing.value = false
    }
  }

  async function executeBatch(): Promise<void> {
    if (executing.value) {
      message.warning('查询正在执行中，请稍后重试')
      return
    }

    const sql = getSelectedText() || getEditorValue()
    if (!sql.trim()) {
      message.warning('No SQL to execute')
      return
    }

    const connId = runtimeConnId.value
    if (!connId) {
      message.warning('No active connection')
      return
    }

    const statements = await splitSql(sql)
    const meaningfulStatements = statements.filter(s => s.trim())

    if (meaningfulStatements.length <= 1) {
      await executeSingleStatement()
      return
    }

    abortController = new AbortController()
    executing.value = true
    const resultStore = useResultStore()

    let totalElapsed = 0
    let successCount = 0
    let errorCount = 0
    let cancelledCount = 0
    let firstError = ''

    for (let i = 0; i < meaningfulStatements.length; i++) {
      if (abortController.signal.aborted) {
        // 未执行的语句标记为"已取消"
        for (let j = i; j < meaningfulStatements.length; j++) {
          cancelledCount++
          const eid = typeof editorId === 'string' ? editorId : editorId.value
          const tab = resultStore.addTab(eid, meaningfulStatements[j], connId)
          tab.title = `语句 #${j + 1} (已取消)`
          resultStore.setTabResult(eid, tab.id, {
            columns: [],
            rows: [],
            rowCount: 0,
            elapsedMs: 0,
          })
        }
        break
      }

      const stmt = meaningfulStatements[i]
      const startTime = performance.now()

      try {
        const result = await executeSql(stmt, connId)
        const elapsed = performance.now() - startTime
        totalElapsed += elapsed

        const eid = typeof editorId === 'string' ? editorId : editorId.value
        const tab = resultStore.addTab(eid, stmt, connId)
        tab.title = `语句 #${i + 1}`
        resultStore.setTabResult(eid, tab.id, {
          columns: result.columns,
          rows: result.rows,
          rowCount: result.totalRows,
          elapsedMs: elapsed,
        })

        if (result.error) {
          errorCount++
        } else {
          successCount++
        }
      } catch (error) {
        const elapsed = performance.now() - startTime
        totalElapsed += elapsed
        errorCount++
        if (!firstError) firstError = String(error)

        const eid = typeof editorId === 'string' ? editorId : editorId.value
        const tab = resultStore.addTab(eid, stmt, connId)
        tab.title = `语句 #${i + 1} (错误)`
        resultStore.setTabResult(eid, tab.id, {
          columns: [],
          rows: [],
          rowCount: 0,
          elapsedMs: elapsed,
        })
      }
    }

    lastExecutionTime.value = totalElapsed
    abortController = undefined
    executing.value = false

    const total = successCount + errorCount + cancelledCount
    if (cancelledCount > 0) {
      message.info(
        `${successCount} ok, ${errorCount} failed, ${cancelledCount} cancelled — ${Math.round(totalElapsed)}ms`
      )
    } else if (errorCount === 0) {
      message.success(`${total} statements executed in ${Math.round(totalElapsed)}ms`)
    } else {
      message.warning(
        `${successCount} ok, ${errorCount} failed — ${Math.round(totalElapsed)}ms${firstError ? ` (${firstError})` : ''}`
      )
    }
  }

  async function cancelExecution(): Promise<void> {
    if (abortController) {
      abortController.abort()
      try {
        await queryService.cancelQuery(runtimeConnId.value)
      } catch {
        console.warn('[useSqlExecution] cancelQuery failed,不影响前端状态')
      }
      message.info('Query cancelled')
    }
  }

  async function executeNewTab(): Promise<void> {
    const sql = getSelectedText() || getEditorValue()
    if (!sql.trim()) {
      message.warning('No SQL to execute')
      return
    }

    const connId = runtimeConnId.value
    if (!connId) {
      message.warning('No active connection')
      return
    }

    executing.value = true

    try {
      const result = await executeSql(sql, connId)
      lastExecutionTime.value = result.elapsedMs

      const executionResult = {
        panelId,
        result: {
          columns: result.columns,
          rows: result.rows,
          rowCount: result.totalRows,
          executionTime: result.elapsedMs,
          affectedRows: result.affectedRows,
        },
        error: result.error,
        timestamp: Date.now(),
      }
      store.requestNewTab(panelId, executionResult)

      if (result.error) {
        message.error(result.error)
      }
    } catch (error) {
      lastExecutionTime.value = 0
      message.error(error instanceof Error ? error.message : String(error))
    } finally {
      executing.value = false
    }
  }

  async function executeDuckDBAccelerated(): Promise<void> {
    const sql = getSelectedText() || getEditorValue()
    if (!sql.trim()) {
      message.warning('No SQL to execute')
      return
    }

    const connId = runtimeConnId.value
    if (!connId) {
      message.warning('No active connection')
      return
    }

    const connName = currentConnectionName?.value ?? 'remote'
    const attachName = generateAttachName(connName)
    const rewrittenSql = rewriteDuckDBSQL(sql, attachName)

    executing.value = true
    const startTime = performance.now()

    try {
      const { appDataDir } = await import('@tauri-apps/api/path')
      const dataDir = await appDataDir()

      const result = await queryService.executeDuckDBAccelerated({
        sql: rewrittenSql,
        connId,
        dbType: currentDatabaseType?.value ?? undefined,
        dataDir,
      })

      const elapsed = performance.now() - startTime
      lastExecutionTime.value = elapsed

      if (result.error) {
        storeResult({
          columns: [],
          rows: [],
          totalRows: 0,
          elapsedMs: elapsed,
          affectedRows: 0,
          error: result.error,
        })
        message.error(result.error)
      } else {
        storeResult({
          columns: result.columns,
          rows: result.rows,
          totalRows: result.rows.length,
          elapsedMs: elapsed,
          affectedRows: 0,
          error: null,
        })
        message.success(`${result.rows.length} rows returned in ${Math.round(elapsed)}ms`)
      }
    } catch (error) {
      const elapsed = performance.now() - startTime
      lastExecutionTime.value = elapsed
      storeResult({
        columns: [],
        rows: [],
        totalRows: 0,
        elapsedMs: elapsed,
        affectedRows: 0,
        error: error instanceof Error ? error.message : String(error),
      })
      message.error(error instanceof Error ? error.message : String(error))
    } finally {
      executing.value = false
    }
  }

  async function estimateRowCount(sql: string, connId: string): Promise<number> {
    try {
      const countSql = `SELECT COUNT(*) AS _cnt FROM (${sql}) AS _sub`
      const result = await queryService.executeSql(countSql, connId, DEFAULT_QUERY_TIMEOUT_MS)
      const rows = result.result?.rows ?? []
      if (rows.length > 0 && rows[0].length > 0) {
        const val = rows[0][0]
        return typeof val === 'number' ? val : parseInt(String(val), 10) || 0
      }
      return 0
    } catch {
      return SMART_MODE_THRESHOLD + 1
    }
  }

  async function executeSmart(sql: string, connId: string): Promise<void> {
    executing.value = true
    const startTime = performance.now()

    try {
      const estimatedRows = await estimateRowCount(sql, connId)

      if (estimatedRows <= SMART_MODE_THRESHOLD) {
        executionMode.value = 'normal'
        executing.value = false
        await executeSingleStatement()
        return
      }

      executionMode.value = 'analysis'
      executing.value = false
      await executeDuckDBAccelerated()
    } catch (error) {
      const elapsed = performance.now() - startTime
      lastExecutionTime.value = elapsed
      storeResult({
        columns: [],
        rows: [],
        totalRows: 0,
        elapsedMs: elapsed,
        affectedRows: 0,
        error: error instanceof Error ? error.message : String(error),
      })
      message.error(`Smart mode failed: ${error instanceof Error ? error.message : String(error)}`)
      executing.value = false
    }
  }

  async function executeWithMode(): Promise<void> {
    const sql = getSelectedText() || getEditorValue()
    if (!sql.trim()) {
      message.warning('No SQL to execute')
      return
    }
    const connId = runtimeConnId.value
    if (!connId) {
      message.warning('No active connection')
      return
    }

    switch (executionMode.value) {
      case 'normal':
        return executeSingleStatement()
      case 'analysis':
        return executeDuckDBAccelerated()
      case 'smart':
        return executeSmart(sql, connId)
    }
  }

  function checkForParams(sql: string) {
    return detectParams(sql)
  }

  function buildBoundSql(sql: string, values: Record<string, string>): string {
    return bindParams(sql, values)
  }

  function cleanup(): void {
    if (parseTimer) {
      clearTimeout(parseTimer)
      parseTimer = null
    }
  }

  return {
    executing,
    lastExecutionTime,
    hasResults,
    executionMode,
    currentResultData,
    inTransaction,
    statementCount,
    scheduleParse,
    executeSingleStatement,
    executeWithMode,
    executeSmart,
    cancelExecution,
    executeNewTab,
    beginTransaction,
    commitTransaction,
    rollbackTransaction,
    executeDuckDBAccelerated,
    executeBatch,
    executeSql,
    storeResult,
    checkForParams,
    buildBoundSql,
    cleanup,
  }
}
