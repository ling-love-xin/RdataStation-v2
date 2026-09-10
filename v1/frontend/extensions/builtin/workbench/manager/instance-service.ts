import { EditorState } from '@codemirror/state'
import { EditorView } from '@codemirror/view'
import { markRaw } from 'vue'

import type { EditorInstance } from '@/extensions/builtin/workbench/types/editor-types'
import { PANEL_PREFIX_EDITOR } from '@/extensions/builtin/workbench/types/editor-types'

import { editorInstances, panelGroupMap, savedStates, openFiles, dockviewApi } from './editor-state'

function instanceId(filePath: string, groupId: string): string {
  return `${groupId}::${filePath}`
}

function sanitize(s: string): string {
  return s.replace(/[^a-zA-Z0-9_-]/g, '_')
}

function filePanelId(filePath: string): string {
  return `${PANEL_PREFIX_EDITOR}${sanitize(filePath)}`
}

export function panelIdToFilePath(panelId: string): string | null {
  for (const [fp] of openFiles.value) {
    if (filePanelId(fp) === panelId) return fp
  }
  return null
}

export function getEditorView(filePath: string): EditorView | undefined {
  for (const [, inst] of editorInstances) {
    if (inst.filePath === filePath && inst.writable) return inst.view
  }
  return undefined
}

export function getEditorViewForPanel(panelId: string): EditorView | undefined {
  const gid = panelGroupMap.get(panelId)
  if (!gid) return undefined
  const fp = panelIdToFilePath(panelId)
  if (!fp) return undefined
  const id = instanceId(fp, gid)
  return editorInstances.get(id)?.view
}

export function registerFileEditor(filePath: string, ed: EditorView): void {
  const pid = filePanelId(filePath)
  const groupId = dockviewApi?.getPanel(pid)?.group?.id ?? filePath
  const id = instanceId(filePath, groupId)
  panelGroupMap.set(pid, groupId)
  const writable = isPrimaryInstance(filePath, id)
  const inst: EditorInstance = {
    instanceId: id,
    filePath,
    groupId,
    view: markRaw(ed),
    state: null,
    writable,
  }
  editorInstances.set(id, inst)

  const info = openFiles.value.get(filePath)
  if (info) {
    if (!info.primaryInstanceId) info.primaryInstanceId = id
    else if (id !== info.primaryInstanceId && !info.readonlyInstanceIds.includes(id)) {
      info.readonlyInstanceIds.push(id)
    }
    if (inst.state) {
      info.states.set(id, inst.state)
    }
  }
}

export function unregisterFileEditor(filePath: string): void {
  const pid = filePanelId(filePath)
  const groupId = panelGroupMap.get(pid) ?? filePath
  const id = instanceId(filePath, groupId)
  const inst = editorInstances.get(id)
  if (inst) {
    saveEditorState(filePath, inst.view.state)
    try {
      inst.view.destroy()
    } catch {
      console.warn('[InstanceService] view.destroy failed during unregister')
    }
  }
  editorInstances.delete(id)
  panelGroupMap.delete(pid)
}

export function isPrimaryInstance(filePath: string, instanceId?: string): boolean {
  for (const [id, inst] of editorInstances) {
    if (inst.filePath === filePath && inst.writable) {
      if (instanceId === undefined) return false
      if (id !== instanceId) return false
    }
  }
  return true
}

export function isFileOpenElsewhere(filePath: string, excludeGroupId?: string): boolean {
  for (const [, inst] of editorInstances) {
    if (inst.filePath === filePath) {
      if (excludeGroupId && inst.groupId === excludeGroupId) continue
      return true
    }
  }
  return false
}

export function saveEditorState(filePath: string, state: EditorState): void {
  savedStates.set(filePath, state)
}

export function getSavedState(filePath: string): EditorState | undefined {
  return savedStates.get(filePath)
}

export function updatePanelGroup(panelId: string, groupId: string): void {
  panelGroupMap.set(panelId, groupId)
}

export function clearAllInstances(): void {
  for (const [, inst] of editorInstances) {
    try {
      inst.view.destroy()
    } catch {
      console.warn('[InstanceService] view.destroy failed during clearAll')
    }
  }
  editorInstances.clear()
  panelGroupMap.clear()
  savedStates.clear()
}

export function removeInstancesForFile(filePath: string): string[] {
  const idToRemove: string[] = []
  for (const [id, inst] of editorInstances) {
    if (inst.filePath === filePath) {
      inst.state = inst.view.state
      try {
        inst.view.destroy()
      } catch {
        console.warn('[InstanceService] view.destroy failed during removeInstancesForFile')
      }
      idToRemove.push(id)
    }
  }
  for (const id of idToRemove) {
    editorInstances.delete(id)
  }
  return idToRemove
}
