<template>
  <div v-if="show" class="search-container">
    <div class="search-input-wrapper">
      <Search :size="14" class="search-icon" aria-label="搜索" role="img" />
      <input
        ref="searchInput"
        :value="query"
        type="text"
        class="search-input"
        :placeholder="t('search.placeholder')"
        @input="$emit('update:query', ($event.target as HTMLInputElement).value)"
        @keydown.esc="$emit('clear')"
        @keydown.enter="handleEnter"
      />
      <button v-if="query" class="clear-btn" aria-label="清除搜索" @click="$emit('clear')">
        <X :size="14" aria-hidden="true" />
      </button>
    </div>
    <div v-if="query && searchResults.length > 0" class="search-results">
      <div
        v-for="(result, index) in searchResults"
        :key="result.nodeKey"
        class="search-result-item"
        :class="{ active: index === activeIndex }"
        @click="handleSelectResult(result)"
        @mouseenter="activeIndex = index"
      >
        <Table :size="12" class="result-icon" aria-hidden="true" />
        <div class="result-info">
          <span class="result-name">{{ result.tableName }}</span>
          <span class="result-path">{{ result.path }}</span>
        </div>
      </div>
    </div>
    <div v-if="query && searchResults.length === 0 && searched" class="search-empty">
      {{ t('search.noResults') }}
    </div>
  </div>
</template>

<script setup lang="ts">
import { X, Search, Table } from 'lucide-vue-next'
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

interface SearchResult {
  nodeKey: string
  tableName: string
  path: string
  connectionId: string
  dbName: string
  schemaName: string
}

const props = defineProps<{
  show: boolean
  query: string
}>()

const emit = defineEmits<{
  'update:query': [value: string]
  clear: []
  select: [result: SearchResult]
}>()

const searchInput = ref<HTMLInputElement | null>(null)
const searchResults = ref<SearchResult[]>([])
const activeIndex = ref(0)
const searched = ref(false)

let debounceTimer: ReturnType<typeof setTimeout> | null = null

watch(
  () => props.query,
  newQuery => {
    if (debounceTimer) {
      clearTimeout(debounceTimer)
    }

    if (!newQuery || newQuery.trim().length === 0) {
      searchResults.value = []
      searched.value = false
      return
    }

    debounceTimer = setTimeout(() => {
      searched.value = true
      activeIndex.value = 0
    }, 300)
  }
)

function handleEnter() {
  if (searchResults.value.length > 0 && activeIndex.value >= 0) {
    handleSelectResult(searchResults.value[activeIndex.value])
  }
}

function handleSelectResult(result: SearchResult) {
  emit('select', result)
  emit('clear')
}

defineExpose({
  focus: () => searchInput.value?.focus(),
  setSearchResults: (results: SearchResult[]) => {
    searchResults.value = results
    searched.value = true
    activeIndex.value = 0
  },
})
</script>

<style scoped>
.search-container {
  padding: 6px 8px;
  background: var(--bg-tertiary);
  border-bottom: 1px solid var(--border-color);
  position: relative;
}

.search-input-wrapper {
  position: relative;
  display: flex;
  align-items: center;
}

.search-icon {
  position: absolute;
  left: 8px;
  color: var(--text-secondary);
  pointer-events: none;
}

.search-input {
  width: 100%;
  padding: 4px 28px 4px 28px;
  border: 1px solid var(--border-color);
  border-radius: 3px;
  background: var(--bg-primary);
  color: var(--text-primary);
  font-size: 13px;
  outline: none;
}

.search-input:focus {
  border-color: var(--primary-color);
}

.clear-btn {
  position: absolute;
  right: 4px;
  width: 18px;
  height: 18px;
  border: none;
  border-radius: 2px;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
}

.clear-btn:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}

.search-results {
  position: absolute;
  top: 100%;
  left: 0;
  right: 0;
  max-height: 300px;
  overflow-y: auto;
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: 4px;
  margin-top: 4px;
  z-index: 100;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
}

.search-result-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 8px;
  cursor: pointer;
  transition: background 0.15s;
}

.search-result-item:hover,
.search-result-item.active {
  background: var(--bg-hover);
}

.result-icon {
  color: var(--primary-color);
  flex-shrink: 0;
}

.result-info {
  display: flex;
  flex-direction: column;
  min-width: 0;
}

.result-name {
  font-size: 13px;
  color: var(--text-primary);
  font-weight: 500;
}

.result-path {
  font-size: 11px;
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.search-empty {
  padding: 12px 8px;
  text-align: center;
  color: var(--text-secondary);
  font-size: 12px;
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: 4px;
  margin-top: 4px;
}
</style>
