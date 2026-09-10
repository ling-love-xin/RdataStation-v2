import { defineStore } from 'pinia'
import { ref, computed, shallowRef, type ComputedRef, type ShallowRef, type Ref } from 'vue'

import type { OpenFileInfo } from '@/extensions/builtin/workbench/types/editor-types'

export const useFileStateStore = defineStore('fileState', () => {
  const openFiles: ShallowRef<Map<string, OpenFileInfo>> = shallowRef(new Map())
  const activeFilePath: Ref<string | null> = ref(null)

  const activeFileInfo: ComputedRef<OpenFileInfo | null> = computed(() => {
    if (!activeFilePath.value) return null
    return openFiles.value.get(activeFilePath.value) ?? null
  })

  function addFile(filePath: string, info: OpenFileInfo): void {
    const m = new Map(openFiles.value)
    m.set(filePath, info)
    openFiles.value = m
  }

  function removeFile(filePath: string): void {
    const m = new Map(openFiles.value)
    m.delete(filePath)
    openFiles.value = m
    if (activeFilePath.value === filePath) {
      const keys = Array.from(m.keys())
      activeFilePath.value = keys.length > 0 ? keys[keys.length - 1] : null
    }
  }

  function switchToFile(filePath: string): void {
    if (openFiles.value.has(filePath)) {
      activeFilePath.value = filePath
    }
  }

  function markDirty(filePath: string, dirty: boolean): void {
    const m = new Map(openFiles.value)
    const info = m.get(filePath)
    if (info) {
      m.set(filePath, { ...info, isDirty: dirty })
      openFiles.value = m
    }
  }

  function hasFile(filePath: string): boolean {
    return openFiles.value.has(filePath)
  }

  return {
    openFiles,
    activeFilePath,
    activeFileInfo,
    addFile,
    removeFile,
    switchToFile,
    markDirty,
    hasFile,
  }
})
