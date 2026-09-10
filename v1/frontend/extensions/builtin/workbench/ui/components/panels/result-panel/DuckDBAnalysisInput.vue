<template>
  <div class="duckdb-analysis-input">
    <div class="filter-row">
      <NInput
        :value="sql"
        size="tiny"
        type="textarea"
        :autosize="{ minRows: 1, maxRows: 3 }"
        :placeholder="t('resultPanel.duckdbPlaceholder')"
        @update:value="emit('update:sql', $event || '')"
        @keydown.enter.ctrl="onExecute"
      />
      <div class="action-btns">
        <NButton size="tiny" type="primary" :loading="loading" @click="onExecute">
          <template #icon><Play :size="12" /></template>
          {{ t('resultPanel.execute') }}
        </NButton>
        <NButton size="tiny" quaternary @click="emit('clear')">
          <template #icon><X :size="12" /></template>
          {{ t('resultPanel.clear') }}
        </NButton>
      </div>
    </div>
    <div class="quick-actions">
      <span class="label">{{ t('resultPanel.quickAnalysis') }}:</span>
      <NButton size="tiny" text @click="emit('quick', 'count')">
        <template #icon><Hash :size="11" /></template>
        {{ t('resultPanel.count') }}
      </NButton>
      <NButton size="tiny" text @click="emit('quick', 'distinct')">
        <template #icon><List :size="11" /></template>
        {{ t('resultPanel.distinct') }}
      </NButton>
      <NButton size="tiny" text @click="emit('quick', 'group')">
        <template #icon><BarChart3 :size="11" /></template>
        {{ t('resultPanel.groupBy') }}
      </NButton>
      <span class="divider" />
      <NButton
        size="tiny"
        text
        :title="t('resultPanel.bridgeFilter')"
        @click="emit('bridgeFilter')"
      >
        <template #icon><Zap :size="11" /></template>
        {{ t('resultPanel.bridgeFilter') }}
      </NButton>
      <span class="hint">{{ t('resultPanel.executeHint') }}</span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Play, X, Hash, List, BarChart3, Zap } from 'lucide-vue-next'
import { NInput, NButton } from 'naive-ui'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

defineProps<{
  sql: string
  loading: boolean
}>()

const emit = defineEmits<{
  'update:sql': [string]
  execute: []
  clear: []
  quick: [type: string]
  bridgeFilter: []
}>()

function onExecute() {
  emit('execute')
}
</script>

<style scoped>
.duckdb-analysis-input {
  padding: 4px 8px;
  border-bottom: 1px solid var(--border-color, #333);
  flex-shrink: 0;
}
.filter-row {
  display: flex;
  gap: 4px;
  align-items: flex-start;
}
.action-btns {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.quick-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 4px;
  font-size: 11px;
  color: var(--text-tertiary, #888);
}
.label {
  white-space: nowrap;
}
.divider {
  width: 1px;
  height: 14px;
  background: var(--border-color, #444);
}
.hint {
  margin-left: auto;
}
</style>
