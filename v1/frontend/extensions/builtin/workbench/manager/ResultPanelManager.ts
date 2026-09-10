import { EditorManager } from '@/extensions/builtin/workbench/manager/EditorManager'
import {
  type ResultSetCreateParams,
  type ResultSetMetadata,
  type IResultPanelManager,
} from '@/extensions/builtin/workbench/types/editor-types'

class ResultPanelManagerImpl implements IResultPanelManager {
  addResultSet(filePath: string, data: ResultSetCreateParams): string {
    return EditorManager.createResultSet(filePath, data)
  }

  removeResultSet(filePath: string, resultSetId: string): void {
    EditorManager.removeResultSet(filePath, resultSetId)
  }

  getResultSetRows(filePath: string, resultSetId: string): unknown[][] {
    const info = EditorManager.openFiles.get(filePath)
    if (!info) return []

    const rs = info.resultSets.find(r => r.id === resultSetId)
    return rs?.rows ?? []
  }

  detachResultPanel(panelId: string): void {
    EditorManager.detachResultPanel(panelId)
  }

  attachResultPanel(panelId: string, filePath: string): void {
    EditorManager.attachResultPanel(panelId, filePath)
  }

  getAllResultSets(filePath: string): ResultSetMetadata[] {
    const info = EditorManager.openFiles.get(filePath)
    return info?.resultSets ?? []
  }

  getActiveResultSet(filePath: string): ResultSetMetadata | null {
    const info = EditorManager.openFiles.get(filePath)
    if (!info || info.activeResultIndex < 0) return null
    return info.resultSets[info.activeResultIndex] ?? null
  }

  createResultPanel(
    _filePath: string,
    _panelId: string,
    _metadata: ResultSetMetadata,
    _rows: unknown[][]
  ): void {
    /* creation is handled by EditorManager.createResultSet via addResultSet() */
  }
}

export const ResultPanelManager = new ResultPanelManagerImpl()
