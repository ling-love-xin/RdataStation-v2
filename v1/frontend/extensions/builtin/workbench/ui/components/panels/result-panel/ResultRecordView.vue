<template>
  <div class="result-record-view">
    <div v-if="!tab || tab.columns.length === 0" class="empty-state">
      <span>{{ emptyText }}</span>
    </div>
    <div v-else-if="recordRow" class="record-detail">
      <div v-for="col in tab.columns" :key="col" class="record-field">
        <span class="field-name">{{ col }}</span>
        <span :class="['field-value', valueClass(recordRow[col])]">
          {{ formatValue(recordRow[col]) }}
        </span>
      </div>
    </div>
    <div v-else class="empty-state">
      <span>No row selected</span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'

import type { ResultTab } from '@/extensions/builtin/workbench/ui/types/result'

const props = withDefaults(
  defineProps<{
    tab: ResultTab | null
    selectedRowIndex: number
    emptyText?: string
  }>(),
  {
    selectedRowIndex: 0,
    emptyText: 'No data',
  }
)

const recordRow = computed(() => {
  const tab = props.tab
  if (!tab || tab.columns.length === 0 || tab.objectRows.length === 0) return null
  if (props.selectedRowIndex < 0 || props.selectedRowIndex >= tab.objectRows.length) return null
  return tab.objectRows[props.selectedRowIndex]
})

function valueClass(value: unknown): string {
  if (value === null) return 'is-null'
  if (typeof value === 'number') return 'is-number'
  if (typeof value === 'boolean') return 'is-bool'
  return ''
}

function formatValue(value: unknown): string {
  if (value === null) return 'NULL'
  if (typeof value === 'object') return JSON.stringify(value)
  return String(value)
}
</script>

<style scoped>
.result-record-view {
  flex: 1;
  overflow: auto;
  padding: 12px;
}
.empty-state {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: var(--text-color-secondary);
}
.record-detail {
  display: flex;
  flex-direction: column;
}
.record-field {
  display: flex;
  gap: 16px;
  padding: 4px 0;
  border-bottom: 1px solid var(--border-color);
}
.field-name {
  font-weight: 600;
  min-width: 140px;
  color: var(--text-color);
}
.field-value {
  flex: 1;
  word-break: break-all;
}
.is-null {
  color: #999;
  font-style: italic;
}
.is-number {
  font-family: monospace;
}
.is-bool {
  font-family: monospace;
}
</style>
