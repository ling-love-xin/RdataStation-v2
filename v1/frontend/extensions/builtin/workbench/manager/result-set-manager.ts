import type {
  ResultSetCreateParams,
  ResultSetMetadata,
} from '@/extensions/builtin/workbench/types/editor-types'

import {
  openFiles,
  dockviewApi,
  tabGroupId,
  MAX_RESULT_SETS,
  DEFAULT_POPOUT_GEOMETRY,
  nextResultId,
  notifyOpenFilesChanged,
} from './editor-state'

function sanitize(s: string): string {
  return s.replace(/[^a-zA-Z0-9_-]/g, '_')
}

export function createResultSet(filePath: string, data: ResultSetCreateParams): string {
  const info = openFiles.value.get(filePath)
  if (!info) {
    console.warn(`[ResultSetManager] File not found: ${filePath}`)
    return ''
  }

  while (info.resultSets.length >= MAX_RESULT_SETS) {
    const oldest = info.resultSets[0]
    if (oldest) {
      removeResultSet(filePath, oldest.id)
    } else {
      break
    }
  }

  const resultId = `result_${sanitize(filePath)}_${nextResultId()}`

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
  const gid = tabGroupId.value
  dockviewApi?.addPanel({
    id: panelIdentifier,
    component: 'fileResultPanel',
    title: metadata.title,
    position: { referenceGroup: gid, direction: 'below' },
    params: { resultSetId: metadata.id, columns: metadata.columns, rows: data.rows, filePath },
  })
  info.resultPanelIds.push(panelIdentifier)

  setActiveResultIndex(filePath, info.resultSets.length - 1)
  return resultId
}

export function removeResultSet(filePath: string, resultSetId: string): void {
  const info = openFiles.value.get(filePath)
  if (!info) return
  const idx = info.resultSets.findIndex(rs => rs.id === resultSetId)
  if (idx < 0) return
  const panelId = `panel_${resultSetId}`
  dockviewApi?.getPanel(panelId)?.api.close()
  info.resultSets.splice(idx, 1)
  info.resultPanelIds = info.resultPanelIds.filter(id => id !== panelId)
  if (info.activeResultIndex >= info.resultSets.length) {
    setActiveResultIndex(filePath, Math.max(-1, info.resultSets.length - 1))
  } else {
    notifyOpenFilesChanged()
  }
}

export function setActiveResultIndex(filePath: string, index: number): void {
  const info = openFiles.value.get(filePath)
  if (!info) return
  if (info.resultSets.length === 0) {
    info.activeResultIndex = -1
    return
  }
  info.activeResultIndex = Math.max(0, Math.min(index, info.resultSets.length - 1))
  notifyOpenFilesChanged()
}

export function detachResultPanel(panelId: string): void {
  let filePath: string | null = null
  for (const [fp, info] of openFiles.value) {
    if (info.resultPanelIds.includes(panelId)) {
      filePath = fp
      info.resultPanelIds = info.resultPanelIds.filter(id => id !== panelId)
      info.detachedResultIds.push(panelId)
      if (info.activeResultIndex >= info.resultSets.length) {
        info.activeResultIndex = Math.max(-1, info.resultSets.length - 1)
      }
      break
    }
  }
  if (!filePath) return

  try {
    dockviewApi?.movePanelOrGroup(panelId, {
      group: `detached_${panelId}`,
      position: { direction: 'right' },
      floating: DEFAULT_POPOUT_GEOMETRY,
    })
  } catch {
    console.warn('[ResultSetManager] dockview movePanelOrGroup failed during detach')
  }

  notifyOpenFilesChanged()
}

export function attachResultPanel(panelId: string, filePath: string): void {
  const info = openFiles.value.get(filePath)
  if (!info) return

  info.resultPanelIds.push(panelId)
  info.detachedResultIds = info.detachedResultIds.filter(id => id !== panelId)

  const gid = tabGroupId.value
  if (gid) {
    try {
      dockviewApi?.movePanelOrGroup(panelId, {
        group: gid,
        position: { direction: 'below' },
      })
    } catch {
      console.warn('[ResultSetManager] dockview movePanelOrGroup failed during attach')
    }
  }

  notifyOpenFilesChanged()
}

export function renameResultSet(panelId: string, newTitle: string): void {
  for (const [, info] of openFiles.value) {
    const idx = info.resultPanelIds.indexOf(panelId)
    const detIdx = info.detachedResultIds.indexOf(panelId)
    if (idx < 0 && detIdx < 0) continue

    const rsIdx = info.resultSets.findIndex(rs => `panel_${rs.id}` === panelId)
    if (rsIdx >= 0) {
      info.resultSets[rsIdx] = { ...info.resultSets[rsIdx], title: newTitle }
    }

    try {
      dockviewApi?.getPanel(panelId)?.api.setTitle(newTitle)
    } catch {
      console.warn('[ResultSetManager] dockview setTitle failed during rename')
    }

    notifyOpenFilesChanged()
    break
  }
}
