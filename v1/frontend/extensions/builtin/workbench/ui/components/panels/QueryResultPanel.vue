<template>
  <div class="query-result-panel">
    <!-- 顶部标签栏 -->
    <div v-if="tabs.length > 0" class="result-tabs">
      <div
        v-for="tabItem in tabs"
        :key="tabItem.id"
        :class="['result-tab', { active: tabItem.id === activeTabId }]"
        @click="switchTab(tabItem.id)"
      >
        <span class="tab-title">{{ tabItem.title }}</span>
        <span class="tab-close" @click.stop="closeTab(tabItem.id)">&times;</span>
      </div>
    </div>

    <!-- 主内容区 -->
    <template v-if="activeTab">
      <!-- SQL 预览 + 模式切换条 -->
      <div class="toolbar-strip">
        <FilterModeSwitcher v-model="tab.filterMode" class="mode-switcher-inline" />
        <FilterPresetSelector
          :filter-mode="tab.filterMode"
          :current-expression="getCurrentExpression(tab)"
          @select="(e: PresetSelectEvent) => applyPreset(tab, e)"
          @save="
            (name: string, expr: string, mode: FilterMode) =>
              saveFilterPreset(tab, name, expr, mode)
          "
        />
        <div class="strip-right">
          <QuickFilterInput
            v-if="tab.filterMode === 'quick'"
            :expression="tab.quickFilterExpression"
            :visible-count="tab.filteredRowCount"
            :total-count="tab.originalRowCount"
            @update:expression="(v: string) => (tab.quickFilterExpression = v)"
            @apply="(v: string) => applyQuickFilter(tab, v)"
            @clear="() => clearQuickFilter(tab)"
          />
          <SqlFilterInput
            v-if="tab.filterMode === 'sql'"
            :expression="tab.sqlFilterExpression"
            :loading="tab.isSqlFilterLoading"
            @update:expression="(v: string) => (tab.sqlFilterExpression = v)"
            @execute="() => executeSqlFilter(tab)"
          />
          <DuckDBAnalysisInput
            v-if="tab.filterMode === 'duckdb'"
            :sql="tab.duckdbSql"
            :loading="tab.isDuckdbLoading"
            @update:sql="(v: string) => (tab.duckdbSql = v)"
            @execute="() => executeDuckdbAnalysis(tab)"
            @clear="() => clearDuckdbAnalysis(tab)"
            @quick="(t: string) => quickDuckdbAction(tab, t)"
            @bridge-filter="() => handleBridgeFilter(tab)"
          />
        </div>
      </div>

      <!-- DBeaver 风格主体布局：左侧栏 + 内容区 + 右侧查看器 -->
      <div class="result-body">
        <!-- 左侧视图切换栏 -->
        <div class="view-sidebar">
          <button
            v-for="v in viewModes"
            :key="v.key"
            :class="['view-btn', { active: currentView === v.key }]"
            :title="v.label"
            @click="switchView(v.key)"
          >
            <component :is="v.icon" :size="18" />
          </button>
        </div>

        <!-- 中间表格区 -->
        <div ref="gridContainerRef" class="grid-area" @contextmenu.prevent="handleGridContextMenu">
          <ResultGridView
            :tab="tab"
            :column-defs="columnDefs"
            :row-data="rowData"
            :default-col-def="defaultColDef"
            :pagination="pagination"
            :pagination-page-size="paginationPageSize"
            :pagination-page-selector="paginationPageSelector"
            :is-dark="uiStore.isDark"
            :loading="tab.isLoading"
            :empty-text="t('workbench.executeSqlToSeeResults')"
            @grid-ready="onGridReady"
            @cell-context-menu="onCellContextMenu"
            @row-clicked="onRowClicked"
            @selection-changed="onSelectionChanged"
            @cell-value-changed="onCellValueChanged"
            @row-data-updated="onRowDataUpdated"
            @sort-changed="onSortChanged"
            @pagination-changed="onPaginationChanged"
            @keydown="handleKeyDown"
          />
          <div v-if="currentView === 'chart'" class="chart-fill">
            <DataVisualizationPanel
              :data="rowData as Record<string, unknown>[]"
              :columns="tab.columns"
            />
          </div>
          <ResultTextView
            v-if="currentView === 'text'"
            :tab="tab"
            :max-rows="10000"
            :empty-text="t('workbench.executeSqlToSeeResults')"
          />
          <div v-if="currentView === 'record'" class="record-view">
            <div class="record-nav">
              <NButton
                size="tiny"
                quaternary
                :disabled="selectedRecordIndex <= 0"
                @click="prevRecord"
              >
                <ChevronLeft :size="14" />
              </NButton>
              <span class="record-nav-text"
                >{{ selectedRecordIndex + 1 }} / {{ rowData.length }}</span
              >
              <NButton
                size="tiny"
                quaternary
                :disabled="selectedRecordIndex >= rowData.length - 1"
                @click="nextRecord"
              >
                <ChevronRight :size="14" />
              </NButton>
            </div>
            <ResultRecordView
              :tab="tab"
              :selected-row-index="selectedRecordIndex"
              :empty-text="t('workbench.executeSqlToSeeResults')"
            />
          </div>
        </div>

        <!-- 右侧数值查看器 -->
        <div v-if="showValueViewer" class="value-viewer">
          <div class="viewer-header">
            <span class="viewer-title">{{ t('workbench.valueViewer') }}</span>
            <NButton size="tiny" quaternary @click="showValueViewer = false">
              <X :size="12" />
            </NButton>
          </div>
          <div class="viewer-content">
            <div class="viewer-field">
              <span class="field-label">{{ t('workbench.columnLabel') }}</span>
              <span class="field-val">{{ selectedCell?.column || '-' }}</span>
            </div>
            <div class="viewer-field">
              <span class="field-label">{{ t('workbench.rowLabel') }}</span>
              <span class="field-val">{{
                selectedCell?.row != null ? selectedCell.row + 1 : '-'
              }}</span>
            </div>
            <textarea
              class="viewer-text"
              :value="selectedCell?.value != null ? String(selectedCell.value) : ''"
              readonly
              rows="8"
            />
          </div>
        </div>
        <NButton
          v-if="!showValueViewer && activeTab"
          size="tiny"
          quaternary
          class="viewer-toggle"
          :title="t('workbench.openValueViewer')"
          @click="showValueViewer = true"
        >
          <PanelRight :size="14" />
        </NButton>
      </div>

      <!-- 底部状态栏（操作 + 行信息 + 分页） -->
      <div v-if="activeTab" class="result-statusbar">
        <div class="sbar-left">
          <span :class="['mode-badge', tab.filterMode]">{{ modeLabel(tab) }}</span>
          <NButton
            size="tiny"
            quaternary
            :title="t('resultPanel.refresh')"
            @click="handleRefresh(tab)"
          >
            <RotateCw :size="11" />
          </NButton>
          <NButton
            size="tiny"
            quaternary
            :disabled="!tabHasDirty(tab)"
            :title="t('resultPanel.save')"
            @click="handleSave(tab)"
          >
            <Save :size="11" />
          </NButton>
          <NButton
            size="tiny"
            quaternary
            :disabled="!tabHasDirty(tab)"
            :title="t('resultPanel.cancel')"
            @click="handleCancel(tab)"
          >
            <X :size="11" />
          </NButton>
          <NButton size="tiny" quaternary title="对比结果集" @click="showDiffModal = true">
            <GitCompare :size="14" />
          </NButton>
          <NDropdown
            trigger="hover"
            :options="exportMenuOptions"
            @select="(k: string) => handleExport(k)"
          >
            <NButton size="tiny" quaternary :title="t('resultPanel.export')">
              <Download :size="11" />
            </NButton>
          </NDropdown>
        </div>
        <div class="sbar-center">
          <span class="row-info">{{ displayRowText }}</span>
          <span v-if="tab.executionTime" class="exec-time"
            >{{ (tab.executionTime / 1000).toFixed(3) }}s</span
          >
        </div>
        <div class="sbar-right">
          <NButton
            size="tiny"
            quaternary
            :disabled="!gridApi"
            :title="t('workbench.firstPage')"
            @click="firstPage"
          >
            <SkipBack :size="11" />
          </NButton>
          <NButton
            size="tiny"
            quaternary
            :disabled="!gridApi"
            :title="t('workbench.prevPage')"
            @click="prevPage"
          >
            <ChevronLeft :size="11" />
          </NButton>
          <span v-if="gridApi" class="page-indicator">{{ pageInfoText }}</span>
          <NButton
            size="tiny"
            quaternary
            :disabled="!gridApi"
            :title="t('workbench.nextPage')"
            @click="nextPage"
          >
            <ChevronRight :size="11" />
          </NButton>
          <NButton
            size="tiny"
            quaternary
            :disabled="!gridApi"
            :title="t('workbench.lastPage')"
            @click="lastPage"
          >
            <SkipForward :size="11" />
          </NButton>
          <NInput
            v-if="gridApi && gridApi.paginationGetTotalPages() > 1"
            :value="goPageInput"
            size="tiny"
            class="go-page-input"
            :placeholder="t('workbench.goPage')"
            @update:value="goPageInput = $event"
            @keyup.enter="goToPage"
          />
          <NButton
            size="tiny"
            quaternary
            :title="
              paginationEnabled ? t('workbench.disablePagination') : t('workbench.enablePagination')
            "
            @click="paginationEnabled = !paginationEnabled"
          >
            <Layers :size="11" :style="{ opacity: paginationEnabled ? 1 : 0.4 }" />
          </NButton>
        </div>
      </div>
    </template>

    <ResultContextMenu
      :visible="contextMenu.visible"
      :x="contextMenu.x"
      :y="contextMenu.y"
      :type="contextMenu.type"
      :value="contextMenu.value"
      :column="contextMenu.column"
      :sort-dir="contextMenu.sortDir"
      @action="handleContextAction"
      @close="closeContextMenu"
    />

    <NModal
      v-model:show="showDiffModal"
      preset="dialog"
      title="结果集对比"
      :show-icon="false"
      style="width: 900px; max-height: 80vh"
      :mask-closable="true"
    >
      <ResultDiffViewer />
    </NModal>
  </div>
</template>

<script setup lang="ts">
import { ClientSideRowModelModule, ModuleRegistry } from 'ag-grid-community'
import 'ag-grid-community/styles/ag-grid.css'
import 'ag-grid-community/styles/ag-theme-alpine.css'
import {
  Database,
  RotateCw,
  Save,
  X,
  Download,
  PanelRight,
  ChevronLeft,
  ChevronRight,
  SkipBack,
  SkipForward,
  AlignLeft,
  List,
  GitCompare,
  BarChart3,
  Layers,
} from 'lucide-vue-next'
import {
  createDiscreteApi,
  darkTheme,
  lightTheme,
  NButton,
  NDropdown,
  NInput,
  NModal,
} from 'naive-ui'
import { computed, ref, onMounted, onUnmounted, watch, type ComputedRef } from 'vue'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

import { useInsightStore } from '@/extensions/builtin/workbench/ui/stores/insight-store'
import { useResultStore } from '@/extensions/builtin/workbench/ui/stores/result-store'
import { useSqlExecutionStore } from '@/extensions/builtin/workbench/ui/stores/sql-execution-store'
import type {
  ResultTab,
  ViewMode,
  FilterMode,
} from '@/extensions/builtin/workbench/ui/types/result'
import { useUiStore } from '@/shared/stores/ui'
import { copyToClipboard } from '@/shared/utils/clipboard'

import DataVisualizationPanel from './DataVisualizationPanel.vue'
import DuckDBAnalysisInput from './result-panel/DuckDBAnalysisInput.vue'
import FilterModeSwitcher from './result-panel/FilterModeSwitcher.vue'
import FilterPresetSelector from './result-panel/FilterPresetSelector.vue'
import QuickFilterInput from './result-panel/QuickFilterInput.vue'
import ResultContextMenu from './result-panel/ResultContextMenu.vue'
import ResultDiffViewer from './result-panel/ResultDiffViewer.vue'
import SqlFilterInput from './result-panel/SqlFilterInput.vue'
import { useGridConfig, isLikelyNumeric } from '../../composables/useGridConfig'
import { useResultExport } from '../../composables/useResultExport'
import { useResultFilterPresets } from '../../composables/useResultFilterPresets'
import { useResultFilters } from '../../composables/useResultFilters'
import { saveCellUpdate as apiSaveCellUpdate } from '../../services/result-analysis'

interface PresetSelectEvent {
  id: string
  name: string
  filterMode: FilterMode
  expression: string
}
import type {
  RowDataUpdatedEvent,
  RowClickedEvent,
  CellContextMenuEvent,
  CellValueChangedEvent,
} from 'ag-grid-community'

ModuleRegistry.registerModules([ClientSideRowModelModule])

// ─── Store ───────────────────────────────────────────────
const uiStore = useUiStore()
const resultStore = useResultStore()
const insightStore = useInsightStore()
const configProviderPropsRef = ref({ theme: uiStore.isDark ? darkTheme : lightTheme })
const { message } = createDiscreteApi(['message'], { configProviderProps: configProviderPropsRef })
watch(
  () => uiStore.isDark,
  v => {
    configProviderPropsRef.value = { theme: v ? darkTheme : lightTheme }
  }
)

// ─── 多标签状态（从 store 读取）──────────────────────────
const tabs = computed(() => resultStore.tabs)
const activeTabId = computed(() => resultStore.activeTabId)
const activeTab = computed<ResultTab | null>(() => {
  const id = resultStore.activeTabId
  if (!id) return null
  return resultStore.tabs.find(t => t.id === id) ?? null
})
// 设计说明：模板第 17 行有 v-if="activeTab" 防护，此处的 throw 是安全网
const tab = computed<ResultTab>(() => {
  const t = activeTab.value
  if (!t) throw new Error('tab accessed when no active tab')
  return t
})

// ─── AG Grid ─────────────────────────────────────────────
const {
  columnDefs,
  defaultColDef,
  pagination,
  paginationEnabled,
  paginationPageSelector,
  paginationPageSize,
  rowData,
  gridApi,
  onGridReady,
  savePageSize,
} = useGridConfig({ activeTab: activeTab as ComputedRef<ResultTab | null>, editable: true })
const showDiffModal = ref(false)
const gridContainerRef = ref<HTMLElement | null>(null)
const selectedRows = ref<unknown[]>([])
const goPageInput = ref('')

// ─── 过滤操作（委托到 useResultFilters）────────────────
const {
  applyQuickFilter,
  clearQuickFilter,
  executeSqlFilter,
  executeDuckdbAnalysis,
  clearDuckdbAnalysis,
  quickDuckdbAction,
  handleBridgeFilter,
  modeLabel: filterModeLabel,
} = useResultFilters(gridApi, message, t)

// ─── 导出操作（委托到 useResultExport）─────────────────
const { handleExport: doExport, copyRowsAsInsert } = useResultExport(
  activeTab as ComputedRef<ResultTab | null>,
  gridApi,
  rowData,
  message
)

function modeLabel(tab: ResultTab): string {
  return filterModeLabel[tab.filterMode] ?? tab.filterMode
}

function goToPage(): void {
  if (!gridApi.value) return
  const page = parseInt(goPageInput.value, 10)
  const total = gridApi.value.paginationGetTotalPages()
  if (isNaN(page) || page < 1 || page > total) return
  gridApi.value.paginationGoToPage(page - 1)
  goPageInput.value = ''
}

function onPaginationChanged(): void {
  savePageSize()
  goPageInput.value = ''
}

const contextMenu = ref({
  visible: false,
  x: 0,
  y: 0,
  type: 'cell' as 'cell' | 'header',
  value: null as unknown,
  column: '',
  sortDir: '',
})

interface DirtyCell {
  rowIndex: number
  colId: string
  oldValue: unknown
  newValue: unknown
}
const dirtyCells = ref<Map<string, DirtyCell>>(new Map())

function dirtyKey(rowIndex: number, colId: string): string {
  return `${rowIndex}:${colId}`
}

// ─── 过滤预设 ───────────────────────────────────────
const { addPreset } = useResultFilterPresets()

function getCurrentExpression(tab: ResultTab): string {
  switch (tab.filterMode) {
    case 'quick':
      return tab.quickFilterExpression
    case 'sql':
      return tab.sqlFilterExpression ?? ''
    default:
      return ''
  }
}

function applyPreset(tab: ResultTab, event: PresetSelectEvent): void {
  tab.filterMode = event.filterMode
  switch (tab.filterMode) {
    case 'quick':
      tab.quickFilterExpression = event.expression
      applyQuickFilter(tab, event.expression)
      break
    case 'sql':
      tab.sqlFilterExpression = event.expression
      executeSqlFilter(tab)
      break
  }
}

function saveFilterPreset(tab: ResultTab, name: string, expr: string, mode: FilterMode): void {
  addPreset(name, mode, expr)
  message.success('预设已保存')
}

// ─── Grid / Text / Record 视图切换 ──────────────────
const currentView = ref<ViewMode>('grid')
const showValueViewer = ref(false)
const selectedRecordIndex = ref(0)
interface SelectedCell {
  column: string
  row: number
  value: unknown
}

interface AGGridSortAPI {
  applySortState(s: Array<{ colId: string; sort: string }>): void
}

const selectedCell = ref<SelectedCell | null>(null)

const viewModes = [
  { key: 'grid' as ViewMode, icon: Database, label: t('workbench.gridView') },
  { key: 'text' as ViewMode, icon: AlignLeft, label: t('workbench.textView') },
  { key: 'record' as ViewMode, icon: List, label: t('workbench.recordView') },
  { key: 'chart' as ViewMode, icon: BarChart3, label: t('workbench.chartView') },
]

function switchView(mode: ViewMode) {
  currentView.value = mode
  if (mode === 'record' && selectedRecordIndex.value >= rowData.value.length) {
    selectedRecordIndex.value = 0
  }
}

function prevRecord() {
  if (selectedRecordIndex.value > 0) selectedRecordIndex.value--
}
function nextRecord() {
  if (selectedRecordIndex.value < rowData.value.length - 1) selectedRecordIndex.value++
}

function firstPage() {
  gridApi.value?.paginationGoToFirstPage()
}
function prevPage() {
  gridApi.value?.paginationGoToPreviousPage()
}
function nextPage() {
  gridApi.value?.paginationGoToNextPage()
}
function lastPage() {
  gridApi.value?.paginationGoToLastPage()
}

const displayRowText = computed(() => {
  if (!activeTab.value) return ''
  if (
    activeTab.value.filterMode === 'quick' &&
    activeTab.value.filteredRowCount !== activeTab.value.originalRowCount
  ) {
    return `${activeTab.value.originalRowCount} → ${activeTab.value.filteredRowCount} ${t('resultPanel.rows')}`
  }
  return `${activeTab.value.displayedRowCount} ${t('resultPanel.rows')}`
})

// ─── 导出菜单 ────────────────────────────────────────────
const exportMenuOptions = computed(() => [
  { key: 'csv', label: t('workbench.exportCsv') },
  { key: 'json', label: t('workbench.exportJson') },
  { key: 'insert', label: t('workbench.exportInsert') },
  { key: 'parquet', label: t('workbench.exportParquet') },
  { key: 'xlsx', label: t('workbench.exportXlsx') },
])

const pageInfoText = computed(() => {
  if (!gridApi.value || !gridApi.value.paginationGetCurrentPage) return ''
  const total = gridApi.value.paginationGetTotalPages()
  const current = gridApi.value.paginationGetCurrentPage() + 1
  return `${current}/${total} ${t('resultPanel.page')}`
})

// ─── 标签管理（委托到 store）───────────────────────────
function tabHasDirty(tab: ResultTab | null): boolean {
  return tab ? tab.dirtyRows.size > 0 : false
}

function switchTab(id: string) {
  resultStore.switchTab('', id)
}

function closeTab(id: string) {
  resultStore.closeTab('', id)
}

// ─── 事件处理（使用 Pinia Store）──────────────────

const sqlExecutionStore = useSqlExecutionStore()

const handleResultUpdate = () => {
  const result = sqlExecutionStore.latestResult
  if (!result || !result.result) return

  const qr = result.result
  const columns = qr.columns || []
  const rows: unknown[][] = qr.rows || []
  const elapsedMs = qr.executionTime || 0
  const panelId = result.panelId || ''

  const existingTab = panelId
    ? resultStore.tabs.find(t => t.id === panelId && t.columns.length === 0)
    : null

  if (existingTab) {
    resultStore.setTabResult('', existingTab.id, {
      columns,
      rows,
      rowCount: rows.length,
      elapsedMs,
    })
  } else {
    const tab = resultStore.addTab('', '', '')
    resultStore.setTabResult('', tab.id, {
      columns,
      rows,
      rowCount: rows.length,
      elapsedMs,
    })
  }
}

const handleResultNew = () => {
  const latest = sqlExecutionStore.consumeNewTabRequest()
  if (!latest || !latest.result) return

  const qr = latest.result
  const columns = qr.columns || []
  const rows: unknown[][] = qr.rows || []
  const elapsedMs = qr.executionTime || 0
  const _panelId = latest.panelId || ''

  const tab = resultStore.addTab('', '', '')
  if (latest.title) {
    tab.title = latest.title
  }
  resultStore.setTabResult('', tab.id, {
    columns,
    rows,
    rowCount: rows.length,
    elapsedMs,
  })
}

watch(
  () => sqlExecutionStore.resultVersion,
  () => {
    handleResultUpdate()
  }
)

watch(
  () => sqlExecutionStore.newTabRequests.size,
  size => {
    if (size > 0) {
      handleResultNew()
    }
  }
)

onMounted(() => {
  window.addEventListener('keydown', handleGlobalKeyDown)
})
onUnmounted(() => {
  window.removeEventListener('keydown', handleGlobalKeyDown)
})

// ─── DuckDB 临时表（委托到 store）──────────────────────
async function _ensureDuckdbTempTable(tabId: string) {
  await resultStore.ensureDuckdbTable('', tabId)
}

function onRowDataUpdated(params: RowDataUpdatedEvent) {
  if (params.api?.getDisplayedRowCount() > 0) params.api.sizeColumnsToFit()
}
function onSelectionChanged() {
  if (!gridApi.value) return
  selectedRows.value = gridApi.value.getSelectedRows()
}

/** 双击或单击行 → 切换到记录模式查看单行详情 */
function onRowClicked(event: RowClickedEvent) {
  selectedRecordIndex.value = event?.rowIndex ?? 0
  currentView.value = 'record'
}
function onSortChanged() {
  /* optional */
}
function onCellValueChanged(event: CellValueChangedEvent) {
  if (!activeTab.value) return
  const { colDef, oldValue, newValue } = event
  const rowIndex = event.rowIndex ?? 0
  const colId = colDef.field ?? ''
  const key = dirtyKey(rowIndex, colId)
  if (!dirtyCells.value.has(key)) {
    dirtyCells.value.set(key, { rowIndex, colId, oldValue, newValue })
  } else {
    const existing = dirtyCells.value.get(key)
    if (!existing) return
    if (existing.oldValue === newValue) {
      dirtyCells.value.delete(key)
    } else {
      dirtyCells.value.set(key, { ...existing, newValue })
    }
  }
}

function onCellContextMenu(params: CellContextMenuEvent) {
  closeContextMenu()
  setTimeout(() => {
    const e = window.event as MouseEvent
    contextMenu.value = {
      visible: true,
      x: e.clientX,
      y: e.clientY,
      type: 'cell',
      value: params.value,
      column: params.colDef.field ?? '',
      sortDir: '',
    }
  }, 10)
}

function handleGridContextMenu(event: MouseEvent) {
  const target = event.target as HTMLElement
  if (target.closest('.ag-header-cell')) {
    const colId = target.closest('.ag-header-cell')?.getAttribute('col-id') || ''
    const sortModel = (gridApi.value as Record<string, unknown>)?.getSortModel as
      | (() => Array<{ colId: string; sort: string }>)
      | undefined
    const sortEntry = sortModel?.().find(s => s.colId === colId)
    closeContextMenu()
    setTimeout(() => {
      contextMenu.value = {
        visible: true,
        x: event.clientX,
        y: event.clientY,
        type: 'header',
        value: null,
        column: colId,
        sortDir: sortEntry?.sort || '',
      }
    }, 10)
  }
}
function closeContextMenu() {
  contextMenu.value.visible = false
}

// ─── 模式1: 即时过滤（委托到 useResultFilters）─────────

// ─── 模式2: SQL 过滤（委托到 useResultFilters）─────────

// ─── 模式3: DuckDB 分析（委托到 useResultFilters）─────

// ─── 桥接模式（委托到 useResultFilters）────────────────

// ─── 操作 ───────────────────────────────────────────────
function _handleCopySql() {
  if (activeTab.value) copyToClipboard(activeTab.value.originalSql)
}
function handleRefresh(tab: ResultTab) {
  sqlExecutionStore.requestRefresh(tab.id)
}
async function handleSave(tab: ResultTab) {
  if (dirtyCells.value.size === 0) return

  const updates = Array.from(dirtyCells.value.values()).map(async cell => {
    try {
      const result = await apiSaveCellUpdate({
        conn_id: tab.connectionId,
        table_name: tab.tableName,
        column_name: cell.colId,
        new_value: cell.newValue,
        row_identity: buildRowIdentity(tab, cell.rowIndex, cell.colId),
      })
      if (result.success) {
        const row = tab.objectRows[cell.rowIndex] as Record<string, unknown>
        row[cell.colId.replace(/\./g, '_')] = cell.newValue
        return { status: 'fulfilled' as const }
      }
      return { status: 'rejected' as const, reason: 'update returned success=false' }
    } catch (err) {
      return { status: 'rejected' as const, reason: String(err) }
    }
  })

  const results = await Promise.allSettled(updates)
  const successCount = results.filter(
    r => r.status === 'fulfilled' && r.value.status === 'fulfilled'
  ).length
  const failCount = results.length - successCount

  dirtyCells.value = new Map()
  if (failCount > 0) {
    message.warning(t('resultPanel.savePartial', { success: successCount, fail: failCount }))
  } else {
    message.success(t('resultPanel.saveSuccess', { count: successCount }))
  }
}

function buildRowIdentity(
  tab: ResultTab,
  rowIndex: number,
  excludeCol: string
): Record<string, unknown> {
  const oldRow = tab.objectRows[rowIndex]
  if (!oldRow) return {}
  const identity: Record<string, unknown> = {}
  for (const col of tab.columns) {
    const key = col.replace(/\./g, '_')
    if (key !== excludeCol.replace(/\./g, '_')) {
      identity[col] = (oldRow as Record<string, unknown>)[key] ?? null
    }
  }
  return identity
}

async function handleCancel(tab: ResultTab) {
  const cells = dirtyCells.value
  if (cells.size === 0) return

  for (const [, cell] of cells) {
    const row = tab.objectRows[cell.rowIndex] as Record<string, unknown>
    row[cell.colId.replace(/\./g, '_')] = cell.oldValue
  }
  tab.objectRows = [...tab.objectRows]
  dirtyCells.value = new Map()
  message.info(t('resultPanel.changesReverted'))
}
async function handleExport(format: string) {
  await doExport(format)
}

/**
 * 安全引用 SQL 标识符（列名/表名），用双引号包裹并转义内部双引号。
 * 防止 SQL 注入：列名可能包含特殊字符或恶意内容。
 */
function quoteIdentifier(ident: string): string {
  return `"${String(ident).replace(/"/g, '""')}"`
}

// ─── 右键菜单操作 ───────────────────────────────────────
function handleContextAction(payload: Record<string, unknown>) {
  closeContextMenu()
  const tab = activeTab.value
  if (!tab) return
  const { action, column, value } = payload
  const col = String(column ?? '')

  switch (action) {
    case 'copyCell':
      if (value !== null && value !== undefined) copyToClipboard(String(value))
      break
    case 'copyRow':
      if (gridApi.value) {
        const selected = gridApi.value.getSelectedRows()
        const rows = selected.length > 0 ? selected : rowData.value
        const text = rows
          .map((r: Record<string, unknown>) => tab.columns.map(c => String(r[c] ?? '')).join('\t'))
          .join('\n')
        copyToClipboard(tab.columns.join('\t') + '\n' + text)
      }
      break
    case 'copyRowJson':
      copyRowsAsJson()
      break
    case 'copyRowInsert':
      copyRowsAsInsert()
      break
    case 'filterByValue':
      if (col && value !== undefined) {
        tab.filterMode = 'quick'
        const qCol = quoteIdentifier(col)
        tab.quickFilterExpression = `${qCol} = ${typeof value === 'string' ? `'${String(value).replace(/'/g, "''")}'` : value}`
        applyQuickFilter(tab, tab.quickFilterExpression)
      }
      break
    case 'sqlFilterByValue':
      if (col && value !== undefined) {
        tab.filterMode = 'sql'
        const sCol = quoteIdentifier(col)
        tab.sqlFilterExpression = `${sCol} = ${typeof value === 'string' ? `'${String(value).replace(/'/g, "''")}'` : value}`
      }
      break
    case 'openColumnInsights':
      if (tab.duckdbTempTable) {
        insightStore.loadColumnInsight(tab.duckdbTempTable, col)
      } else {
        message.warning(t('resultPanel.needDuckdbFirst'))
      }
      break
    case 'openColumnVisualization':
      if (tab.duckdbTempTable) {
        insightStore.autoOpenVisualization = true
        insightStore.loadColumnInsight(tab.duckdbTempTable, col)
      } else {
        message.warning(t('resultPanel.needDuckdbFirst'))
      }
      break
    case 'sortAsc':
      if (gridApi.value) {
        const api = gridApi.value as unknown as AGGridSortAPI
        api.applySortState([{ colId: col, sort: 'asc' }])
      }
      break
    case 'sortDesc':
      if (gridApi.value) {
        const api = gridApi.value as unknown as AGGridSortAPI
        api.applySortState([{ colId: col, sort: 'desc' }])
      }
      break
    case 'sendSortToSql':
      tab.filterMode = 'sql'
      tab.sqlFilterExpression = `1=1 ORDER BY ${quoteIdentifier(col)} ${payload.sortDir === 'desc' ? 'DESC' : 'ASC'}`
      break
    case 'sendSortToDuckdb':
      tab.filterMode = 'duckdb'
      tab.duckdbSql = `SELECT * FROM ${quoteIdentifier(tab.duckdbTempTable || 'result_temp')} ORDER BY ${quoteIdentifier(col)} ${payload.sortDir === 'desc' ? 'DESC' : 'ASC'} LIMIT 1000`
      break
    case 'hideColumn':
      if (gridApi.value) gridApi.value.setColumnsVisible([col], false)
      break
    case 'autoSizeColumn':
      if (gridApi.value) gridApi.value.autoSizeColumns([col])
      break
    case 'autoSizeAll':
      if (gridApi.value) {
        const allCols: string[] = []
        const columns = gridApi.value.getColumns()
        if (columns) {
          columns.forEach(c => {
            if (c.getColDef().field !== '__rowNumber') allCols.push(c.getId())
          })
        }
        if (allCols.length > 0) gridApi.value.autoSizeColumns(allCols)
      }
      break
    case 'columnSummary':
      if (tab.duckdbTempTable) {
        tab.filterMode = 'duckdb'
        const tbl = quoteIdentifier(tab.duckdbTempTable)
        if (isLikelyNumeric(col)) {
          tab.duckdbSql = `SELECT COUNT(*) as count, AVG(${quoteIdentifier(col)}) as avg, MIN(${quoteIdentifier(col)}) as min, MAX(${quoteIdentifier(col)}) as max, SUM(${quoteIdentifier(col)}) as sum FROM ${tbl}`
        } else {
          tab.duckdbSql = `SELECT COUNT(*) as count, MIN(${quoteIdentifier(col)}) as min, MAX(${quoteIdentifier(col)}) as max FROM ${tbl}`
        }
      }
      break
  }
}

function copyRowsAsJson() {
  if (!activeTab.value) return
  const rows = gridApi.value?.getSelectedRows() || rowData.value
  copyToClipboard(JSON.stringify(rows, null, 2))
}

// ─── 键盘快捷键 ───────────────────────────────────────────
function handleKeyDown(event: KeyboardEvent) {
  if ((event.ctrlKey || event.metaKey) && event.key === 'Enter') {
    const tab = activeTab.value
    if (!tab) return
    if (tab.filterMode === 'sql') executeSqlFilter(tab)
    else if (tab.filterMode === 'duckdb') executeDuckdbAnalysis(tab)
  }
  if ((event.ctrlKey || event.metaKey) && event.key === 's') {
    event.preventDefault()
    if (activeTab.value) handleSave(activeTab.value)
  }
  if ((event.ctrlKey || event.metaKey) && event.key === 'r') {
    event.preventDefault()
    if (activeTab.value) handleRefresh(activeTab.value)
  }
  if ((event.ctrlKey || event.metaKey) && event.shiftKey && event.key === 'z') {
    event.preventDefault()
    if (activeTab.value && dirtyCells.value.size > 0) handleCancel(activeTab.value)
  }
}
function handleGlobalKeyDown(event: KeyboardEvent) {
  if ((event.ctrlKey || event.metaKey) && event.key === 'c') {
    if (document.activeElement?.closest('.ag-cell') && activeTab.value) {
      const tab = activeTab.value
      const selected = gridApi.value?.getSelectedRows() || []
      const rows = selected.length > 0 ? selected : rowData.value
      const text = rows
        .map((r: Record<string, unknown>) => tab.columns.map(c => String(r[c] ?? '')).join('\t'))
        .join('\n')
      copyToClipboard(tab.columns.join('\t') + '\n' + text)
    }
  }
}
</script>

<style scoped>
.query-result-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  background: var(--bg-primary);
}
.query-result-panel.compact .result-tabs {
  height: 24px;
  font-size: 10px;
}
.query-result-panel.compact .result-tab {
  padding: 0 6px;
}

/* ─── 标签栏 ─────────────────────────────────────────── */
.result-tabs {
  display: flex;
  align-items: stretch;
  height: 26px;
  flex-shrink: 0;
  background: var(--bg-tertiary, #2d2d30);
  border-bottom: 1px solid var(--border-color, #3e3e42);
  overflow-x: auto;
}
.result-tab {
  display: flex;
  align-items: center;
  gap: 3px;
  padding: 0 8px;
  font-size: 11px;
  cursor: pointer;
  color: var(--text-secondary, #888);
  white-space: nowrap;
  border-right: 1px solid var(--border-color, #3e3e42);
  user-select: none;
}
.result-tab:hover {
  background: var(--bg-hover, #333);
  color: var(--text-primary);
}
.result-tab.active {
  background: var(--bg-primary);
  color: var(--text-primary);
  border-bottom: 2px solid var(--primary-color, #0078d4);
}
.tab-close {
  font-size: 13px;
  line-height: 1;
  opacity: 0.5;
  margin-left: 2px;
}
.tab-close:hover {
  opacity: 1;
}

/* ─── 顶条 ───────────────────────────────────────────── */
.toolbar-strip {
  display: flex;
  align-items: center;
  padding: 1px 4px;
  gap: 4px;
  flex-shrink: 0;
  min-height: 26px;
  background: var(--bg-secondary, #252526);
  border-bottom: 1px solid var(--border-color, #333);
}
.toolbar-strip .strip-right {
  flex: 1;
  min-width: 0;
}

/* ─── 主体布局 ────────────────────────────────────────── */
.result-body {
  flex: 1;
  min-height: 0;
  display: flex;
  position: relative;
}

/* 左侧栏 */
.view-sidebar {
  display: flex;
  flex-direction: column;
  gap: 1px;
  padding: 2px;
  flex-shrink: 0;
  background: var(--bg-tertiary, #2d2d30);
  border-right: 1px solid var(--border-color, #3e3e42);
}
.view-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border: none;
  border-radius: 3px;
  background: transparent;
  color: var(--text-secondary, #888);
  cursor: pointer;
  transition: all 0.12s;
}
.view-btn:hover {
  background: var(--bg-hover, #3c3c3c);
  color: var(--text-primary);
}
.view-btn.active {
  background: var(--primary-color);
  color: var(--color-bg-primary);
}

/* 中间网格 */
.grid-area {
  flex: 1;
  min-width: 0;
  position: relative;
  overflow: hidden;
}
:deep(.ag-theme-alpine),
:deep(.ag-theme-alpine-dark) {
  height: 100% !important;
  font-size: 11px;
}
:deep(.ag-root-wrapper) {
  border: none;
}
:deep(.ag-header) {
  min-height: 24px !important;
}
:deep(.ag-header-cell) {
  font-size: 10px;
  font-weight: 600;
  padding: 0 4px !important;
}
:deep(.ag-header-cell-label) {
  padding: 0;
}
:deep(.ag-row) {
  font-size: 11px;
  min-height: 22px !important;
}
:deep(.ag-cell) {
  padding: 0 4px !important;
  line-height: 22px !important;
}
:deep(.null-value) {
  color: var(--color-text-muted);
  font-style: italic;
  font-size: 10px;
}
:deep(.text-right) {
  text-align: right;
  font-family: var(--font-mono);
}
:deep(.ag-pinned-left-cols-container) {
  border-right: 1px solid var(--border-color, #444);
}
:deep(.ag-row-even) {
  background: var(--bg-row-even, rgba(128, 128, 128, 0.03));
}
:deep(.ag-row-odd) {
  background: transparent;
}
:deep(.ag-row:hover) {
  background: var(--bg-hover, rgba(255, 255, 255, 0.04)) !important;
}
/* 自动列宽 */
:deep(.ag-cell) {
  overflow: hidden;
  text-overflow: ellipsis;
}

/* 文本视图样式由 ResultTextView.vue 管理 */

/* 记录视图 */
.record-view {
  height: 100%;
  overflow-y: auto;
  padding: 6px;
  display: flex;
  flex-direction: column;
}
.record-nav {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 6px;
  font-size: 11px;
  flex-shrink: 0;
}
.record-nav-text {
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-secondary, #999);
  min-width: 60px;
  text-align: center;
}

/* 右侧值查看器 */
.value-viewer {
  width: 220px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  border-left: 1px solid var(--border-color, #3e3e42);
  background: var(--bg-secondary, #252526);
}
.viewer-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 4px 6px;
  border-bottom: 1px solid var(--border-color, #333);
}
.viewer-title {
  font-size: 11px;
  font-weight: 600;
}
.viewer-content {
  padding: 6px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  overflow: auto;
}
.viewer-field {
  display: flex;
  gap: 6px;
  font-size: 10px;
}
.field-label {
  font-weight: 600;
  color: var(--text-secondary);
  min-width: 24px;
}
.field-val {
  font-family: var(--font-mono);
}
.viewer-text {
  width: 100%;
  min-height: 100px;
  border: 1px solid var(--border-color, #333);
  background: var(--bg-primary);
  color: var(--text-primary);
  font-family: var(--font-mono);
  font-size: 10px;
  padding: 4px;
  resize: none;
}
.viewer-toggle {
  position: absolute;
  top: 4px;
  right: 2px;
  z-index: 10;
}

/* ─── 底部状态栏（操作/行信息/分页合并） ──────────────── */
.result-statusbar {
  display: flex;
  align-items: center;
  height: 24px;
  padding: 0 4px;
  gap: 4px;
  flex-shrink: 0;
  font-size: 10px;
  color: var(--text-secondary, #999);
  background: var(--bg-tertiary, #2d2d30);
  border-top: 1px solid var(--border-color, #3e3e42);
}
.sbar-left,
.sbar-center,
.sbar-right {
  display: flex;
  align-items: center;
  gap: 1px;
}
.sbar-center {
  flex: 1;
  justify-content: center;
  gap: 6px;
}
.sbar-right {
  gap: 1px;
}
.mode-badge {
  padding: 0 4px;
  border-radius: 2px;
  font-size: 9px;
  font-weight: 600;
  line-height: 16px;
}
.mode-badge.quick {
  background: rgba(0, 184, 148, 0.2);
  color: var(--brand-success);
}
.mode-badge.sql {
  background: rgba(26, 90, 138, 0.2);
  color: #1890ff;
}
.mode-badge.duckdb {
  background: rgba(97, 58, 138, 0.2);
  color: #b37feb;
}
.separator {
  color: var(--border-color, #444);
}
.row-info {
  font-family: var(--font-mono);
}
.exec-time {
  color: var(--primary-color);
  font-family: var(--font-mono);
}
.page-indicator {
  font-family: var(--font-mono);
  margin: 0 2px;
}

.go-page-input {
  width: 60px;
  margin: 0 4px;
}

.analysis-notice {
  display: flex;
  align-items: center;
  height: 20px;
  padding: 0 6px;
  background: rgba(253, 203, 110, 0.15);
  border-bottom: 1px solid rgba(253, 203, 110, 0.3);
  font-size: 10px;
  color: var(--brand-warning);
  flex-shrink: 0;
}
.empty-icon {
  opacity: 0.4;
}
.empty-text {
  font-size: 13px;
  color: var(--text-secondary, #888);
}
</style>
