<template>
  <div class="preview-table">
    <div class="table-wrapper">
      <table>
        <thead>
          <tr>
            <th class="row-number">#</th>
            <th v-for="col in columns" :key="col.key" class="sortable" @click="handleSort(col.key)">
              <span>{{ col.title }}</span>
              <span v-if="sortField === col.key" class="sort-icon">
                {{ sortOrder === 'asc' ? '↑' : '↓' }}
              </span>
            </th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="(row, index) in data" :key="index">
            <td class="row-number">{{ index + 1 }}</td>
            <td v-for="col in columns" :key="col.key">
              {{ formatValue(row[col.key]) }}
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <div v-if="data.length === 0" class="empty-state">
      <Database :size="48" />
      <span>{{ t('dataPreview.noData') }}</span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Database } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

interface Column {
  key: string
  title: string
  dataType: string
}

interface Props {
  columns: Column[]
  data: Record<string, unknown>[]
  sortField: string | null
  sortOrder: 'asc' | 'desc' | null
}

const props = defineProps<Props>()

const emit = defineEmits<{
  sort: [field: string, order: 'asc' | 'desc']
  filter: [condition: string]
}>()

let currentSortOrder: 'asc' | 'desc' | null = null

function handleSort(field: string) {
  if (props.sortField === field) {
    currentSortOrder = currentSortOrder === 'asc' ? 'desc' : 'asc'
  } else {
    currentSortOrder = 'asc'
  }

  emit('sort', field, currentSortOrder)
}

function formatValue(value: unknown): string {
  if (value === null || value === undefined) {
    return 'NULL'
  }

  if (typeof value === 'object') {
    return JSON.stringify(value)
  }

  return String(value)
}
</script>

<style scoped>
.preview-table {
  flex: 1;
  overflow: auto;
  position: relative;
}

.table-wrapper {
  width: 100%;
  overflow: auto;
}

table {
  width: 100%;
  border-collapse: collapse;
  font-size: 13px;
}

thead {
  position: sticky;
  top: 0;
  z-index: 1;
  background: var(--bg-tertiary);
}

th {
  padding: 8px 12px;
  text-align: left;
  font-weight: 500;
  color: var(--text-primary);
  border-bottom: 2px solid var(--border-color);
  white-space: nowrap;
  user-select: none;
}

th.sortable {
  cursor: pointer;
}

th.sortable:hover {
  background: var(--bg-hover);
}

.sort-icon {
  margin-left: 4px;
  color: var(--primary-color);
}

td {
  padding: 6px 12px;
  border-bottom: 1px solid var(--border-color);
  color: var(--text-secondary);
  max-width: 300px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

tr:hover td {
  background: var(--bg-hover);
}

.row-number {
  width: 50px;
  text-align: center;
  color: var(--text-tertiary);
  font-size: 11px;
}

.empty-state {
  position: absolute;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  color: var(--text-tertiary);
}
</style>
