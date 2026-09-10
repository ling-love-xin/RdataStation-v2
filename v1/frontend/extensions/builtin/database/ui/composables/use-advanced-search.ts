import { ref, computed } from 'vue'

import { searchDatabaseObjects, highlightText } from '../utils/search-utils'

import type { SearchObjectResult, DatabaseInfo } from '../types/navigator'

export interface SearchHistoryItem {
  query: string
  timestamp: Date
  resultCount: number
}

export interface SearchFilter {
  objectType: 'all' | 'table' | 'view' | 'column' | 'schema'
  connectionId: string | 'all'
  databaseName: string | 'all'
}

export function useAdvancedSearch() {
  const searchQuery = ref('')
  const searchResults = ref<SearchObjectResult[]>([])
  const searchHistory = ref<SearchHistoryItem[]>([])
  const isSearching = ref(false)
  const selectedResultIndex = ref(-1)

  const searchFilter = ref<SearchFilter>({
    objectType: 'all',
    connectionId: 'all',
    databaseName: 'all',
  })

  const filteredResults = computed(() => {
    let results = searchResults.value

    if (searchFilter.value.objectType !== 'all') {
      results = results.filter(r => r.objectType === searchFilter.value.objectType)
    }

    if (searchFilter.value.connectionId !== 'all') {
      results = results.filter(r => r.connectionId === searchFilter.value.connectionId)
    }

    if (searchFilter.value.databaseName !== 'all') {
      results = results.filter(r => r.databaseName === searchFilter.value.databaseName)
    }

    return results
  })

  const recentSearches = computed(() => {
    return searchHistory.value.slice(0, 10)
  })

  function performSearch(
    connections: Array<{ id: string; databases: DatabaseInfo[] }>,
    query: string
  ) {
    if (!query || query.trim().length === 0) {
      searchResults.value = []
      return
    }

    isSearching.value = true
    searchQuery.value = query

    const results = searchDatabaseObjects(connections, query)
    searchResults.value = results
    selectedResultIndex.value = -1

    searchHistory.value.unshift({
      query,
      timestamp: new Date(),
      resultCount: results.length,
    })

    if (searchHistory.value.length > 50) {
      searchHistory.value = searchHistory.value.slice(0, 50)
    }

    isSearching.value = false
  }

  function clearSearch() {
    searchQuery.value = ''
    searchResults.value = []
    selectedResultIndex.value = -1
  }

  function selectNextResult() {
    if (filteredResults.value.length === 0) return

    selectedResultIndex.value = (selectedResultIndex.value + 1) % filteredResults.value.length
  }

  function selectPrevResult() {
    if (filteredResults.value.length === 0) return

    selectedResultIndex.value =
      selectedResultIndex.value <= 0
        ? filteredResults.value.length - 1
        : selectedResultIndex.value - 1
  }

  function getSelectedResult(): SearchObjectResult | null {
    if (
      selectedResultIndex.value < 0 ||
      selectedResultIndex.value >= filteredResults.value.length
    ) {
      return null
    }
    return filteredResults.value[selectedResultIndex.value]
  }

  function clearHistory() {
    searchHistory.value = []
  }

  function getHighlightedName(result: SearchObjectResult): string {
    return highlightText(result.objectName, searchQuery.value)
  }

  function updateFilter(filter: Partial<SearchFilter>) {
    searchFilter.value = { ...searchFilter.value, ...filter }
  }

  return {
    searchQuery,
    searchResults,
    searchHistory,
    isSearching,
    selectedResultIndex,
    searchFilter,
    filteredResults,
    recentSearches,
    performSearch,
    clearSearch,
    selectNextResult,
    selectPrevResult,
    getSelectedResult,
    clearHistory,
    getHighlightedName,
    updateFilter,
  }
}
