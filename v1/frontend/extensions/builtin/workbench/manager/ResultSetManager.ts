import type {
  OpenFileInfo,
  ResultSetCreateParams,
  ResultSetMetadata,
} from '@/extensions/builtin/workbench/types/editor-types'

const MAX_RESULT_SETS = 5

interface DockviewApi {
  addPanel(opts: Record<string, unknown>): void
  getPanel(id: string): { api: { close(): void; setVisible(v: boolean): void } } | undefined
  movePanelOrGroup(panelId: string, opts: Record<string, unknown>): void
}

export class ResultSetManager {
  private resultIdCounter = 0
  private dockviewApi: DockviewApi | null = null
  private onToast: ((msg: string) => void) | null = null
  private tabGroupId: string | null = null

  setDockviewApi(api: DockviewApi): void {
    this.dockviewApi = api
  }

  setToastHandler(handler: (msg: string) => void): void {
    this.onToast = handler
  }

  setTabGroupId(gid: string | null): void {
    this.tabGroupId = gid
  }

  createResultSet(
    filePath: string,
    info: OpenFileInfo,
    sanitize: (s: string) => string,
    data: ResultSetCreateParams
  ): string {
    while (info.resultSets.length >= MAX_RESULT_SETS) {
      const oldest = info.resultSets[0]
      if (oldest) {
        this.removeResultSet(filePath, info, oldest.id)
        this.onToast?.('已移除最早的结果集')
      } else {
        break
      }
    }

    this.resultIdCounter++
    const resultId = `result_${sanitize(filePath)}_${this.resultIdCounter}`

    const metadata: ResultSetMetadata = {
      id: resultId,
      title: data.error ? '错误' : `结果${info.resultSets.length + 1}`,
      columns: data.columns,
      totalRowCount: data.totalRows,
      elapsedMs: data.elapsedMs,
      affectedRows: data.affectedRows,
      messages: data.error ? `错误: ${data.error}` : `${data.totalRows} 行, ${data.elapsedMs}ms`,
      sql: data.sql,
      timestamp: Date.now(),
      dataSource: 'memory',
      duckdbTable: null,
      rows: data.rows,
    }

    info.resultSets.push(metadata)

    const panelIdentifier = `panel_${resultId}`
    this.dockviewApi?.addPanel({
      id: panelIdentifier,
      component: 'fileResultPanel',
      title: metadata.title,
      position: { referenceGroup: this.tabGroupId, direction: 'below' },
      params: { resultSetId: metadata.id, columns: metadata.columns, rows: data.rows, filePath },
    })
    info.resultPanelIds.push(panelIdentifier)

    this.setActiveResultIndex(info, info.resultSets.length - 1)
    return resultId
  }

  removeResultSet(filePath: string, info: OpenFileInfo, resultSetId: string): void {
    const idx = info.resultSets.findIndex(rs => rs.id === resultSetId)
    if (idx < 0) return
    const panelId = `panel_${resultSetId}`
    this.dockviewApi?.getPanel(panelId)?.api.close()
    info.resultSets.splice(idx, 1)
    info.resultPanelIds = info.resultPanelIds.filter(id => id !== panelId)
    if (info.activeResultIndex >= info.resultSets.length) {
      this.setActiveResultIndex(info, Math.max(-1, info.resultSets.length - 1))
    }
  }

  setActiveResultIndex(info: OpenFileInfo, index: number): void {
    if (info.resultSets.length === 0) {
      info.activeResultIndex = -1
      return
    }
    info.activeResultIndex = Math.max(0, Math.min(index, info.resultSets.length - 1))
  }
}
