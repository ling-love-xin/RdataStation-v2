<template>
  <div class="result-diff-viewer">
    <div class="diff-config">
      <div class="config-row">
        <div class="config-item">
          <span class="config-label">对比表 A</span>
          <NSelect
            v-model:value="selectedTabAId"
            :options="tabOptions"
            placeholder="选择结果集 A"
            filterable
            clearable
          />
        </div>
        <Swap :size="18" class="swap-icon" />
        <div class="config-item">
          <span class="config-label">对比表 B</span>
          <NSelect
            v-model:value="selectedTabBId"
            :options="tabOptions"
            placeholder="选择结果集 B"
            filterable
            clearable
          />
        </div>
      </div>
      <div class="config-row">
        <div class="config-item key-select">
          <span class="config-label">比对键列（多选）</span>
          <NSelect
            v-model:value="keyColumns"
            :options="commonColumnOptions"
            placeholder="自动选前2列作为键"
            multiple
            filterable
            clearable
          />
        </div>
      </div>
    </div>

    <div v-if="diffResult" class="diff-content">
      <div class="diff-summary">
        <NTag type="info" :bordered="false"> 列: {{ diffResult.summary.totalColumns }} </NTag>
        <NTag type="default" :bordered="false"> 共同: {{ diffResult.summary.commonColumns }} </NTag>
        <NTag v-if="diffResult.summary.onlyAColumns" type="warning" :bordered="false">
          A独有: {{ diffResult.summary.onlyAColumns }}
        </NTag>
        <NTag v-if="diffResult.summary.onlyBColumns" type="warning" :bordered="false">
          B独有: {{ diffResult.summary.onlyBColumns }}
        </NTag>
      </div>

      <div class="diff-summary row-summary">
        <NTag type="success" :bordered="false"> 未变: {{ diffResult.summary.unchangedRows }} </NTag>
        <NTag v-if="diffResult.summary.removedRows" type="error" :bordered="false">
          删除: {{ diffResult.summary.removedRows }}
        </NTag>
        <NTag v-if="diffResult.summary.addedRows" type="info" :bordered="false">
          新增: {{ diffResult.summary.addedRows }}
        </NTag>
        <NTag v-if="diffResult.summary.modifiedRows" type="warning" :bordered="false">
          修改: {{ diffResult.summary.modifiedRows }}
        </NTag>
      </div>

      <div class="column-diff-section">
        <h4 class="section-title">列差异</h4>
        <div class="column-diff-list">
          <div
            v-for="col in diffResult.columns"
            :key="col.name"
            :class="['column-diff-item', colClass(col)]"
          >
            <span class="col-name">{{ col.name }}</span>
            <NTag :type="colTagType(col)" size="small" :bordered="false">
              {{ colLabel(col) }}
            </NTag>
          </div>
        </div>
      </div>

      <div class="row-diff-section">
        <h4 class="section-title">
          行差异
          <span class="row-count">（显示前 200 条）</span>
        </h4>
        <div class="diff-table-wrapper">
          <table class="diff-table">
            <thead>
              <tr>
                <th class="status-col">状态</th>
                <th class="key-col">Key</th>
                <th class="data-col">{{ diffResult.tabAName }}</th>
                <th class="data-col">{{ diffResult.tabBName }}</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="(row, idx) in displayRows" :key="idx" :class="rowDiffClass(row.status)">
                <td class="status-col">
                  <NTag :type="statusTagType(row.status)" size="tiny" :bordered="false">
                    {{ statusLabel(row.status) }}
                  </NTag>
                </td>
                <td class="key-col">{{ row.key }}</td>
                <td class="data-col">
                  <div v-if="row.rowA" class="row-preview">
                    {{ rowPreview(row.rowA) }}
                  </div>
                  <span v-else class="null-marker">—</span>
                </td>
                <td class="data-col">
                  <div v-if="row.rowB" class="row-preview">
                    {{ rowPreview(row.rowB) }}
                  </div>
                  <span v-else class="null-marker">—</span>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    </div>

    <div v-else class="diff-empty">
      <Diff :size="40" class="empty-icon" />
      <span>{{ emptyMessage }}</span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Diff } from 'lucide-vue-next'
import { NSelect, NTag } from 'naive-ui'
import { ref, computed, toRef } from 'vue'

import { useResultStore } from '@/extensions/builtin/workbench/ui/stores/result-store'

import { useResultDiff } from '../../../composables/useResultDiff'

import type { ColumnDiff, RowDiff } from '../../../composables/useResultDiff'
import type { SelectOption } from 'naive-ui'

const resultStore = useResultStore()

const selectedTabAId = ref<string | null>(null)
const selectedTabBId = ref<string | null>(null)
const keyColumns = ref<string[]>([])

const tabOptions = computed<SelectOption[]>(() =>
  resultStore.tabs.map(t => ({
    label: `${t.title} (${t.displayedRowCount} 行)`,
    value: t.id,
    disabled: t.columns.length === 0,
  }))
)

const tabA = computed(() =>
  selectedTabAId.value ? (resultStore.tabs.find(t => t.id === selectedTabAId.value) ?? null) : null
)

const tabB = computed(() =>
  selectedTabBId.value ? (resultStore.tabs.find(t => t.id === selectedTabBId.value) ?? null) : null
)

const commonColumnOptions = computed<SelectOption[]>(() => {
  if (!tabA.value || !tabB.value) return []
  const bColSet = new Set(tabB.value.columns)
  return tabA.value.columns.filter(c => bColSet.has(c)).map(c => ({ label: c, value: c }))
})

const diffResult = useResultDiff(
  toRef(tabA),
  toRef(tabB),
  computed(() => keyColumns.value)
)

const emptyMessage = computed(() => {
  if (!selectedTabAId.value || !selectedTabBId.value) return '选择两个结果集以对比差异'
  if (!tabA.value || !tabB.value) return '结果集数据不可用'
  if (tabA.value.columns.length === 0 && tabB.value.columns.length === 0) return '两个结果集均为空'
  return '结果集数据不可用'
})

const displayRows = computed(() => diffResult.value?.rows.slice(0, 200) ?? [])

function colClass(col: ColumnDiff): string {
  if (col.inBoth) return 'col-common'
  if (col.onlyInA) return 'col-only-a'
  return 'col-only-b'
}

function colTagType(col: ColumnDiff): 'default' | 'warning' | 'info' {
  if (col.inBoth) return 'default'
  return 'warning'
}

function colLabel(col: ColumnDiff): string {
  if (col.inBoth) return '共同'
  if (col.onlyInA) return '仅 A'
  return '仅 B'
}

function rowDiffClass(status: RowDiff['status']): string {
  return `row-${status}`
}

function statusTagType(
  status: RowDiff['status']
): 'default' | 'success' | 'error' | 'warning' | 'info' {
  switch (status) {
    case 'unchanged':
      return 'default'
    case 'added':
      return 'success'
    case 'removed':
      return 'error'
    case 'modified':
      return 'warning'
    default:
      return 'default'
  }
}

function statusLabel(status: RowDiff['status']): string {
  switch (status) {
    case 'unchanged':
      return '未变'
    case 'added':
      return '新增'
    case 'removed':
      return '删除'
    case 'modified':
      return '修改'
    default:
      return status
  }
}

function rowPreview(row: Record<string, unknown>): string {
  const entries = Object.entries(row).slice(0, 5)
  return entries
    .map(([k, v]) => {
      const val = v === null ? 'NULL' : typeof v === 'object' ? JSON.stringify(v) : String(v)
      return `${k}: ${val.length > 30 ? val.slice(0, 30) + '...' : val}`
    })
    .join(' | ')
}
</script>

<style scoped>
.result-diff-viewer {
  flex: 1;
  overflow: auto;
  padding: 16px;
}

.diff-config {
  margin-bottom: 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.config-row {
  display: flex;
  align-items: flex-end;
  gap: 12px;
}

.config-item {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.config-label {
  font-size: 12px;
  color: var(--text-color-secondary);
}

.swap-icon {
  color: var(--text-color-tertiary);
  margin-bottom: 6px;
  flex-shrink: 0;
}

.key-select {
  max-width: 480px;
}

.diff-summary {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
  margin-bottom: 8px;
}

.row-summary {
  margin-top: 4px;
  margin-bottom: 16px;
}

.column-diff-section,
.row-diff-section {
  margin-top: 16px;
}

.section-title {
  font-size: 14px;
  font-weight: 600;
  margin-bottom: 8px;
  color: var(--text-color);
}

.row-count {
  font-weight: 400;
  font-size: 12px;
  color: var(--text-color-tertiary);
}

.column-diff-list {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.column-diff-item {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 10px;
  border-radius: 6px;
  background: var(--bg-color-card);
  border: 1px solid var(--border-color);
}

.col-common {
  opacity: 0.8;
}

.col-only-a {
  border-color: var(--warning-color);
  background: var(--warning-color-suppl);
}

.col-only-b {
  border-color: var(--warning-color);
  background: var(--warning-color-suppl);
}

.col-name {
  font-size: 13px;
  font-family: monospace;
}

.diff-table-wrapper {
  overflow-x: auto;
  max-height: 480px;
  overflow-y: auto;
  border: 1px solid var(--border-color);
  border-radius: 8px;
}

.diff-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 13px;
}

.diff-table th {
  position: sticky;
  top: 0;
  background: var(--bg-color-table-header);
  padding: 8px 12px;
  text-align: left;
  font-weight: 600;
  border-bottom: 2px solid var(--border-color);
  z-index: 1;
}

.diff-table td {
  padding: 6px 12px;
  border-bottom: 1px solid var(--border-color-subtle);
  vertical-align: top;
}

.status-col {
  width: 64px;
  text-align: center;
}

.key-col {
  width: 160px;
  font-family: monospace;
  font-size: 12px;
  word-break: break-all;
}

.data-col {
  min-width: 200px;
}

.row-preview {
  max-width: 300px;
  font-size: 12px;
  color: var(--text-color-secondary);
  word-break: break-all;
  line-height: 1.5;
}

.null-marker {
  color: var(--text-color-tertiary);
}

.row-added td {
  background: rgba(16, 185, 129, 0.08);
}

.row-removed td {
  background: rgba(239, 68, 68, 0.08);
}

.row-modified td {
  background: rgba(245, 158, 11, 0.08);
}

.diff-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 16px;
  padding: 48px;
  color: var(--text-color-secondary);
}

.empty-icon {
  opacity: 0.3;
}
</style>
