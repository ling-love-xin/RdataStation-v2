import { ref } from 'vue'

const STORAGE_KEY = 'analytics_resource_search_history'
const MAX_HISTORY = 10

export function useSearchHistory() {
  const history = ref<string[]>(loadHistory())

  function loadHistory(): string[] {
    try {
      const stored = localStorage.getItem(STORAGE_KEY)
      return stored ? JSON.parse(stored) : []
    } catch {
      return []
    }
  }

  function saveHistory(items: string[]) {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(items))
  }

  function addToHistory(query: string) {
    if (!query.trim()) return
    const trimmed = query.trim()
    const filtered = history.value.filter(h => h !== trimmed)
    filtered.unshift(trimmed)
    const updated = filtered.slice(0, MAX_HISTORY)
    history.value = updated
    saveHistory(updated)
  }

  function removeFromHistory(query: string) {
    const updated = history.value.filter(h => h !== query)
    history.value = updated
    saveHistory(updated)
  }

  function clearHistory() {
    history.value = []
    localStorage.removeItem(STORAGE_KEY)
  }

  return {
    history,
    addToHistory,
    removeFromHistory,
    clearHistory,
  }
}
