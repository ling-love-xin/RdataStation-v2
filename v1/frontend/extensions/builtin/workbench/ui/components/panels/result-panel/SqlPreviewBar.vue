<template>
  <div class="sql-preview-bar">
    <span class="label">SQL:</span>
    <code class="sql-text">{{ sql }}</code>
    <NButton size="tiny" text @click="handleCopy">
      <template #icon><Copy :size="12" /></template>
    </NButton>
  </div>
</template>

<script setup lang="ts">
import { Copy } from 'lucide-vue-next'
import { NButton } from 'naive-ui'

defineProps<{ sql: string }>()

const emit = defineEmits<{ copy: [] }>()

function handleCopy() {
  emit('copy')
}
</script>

<style scoped>
.sql-preview-bar {
  display: flex;
  align-items: center;
  height: 26px;
  padding: 0 8px;
  background: var(--bg-tertiary, #2d2d30);
  border-bottom: 1px solid var(--border-color, #333);
  gap: 6px;
  font-size: 12px;
  overflow: hidden;
  flex-shrink: 0;
}
.label {
  color: var(--text-tertiary, #888);
  white-space: nowrap;
}
.sql-text {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-primary, #ccc);
  font-family: var(--font-mono);
  font-size: 11px;
}
</style>
