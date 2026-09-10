<template>
  <div class="snippet-panel">
    <div class="snippet-search">
      <input
        v-model="searchQuery"
        type="text"
        :placeholder="t('sqlEditor.searchSnippets')"
        class="snippet-search-input"
        @input="filterSnippets"
      />
    </div>
    <div class="snippet-list">
      <template v-if="isLoading">
        <div class="snippet-loading">{{ t('common.loading') }}</div>
      </template>
      <template v-else-if="Object.keys(groupedSnippets).length === 0">
        <div class="snippet-empty">{{ t('sqlEditor.noSnippets') }}</div>
      </template>
      <template v-else>
        <div v-for="(snippets, category) in groupedSnippets" :key="category" class="snippet-group">
          <div class="snippet-category">{{ category }}</div>
          <div
            v-for="snippet in snippets"
            :key="snippet.id"
            class="snippet-item"
            @click="insertSnippet(snippet)"
          >
            <div class="snippet-label">{{ snippet.label }}</div>
            <div class="snippet-detail">{{ snippet.detail }}</div>
            <button
              v-if="snippet.isCustom"
              class="snippet-delete"
              title="Delete"
              @click.stop="handleDelete(snippet.id)"
            >
              ×
            </button>
          </div>
        </div>
      </template>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'

import {
  getAllSnippets,
  deleteCustomSnippet,
  type SqlSnippet,
} from '@/extensions/builtin/workbench/services/sql-snippets'

const { t } = useI18n()
const searchQuery = ref('')
const isLoading = ref(true)
const groupedSnippets = reactive<Record<string, SqlSnippet[]>>({})

function groupSnippets(snippets: SqlSnippet[]): Record<string, SqlSnippet[]> {
  const groups: Record<string, SqlSnippet[]> = {}
  for (const s of snippets) {
    if (!groups[s.category]) groups[s.category] = []
    groups[s.category].push(s)
  }
  return groups
}

function filterSnippets(): void {
  const query = searchQuery.value.toLowerCase().trim()
  const all = getAllSnippets()
  const filtered = query
    ? all.filter(
        s =>
          s.label.toLowerCase().includes(query) ||
          s.detail.toLowerCase().includes(query) ||
          s.insertText.toLowerCase().includes(query)
      )
    : all

  const grouped = groupSnippets(filtered)
  Object.keys(groupedSnippets).forEach(k => delete groupedSnippets[k])
  Object.assign(groupedSnippets, grouped)
}

function insertSnippet(snippet: SqlSnippet): void {
  const text = snippet.insertText.replace(/\$\{\d+:(.*?)\}/g, '$1')
  window.dispatchEvent(new CustomEvent('insert-snippet', { detail: { text } }))
}

function handleDelete(id: string): void {
  deleteCustomSnippet(id)
  filterSnippets()
}

onMounted(() => {
  filterSnippets()
  isLoading.value = false
})
</script>

<style scoped>
.snippet-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg-primary, #1e1e1e);
  color: var(--text-primary, #cccccc);
}

.snippet-search {
  padding: 8px;
  border-bottom: 1px solid var(--border-color, #3e3e42);
}

.snippet-search-input {
  width: 100%;
  padding: 6px 10px;
  border: 1px solid var(--border-color, #3e3e42);
  border-radius: 4px;
  background: var(--bg-input, #3c3c3c);
  color: var(--text-primary, #cccccc);
  font-size: 12px;
  outline: none;
  box-sizing: border-box;
}

.snippet-search-input:focus {
  border-color: var(--accent-color, #0078d4);
}

.snippet-list {
  flex: 1;
  overflow-y: auto;
  padding: 8px;
}

.snippet-loading,
.snippet-empty {
  padding: 16px;
  text-align: center;
  color: var(--text-secondary, #858585);
  font-size: 12px;
}

.snippet-group {
  margin-bottom: 12px;
}

.snippet-category {
  font-size: 11px;
  font-weight: 600;
  text-transform: uppercase;
  color: var(--text-secondary, #858585);
  margin-bottom: 4px;
  padding: 0 4px;
}

.snippet-item {
  position: relative;
  padding: 6px 8px;
  border-radius: 4px;
  cursor: pointer;
  transition: background 0.15s;
}

.snippet-item:hover {
  background: var(--bg-hover, #2a2d2e);
}

.snippet-label {
  font-size: 12px;
  font-weight: 500;
  color: var(--text-primary, #cccccc);
}

.snippet-detail {
  font-size: 11px;
  color: var(--text-tertiary, #858585);
  margin-top: 2px;
  font-family: 'Consolas', 'Courier New', monospace;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.snippet-delete {
  position: absolute;
  right: 6px;
  top: 50%;
  transform: translateY(-50%);
  background: none;
  border: none;
  color: var(--text-tertiary, #858585);
  font-size: 14px;
  cursor: pointer;
  padding: 0 4px;
  line-height: 1;
  opacity: 0;
  transition: opacity 0.15s;
}

.snippet-item:hover .snippet-delete {
  opacity: 1;
}

.snippet-delete:hover {
  color: var(--danger-color, #f44747);
}
</style>
