import type { EditorState } from '@codemirror/state'
import type { EditorView } from '@codemirror/view'

export const PANEL_PREFIX_EDITOR = 'panel_editor_'
export const PANEL_PREFIX_RESULT = 'panel_result_'
export const PANEL_ID_EMPTY_WORKBENCH = 'panel_emptyWorkbench'
export const PANEL_ID_SCRATCHPAD = 'scratchpad'

export function isEditorPanel(panelId: string): boolean {
  return panelId.startsWith(PANEL_PREFIX_EDITOR)
}

export function isResultPanel(panelId: string): boolean {
  return panelId.startsWith(PANEL_PREFIX_RESULT)
}

export interface EditorInstance {
  instanceId: string
  filePath: string
  groupId: string
  view: EditorView
  state: EditorState | null
  writable: boolean
}

export interface GridStateSnapshot {
  columnState?: Record<string, unknown>[]
  filterModel?: Record<string, unknown>
  sortModel?: Record<string, unknown>[]
}

export type ShortcutScope = 'global' | 'editor' | 'scratchpad' | 'result' | 'none'

export interface ShortcutRegistration {
  key: string
  scope: ShortcutScope
  handler: () => void
  description: string
}

export type DataSource = 'memory' | 'duckdb'

export type FileType = 'file' | 'analysis'

export interface ResultSetMetadata {
  id: string
  title: string
  columns: string[]
  totalRowCount: number
  elapsedMs: number
  affectedRows: number
  messages: string
  sql: string
  timestamp: number
  dataSource: DataSource
  duckdbTable: string | null
  gridState?: GridStateSnapshot
  rows?: unknown[][]
}

export interface OpenFileInfo {
  filePath: string
  fileName: string
  language: string
  type: FileType
  isDirty: boolean
  pinned: boolean
  exists: boolean
  lastModifiedAt: number | null
  connectionId: string
  databaseName: string
  resultSets: ResultSetMetadata[]
  activeResultIndex: number
  resultPanelIds: string[]
  detachedResultIds: string[]
  states: Map<string, EditorState>
  primaryInstanceId: string | null
  readonlyInstanceIds: string[]
}

export interface OpenFileParams {
  filePath: string
  fileName: string
  language: string
  sql: string
  connectionId?: string
  databaseName?: string
  type?: FileType
}

/** 编辑器面板 Props 统一类型 */
export interface EditorPanelParams {
  filePath: string
  fileName: string
  language: string
  content?: string
  connectionId?: string
  databaseName?: string
  type?: FileType
}

export interface ResultSetCreateParams {
  columns: string[]
  rows: unknown[][]
  totalRows: number
  elapsedMs: number
  affectedRows: number
  sql: string
  error: string | null
}

export interface DockviewPanelHandle {
  api: {
    close(): void
    setTitle(title: string): void
  }
}

export interface DockviewGroupHandle {
  id: string
}

/** dockview-vue 的薄封装接口，避免全量导入 */
export interface IDockviewApi {
  addPanel(opts: Record<string, unknown>): void
  getPanel(id: string): DockviewPanelHandle | undefined
  getGroup(id: string): DockviewGroupHandle | undefined
  movePanelOrGroup(panelId: string, opts: Record<string, unknown>): void
}

export interface IEditorManager {
  init(dockviewApi: IDockviewApi): void
  destroy(): void
  openFile(params: OpenFileParams): void
  closeFile(filePath: string): void
  panelIdToFilePath(panelId: string): string | null
  switchToFile(filePath: string): void
  onPanelActivated(panelId: string): void
  saveCurrentFile(): Promise<void>
  openNewQuery(connectionId?: string, databaseName?: string): Promise<void>
  openAnalysisPanel(connectionId?: string, databaseName?: string): void
  executeCurrentSQL(): Promise<void>
  executeNewTabSQL(): Promise<void>
  executeDuckDBAccelerated(): Promise<void>
  executeBatchSQL(): Promise<void>
  cancelExecution(): void
  formatSQL(): Promise<void>
  validateSQL(): Promise<void>
  transpileSQL(): Promise<void>
  explainSQL(): Promise<void>
  saveSnippet(): Promise<void>
  toggleComment(): void
  changeFileType(filePath: string, newType: FileType, newLanguage: string): boolean
  renameResultSet(panelId: string, newTitle: string): void
  setEditor(ed: EditorView): void
  setActiveResultIndex(filePath: string, index: number): void
  readonly openFiles: Map<string, OpenFileInfo>
  readonly activeFilePath: string | null
  readonly activeFileInfo: OpenFileInfo | null
  readonly editor: EditorView | null
  readonly isExecuting: boolean
  readonly lastExecutionTime: number | null
  readonly isInitialized: boolean
  readonly dockviewApi: IDockviewApi | null

  registerFileEditor(filePath: string, ed: EditorView): void
  unregisterFileEditor(filePath: string): void
  isPrimaryInstance(filePath: string, instanceId?: string): boolean
  isFileOpenElsewhere(filePath: string, excludeGroupId?: string): boolean
  getSavedStateForFile(filePath: string): EditorState | undefined
  saveEditorStateForFile(filePath: string, state: EditorState): void
  hasRecoveryData(): boolean
  loadRecoverySnapshots(): {
    filePath: string
    fileName: string
    language: string
    isDirty: boolean
  }[]
  clearRecovery(): void

  createResultSet(filePath: string, data: ResultSetCreateParams): string
  removeResultSet(filePath: string, resultSetId: string): void
  findCenterGroup(): string | undefined
  detachResultPanel(panelId: string): void
  attachResultPanel(panelId: string, filePath: string): void

  updatePanelGroup(panelId: string, groupId: string): void
  onPanelUndocked(panelId: string): void
  setupCrossWindowListeners(): void
  popoutActiveFile(): void
}

export interface IShortcutManager {
  register(key: string, scope: ShortcutScope, handler: () => void, desc: string): void
  unregister(key: string): void
  unregisterByScope(scope: ShortcutScope): void
  setActiveScope(scope: ShortcutScope): void
  handleKeydown(e: KeyboardEvent): void
}

export interface IResultPanelManager {
  addResultSet(filePath: string, data: ResultSetCreateParams): string
  removeResultSet(filePath: string, resultSetId: string): void
  getResultSetRows(filePath: string, resultSetId: string): unknown[][]
  createResultPanel(
    filePath: string,
    panelId: string,
    metadata: ResultSetMetadata,
    rows: unknown[][]
  ): void
  detachResultPanel(panelId: string): void
  attachResultPanel(panelId: string, filePath: string): void
  getAllResultSets(filePath: string): ResultSetMetadata[]
  getActiveResultSet(filePath: string): ResultSetMetadata | null
}

// ========== 多模式编辑器类型系统 ==========

/** 编辑器模式 */
export type EditorType = 'query' | 'analysis' | 'code'

/** 工具栏按钮分组 */
export type ToolbarGroup = 'execute' | 'edit' | 'mode' | 'transaction' | 'federation' | 'general'

/** 工具栏按钮配置 */
export interface ToolbarButton {
  id: string
  group: ToolbarGroup
  label: string
  icon?: string
  shortcut?: string
  visible: (ctx: EditorContext) => boolean
  action: (ctx: EditorContext) => void
}

/** 工具栏分隔符 */
export interface ToolbarSeparator {
  type: 'separator'
  group: string
}

export type ToolbarItem = ToolbarButton | ToolbarSeparator

/** 工具栏配置 */
export interface EditorToolbarConfig {
  items: ToolbarItem[]
}

/** 状态栏字段配置 */
export interface StatusBarField {
  id: string
  label: string
  value: () => string
  visible: (type: EditorType) => boolean
}

/** 状态栏配置 */
export interface EditorStatusBarConfig {
  fields: StatusBarField[]
}

/** 编辑器面板上下文 */
export interface EditorContext {
  editorType: EditorType
  filePath: string
  language: string
  editorView: import('@codemirror/view').EditorView | null
  connectionId: string | null
  isExecuting: boolean
}

/** 编辑器类型选项（用于下拉选择器） */
export const EDITOR_TYPE_OPTIONS: Array<{ label: string; value: EditorType }> = [
  { label: '查询编辑器', value: 'query' },
  { label: '分析编辑器', value: 'analysis' },
  { label: '代码编辑器', value: 'code' },
]

/** 编辑器模式解析器配置 */
export interface EditorModeRule {
  /** 匹配的文件扩展名列表（不含点），如 ['sql', 'mysql', 'pgsql'] */
  extensions: string[]
  /** 匹配的语言标识 */
  languages: string[]
  /** 对应的编辑器类型 */
  editorType: EditorType
}

/** 默认编辑器模式解析规则 */
export const EDITOR_MODE_RULES: EditorModeRule[] = [
  {
    extensions: ['sql', 'mysql', 'pgsql', 'sqlite'],
    languages: ['sql', 'mysql', 'pgsql', 'plpgsql'],
    editorType: 'query',
  },
  { extensions: ['duckdb.sql'], languages: ['duckdb'], editorType: 'analysis' },
  {
    extensions: ['rs', 'ts', 'tsx', 'js', 'jsx', 'py', 'go', 'java', 'c', 'cpp', 'h', 'hpp'],
    languages: ['rust', 'typescript', 'javascript', 'python', 'go', 'java', 'c', 'cpp'],
    editorType: 'code',
  },
]
