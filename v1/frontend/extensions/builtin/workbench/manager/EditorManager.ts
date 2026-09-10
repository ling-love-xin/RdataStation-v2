import { EditorState } from '@codemirror/state'
import { EditorView } from '@codemirror/view'
import { markRaw } from 'vue'

import { ShortcutManager } from '@/extensions/builtin/workbench/manager/ShortcutManager'
import {
  type OpenFileParams,
  type ResultSetCreateParams,
} from '@/extensions/builtin/workbench/types/editor-types'
import { useEditorRecovery } from '@/extensions/builtin/workbench/ui/composables/useEditorRecovery'

import {
  setupCrossWindowListeners as setupCrossWindowListenersImpl,
  popoutActiveFile as popoutActiveFileImpl,
  onPanelUndocked as onPanelUndockedImpl,
} from './cross-window-service'
import {
  openFiles,
  activeFilePath,
  activeFileInfo,
  editorRef,
  isInitialized,
  dockviewApi,
  crossWindowUnlisteners,
  tabGroupId,
  editorInstances,
  runtimeState,
  setDockviewApi,
  clearDockviewApi,
  resetResultIdCounter,
  type DockviewApiFacade,
} from './editor-state'
import {
  openFile as openFileImpl,
  closeFile as closeFileImpl,
  closeFileChecked as closeFileCheckedImpl,
  newFile as newFileImpl,
  saveCurrentFileToDisk,
  saveFileAs as saveFileAsImpl,
  checkExternalFileChanges,
  switchToFile as switchToFileImpl,
  onPanelActivated as onPanelActivatedImpl,
  findCenterGroup as findCenterGroupImpl,
  changeFileType as changeFileTypeImpl,
} from './file-manager'
import {
  getSavedState,
  saveEditorState,
  registerFileEditor,
  unregisterFileEditor,
  isPrimaryInstance,
  isFileOpenElsewhere,
  updatePanelGroup,
  panelIdToFilePath,
  clearAllInstances,
} from './instance-service'
import {
  createResultSet as createResultSetImpl,
  removeResultSet as removeResultSetImpl,
  setActiveResultIndex as setActiveResultIndexImpl,
  detachResultPanel as detachResultPanelImpl,
  attachResultPanel as attachResultPanelImpl,
  renameResultSet as renameResultSetImpl,
} from './result-set-manager'
import {
  executeCurrentSQL as executeCurrentSQLService,
  executeNewTabSQL as executeNewTabSQLService,
  executeDuckDBAccelerated as executeDuckDBAcceleratedService,
  cancelExecution as cancelExecutionService,
  formatActiveSQL,
  validateActiveSQL,
  toggleComment as toggleCommentService,
  executeBatchSQL as executeBatchSQLService,
  transpileActiveSQL as transpileActiveSQLService,
  explainActiveSQL as explainActiveSQLService,
  saveActiveSnippet as saveActiveSnippetService,
} from '../services/sql-execution-service'

type CodeMirrorStateJSON = Record<string, unknown>

function onKeydown(e: KeyboardEvent): void {
  ShortcutManager.handleKeydown(e)
}

function onCanvasClick(): void {
  ShortcutManager.setActiveScope('editor')
}

export const EditorManager = {
  get openFiles() {
    return openFiles.value
  },
  get activeFilePath() {
    return activeFilePath.value
  },
  get activeFileInfo() {
    return activeFileInfo.value
  },
  get editor() {
    return editorRef.value
  },
  get isExecuting() {
    return runtimeState.isExecuting
  },
  get lastExecutionTime() {
    return runtimeState.lastExecutionTime
  },
  get isInitialized() {
    return isInitialized.value
  },
  get dockviewApi() {
    return dockviewApi
  },

  getSavedStateForFile(filePath: string): EditorState | undefined {
    return getSavedState(filePath)
  },

  saveEditorStateForFile(filePath: string, state: EditorState): void {
    saveEditorState(filePath, state)
  },

  hasRecoveryData(): boolean {
    const { hasRecoveryData } = useEditorRecovery()
    return hasRecoveryData()
  },

  loadRecoverySnapshots(): {
    filePath: string
    fileName: string
    language: string
    isDirty: boolean
  }[] {
    const { loadSnapshots } = useEditorRecovery()
    return loadSnapshots().map(s => ({
      filePath: s.filePath,
      fileName: s.fileName,
      language: s.language,
      isDirty: s.isDirty,
    }))
  },

  clearRecovery(): void {
    const { clearRecovery } = useEditorRecovery()
    clearRecovery()
  },

  setEditor(ed: EditorView): void {
    editorRef.value = markRaw(ed)
  },

  registerFileEditor(filePath: string, ed: EditorView): void {
    registerFileEditor(filePath, ed)
  },

  unregisterFileEditor(filePath: string): void {
    unregisterFileEditor(filePath)
  },

  isPrimaryInstance(filePath: string, instanceId?: string): boolean {
    return isPrimaryInstance(filePath, instanceId)
  },

  isFileOpenElsewhere(filePath: string, excludeGroupId?: string): boolean {
    return isFileOpenElsewhere(filePath, excludeGroupId)
  },

  onPanelActivated(panelId: string): void {
    onPanelActivatedImpl(panelId)
  },

  updatePanelGroup(panelId: string, groupId: string): void {
    updatePanelGroup(panelId, groupId)
  },

  onPanelUndocked(panelId: string): void {
    onPanelUndockedImpl(panelId)
  },

  setActiveResultIndex(filePath: string, index: number): void {
    setActiveResultIndexImpl(filePath, index)
  },

  detachResultPanel(panelId: string): void {
    detachResultPanelImpl(panelId)
  },

  attachResultPanel(panelId: string, filePath: string): void {
    attachResultPanelImpl(panelId, filePath)
  },

  createResultSet(filePath: string, data: ResultSetCreateParams): string {
    return createResultSetImpl(filePath, data)
  },

  removeResultSet(filePath: string, resultSetId: string): void {
    removeResultSetImpl(filePath, resultSetId)
  },

  init(api: unknown): void {
    if (isInitialized.value) return
    setDockviewApi(api as DockviewApiFacade)
    isInitialized.value = true
    window.addEventListener('keydown', onKeydown)
    const canvas = document.querySelector('.dv-canvas')
    if (canvas) canvas.addEventListener('click', onCanvasClick)

    ShortcutManager.register('Ctrl+S', 'editor', () => EditorManager.saveCurrentFile(), '保存')
    ShortcutManager.register(
      'Ctrl+Enter',
      'editor',
      () => EditorManager.executeCurrentSQL(),
      '执行'
    )
    ShortcutManager.register('Ctrl+/', 'editor', () => EditorManager.toggleComment(), '注释')
  },

  destroy(): void {
    window.removeEventListener('keydown', onKeydown)
    ShortcutManager.unregisterByScope('editor')
    const canvas = document.querySelector('.dv-canvas')
    if (canvas) canvas.removeEventListener('click', onCanvasClick)

    // 异步保存快照，避免阻塞 destroy 流程
    const { saveSnapshot } = useEditorRecovery()
    const instancesToSave = [...editorInstances.entries()]
    if (instancesToSave.length > 0) {
      setTimeout(() => {
        for (const [_id, inst] of instancesToSave) {
          try {
            const stateJSON = inst.view.state.toJSON() as unknown as CodeMirrorStateJSON
            const info = openFiles.value.get(inst.filePath)
            saveSnapshot(inst.filePath, stateJSON, {
              fileName: info?.fileName ?? inst.filePath.split(/[/\\]/).pop() ?? inst.filePath,
              language: info?.language ?? 'plaintext',
              isDirty: info?.isDirty ?? false,
              scrollTop: inst.view.scrollDOM.scrollTop,
              scrollLeft: inst.view.scrollDOM.scrollLeft,
            })
          } catch {
            console.warn('[EditorManager] recovery saveSnapshot failed during destroy')
          }
        }
      }, 0)
    }
    clearAllInstances()
    openFiles.value = new Map()
    activeFilePath.value = null
    editorRef.value = null
    tabGroupId.value = null
    resetResultIdCounter()
    for (const unlisten of crossWindowUnlisteners) {
      try {
        unlisten()
      } catch {
        console.warn('[EditorManager] unlisten failed during destroy')
      }
    }
    crossWindowUnlisteners.length = 0
    clearDockviewApi()
    isInitialized.value = false
  },

  openFile(params: OpenFileParams): void {
    openFileImpl(params)
    // 记录文件初始修改时间，用于外部变更检测
    import('@tauri-apps/plugin-fs')
      .then(({ stat }) =>
        stat(params.filePath)
          .then(m => {
            const info = openFiles.value.get(params.filePath)
            if (info) {
              info.lastModifiedAt = (m as unknown as { mtime?: { ms: number } }).mtime?.ms ?? Date.now()
            }
          })
          .catch(() => {
            /* 不可达文件，不影响流程 */
          })
      )
      .catch(() => {
        /* plugin-fs 不可用 */
      })
  },

  closeFile(filePath: string): void {
    closeFileImpl(filePath)
  },

  async closeFileChecked(filePath: string): Promise<boolean> {
    return closeFileCheckedImpl(filePath)
  },

  newFile(language?: string, content?: string): string {
    return newFileImpl(language, content)
  },

  async saveFileAs(filePath: string): Promise<void> {
    return saveFileAsImpl(filePath)
  },

  async checkExternalFileChanges(): Promise<void> {
    return checkExternalFileChanges()
  },

  panelIdToFilePath(panelId: string): string | null {
    return panelIdToFilePath(panelId)
  },

  findCenterGroup(): string | undefined {
    return findCenterGroupImpl()
  },

  switchToFile(filePath: string): void {
    switchToFileImpl(filePath)
  },

  async saveCurrentFile(): Promise<void> {
    const info = activeFileInfo.value
    if (!info) return
    try {
      await saveCurrentFileToDisk(info.filePath)
    } catch (e) {
      console.warn('[EditorManager] Save:', e)
    }
    const { removeSnapshot } = useEditorRecovery()
    removeSnapshot(info.filePath)
  },

  async openNewQuery(connectionId?: string, databaseName?: string): Promise<void> {
    try {
      const { createScratchpadEntry, listScratchpadFiles } =
        await import('@/extensions/builtin/scratchpad/infrastructure/api/scratchpad-api')
      const ts = Date.now()
      const fileName = `Untitled-${ts}.sql`
      try {
        await listScratchpadFiles()
      } catch {
        console.warn('[EditorManager] listScratchpadFiles failed')
      }
      const entry = await createScratchpadEntry(fileName, false)
      const path = (entry as { path?: string }).path || fileName
      openFileImpl({
        filePath: path,
        fileName,
        language: 'sql',
        sql: '',
        type: 'file',
        connectionId: connectionId ?? '',
        databaseName: databaseName ?? '',
      })
    } catch (e) {
      console.error('[EditorManager] openNewQuery failed:', e)
    }
  },

  async openAnalysisPanel(connectionId?: string, databaseName?: string): Promise<void> {
    try {
      const { createScratchpadEntry, listScratchpadFiles } =
        await import('@/extensions/builtin/scratchpad/infrastructure/api/scratchpad-api')
      const ts = Date.now()
      const fileName = `分析-${ts}.sql`
      try {
        await listScratchpadFiles()
      } catch {
        console.warn('[EditorManager] listScratchpadFiles failed')
      }
      const entry = await createScratchpadEntry(fileName, false)
      const path = (entry as { path?: string }).path || fileName
      openFileImpl({
        filePath: path,
        fileName,
        language: 'sql',
        sql: '',
        type: 'analysis',
        connectionId: connectionId ?? '',
        databaseName: databaseName ?? '',
      })
    } catch (e) {
      console.error('[EditorManager] openAnalysisPanel failed:', e)
    }
  },

  async popoutActiveFile(): Promise<void> {
    popoutActiveFileImpl()
  },

  setupCrossWindowListeners(): void {
    setupCrossWindowListenersImpl()
  },

  executeCurrentSQL(): Promise<void> {
    return executeCurrentSQLService()
  },

  async executeNewTabSQL(): Promise<void> {
    return executeNewTabSQLService()
  },

  async executeDuckDBAccelerated(): Promise<void> {
    return executeDuckDBAcceleratedService()
  },

  cancelExecution(): void {
    cancelExecutionService()
  },

  async formatSQL(): Promise<void> {
    return formatActiveSQL()
  },

  async validateSQL(): Promise<void> {
    return validateActiveSQL()
  },

  /** 批量执行 SQL */
  executeBatchSQL(): Promise<void> {
    return executeBatchSQLService()
  },

  /** 方言转译 */
  async transpileSQL(): Promise<void> {
    return transpileActiveSQLService()
  },

  /** 执行计划 */
  async explainSQL(): Promise<void> {
    return explainActiveSQLService()
  },

  /** 保存为片段 */
  async saveSnippet(): Promise<void> {
    return saveActiveSnippetService()
  },

  /** 切换文件类型 */
  changeFileType(
    filePath: string,
    newType: import('@/extensions/builtin/workbench/types/editor-types').FileType,
    newLanguage: string
  ): boolean {
    return changeFileTypeImpl(filePath, newType, newLanguage)
  },

  toggleComment(): void {
    toggleCommentService()
  },

  renameResultSet(panelId: string, newTitle: string): void {
    renameResultSetImpl(panelId, newTitle)
  },
}
