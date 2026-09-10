const RECOVERY_PREFIX = 'rdata:workbench:recovery:'
const SNAPSHOT_LIST_KEY = `${RECOVERY_PREFIX}snapshot_list`
const MAX_TOTAL_SIZE = 4 * 1024 * 1024
const MAX_SINGLE_SIZE = 2 * 1024 * 1024

interface RecoverySnapshot {
  filePath: string
  fileName: string
  language: string
  stateJSON: Record<string, unknown> | null
  scrollTop: number
  scrollLeft: number
  isDirty: boolean
  timestamp: number
}

function snapshotKey(filePath: string): string {
  const sanitized = filePath.replace(/[^a-zA-Z0-9_\-./\\]/g, '_')
  return `${RECOVERY_PREFIX}${sanitized}`
}

function getTotalRecoverySize(): number {
  let size = 0
  for (let i = localStorage.length - 1; i >= 0; i--) {
    const key = localStorage.key(i)
    if (key?.startsWith(RECOVERY_PREFIX)) {
      const value = localStorage.getItem(key)
      if (value) size += value.length * 2
    }
  }
  return size
}

export function useEditorRecovery() {
  function saveSnapshot(
    filePath: string,
    stateJSON: Record<string, unknown> | null,
    meta: {
      fileName: string
      language: string
      isDirty: boolean
      scrollTop?: number
      scrollLeft?: number
    }
  ): boolean {
    try {
      const payload: RecoverySnapshot = {
        filePath,
        fileName: meta.fileName,
        language: meta.language,
        stateJSON,
        scrollTop: meta.scrollTop ?? 0,
        scrollLeft: meta.scrollLeft ?? 0,
        isDirty: meta.isDirty,
        timestamp: Date.now(),
      }

      const serialized = JSON.stringify(payload)
      const entrySize = serialized.length * 2

      if (entrySize > MAX_SINGLE_SIZE) {
        const fallback: RecoverySnapshot = { ...payload, stateJSON: null }
        const fallbackSerialized = JSON.stringify(fallback)
        if (getTotalRecoverySize() + fallbackSerialized.length * 2 < MAX_TOTAL_SIZE) {
          localStorage.setItem(snapshotKey(filePath), fallbackSerialized)
        }
        return false
      }

      if (getTotalRecoverySize() + entrySize > MAX_TOTAL_SIZE) {
        const fallback: RecoverySnapshot = { ...payload, stateJSON: null }
        const fallbackSerialized = JSON.stringify(fallback)
        localStorage.setItem(snapshotKey(filePath), fallbackSerialized)
        return false
      }

      localStorage.setItem(snapshotKey(filePath), serialized)
      return true
    } catch {
      return false
    }
  }

  function loadSnapshots(): RecoverySnapshot[] {
    const snapshots: RecoverySnapshot[] = []
    const seen = new Set<string>()

    for (let i = localStorage.length - 1; i >= 0; i--) {
      const key = localStorage.key(i)
      if (key?.startsWith(RECOVERY_PREFIX) && key !== SNAPSHOT_LIST_KEY) {
        try {
          const raw = localStorage.getItem(key)
          if (raw) {
            const snapshot = JSON.parse(raw) as RecoverySnapshot
            if (snapshot.filePath && !seen.has(snapshot.filePath)) {
              seen.add(snapshot.filePath)
              snapshots.push(snapshot)
            }
          }
        } catch {
          localStorage.removeItem(key)
        }
      }
    }

    return snapshots
  }

  function hasRecoveryData(): boolean {
    for (let i = localStorage.length - 1; i >= 0; i--) {
      const key = localStorage.key(i)
      if (key?.startsWith(RECOVERY_PREFIX) && key !== SNAPSHOT_LIST_KEY) {
        try {
          const raw = localStorage.getItem(key)
          if (raw) {
            const snapshot = JSON.parse(raw) as RecoverySnapshot
            if (snapshot.filePath) return true
          }
        } catch {
          localStorage.removeItem(key)
        }
      }
    }
    return false
  }

  function clearRecovery(): void {
    const keysToRemove: string[] = []
    for (let i = localStorage.length - 1; i >= 0; i--) {
      const key = localStorage.key(i)
      if (key?.startsWith(RECOVERY_PREFIX)) {
        keysToRemove.push(key)
      }
    }
    keysToRemove.forEach(k => localStorage.removeItem(k))
  }

  function removeSnapshot(filePath: string): void {
    localStorage.removeItem(snapshotKey(filePath))
  }

  return {
    saveSnapshot,
    loadSnapshots,
    hasRecoveryData,
    clearRecovery,
    removeSnapshot,
  }
}
