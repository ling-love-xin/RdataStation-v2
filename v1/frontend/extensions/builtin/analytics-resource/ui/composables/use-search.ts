import { ref, computed, watch } from 'vue'

import { useDebounceFn } from './use-debounce'

export interface UseSearchOptions {
  debounceMs?: number
  onSearch?: (query: string) => void | Promise<void>
}

export function useSearch(options: UseSearchOptions = {}) {
  const { debounceMs = 300, onSearch } = options

  const searchQuery = ref('')
  const debouncedQuery = ref('')
  const isSearching = ref(false)

  const debouncedSearch = useDebounceFn(async (...args: unknown[]) => {
    const query = args[0] as string
    debouncedQuery.value = query
    isSearching.value = true
    try {
      await onSearch?.(query)
    } finally {
      isSearching.value = false
    }
  }, debounceMs)

  watch(searchQuery, newQuery => {
    debouncedSearch(newQuery)
  })

  function clearSearch() {
    searchQuery.value = ''
    debouncedQuery.value = ''
  }

  const hasSearchQuery = computed(() => searchQuery.value.length > 0)

  return {
    searchQuery,
    debouncedQuery,
    isSearching,
    hasSearchQuery,
    clearSearch,
  }
}
