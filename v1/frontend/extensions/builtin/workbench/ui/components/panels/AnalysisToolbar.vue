<template>
  <div class="analysis-toolbar">
    <div class="toolbar-group">
      <NTooltip trigger="hover">
        <template #trigger>
          <NButton size="tiny" quaternary :disabled="isExecuting" @click="$emit('execute')">
            <template #icon><Play :size="14" /></template>
            执行
          </NButton>
        </template>
        执行 DuckDB 查询 (Ctrl+Enter)
      </NTooltip>
      <NTooltip trigger="hover">
        <template #trigger>
          <NButton size="tiny" quaternary :disabled="isExecuting" @click="$emit('executeNew')">
            <template #icon><ExternalLink :size="14" /></template>
            新标签
          </NButton>
        </template>
        新标签页执行
      </NTooltip>
    </div>

    <div class="toolbar-separator" />

    <div class="toolbar-group federation-group">
      <span class="toolbar-label">联邦查询:</span>
      <NSelect
        v-model:value="selectedSources"
        :options="availableSources"
        multiple
        size="tiny"
        placeholder="选择数据源..."
        style="width: 200px"
        @update:value="onSourcesChange"
      />
    </div>

    <div class="toolbar-separator" />

    <div class="toolbar-group">
      <NTooltip trigger="hover">
        <template #trigger>
          <NButton size="tiny" quaternary @click="$emit('format')">
            <template #icon><AlignLeft :size="14" /></template>
            格式化
          </NButton>
        </template>
        格式化 SQL
      </NTooltip>
    </div>

    <div class="toolbar-spacer" />

    <div class="toolbar-group">
      <NTooltip trigger="hover">
        <template #trigger>
          <NButton size="tiny" quaternary>
            <template #icon><Clock :size="14" /></template>
            历史
          </NButton>
        </template>
        SQL 历史
      </NTooltip>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Play, ExternalLink, AlignLeft, Clock } from 'lucide-vue-next'
import { NButton, NSelect, NTooltip } from 'naive-ui'
import { ref, onMounted } from 'vue'

defineProps<{
  isExecuting: boolean
}>()

const emit = defineEmits<{
  (e: 'execute'): void
  (e: 'executeNew'): void
  (e: 'format'): void
  (e: 'federationChange', sources: string[]): void
}>()

const selectedSources = ref<string[]>([])
const availableSources = ref<Array<{ label: string; value: string }>>([])

function onSourcesChange(sources: string[]): void {
  selectedSources.value = sources
  emit('federationChange', sources)
}

onMounted(async () => {
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    const result = await invoke<{ connections: Array<{ name: string; conn_id: string }> }>(
      'get_registered_external_databases'
    )
    availableSources.value = (result.connections ?? []).map(c => ({
      label: c.name,
      value: c.conn_id,
    }))
  } catch {
    console.warn('[AnalysisToolbar] failed to load federation sources')
  }
})
</script>

<style scoped>
.analysis-toolbar {
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 4px 8px;
  background: var(--toolbar-bg, #252526);
  border-bottom: 1px solid var(--toolbar-border, #3c3c3c);
  flex-shrink: 0;
  min-height: 32px;
  overflow-x: auto;
}
.toolbar-group {
  display: flex;
  align-items: center;
  gap: 4px;
}
.toolbar-separator {
  width: 1px;
  height: 20px;
  background: var(--toolbar-separator, #555);
  margin: 0 6px;
}
.toolbar-spacer {
  flex: 1;
}
.federation-group {
  gap: 6px;
}
.toolbar-label {
  font-size: 11px;
  color: var(--toolbar-label, #888);
  white-space: nowrap;
}
</style>
