<template>
  <div class="result-statusbar">
    <div class="status-left">
      <NButton size="tiny" quaternary @click="$emit('refresh')">
        <template #icon><RotateCw :size="12" /></template>
        {{ t('resultPanel.refresh') }}
      </NButton>
      <NButton size="tiny" quaternary :disabled="!hasDirty" @click="$emit('save')">
        <template #icon><Save :size="12" /></template>
        {{ t('resultPanel.save') }}
      </NButton>
      <NButton size="tiny" quaternary :disabled="!hasDirty" @click="$emit('cancel')">
        <template #icon><X :size="12" /></template>
        {{ t('resultPanel.cancel') }}
      </NButton>
      <NButton size="tiny" quaternary @click="$emit('export')">
        <template #icon><Download :size="12" /></template>
        {{ t('resultPanel.export') }}
      </NButton>
    </div>
    <div class="status-center">
      <span class="mode-tag" :class="filterMode">{{ modeLabel }}</span>
      <span class="row-info">{{ rowInfoText }}</span>
    </div>
    <div class="status-right">
      <span v-if="durationText" class="duration">{{ durationText }}</span>
      <span class="timestamp">{{ timestamp }}</span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { RotateCw, Save, X, Download } from 'lucide-vue-next'
import { NButton } from 'naive-ui'
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

import type { FilterMode } from '../../../types/result'

const { t } = useI18n()

const props = defineProps<{
  filterMode: FilterMode
  totalRows: number
  visibleRows: number
  originalRows: number
  hasDirty: boolean
  duration: number
  timestamp: string
}>()

defineEmits<{
  refresh: []
  save: []
  cancel: []
  export: []
}>()

const modeLabel = computed(() => {
  const map: Record<FilterMode, string> = {
    quick: t('resultPanel.instantFilter'),
    sql: t('resultPanel.sqlFilter'),
    duckdb: t('resultPanel.duckdbAnalysis'),
  }
  return map[props.filterMode]
})

const rowInfoText = computed(() => {
  if (props.filterMode === 'duckdb') {
    return `${t('resultPanel.originalRows', { rows: props.originalRows })} | ${t('resultPanel.analysisResult', { rows: props.visibleRows })}`
  }
  if (props.visibleRows !== props.totalRows) {
    return `${t('resultPanel.originalRows', { rows: props.totalRows })} → ${t('resultPanel.filteredRows', { rows: props.visibleRows })}`
  }
  return `${props.totalRows} ${t('resultPanel.rowCount')}`
})

const durationText = computed(() => {
  if (!props.duration) return ''
  const sec = (props.duration / 1000).toFixed(3)
  const map: Record<FilterMode, string> = {
    quick: '',
    sql: t('resultPanel.databaseTime', { sec }),
    duckdb: t('resultPanel.duckdbTime', { sec }),
  }
  return map[props.filterMode]
})
</script>

<style scoped>
.result-statusbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 26px;
  padding: 0 4px;
  background: var(--bg-tertiary, #2d2d30);
  border-top: 1px solid var(--border-color, #3e3e42);
  font-size: 11px;
  color: var(--text-secondary, #858585);
  flex-shrink: 0;
}
.status-left,
.status-center,
.status-right {
  display: flex;
  align-items: center;
  gap: 2px;
}
.mode-tag {
  padding: 0 6px;
  border-radius: 3px;
  font-size: 10px;
  font-weight: 600;
}
.mode-tag.quick {
  background: #2d6a4f33;
  color: #52c41a;
}
.mode-tag.sql {
  background: #1a5a8a33;
  color: #1890ff;
}
.mode-tag.duckdb {
  background: #613a8a33;
  color: #b37feb;
}
.row-info {
  margin-left: 8px;
}
.duration {
  color: var(--primary-color, #0078d4);
}
.timestamp {
  color: var(--text-tertiary, #666);
  font-size: 10px;
  margin-left: 8px;
}
</style>
