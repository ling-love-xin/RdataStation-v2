<template>
  <div class="filter-bar">
    <div class="filter-row">
      <div class="filter-group compact">
        <span class="filter-label">{{ t('analyticsResource.scope') }}:</span>
        <select
          class="filter-select"
          :value="selectedScope ?? 'all'"
          @change="selectScope($event)"
        >
          <option value="all">{{ t('analyticsResource.all') }}</option>
          <option value="global">🌍 {{ t('analyticsResource.global') }}</option>
          <option value="project">📂 {{ t('analyticsResource.project') }}</option>
          <option value="session">📌 {{ t('analyticsResource.session') }}</option>
        </select>
      </div>

      <div class="filter-group compact">
        <span class="filter-label">{{ t('analyticsResource.type') }}:</span>
        <select
          class="filter-select"
          :value="selectedType ?? 'all'"
          @change="selectType($event)"
        >
          <option value="all">{{ t('analyticsResource.all') }}</option>
          <option value="connection">🔌 {{ t('analyticsResource.connection') }}</option>
          <option value="table">📊 {{ t('analyticsResource.table') }}</option>
          <option value="file">📄 {{ t('analyticsResource.file') }}</option>
        </select>
      </div>

      <div class="filter-group compact">
        <span class="filter-label">{{ t('analyticsResource.sort') }}:</span>
        <select
          class="filter-select"
          :value="selectedSort ?? 'name'"
          @change="selectSort(($event.target as HTMLSelectElement).value as SortField)"
        >
          <option value="name">{{ t('analyticsResource.name') }}</option>
          <option value="created_at">{{ t('analyticsResource.createdAt') }}</option>
          <option value="updated_at">{{ t('analyticsResource.updatedAt') }}</option>
          <option value="row_count">{{ t('analyticsResource.rowCount') }}</option>
          <option value="file_size">{{ t('analyticsResource.fileSize') }}</option>
        </select>
        <button
          class="sort-order-btn"
          :title="
            sortOrder === 'asc'
              ? t('analyticsResource.ascending')
              : t('analyticsResource.descending')
          "
          @click="toggleSortOrder"
        >
          {{ sortOrder === 'asc' ? '↑' : '↓' }}
        </button>
      </div>

      <div v-if="selectedCount > 0" class="selection-info">
        <span>{{ t('analyticsResource.selectedCount', { count: selectedCount }) }}</span>
        <button class="batch-action-btn danger" @click="emit('batchDelete')">🗑️</button>
        <button class="clear-selection-btn" @click="clearSelection">{{
          t('analyticsResource.clearSelection')
        }}</button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'

import type { SortField, SortOrder } from '../../types'

const { t } = useI18n()

const props = defineProps<{
  selectedScope: string | null
  selectedType: string | null
  selectedCount: number
  selectedSort: SortField | null
  sortOrder: SortOrder
}>()

const emit = defineEmits<{
  'update:selectedScope': [value: string | null]
  'update:selectedType': [value: string | null]
  'update:selectedSort': [value: SortField | null]
  'update:sortOrder': [value: SortOrder]
  clearSelection: []
  batchDelete: []
}>()

const _types = [
  { value: null, label: t('analyticsResource.all') },
  { value: 'connection', label: '🔌 ' + t('analyticsResource.connection') },
  { value: 'table', label: '📊 ' + t('analyticsResource.table') },
  { value: 'file', label: '📄 ' + t('analyticsResource.file') },
]

const _sortOptions: { value: SortField; label: string }[] = [
  { value: 'name', label: t('analyticsResource.name') },
  { value: 'created_at', label: t('analyticsResource.createdAt') },
  { value: 'updated_at', label: t('analyticsResource.updatedAt') },
  { value: 'row_count', label: t('analyticsResource.rowCount') },
  { value: 'file_size', label: t('analyticsResource.fileSize') },
]

function selectScope(e: Event) {
  const value = (e.target as HTMLSelectElement).value
  emit('update:selectedScope', value === 'all' ? null : value)
}

function selectType(e: Event) {
  const value = (e.target as HTMLSelectElement).value
  emit('update:selectedType', value === 'all' ? null : value)
}

function selectSort(value: SortField | null) {
  if (props.selectedSort === value) {
    emit('update:sortOrder', props.sortOrder === 'asc' ? 'desc' : 'asc')
  } else {
    emit('update:selectedSort', value)
    emit('update:sortOrder', 'asc')
  }
}

function toggleSortOrder() {
  emit('update:sortOrder', props.sortOrder === 'asc' ? 'desc' : 'asc')
}

function clearSelection() {
  emit('clearSelection')
}
</script>

<style scoped>
.filter-bar {
  padding: var(--size-sm) var(--size-md);
  border-bottom: 1px solid var(--border-color);
  background: var(--bg-secondary);
}

.filter-row {
  display: flex;
  align-items: center;
  gap: var(--size-md);
  flex-wrap: nowrap;
}

.filter-group {
  display: flex;
  align-items: center;
  gap: var(--size-xs);
}

.filter-group.compact {
  gap: var(--spacing-xs);
}

.filter-label {
  font-size: var(--font-size-sm);
  color: var(--text-secondary);
  white-space: nowrap;
}

.filter-select {
  padding: 2px 6px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--bg-primary);
  color: var(--text-primary);
  font-size: var(--font-size-sm);
  cursor: pointer;
  height: 26px;
  min-width: 80px;
  outline: none;
  transition: border-color 0.2s;
}

.filter-select:hover {
  border-color: var(--primary-color);
}

.filter-select:focus {
  border-color: var(--primary-color);
  box-shadow: 0 0 0 1px var(--primary-color);
}

.sort-order-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 26px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--bg-primary);
  color: var(--text-secondary);
  font-size: var(--font-size-sm);
  cursor: pointer;
  transition: all 0.2s;
}

.sort-order-btn:hover {
  border-color: var(--primary-color);
  color: var(--primary-color);
}

.selection-info {
  display: flex;
  align-items: center;
  gap: var(--size-sm);
  margin-left: auto;
  font-size: var(--font-size-sm);
  color: var(--text-secondary);
}

.clear-selection-btn {
  padding: 2px 6px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--font-size-xs);
  cursor: pointer;
  transition: all 0.15s;
  height: 24px;
}

.clear-selection-btn:hover {
  border-color: var(--danger-color);
  color: var(--danger-color);
}

.batch-action-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: transparent;
  font-size: var(--font-size-sm);
  cursor: pointer;
  transition: all 0.15s;
}

.batch-action-btn.danger {
  color: var(--danger-color);
}

.batch-action-btn.danger:hover {
  border-color: var(--danger-color);
  background: var(--danger-light);
}
</style>
