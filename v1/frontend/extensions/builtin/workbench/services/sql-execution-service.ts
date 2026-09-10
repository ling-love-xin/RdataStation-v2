/**
 * SQL 执行编排服务
 *
 * 从 EditorManager 中提取的 SQL 执行、格式化、验证等编排逻辑。
 * EditorManager 仅做门面代理，实际逻辑在此服务中。
 */

import { setEditorDiagnostics } from './cm-sql-extensions'
import {
  formatSql,
  validateSql,
  transpileSql,
  splitSql,
  executeDuckDBAccelerated as executeDuckDBAcceleratedApi,
  generateAttachName,
  rewriteDuckDBSQL,
  setErrorMarker,
  clearErrorMarkers,
} from './sql-editor-service'
import { activeFileInfo, runtimeState, SQL_LOG_TRUNCATE_LENGTH } from '../manager/editor-state'
import { getEditorView } from '../manager/instance-service'
import {
  createResultSet as createResultSetImpl,
  setActiveResultIndex as setActiveResultIndexImpl,
} from '../manager/result-set-manager'
import { useResultStore } from '../ui/stores/result-store'

type ApiResponseJSON = Record<string, unknown>

/** 默认 SQL 执行超时（毫秒），0 表示无限制 */
const DEFAULT_EXECUTION_TIMEOUT_MS = 30_000

// ─── SQL 执行 ────────────────────────────────────────────

export async function executeCurrentSQL(): Promise<void> {
  const info = activeFileInfo.value
  if (!info) return
  const ed = getEditorView(info.filePath)
  if (!ed) return
  const sel = ed.state.selection.main
  const sql = sel.empty ? ed.state.doc.toString() : ed.state.doc.sliceString(sel.from, sel.to)
  if (!sql.trim()) return

  runtimeState.startExecution()
  try {
    const { executeSql } = await import('@/extensions/builtin/query/ui/services/query')
    const result = (await executeSql(
      sql,
      info.connectionId || undefined,
      DEFAULT_EXECUTION_TIMEOUT_MS
    )) as unknown as ApiResponseJSON

    if (result.success) {
      createResultSetImpl(info.filePath, {
        columns: (result.columns as string[]) ?? [],
        rows: (result.rows as unknown[][]) ?? [],
        totalRows: (result.totalRows as number) ?? (result.rowCount as number) ?? 0,
        elapsedMs: (result.elapsedMs as number) ?? (result.elapsed_ms as number) ?? 0,
        affectedRows: (result.affectedRows as number) ?? (result.affected_rows as number) ?? 0,
        sql: sql.slice(0, SQL_LOG_TRUNCATE_LENGTH),
        error: null,
      })
      // 写入 resultStore（编辑器内嵌结果集）
      const resultStore = useResultStore()
      const tab = resultStore.addTab(info.filePath, sql, info.connectionId || '')
      resultStore.setTabResult(info.filePath, tab.id, {
        columns: (result.columns as string[]) ?? [],
        rows: (result.rows as unknown[][]) ?? [],
        rowCount: (result.totalRows as number) ?? (result.rowCount as number) ?? 0,
        elapsedMs: (result.elapsedMs as number) ?? (result.elapsed_ms as number) ?? 0,
      })
      // 执行成功清除之前的错误标记
      clearErrorMarkers(ed)
    } else {
      const errorMsg = (result.error as string) ?? 'Unknown error'
      createResultSetImpl(info.filePath, {
        columns: [],
        rows: [],
        totalRows: 0,
        elapsedMs: (result.elapsedMs as number) ?? (result.elapsed_ms as number) ?? 0,
        affectedRows: 0,
        sql: sql.slice(0, SQL_LOG_TRUNCATE_LENGTH),
        error: errorMsg,
      })
      // 写入 resultStore（错误结果）
      const resultStore = useResultStore()
      const tab = resultStore.addTab(info.filePath, sql, info.connectionId || '')
      tab.title = '错误'
      resultStore.setTabResult(info.filePath, tab.id, {
        columns: [],
        rows: [],
        rowCount: 0,
        elapsedMs: (result.elapsedMs as number) ?? (result.elapsed_ms as number) ?? 0,
      })
      // 在编辑器中标记错误位置
      setErrorMarker(ed, errorMsg, sql)
    }
  } catch (e) {
    const errorMsg = e instanceof Error ? e.message : 'Unknown error'
    console.error('[SqlExecution] Exec:', e)
    createResultSetImpl(info.filePath, {
      columns: [],
      rows: [],
      totalRows: 0,
      elapsedMs: 0,
      affectedRows: 0,
      sql: sql.slice(0, SQL_LOG_TRUNCATE_LENGTH),
      error: errorMsg,
    })
    // 写入 resultStore（异常结果）
    const resultStore = useResultStore()
    const tab = resultStore.addTab(info.filePath, sql, info.connectionId || '')
    tab.title = '错误'
    resultStore.setTabResult(info.filePath, tab.id, {
      columns: [],
      rows: [],
      rowCount: 0,
      elapsedMs: 0,
    })
    setErrorMarker(ed, errorMsg, sql)
  } finally {
    runtimeState.finishExecution()
  }
}

/**
 * 在新标签页执行 SQL（保持上一个结果集选中状态）
 */
export async function executeNewTabSQL(): Promise<void> {
  const info = activeFileInfo.value
  if (!info) {
    await executeCurrentSQL()
    return
  }
  const prevIndex = info.activeResultIndex
  await executeCurrentSQL()
  if (prevIndex >= 0 && prevIndex !== info.activeResultIndex) {
    setActiveResultIndexImpl(info.filePath, prevIndex)
  }
}

// ─── DuckDB 加速执行 ──────────────────────────────────────

export async function executeDuckDBAccelerated(): Promise<void> {
  const info = activeFileInfo.value
  if (!info) return
  const ed = getEditorView(info.filePath)
  if (!ed) return
  const sel = ed.state.selection.main
  const sql = sel.empty ? ed.state.doc.toString() : ed.state.doc.sliceString(sel.from, sel.to)
  if (!sql.trim()) return

  runtimeState.startExecution()
  try {
    const { appDataDir } = await import('@tauri-apps/api/path')
    const attachName = generateAttachName(info.connectionId || 'remote')
    const result = await executeDuckDBAcceleratedApi({
      sql: rewriteDuckDBSQL(sql, attachName),
      connId: info.connectionId || '',
      dataDir: await appDataDir(),
    })

    // 写入 resultStore（编辑器内嵌结果集）
    const resultStore = useResultStore()
    const tab = resultStore.addTab(info.filePath, sql, info.connectionId || '')
    const rows = (result.result?.rows as unknown[][]) ?? []
    resultStore.setTabResult(info.filePath, tab.id, {
      columns: (result.result?.columns as string[]) ?? [],
      rows,
      rowCount: rows.length,
      elapsedMs: result.elapsed_ms ?? 0,
    })
  } catch (e) {
    console.error('[SqlExecution] DuckDB:', e)
  } finally {
    runtimeState.finishExecution()
  }
}

// ─── 取消执行 ────────────────────────────────────────────

export function cancelExecution(): void {
  runtimeState.cancelExecution()
}

// ─── SQL 格式化 ──────────────────────────────────────────

export async function formatActiveSQL(): Promise<void> {
  const info = activeFileInfo.value
  if (!info) return
  const ed = getEditorView(info.filePath)
  if (!ed) return
  try {
    const formatted = await formatSql(ed.state.doc.toString())
    if (formatted) {
      ed.dispatch({ changes: { from: 0, to: ed.state.doc.length, insert: formatted } })
    }
  } catch (e) {
    console.error('[SqlExecution] Format:', e)
  }
}

// ─── SQL 验证 ────────────────────────────────────────────

export async function validateActiveSQL(): Promise<void> {
  const info = activeFileInfo.value
  if (!info) return
  const ed = getEditorView(info.filePath)
  if (!ed) return
  try {
    const markers = await validateSql(ed.state.doc.toString())
    setEditorDiagnostics(
      ed,
      markers.map(m => ({
        from: 0,
        to: 0,
        severity: m.severity as 'error' | 'warning' | 'info',
        message: m.message,
      }))
    )
  } catch {
    console.warn('[SqlExecution] SQL validation failed')
  }
}

// ─── 注释切换 ────────────────────────────────────────────

export async function toggleComment(): Promise<void> {
  const info = activeFileInfo.value
  if (!info) return
  const ed = getEditorView(info.filePath)
  if (!ed) return
  ed.focus()
  try {
    const { toggleComment: cmToggleComment } = await import('@codemirror/commands')
    cmToggleComment(ed)
  } catch {
    console.warn('[SqlExecution] toggleComment extension unavailable')
  }
}

// ─── 批量执行 ────────────────────────────────────────────

export async function executeBatchSQL(): Promise<void> {
  const info = activeFileInfo.value
  if (!info) return
  const ed = getEditorView(info.filePath)
  if (!ed) return
  const sel = ed.state.selection.main
  const sql = sel.empty ? ed.state.doc.toString() : ed.state.doc.sliceString(sel.from, sel.to)
  if (!sql.trim()) return

  const statements = await splitSql(sql)
  const meaningfulStatements = statements.filter(s => s.trim())
  if (meaningfulStatements.length <= 1) {
    await executeCurrentSQL()
    return
  }

  runtimeState.startExecution()
  const resultStore = useResultStore()

  try {
    for (let i = 0; i < meaningfulStatements.length; i++) {
      if (runtimeState.cancelled) break
      const stmt = meaningfulStatements[i]
      try {
        const { executeSql } = await import('@/extensions/builtin/query/ui/services/query')
        const result = (await executeSql(
          stmt,
          info.connectionId || undefined,
          DEFAULT_EXECUTION_TIMEOUT_MS,
        )) as unknown as ApiResponseJSON

        const tab = resultStore.addTab(info.filePath, stmt, info.connectionId || '')
        tab.title = `语句 #${i + 1}`
        if (result.success) {
          resultStore.setTabResult(info.filePath, tab.id, {
            columns: (result.columns as string[]) ?? [],
            rows: (result.rows as unknown[][]) ?? [],
            rowCount: (result.totalRows as number) ?? (result.rowCount as number) ?? 0,
            elapsedMs: (result.elapsedMs as number) ?? (result.elapsed_ms as number) ?? 0,
          })
        } else {
          tab.title = `语句 #${i + 1} (错误)`
          resultStore.setTabResult(info.filePath, tab.id, {
            columns: [],
            rows: [],
            rowCount: 0,
            elapsedMs: 0,
          })
        }
      } catch {
        const tab = resultStore.addTab(info.filePath, stmt, info.connectionId || '')
        tab.title = `语句 #${i + 1} (错误)`
        resultStore.setTabResult(info.filePath, tab.id, {
          columns: [],
          rows: [],
          rowCount: 0,
          elapsedMs: 0,
        })
      }
    }
  } finally {
    runtimeState.finishExecution()
  }
}

// ─── 方言转译 ────────────────────────────────────────────

export async function transpileActiveSQL(): Promise<void> {
  const info = activeFileInfo.value
  if (!info) return
  const ed = getEditorView(info.filePath)
  if (!ed) return
  try {
    const sql = ed.state.doc.toString()
    const transpiled = await transpileSql(sql, 'generic', 'mysql')
    if (transpiled && transpiled !== sql) {
      ed.dispatch({ changes: { from: 0, to: ed.state.doc.length, insert: transpiled } })
    }
  } catch (e) {
    console.error('[SqlExecution] Transpile:', e)
  }
}

// ─── 执行计划 ────────────────────────────────────────────

export async function explainActiveSQL(): Promise<void> {
  const info = activeFileInfo.value
  if (!info) return
  const ed = getEditorView(info.filePath)
  if (!ed) return
  const sel = ed.state.selection.main
  const sql = sel.empty ? ed.state.doc.toString() : ed.state.doc.sliceString(sel.from, sel.to)
  if (!sql.trim()) return

  const explainSql = `EXPLAIN ${sql}`
  runtimeState.startExecution()
  try {
    const { executeSql } = await import('@/extensions/builtin/query/ui/services/query')
    const result = (await executeSql(
      explainSql,
      info.connectionId || undefined,
      DEFAULT_EXECUTION_TIMEOUT_MS,
    )) as unknown as ApiResponseJSON

    const resultStore = useResultStore()
    const tab = resultStore.addTab(info.filePath, explainSql, info.connectionId || '')
    tab.title = '执行计划'
    resultStore.setTabResult(info.filePath, tab.id, {
      columns: (result.columns as string[]) ?? [],
      rows: (result.rows as unknown[][]) ?? [],
      rowCount: (result.totalRows as number) ?? (result.rowCount as number) ?? 0,
      elapsedMs: (result.elapsedMs as number) ?? (result.elapsed_ms as number) ?? 0,
    })
  } catch (e) {
    console.error('[SqlExecution] Explain:', e)
  } finally {
    runtimeState.finishExecution()
  }
}

// ─── 保存片段 ────────────────────────────────────────────

export async function saveActiveSnippet(): Promise<void> {
  const info = activeFileInfo.value
  if (!info) return
  const ed = getEditorView(info.filePath)
  if (!ed) return
  const sql = ed.state.doc.toString()
  if (!sql.trim()) return

  try {
    const { createScratchpadEntry } = await import('@/extensions/builtin/scratchpad/infrastructure/api/scratchpad-api')
    const ts = Date.now()
    const fileName = `snippet-${ts}.sql`
    await createScratchpadEntry(fileName, true)
    const { message } = (await import('naive-ui')).createDiscreteApi(['message'])
    message.success(`已保存片段: ${fileName}`)
  } catch (e) {
    console.error('[SqlExecution] saveSnippet:', e)
  }
}
