import { defineStore } from 'pinia'
import { ref, computed } from 'vue'

export interface OpenFileInfo {
  panelId: string
  path: string
  name: string
}

export const useScratchpadEditorStore = defineStore('scratchpad-editor', () => {
  const openFiles = ref<Map<string, OpenFileInfo>>(new Map())
  const dirtyFiles = ref<Set<string>>(new Set())

  const openFileList = computed(() => Array.from(openFiles.value.values()))

  function isOpen(path: string): boolean {
    const normalized = normalizePath(path)
    for (const key of openFiles.value.keys()) {
      if (normalizePath(key) === normalized) return true
    }
    return false
  }

  function getPanelId(path: string): string | null {
    const normalized = normalizePath(path)
    for (const [key, info] of openFiles.value) {
      if (normalizePath(key) === normalized) return info.panelId
    }
    return null
  }

  function setOpen(path: string, panelId: string, name: string): void {
    openFiles.value.set(path, { panelId, path, name })
  }

  function removeOpen(path: string): void {
    const normalized = normalizePath(path)
    for (const key of openFiles.value.keys()) {
      if (normalizePath(key) === normalized) {
        openFiles.value.delete(key)
        dirtyFiles.value.delete(key)
        return
      }
    }
  }

  function removeByPanelId(panelId: string): void {
    for (const [key, info] of openFiles.value) {
      if (info.panelId === panelId) {
        openFiles.value.delete(key)
        dirtyFiles.value.delete(key)
        return
      }
    }
  }

  function syncRename(oldPath: string, newPath: string, newName: string): void {
    const normalizedOld = normalizePath(oldPath)
    for (const [key] of openFiles.value) {
      if (normalizePath(key) === normalizedOld) {
        const info = openFiles.value.get(key)
        if (info) {
          openFiles.value.delete(key)
          openFiles.value.set(newPath, { ...info, path: newPath, name: newName })
        }
        if (dirtyFiles.value.has(key)) {
          dirtyFiles.value.delete(key)
          dirtyFiles.value.add(newPath)
        }
        return
      }
    }
  }

  function markDirty(path: string): void {
    dirtyFiles.value.add(path)
  }

  function markClean(path: string): void {
    dirtyFiles.value.delete(path)
  }

  function isDirty(path: string): boolean {
    const normalized = normalizePath(path)
    for (const key of dirtyFiles.value) {
      if (normalizePath(key) === normalized) return true
    }
    return false
  }

  function hasDirtyFiles(): boolean {
    return dirtyFiles.value.size > 0
  }

  function clearAll(): void {
    openFiles.value.clear()
    dirtyFiles.value.clear()
  }

  return {
    openFiles,
    dirtyFiles,
    openFileList,
    isOpen,
    getPanelId,
    setOpen,
    removeOpen,
    removeByPanelId,
    syncRename,
    markDirty,
    markClean,
    isDirty,
    hasDirtyFiles,
    clearAll,
  }
})

function normalizePath(p: string): string {
  return p.replace(/\\/g, '/').replace(/\/$/, '')
}
