import { ref, onMounted } from 'vue'

const CONTEXT_STORAGE_KEY = 'navigator:context'

export interface NavigatorContext {
  lastExpandedKeys: string[]
  lastSelectedNode: string | null
  lastFilterConfig: Record<string, unknown>
  lastSearchQuery: string
  lastConnectionId: string | null
  lastScrollPosition: number
  timestamp: number
}

const DEFAULT_CONTEXT: NavigatorContext = {
  lastExpandedKeys: [],
  lastSelectedNode: null,
  lastFilterConfig: {},
  lastSearchQuery: '',
  lastConnectionId: null,
  lastScrollPosition: 0,
  timestamp: 0,
}

export function useContextMemory() {
  const context = ref<NavigatorContext>({ ...DEFAULT_CONTEXT })

  function loadContext(): NavigatorContext {
    try {
      const saved = localStorage.getItem(CONTEXT_STORAGE_KEY)
      if (saved) {
        const parsed = JSON.parse(saved) as NavigatorContext
        const age = Date.now() - parsed.timestamp
        if (age < 7 * 24 * 60 * 60 * 1000) {
          context.value = parsed
          return parsed
        } else {
          localStorage.removeItem(CONTEXT_STORAGE_KEY)
        }
      }
    } catch (e) {
      console.error('Failed to load navigator context:', e)
    }
    return { ...DEFAULT_CONTEXT }
  }

  function saveContext(data: Partial<NavigatorContext>) {
    try {
      context.value = {
        ...context.value,
        ...data,
        timestamp: Date.now(),
      }

      localStorage.setItem(CONTEXT_STORAGE_KEY, JSON.stringify(context.value))
    } catch (e) {
      console.error('Failed to save navigator context:', e)
    }
  }

  function clearContext() {
    try {
      localStorage.removeItem(CONTEXT_STORAGE_KEY)
      context.value = { ...DEFAULT_CONTEXT }
    } catch (e) {
      console.error('Failed to clear navigator context:', e)
    }
  }

  function updateExpandedKeys(keys: string[]) {
    saveContext({ lastExpandedKeys: keys })
  }

  function updateSelectedNode(node: string | null) {
    saveContext({ lastSelectedNode: node })
  }

  function updateFilterConfig(config: Record<string, unknown>) {
    saveContext({ lastFilterConfig: config })
  }

  function updateSearchQuery(query: string) {
    saveContext({ lastSearchQuery: query })
  }

  function updateConnectionId(id: string | null) {
    saveContext({ lastConnectionId: id })
  }

  function updateScrollPosition(position: number) {
    saveContext({ lastScrollPosition: position })
  }

  onMounted(() => {
    loadContext()
  })

  return {
    context,
    loadContext,
    saveContext,
    clearContext,
    updateExpandedKeys,
    updateSelectedNode,
    updateFilterConfig,
    updateSearchQuery,
    updateConnectionId,
    updateScrollPosition,
  }
}
