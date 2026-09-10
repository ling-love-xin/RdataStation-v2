import { onUnmounted } from 'vue'

const MAX_CACHE_SIZE = 1000
const MAX_NOTIFICATIONS = 50
const MAX_SEARCH_HISTORY = 50

interface CacheManager {
  caches: Map<string, unknown>[]
  maxSize: number
}

const cacheManagers: CacheManager[] = []

export function useMemoryProtection() {
  const cleanupFns: Array<() => void> = []
  const intervals: ReturnType<typeof setInterval>[] = []
  const timeouts: ReturnType<typeof setTimeout>[] = []

  function registerCache(cache: Map<string, unknown>, maxSize = MAX_CACHE_SIZE) {
    cacheManagers.push({ caches: [cache], maxSize })
  }

  function enforceCacheLimit(cache: Map<string, unknown>, maxSize = MAX_CACHE_SIZE) {
    if (cache.size > maxSize) {
      const entriesToRemove = cache.size - maxSize
      const iterator = cache.keys()

      for (let i = 0; i < entriesToRemove; i++) {
        const key = iterator.next().value
        if (key) {
          cache.delete(key)
        }
      }
    }
  }

  function registerCleanup(fn: () => void) {
    cleanupFns.push(fn)
  }

  function registerInterval(interval: ReturnType<typeof setInterval>) {
    intervals.push(interval)
  }

  function registerTimeout(timeout: ReturnType<typeof setTimeout>) {
    timeouts.push(timeout)
  }

  function clearAllIntervals() {
    intervals.forEach(clearInterval)
    intervals.length = 0
  }

  function clearAllTimeouts() {
    timeouts.forEach(clearTimeout)
    timeouts.length = 0
  }

  function cleanup() {
    cleanupFns.forEach(fn => fn())
    cleanupFns.length = 0
    clearAllIntervals()
    clearAllTimeouts()
  }

  function trimNotifications(notifications: unknown[]) {
    if (notifications.length > MAX_NOTIFICATIONS) {
      notifications.splice(0, notifications.length - MAX_NOTIFICATIONS)
    }
  }

  function trimSearchHistory(history: unknown[]) {
    if (history.length > MAX_SEARCH_HISTORY) {
      history.splice(MAX_SEARCH_HISTORY)
    }
  }

  function getMemoryStats(): Record<string, number> {
    return {
      cacheManagers: cacheManagers.length,
      cleanupFns: cleanupFns.length,
      intervals: intervals.length,
      timeouts: timeouts.length,
    }
  }

  onUnmounted(() => {
    cleanup()
  })

  return {
    registerCache,
    enforceCacheLimit,
    registerCleanup,
    registerInterval,
    registerTimeout,
    clearAllIntervals,
    clearAllTimeouts,
    cleanup,
    trimNotifications,
    trimSearchHistory,
    getMemoryStats,
  }
}
