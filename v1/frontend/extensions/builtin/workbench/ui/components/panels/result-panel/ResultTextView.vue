<template>
  <div class="result-text-view">
    <div v-if="!content" class="empty-state">
      <span>{{ emptyText }}</span>
    </div>
    <pre v-else class="text-content">{{ content }}</pre>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'

import type { ResultTab } from '@/extensions/builtin/workbench/ui/types/result'

const props = withDefaults(
  defineProps<{
    tab: ResultTab | null
    maxRows?: number
    emptyText?: string
  }>(),
  {
    maxRows: 10000,
    emptyText: 'No data to display',
  }
)

const content = computed(() => {
  const tab = props.tab
  if (!tab || tab.columns.length === 0) return ''

  const displayRows =
    props.maxRows && props.maxRows > 0 ? tab.rows.slice(0, props.maxRows) : tab.rows
  const header = tab.columns.join('\t')
  const body = displayRows.map(r => r.map(v => (v === null ? 'NULL' : String(v))).join('\t'))
  return [header, ...body].join('\n')
})
</script>

<style scoped>
.result-text-view {
  flex: 1;
  overflow: auto;
}
.empty-state {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: var(--text-color-secondary);
}
.text-content {
  margin: 0;
  padding: 12px;
  font-family: 'Cascadia Code', 'Fira Code', monospace;
  font-size: 12px;
  line-height: 1.6;
  white-space: pre;
}
</style>
