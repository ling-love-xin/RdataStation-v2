import {
  sendPopoutTransfer,
  listenMergeTransfer,
  listenWindowReady,
  listenStateSync,
  type StateSyncPayload,
} from '@/extensions/builtin/workbench/ui/composables/useCrossWindow'

import {
  openFiles,
  activeFilePath,
  crossWindowUnlisteners,
  notifyOpenFilesChanged,
} from './editor-state'
import { openFile } from './file-manager'
import { getEditorView, panelIdToFilePath } from './instance-service'

type CodeMirrorStateJSON = Record<string, unknown>

export function setupCrossWindowListeners(): void {
  listenWindowReady(() => {
    /* popout window is ready */
  })
    .then(unlisten => {
      crossWindowUnlisteners.push(unlisten)
    })
    .catch(() => {
      console.warn('[CrossWindowService] listenStateSync setup failed')
    })

  listenMergeTransfer(payload => {
    openFile({
      filePath: payload.filePath,
      fileName: payload.filePath.split(/[/\\]/).pop() ?? payload.filePath,
      language: 'sql',
      sql: payload.content,
      type: 'file',
    })
  })
    .then(unlisten => {
      crossWindowUnlisteners.push(unlisten)
    })
    .catch(() => {
      console.warn('[CrossWindowService] listenMergeTransfer setup failed')
    })

  listenStateSync(async (payload: StateSyncPayload) => {
    const info = openFiles.value.get(payload.filePath)
    if (!info) return

    // 内容相同则跳过
    const ed = getEditorView(payload.filePath)
    if (ed) {
      const localContent = ed.state.doc.toString()
      if (payload.content === localContent) {
        info.isDirty = payload.isDirty
        notifyOpenFilesChanged()
        return
      }

      // 有冲突 → 调用 reconcile
      if (info.isDirty) {
        const { reconcileCrossWindowConflict } = await import('./file-manager')
        await reconcileCrossWindowConflict(payload.filePath, payload.content, payload.isDirty)
        return
      }
    }

    // 本地无修改 → 直接应用远端
    if (ed) {
      ed.dispatch({
        changes: { from: 0, to: ed.state.doc.length, insert: payload.content },
      })
    }
    info.isDirty = payload.isDirty
    notifyOpenFilesChanged()
  })
    .then(unlisten => {
      crossWindowUnlisteners.push(unlisten)
    })
    .catch(() => {
      console.warn('[CrossWindowService] listenStateSync setup failed')
    })
}

export function popoutActiveFile(): void {
  const info = openFiles.value.get(activeFilePath.value ?? '')
  if (!info) return
  const ed = getEditorView(info.filePath)
  if (!ed) return
  try {
    const stateJSON = ed.state.toJSON() as unknown as CodeMirrorStateJSON
    const content = ed.state.doc.toString()
    sendPopoutTransfer({
      filePath: info.filePath,
      fileName: info.fileName,
      language: info.language,
      content,
      stateJSON,
      connectionId: info.connectionId,
      databaseName: info.databaseName,
    })
  } catch {
    console.warn('[CrossWindowService] popout transfer failed')
  }
}

export function onPanelUndocked(panelId: string): void {
  const fp = panelIdToFilePath(panelId)
  if (!fp) return
  const info = openFiles.value.get(fp)
  if (!info) return
  const ed = getEditorView(fp)
  if (ed) {
    try {
      const stateJSON = ed.state.toJSON() as Record<string, unknown>
      sendPopoutTransfer({
        filePath: fp,
        fileName: info.fileName,
        language: info.language,
        content: ed.state.doc.toString(),
        stateJSON,
        connectionId: info.connectionId,
        databaseName: info.databaseName,
      })
    } catch {
      console.warn('[CrossWindowService] serialization failed during popout')
    }
  }
}
