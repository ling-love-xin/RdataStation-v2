import { ref, watch, onBeforeUnmount, getCurrentInstance, type Ref, type ComputedRef } from 'vue'

import { saveScratchpadFile } from '@/extensions/builtin/scratchpad/infrastructure/api/scratchpad-api'

export type SaveStatus = 'idle' | 'saving' | 'saved' | 'unsaved' | 'error'

export interface FileSaveOptions {
  filePath: Ref<string> | ComputedRef<string>
  getContent: () => string
  autoSaveInterval?: number
  maxRetries?: number
  retryDelay?: number
  onSaveSuccess?: () => void
  onSaveError?: (error: string) => void
}

export function useFileSave(options: FileSaveOptions) {
  const {
    filePath,
    getContent,
    autoSaveInterval = 30000,
    maxRetries = 3,
    retryDelay = 2000,
    onSaveSuccess,
    onSaveError,
  } = options

  const saveStatus = ref<SaveStatus>('idle')
  const lastSaveTime = ref<number | null>(null)
  const saveError = ref<string | null>(null)
  const retryCount = ref(0)
  const isDirty = ref(false)

  let autoSaveTimer: ReturnType<typeof setInterval> | null = null
  let retryTimer: ReturnType<typeof setTimeout> | null = null

  function clearRetry() {
    if (retryTimer) {
      clearTimeout(retryTimer)
      retryTimer = null
    }
  }

  function scheduleRetry() {
    if (retryTimer) {
      clearTimeout(retryTimer)
      retryTimer = null
    }
    retryTimer = setTimeout(() => {
      performSave()
    }, retryDelay * retryCount.value)
  }

  async function performSave(): Promise<boolean> {
    const path = filePath.value
    if (!path) {
      return false
    }

    saveStatus.value = 'saving'
    saveError.value = null

    try {
      const content = getContent()
      await saveScratchpadFile(path, content)

      saveStatus.value = 'saved'
      lastSaveTime.value = Date.now()
      clearRetry()
      retryCount.value = 0
      isDirty.value = false
      onSaveSuccess?.()
      return true
    } catch (err) {
      saveStatus.value = 'error'
      saveError.value = err instanceof Error ? err.message : String(err)
      onSaveError?.(saveError.value)

      if (retryCount.value < maxRetries) {
        retryCount.value++
        scheduleRetry()
      }

      return false
    }
  }

  function markDirty() {
    if (saveStatus.value !== 'saving') {
      saveStatus.value = 'unsaved'
    }
    isDirty.value = true
  }

  function manualSave(): Promise<boolean> {
    clearRetry()
    return performSave()
  }

  function startAutoSave() {
    stopAutoSave()
    if (autoSaveInterval > 0 && filePath.value) {
      autoSaveTimer = setInterval(() => {
        if (isDirty.value && filePath.value) {
          performSave()
        }
      }, autoSaveInterval)
    }
  }

  function stopAutoSave() {
    if (autoSaveTimer) {
      clearInterval(autoSaveTimer)
      autoSaveTimer = null
    }
  }

  function setAutoSaveInterval(ms: number) {
    if (autoSaveTimer) {
      stopAutoSave()
      if (ms > 0 && filePath.value) {
        autoSaveTimer = setInterval(() => {
          if (isDirty.value && filePath.value) {
            performSave()
          }
        }, ms)
      }
    }
  }

  function triggerBeforeUnloadSave() {
    if (isDirty.value && filePath.value) {
      const content = getContent()
      saveScratchpadFile(filePath.value, content).catch(() => {
        // 静默失败，不阻塞页面关闭
      })
    }
  }

  watch(
    () => filePath.value,
    newPath => {
      if (newPath) {
        startAutoSave()
      } else {
        stopAutoSave()
      }
    },
    { immediate: true }
  )

  if (getCurrentInstance()) {
    onBeforeUnmount(() => {
      stopAutoSave()
      clearRetry()
      triggerBeforeUnloadSave()
    })
  }

  return {
    saveStatus,
    lastSaveTime,
    saveError,
    retryCount,
    isDirty,
    manualSave,
    markDirty,
    startAutoSave,
    stopAutoSave,
    setAutoSaveInterval,
    performSave,
    triggerBeforeUnloadSave,
  }
}
