<template>
  <div class="grid-toolbar">
    <div class="toolbar-group">
      <span class="toolbar-label">行数</span>
      <NTag type="info" size="small">{{ formattedRowCount }}</NTag>
    </div>

    <div class="toolbar-group">
      <span class="toolbar-label">耗时</span>
      <NTag size="small">{{ formattedElapsed }}</NTag>
    </div>

    <div class="toolbar-group">
      <NButton quaternary size="tiny" @click="$emit('export-csv')"> 导出 CSV </NButton>
      <NButton quaternary size="tiny" @click="$emit('export-json')"> 导出 JSON </NButton>
    </div>

    <div class="toolbar-spacer" />

    <div v-if="isDuckdbReady" class="toolbar-group">
      <NTag type="success" size="small">⚡ DuckDB 已就绪</NTag>
    </div>

    <div class="toolbar-group">
      <NButton quaternary size="tiny" @click="$emit('close')"> 关闭 </NButton>
    </div>
  </div>
</template>

<script setup lang="ts">
import { NButton, NTag } from 'naive-ui'
import { computed } from 'vue'

const props = defineProps<{
  rowCount?: number
  elapsedMs?: number
  isDuckdbReady?: boolean
}>()

defineEmits<{
  close: []
  'export-csv': []
  'export-json': []
}>()

const formattedRowCount = computed(() => {
  const count = props.rowCount ?? 0
  if (count >= 1_000_000) return `${(count / 1_000_000).toFixed(1)}M`
  if (count >= 1_000) return `${(count / 1_000).toFixed(1)}K`
  return String(count)
})

const formattedElapsed = computed(() => {
  const ms = props.elapsedMs ?? 0
  if (ms < 1000) return `${ms}ms`
  return `${(ms / 1000).toFixed(1)}s`
})
</script>

<style scoped>
.grid-toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 4px 8px;
  background: var(--n-color-embedded);
  border-bottom: 1px solid var(--n-border-color);
  flex-shrink: 0;
  min-height: 28px;
}

.toolbar-group {
  display: flex;
  align-items: center;
  gap: 4px;
}

.toolbar-label {
  font-size: 12px;
  color: var(--n-text-color-3);
}

.toolbar-spacer {
  flex: 1;
}
</style>
