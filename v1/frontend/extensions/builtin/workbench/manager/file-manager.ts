import type {
  OpenFileInfo,
  OpenFileParams,
  FileType,
} from '@/extensions/builtin/workbench/types/editor-types'
import { PANEL_PREFIX_EDITOR } from '@/extensions/builtin/workbench/types/editor-types'
import { useEditorPersistence } from '@/extensions/builtin/workbench/ui/composables/useEditorPersistence'
import {
  confirmUnsavedClose,
  confirmFileConflict,
} from '@/extensions/builtin/workbench/ui/composables/useFileDialogs'

import {
  openFiles,
  activeFilePath,
  editorRef,
  tabGroupId,
  dockviewApi,
  savedStates,
  notifyOpenFilesChanged,
} from './editor-state'
import { saveCurrentFileToDisk } from './file-save'
import {
  getEditorView,
  saveEditorState,
  panelIdToFilePath,
  removeInstancesForFile,
  isFileOpenElsewhere,
} from './instance-service'

let untitledCounter = 0

function sanitize(s: string): string {
  return s.replace(/[^a-zA-Z0-9_-]/g, '_')
}

function filePanelId(filePath: string): string {
  return `${PANEL_PREFIX_EDITOR}${sanitize(filePath)}`
}

function syncDirtyToTab(filePath: string, isDirty: boolean): void {
  const info = openFiles.value.get(filePath)
  if (!info) return
  const title = isDirty ? `\u2022 ${info.fileName}` : info.fileName
  try {
    dockviewApi?.getPanel(filePanelId(filePath))?.api.setTitle(title)
  } catch {
    console.warn('[FileManager] dockview setTitle failed')
  }
}

function makeFileInfo(params: OpenFileParams): OpenFileInfo {
  return {
    filePath: params.filePath,
    fileName: params.fileName,
    language: params.language,
    type: params.type ?? 'file',
    isDirty: false,
    pinned: false,
    exists: true,
    lastModifiedAt: null,
    connectionId: params.connectionId ?? '',
    databaseName: params.databaseName ?? '',
    resultSets: [],
    activeResultIndex: -1,
    resultPanelIds: [],
    detachedResultIds: [],
    states: new Map(),
    primaryInstanceId: null,
    readonlyInstanceIds: [],
  }
}

/** 切换文件类型和语言模式 */
export function changeFileType(filePath: string, newType: FileType, newLanguage: string): boolean {
  const info = openFiles.value.get(filePath)
  if (!info) return false
  info.type = newType
  info.language = newLanguage
  notifyOpenFilesChanged()
  return true
}

export function openFile(params: OpenFileParams): void {
  if (openFiles.value.has(params.filePath)) {
    switchToFile(params.filePath)
    if (isFileOpenElsewhere(params.filePath)) {
      // eslint-disable-next-line no-console
      console.debug(`[FileManager] File already open in another group: ${params.filePath}`)
    }
    return
  }

  const pid = filePanelId(params.filePath)
  const isFirstFile = openFiles.value.size === 0
  const refGroup = tabGroupId.value

  const info = makeFileInfo(params)

  openFiles.value.set(params.filePath, info)

  dockviewApi?.addPanel({
    id: pid,
    component: 'editorPanel',
    title: params.fileName,
    position:
      isFirstFile || !refGroup
        ? { direction: 'right' }
        : { referenceGroup: refGroup, direction: 'within' },
    params: {
      filePath: params.filePath,
      fileName: params.fileName,
      language: params.language,
      content: params.sql,
    },
  })

  if (isFirstFile) {
    const captureGroup = (retry: number) => {
      if (retry > 20) return
      const panel = dockviewApi?.getPanel(pid)
      if (panel?.group?.id) {
        tabGroupId.value = panel.group.id
      } else {
        setTimeout(() => captureGroup(retry + 1), 50)
      }
    }
    setTimeout(() => captureGroup(0), 50)
  }

  notifyOpenFilesChanged()
}

/**
 * 创建新建未保存文件（Ctrl+N 场景）
 * filePath 为虚拟路径 "untitled:N" 不指向磁盘文件
 */
export function newFile(language?: string, content?: string): string {
  untitledCounter++
  const filePath = `untitled:${untitledCounter}`
  const lang = language ?? 'sql'
  const fileName = `Untitled-${untitledCounter}.${lang}`

  const pid = filePanelId(filePath)
  const isFirstFile = openFiles.value.size === 0
  const refGroup = tabGroupId.value

  const params: OpenFileParams = {
    filePath,
    fileName,
    language: lang,
    sql: content ?? '',
    type: 'file',
  }

  const info = makeFileInfo(params)
  info.exists = false

  openFiles.value.set(filePath, info)

  dockviewApi?.addPanel({
    id: pid,
    component: 'editorPanel',
    title: fileName,
    position:
      isFirstFile || !refGroup
        ? { direction: 'right' }
        : { referenceGroup: refGroup, direction: 'within' },
    params: {
      filePath,
      fileName,
      language: lang,
      content: content ?? '',
    },
  })

  if (isFirstFile) {
    const captureGroup = (retry: number) => {
      if (retry > 20) return
      const panel = dockviewApi?.getPanel(pid)
      if (panel?.group?.id) {
        tabGroupId.value = panel.group.id
      } else {
        setTimeout(() => captureGroup(retry + 1), 50)
      }
    }
    setTimeout(() => captureGroup(0), 50)
  }

  notifyOpenFilesChanged()
  return filePath
}

/**
 * 关闭文件前检查脏状态，如果修改未保存则弹出确认对话框
 * 返回 false 表示用户取消了关闭
 */
export async function closeFileChecked(filePath: string): Promise<boolean> {
  const info = openFiles.value.get(filePath)
  if (!info) return true

  if (info.isDirty) {
    const result = await confirmUnsavedClose(info.fileName)
    if (result === 'cancel') return false
    if (result === 'save') {
      try {
        await saveCurrentFileToDisk(filePath)
      } catch {
        // 保存失败，询问是否仍然关闭
        const forceResult = await confirmUnsavedClose(info.fileName)
        if (forceResult !== 'discard') return false
      }
    }
  }

  closeFile(filePath)
  return true
}

/**
 * 批量关闭文件时检查脏状态
 */
export async function closeFilesChecked(filePaths: string[]): Promise<void> {
  for (const fp of filePaths) {
    const closed = await closeFileChecked(fp)
    if (!closed) break
  }
}

export function closeFile(filePath: string): void {
  const info = openFiles.value.get(filePath)
  if (!info) return

  const pid = filePanelId(filePath)

  for (const rpId of info.resultPanelIds) {
    dockviewApi?.getPanel(rpId)?.api.close()
  }

  const idToRemove = removeInstancesForFile(filePath)
  for (const id of idToRemove) {
    const removed = info.states.get(id)
    if (removed) {
      info.states.set(id, removed)
    }
  }

  try {
    const { draft } = useEditorPersistence(pid, filePath)
    draft.remove()
  } catch {
    console.warn('[FileManager] draft.remove failed during closeFile')
  }

  const wasActive = activeFilePath.value === filePath

  openFiles.value.delete(filePath)
  dockviewApi?.getPanel(pid)?.api.close()

  if (openFiles.value.size === 0) {
    openFiles.value = new Map()
    activeFilePath.value = null
    editorRef.value = null
    tabGroupId.value = null
    savedStates.clear()
    return
  }

  notifyOpenFilesChanged()

  if (wasActive) {
    const next = openFiles.value.keys().next().value as string | undefined
    if (next) switchToFile(next)
    else activeFilePath.value = null
  }
}

export function switchToFile(filePath: string): void {
  const info = openFiles.value.get(filePath)
  if (!info) {
    console.warn(`[FileManager] File not found: ${filePath}`)
    return
  }
  if (activeFilePath.value === filePath) return

  if (activeFilePath.value) {
    const prevEd = getEditorView(activeFilePath.value)
    if (prevEd) {
      const prevState = prevEd.state
      saveEditorState(activeFilePath.value, prevState)
      const prevInfo = openFiles.value.get(activeFilePath.value)
      if (prevInfo?.primaryInstanceId) {
        prevInfo.states.set(prevInfo.primaryInstanceId, prevState)
      }
    }
  }

  activeFilePath.value = filePath
  const ed = getEditorView(filePath)
  if (ed) {
    const saved = info.states.get(info.primaryInstanceId ?? '')
    if (saved) ed.setState(saved)
    editorRef.value = ed
  }
  const pid = filePanelId(filePath)
  dockviewApi?.getPanel(pid)?.focus()
}

export function onPanelActivated(panelId: string): void {
  const fp = panelIdToFilePath(panelId)
  if (!fp) return
  activeFilePath.value = fp
  const ed = getEditorView(fp)
  if (ed) editorRef.value = ed

  for (const [path, fileInfo] of openFiles.value) {
    if (fileInfo.resultPanelIds.length === 0) continue
    for (const rpId of fileInfo.resultPanelIds) {
      try {
        dockviewApi?.getPanel(rpId)?.api.setVisible(path === fp)
      } catch {
        console.warn('[FileManager] result panel visibility update failed')
      }
    }
  }
}

export function findCenterGroup(): string | undefined {
  return tabGroupId.value ?? undefined
}

export function syncDirty(filePath: string, isDirty: boolean): void {
  syncDirtyToTab(filePath, isDirty)
}

export { saveCurrentFileToDisk, saveFileAs, checkExternalFileChanges } from './file-save'

// ========== 跨窗口冲突解决 ==========

/**
 * 多窗口编辑同一文件时处理冲突
 * 调用方应传入远端和本地的内容，让用户选择保留哪个版本
 */
export async function reconcileCrossWindowConflict(
  filePath: string,
  remoteContent: string,
  remoteIsDirty: boolean
): Promise<void> {
  const info = openFiles.value.get(filePath)
  if (!info) return

  const ed = getEditorView(filePath)
  if (!ed) return

  const localContent = ed.state.doc.toString()

  if (remoteContent === localContent) {
    // 内容相同，同步 dirty 状态
    info.isDirty = remoteIsDirty
    syncDirtyToTab(filePath, remoteIsDirty)
    notifyOpenFilesChanged()
    return
  }

  const result = await confirmFileConflict(info.fileName)

  if (result === 'keep-local') return

  if (result === 'keep-remote') {
    ed.dispatch({
      changes: { from: 0, to: ed.state.doc.length, insert: remoteContent },
    })
    info.isDirty = remoteIsDirty
    syncDirtyToTab(filePath, remoteIsDirty)
  }

  // merge -> 保留本地 + 远端追加为注释（最简策略）
  if (result === 'merge') {
    const merged = `${localContent}\n/* ===== Remote =====\n${remoteContent}\n*/`
    ed.dispatch({
      changes: { from: 0, to: ed.state.doc.length, insert: merged },
    })
    info.isDirty = true
    syncDirtyToTab(filePath, true)
  }

  notifyOpenFilesChanged()
}
