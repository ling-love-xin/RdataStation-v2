import { EditorState } from '@codemirror/state'
import { EditorView } from '@codemirror/view'
import { ref, shallowRef, computed, type Ref, type ShallowRef } from 'vue'

import type {
  OpenFileInfo,
  EditorInstance,
} from '@/extensions/builtin/workbench/types/editor-types'
import { useEditorRuntime } from '@/extensions/builtin/workbench/ui/stores/editor-runtime-store'

interface DockviewPanelHandle {
  api: { close(): void; setTitle(t: string): void; setVisible(v: boolean): void }
  focus(): void
  id: string
  group?: { id: string }
}

interface DockviewGroupHandle {
  api: { close(): void; setVisible(v: boolean): void; moveTo(p: { group: string }): void }
  id: string
  panels: Array<{ id: string }>
}

export interface DockviewApiFacade {
  addPanel(opts: Record<string, unknown>): void
  getPanel(id: string): DockviewPanelHandle | undefined
  getGroup(id: string): DockviewGroupHandle | undefined
  movePanelOrGroup(panelId: string, opts: Record<string, unknown>): void
}

export const MAX_RESULT_SETS = 5
export const SQL_LOG_TRUNCATE_LENGTH = 500
export const DEFAULT_POPOUT_GEOMETRY = { x: 200, y: 200, width: 800, height: 400 } as const

const {
  runtime,
  startExecution,
  finishExecution,
  cancelExecution: cancelRuntime,
} = useEditorRuntime()

export const openFiles: ShallowRef<Map<string, OpenFileInfo>> = shallowRef(new Map())
export const activeFilePath: Ref<string | null> = ref(null)
export const editorRef: ShallowRef<EditorView | null> = shallowRef(null)
export const isInitialized: Ref<boolean> = ref(false)
export const crossWindowUnlisteners: (() => void)[] = []
export const tabGroupId: Ref<string | null> = ref(null)

export const editorInstances = new Map<string, EditorInstance>()
export const panelGroupMap = new Map<string, string>()
export const savedStates = new Map<string, EditorState>()

export let dockviewApi: DockviewApiFacade | null = null
export function setDockviewApi(api: DockviewApiFacade): void {
  dockviewApi = api
}
export function clearDockviewApi(): void {
  dockviewApi = null
}

export let resultIdCounter = 0
export function nextResultId(): number {
  resultIdCounter++
  return resultIdCounter
}
export function resetResultIdCounter(): void {
  resultIdCounter = 0
}

export const activeFileInfo = computed(() => {
  if (!activeFilePath.value) return null
  return openFiles.value.get(activeFilePath.value) ?? null
})

export const runtimeState = {
  get isExecuting() {
    return runtime.isExecuting
  },
  get cancelled() {
    return runtime.cancelled
  },
  get lastExecutionTime() {
    return runtime.lastExecutionTime
  },
  startExecution,
  finishExecution,
  cancelExecution: cancelRuntime,
}

export function notifyOpenFilesChanged(): void {
  openFiles.value = new Map(openFiles.value)
}
