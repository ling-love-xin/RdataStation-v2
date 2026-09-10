<template>
  <teleport to="body">
    <div
      v-if="visible"
      class="result-context-menu"
      :style="{ left: x + 'px', top: y + 'px' }"
      @click.stop="close"
      @contextmenu.prevent="close"
    >
      <div v-if="type === 'cell'" class="menu-group">
        <div class="menu-item" @click.stop="emit('action', { action: 'copyCell', value, column })">
          <Copy :size="12" /><span>{{ t('resultPanel.copyCell') }}</span>
        </div>
        <div class="menu-item" @click.stop="emit('action', { action: 'copyRow', value, column })">
          <ClipboardList :size="12" /><span>{{ t('resultPanel.copyRow') }}</span>
        </div>
        <div
          class="menu-item"
          @click.stop="emit('action', { action: 'copyRowJson', value, column })"
        >
          <FileJson :size="12" /><span>{{ t('resultPanel.copyJson') }}</span>
        </div>
        <div
          class="menu-item"
          @click.stop="emit('action', { action: 'copyRowInsert', value, column })"
        >
          <FileCode :size="12" /><span>{{ t('resultPanel.copyInsert') }}</span>
        </div>
        <div class="menu-divider" />
        <div
          class="menu-item"
          @click.stop="emit('action', { action: 'filterByValue', value, column })"
        >
          <Filter :size="12" /><span>{{ t('resultPanel.filterByValueQuick') }}</span>
        </div>
        <div
          class="menu-item"
          @click.stop="emit('action', { action: 'sqlFilterByValue', value, column })"
        >
          <Database :size="12" /><span>{{ t('resultPanel.filterByValueSql') }}</span>
        </div>
        <div class="menu-divider" />
        <div
          class="menu-item"
          @click.stop="emit('action', { action: 'openColumnInsights', value, column })"
        >
          <BarChart3 :size="12" /><span>{{ t('resultPanel.columnInsight') }}</span>
        </div>
        <div
          class="menu-item"
          @click.stop="emit('action', { action: 'openColumnVisualization', value, column })"
        >
          <PieChart :size="12" /><span>{{ t('resultPanel.openVisualization') }}</span>
        </div>
      </div>

      <div v-if="type === 'header'" class="menu-group">
        <div class="menu-item" @click.stop="emit('action', { action: 'sortAsc', column })">
          <ArrowUp :size="12" /><span>{{ t('resultPanel.sortAsc') }}</span>
        </div>
        <div class="menu-item" @click.stop="emit('action', { action: 'sortDesc', column })">
          <ArrowDown :size="12" /><span>{{ t('resultPanel.sortDesc') }}</span>
        </div>
        <div class="menu-divider" />
        <div
          class="menu-item"
          @click.stop="emit('action', { action: 'sendSortToSql', column, sortDir })"
        >
          <Database :size="12" /><span>{{ t('resultPanel.sendSortToSql') }}</span>
        </div>
        <div
          class="menu-item"
          @click.stop="emit('action', { action: 'sendSortToDuckdb', column, sortDir })"
        >
          <Brain :size="12" /><span>{{ t('resultPanel.sendSortToDuckdb') }}</span>
        </div>
        <div class="menu-divider" />
        <div class="menu-item" @click.stop="emit('action', { action: 'hideColumn', column })">
          <EyeOff :size="12" /><span>{{ t('resultPanel.hideColumn') }}</span>
        </div>
        <div class="menu-item" @click.stop="emit('action', { action: 'autoSizeColumn', column })">
          <Maximize2 :size="12" /><span>{{ t('resultPanel.autoColumnWidth') }}</span>
        </div>
        <div class="menu-item" @click.stop="emit('action', { action: 'autoSizeAll', column })">
          <Maximize2 :size="12" /><span>{{ t('resultPanel.autoAllColumnWidth') }}</span>
        </div>
        <div class="menu-divider" />
        <div class="menu-item" @click.stop="emit('action', { action: 'columnSummary', column })">
          <Calculator :size="12" /><span>{{ t('resultPanel.columnSummary') }}</span>
        </div>
        <div
          class="menu-item"
          @click.stop="emit('action', { action: 'openColumnInsights', column })"
        >
          <BarChart3 :size="12" /><span>{{ t('resultPanel.columnInsightPanel') }}</span>
        </div>
      </div>
    </div>
  </teleport>
</template>

<script setup lang="ts">
import {
  Copy,
  ClipboardList,
  FileJson,
  FileCode,
  Filter,
  Database,
  BarChart3,
  PieChart,
  ArrowUp,
  ArrowDown,
  EyeOff,
  Maximize2,
  Calculator,
  Brain,
} from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

defineProps<{
  visible: boolean
  x: number
  y: number
  type: 'cell' | 'header'
  value?: unknown
  column?: string
  sortDir?: string
}>()

const emit = defineEmits<{
  action: [payload: Record<string, unknown>]
  close: []
}>()

function close() {
  emit('close')
}
</script>

<style scoped>
.result-context-menu {
  position: fixed;
  z-index: 10000;
  min-width: 200px;
  background: var(--bg-primary, #2d2d30);
  border: 1px solid var(--border-color, #3e3e42);
  border-radius: 6px;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.4);
  padding: 4px 0;
  font-size: 12px;
}
.menu-group {
}
.menu-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 12px;
  cursor: pointer;
  color: var(--text-primary, #ccc);
  transition: background 0.1s;
}
.menu-item:hover {
  background: var(--primary-color, #0078d4);
  color: #fff;
}
.menu-divider {
  height: 1px;
  background: var(--border-color, #3e3e42);
  margin: 4px 0;
}
</style>
