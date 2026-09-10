<template>
  <div ref="resultPanelRef" class="query-result-panel">
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
        <!-- 左侧视图切换栏（深度优化版） -->
        <div class="view-sidebar-wrap">
          <div
            :class="['view-sidebar', { expanded: sidebarExpanded }]"
            :style="{ width: sidebarExpanded ? sidebarWidth + 'px' : '32px' }"
          >
            <button
              class="view-sidebar-toggle"
              :title="sidebarExpanded ? '折叠视图栏' : '展开视图栏'"
              @click="sidebarExpanded = !sidebarExpanded"
            >
              <PanelLeftClose v-if="sidebarExpanded" :size="14" />
              <PanelLeft v-else :size="14" />
            </button>
            <button
              v-for="v in viewModes"
              :key="v.key"
              :class="['view-btn', { active: currentView === v.key }]"
              :title="`${v.label} (${v.shortcut})`"
              @click="switchView(v.key)"
            >
              <component :is="v.icon" :size="18" />
              <span v-if="sidebarExpanded" class="view-btn-label">{{ v.label }}</span>
              <span
                v-if="sidebarExpanded"
                class="view-btn-badge"
                :class="{ 'badge-visible': v.badge }"
                >{{ v.badge || '' }}</span
              >
              <span v-if="sidebarExpanded" class="view-btn-shortcut">{{ v.shortcut }}</span>
            </button>
            <!-- 视图栏底部快捷操作 -->
            <div class="view-sidebar-footer">
              <button
                class="view-footer-btn"
                title="导出结果集"
                @click="showExportDropdown = !showExportDropdown"
              >
                <Download :size="14" />
              </button>
              <button
                class="view-footer-btn"
                title="自动刷新"
                :class="{ active: autoRefreshEnabled }"
                @click="autoRefreshEnabled = !autoRefreshEnabled"
              >
                <RotateCw :size="14" />
              </button>
            </div>
          </div>
          <div class="view-sidebar-resize" @mousedown="startSidebarResize" />
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

        <!-- 右侧多Tab面板（优化版） -->
        <div v-if="showRightPanel" class="right-panel-wrap">
          <div class="right-panel" :style="{ width: rightPanelWidth + 'px' }">
            <div class="rrp-tabs">
              <div
                v-for="rt in rightTabs"
                :key="rt.key"
                :class="['rrp-tab', { active: activeRightTab === rt.key }]"
                @click="activeRightTab = rt.key"
              >
                <component :is="rt.icon" :size="12" />
                <span>{{ rt.label }}</span>
              </div>
              <div class="rrp-header-actions">
                <button
                  class="rrp-pin-btn"
                  :class="{ pinned: rightPanelPinned }"
                  :title="rightPanelPinned ? '取消固定' : '固定面板'"
                  @click="rightPanelPinned = !rightPanelPinned"
                >
                  <Pin :size="11" />
                </button>
                <button class="rrp-collapse-btn" title="折叠面板" @click="showRightPanel = false">
                  <ChevronRight :size="12" />
                </button>
              </div>
            </div>

            <div class="rrp-content">
              <!-- 值查看器 -->
              <div v-show="activeRightTab === 'value'" class="rrp-panel-content">
                <div class="rrp-section">
                  <div class="rrp-section-header collapsible" @click="toggleSection('valueFields')">
                    <span>单元格信息</span>
                    <ChevronDown
                      :size="10"
                      :class="['rrp-chevron', { rotated: !sectionsExpanded.valueFields }]"
                    />
                  </div>
                  <div v-show="sectionsExpanded.valueFields" class="rrp-section-body">
                    <div class="field-row">
                      <span class="field-lbl">列</span>
                      <span class="field-val">{{ selectedCell?.column || '-' }}</span>
                    </div>
                    <div class="field-row">
                      <span class="field-lbl">行</span>
                      <span class="field-val">{{
                        selectedCell?.row != null ? selectedCell.row + 1 : '-'
                      }}</span>
                    </div>
                  </div>
                </div>
                <textarea
                  class="viewer-text"
                  :value="selectedCell?.value != null ? String(selectedCell.value) : ''"
                  readonly
                  rows="8"
                />
              </div>

              <!-- 元数据 -->
              <div v-show="activeRightTab === 'metadata'" class="rrp-panel-content">
                <div class="rrp-section">
                  <div class="rrp-section-header collapsible" @click="toggleSection('metaInfo')">
                    <span>结果集信息</span>
                    <ChevronDown
                      :size="10"
                      :class="['rrp-chevron', { rotated: !sectionsExpanded.metaInfo }]"
                    />
                  </div>
                  <div v-show="sectionsExpanded.metaInfo" class="rrp-section-body">
                    <div class="field-row">
                      <span class="field-lbl">行数</span>
                      <span class="field-val">{{ activeTab?.displayedRowCount ?? 0 }}</span>
                    </div>
                    <div class="field-row">
                      <span class="field-lbl">列数</span>
                      <span class="field-val">{{ activeTab?.columns?.length ?? 0 }}</span>
                    </div>
                    <div class="field-row">
                      <span class="field-lbl">耗时</span>
                      <span class="field-val">{{
                        activeTab?.executionTime
                          ? (activeTab.executionTime / 1000).toFixed(3) + 's'
                          : '-'
                      }}</span>
                    </div>
                  </div>
                </div>
                <div class="rrp-section">
                  <div class="rrp-section-header collapsible" @click="toggleSection('metaCols')">
                    <span>列信息</span>
                    <ChevronDown
                      :size="10"
                      :class="['rrp-chevron', { rotated: !sectionsExpanded.metaCols }]"
                    />
                  </div>
                  <div v-show="sectionsExpanded.metaCols" class="rrp-section-body">
                    <div
                      v-for="col in (activeTab?.columns ?? []).slice(0, 10)"
                      :key="col"
                      class="field-row"
                    >
                      <span class="field-lbl">{{ col }}</span>
                      <span class="field-val-col">{{ getColumnType(col) }}</span>
                    </div>
                    <div v-if="(activeTab?.columns?.length ?? 0) > 10" class="rrp-more-hint">
                      ...还有 {{ (activeTab?.columns?.length ?? 0) - 10 }} 列
                    </div>
                  </div>
                </div>
              </div>

              <!-- 计算 -->
              <div v-show="activeRightTab === 'calc'" class="rrp-panel-content">
                <div class="rrp-section">
                  <div class="rrp-section-header collapsible" @click="toggleSection('calcSum')">
                    <span>选中统计</span>
                    <ChevronDown
                      :size="10"
                      :class="['rrp-chevron', { rotated: !sectionsExpanded.calcSum }]"
                    />
                  </div>
                  <div v-show="sectionsExpanded.calcSum" class="rrp-section-body">
                    <div class="field-row">
                      <span class="field-lbl">已选行</span>
                      <span class="field-val">{{ selectedRows.length }}</span>
                    </div>
                    <div v-if="selectedRows.length > 0" class="field-row">
                      <span class="field-lbl">求和</span>
                      <span class="field-val">{{ selectedSumText }}</span>
                    </div>
                  </div>
                </div>
              </div>

              <!-- 分组 -->
              <div v-show="activeRightTab === 'group'" class="rrp-panel-content">
                <div class="rrp-section">
                  <div class="rrp-section-header">
                    <span>分组面板</span>
                  </div>
                  <div class="rrp-section-body">
                    <div class="rrp-empty-hint">
                      <Layers :size="20" />
                      <span>拖拽列到此处进行分组聚合</span>
                      <span class="rrp-hint-sub">支持多级分组和聚合函数</span>
                    </div>
                  </div>
                </div>
                <div class="rrp-section">
                  <div
                    class="rrp-section-header collapsible"
                    @click="toggleSection('groupPresets')"
                  >
                    <span>预设聚合</span>
                    <ChevronDown
                      :size="10"
                      :class="['rrp-chevron', { rotated: !sectionsExpanded.groupPresets }]"
                    />
                  </div>
                  <div v-show="sectionsExpanded.groupPresets" class="rrp-section-body">
                    <div class="rrp-quick-actions">
                      <button class="rrp-quick-btn" title="按第一列分组计数">COUNT</button>
                      <button class="rrp-quick-btn" title="按第一列分组求和">SUM</button>
                      <button class="rrp-quick-btn" title="按第一列分组求平均">AVG</button>
                      <button class="rrp-quick-btn" title="按第一列分组求最大/最小">MIN/MAX</button>
                    </div>
                  </div>
                </div>
              </div>

              <!-- 洞察 -->
              <div v-show="activeRightTab === 'insight'" class="rrp-panel-content">
                <div class="rrp-section">
                  <div
                    class="rrp-section-header collapsible"
                    @click="toggleSection('insightStats')"
                  >
                    <span>统计概览</span>
                    <ChevronDown
                      :size="10"
                      :class="['rrp-chevron', { rotated: !sectionsExpanded.insightStats }]"
                    />
                  </div>
                  <div v-show="sectionsExpanded.insightStats" class="rrp-section-body">
                    <div class="field-row">
                      <span class="field-lbl">总行数</span>
                      <span class="field-val">{{ activeTab?.displayedRowCount ?? '-' }}</span>
                    </div>
                    <div class="field-row">
                      <span class="field-lbl">列数</span>
                      <span class="field-val">{{ activeTab?.columns?.length ?? '-' }}</span>
                    </div>
                    <div class="field-row">
                      <span class="field-lbl">NULL 列</span>
                      <span class="field-val">{{ nullColumnCount }}</span>
                    </div>
                    <div class="field-row">
                      <span class="field-lbl">数值列</span>
                      <span class="field-val">{{ numericColumnCount }}</span>
                    </div>
                  </div>
                </div>
                <div class="rrp-section">
                  <div class="rrp-section-header collapsible" @click="toggleSection('insightDist')">
                    <span>数据分布</span>
                    <ChevronDown
                      :size="10"
                      :class="['rrp-chevron', { rotated: !sectionsExpanded.insightDist }]"
                    />
                  </div>
                  <div v-show="sectionsExpanded.insightDist" class="rrp-section-body">
                    <div class="rrp-mini-chart">
                      <div class="mini-bar" style="height: 100%" />
                      <div class="mini-bar" style="height: 60%" />
                      <div class="mini-bar" style="height: 30%" />
                      <div class="mini-bar" style="height: 50%" />
                      <div class="mini-bar" style="height: 80%" />
                      <div class="mini-bar" style="height: 25%" />
                      <div class="mini-bar" style="height: 45%" />
                      <div class="mini-bar" style="height: 70%" />
                    </div>
                    <div class="rrp-mini-hint">值分布概览</div>
                  </div>
                </div>
                <div class="rrp-section">
                  <div
                    class="rrp-section-header collapsible"
                    @click="toggleSection('insightQuality')"
                  >
                    <span>数据质量</span>
                    <ChevronDown
                      :size="10"
                      :class="['rrp-chevron', { rotated: !sectionsExpanded.insightQuality }]"
                    />
                  </div>
                  <div v-show="sectionsExpanded.insightQuality" class="rrp-section-body">
                    <div class="field-row">
                      <span class="field-lbl">完整度</span>
                      <span class="field-val">-</span>
                    </div>
                    <div class="field-row">
                      <span class="field-lbl">唯一值</span>
                      <span class="field-val">-</span>
                    </div>
                    <div class="rrp-mini-hint">点击列洞察查看详细分析</div>
                  </div>
                </div>
              </div>
            </div>
          </div>
          <div class="rrp-resize" @mousedown="startRRPResize" />
        </div>
        <NButton
          v-if="!showRightPanel && activeTab"
          size="tiny"
          quaternary
          class="viewer-toggle"
          :title="t('workbench.openValueViewer')"
          @click="showRightPanel = true"
        >
          <PanelRight :size="14" />
        </NButton>
      </div>

      <!-- 底部状态栏（深度优化版） -->
      <div v-if="activeTab" class="result-statusbar">
        <div class="sbar-left">
          <span :class="['mode-badge', tab.filterMode]">{{ modeLabel(tab) }}</span>
          <span class="rsb-sep" />
          <!-- 结果集导航 -->
          <NButton
            size="tiny"
            quaternary
            :disabled="tabs.length <= 1"
            :title="t('resultPanel.prevResult')"
            @click="navigateResult(-1)"
          >
            <ChevronUp :size="11" />
          </NButton>
          <span class="result-nav-text">{{ resultNavText }}</span>
          <NButton
            size="tiny"
            quaternary
            :disabled="tabs.length <= 1"
            :title="t('resultPanel.nextResult')"
            @click="navigateResult(1)"
          >
            <ChevronDown :size="11" />
          </NButton>
          <span class="rsb-sep" />
          <NButton
            size="tiny"
            quaternary
            :title="t('resultPanel.refresh')"
            @click="handleRefresh(tab)"
          >
            <RotateCw :size="11" />
          </NButton>
          <div
            :class="['rsb-auto-refresh', { active: autoRefreshEnabled }]"
            :title="autoRefreshEnabled ? '停止自动刷新' : '自动刷新'"
            @click="autoRefreshEnabled = !autoRefreshEnabled"
          >
            <span v-if="autoRefreshEnabled" class="refresh-dot" />
            自动
          </div>
          <span class="rsb-sep" />
          <span class="rsb-fetch-size" title="每次抓取行数">抓取 {{ fetchSize }}</span>
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
          <span v-if="tab.executionTime" class="exec-time">{{
            formatExecTime(tab.executionTime)
          }}</span>
          <span v-if="tab.connectionId" class="rsb-conn-info" :title="tab.connectionId">
            <Database :size="10" />
            {{ tab.connectionId.split('/').pop()?.split('?')[0] || tab.connectionId }}
          </span>
          <span v-if="selectedRows.length > 0" class="rsb-selected-info"
            >| 已选 {{ selectedRows.length }} 行</span
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
  ChevronUp,
  ChevronDown,
  SkipBack,
  SkipForward,
  AlignLeft,
  List,
  GitCompare,
  BarChart3,
  Layers,
  PanelLeft,
  PanelLeftClose,
  Pin,
  BadgeInfo,
  Sigma,
  Group,
  ChartLine,
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

// ─── Props ───────────────────────────────────────────────
const props = defineProps<{
  editorId: string
}>()

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

// ─── 编辑器生命周期 ──────────────────────────────────────
const resultPanelRef = ref<HTMLElement | null>(null)

onMounted(() => {
  resultStore.setCurrentEditor(props.editorId)
  // 使用组件容器级键盘事件，避免多实例冲突
  const el = resultPanelRef.value
  if (el) {
    el.addEventListener('keydown', handleGlobalKeyDown as (e: Event) => void)
  }
})

watch(
  () => props.editorId,
  newId => {
    resultStore.setCurrentEditor(newId)
  }
)

onUnmounted(() => {
  const el = resultPanelRef.value
  if (el) {
    el.removeEventListener('keydown', handleGlobalKeyDown as (e: Event) => void)
  }
})

// ─── 多标签状态（按 editorId 从 store 读取）──────────────
const tabs = computed(() => resultStore.getTabs(props.editorId))
const activeTabId = computed(() => {
  const t = resultStore.getActiveTab(props.editorId)
  return t?.id ?? null
})
const activeTab = computed<ResultTab | null>(() => {
  return resultStore.getActiveTab(props.editorId)
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
const showRightPanel = ref(false)
const rightPanelWidth = ref(220)
const rightPanelPinned = ref(false)
const activeRightTab = ref('value')
const sidebarExpanded = ref(false)
const sidebarWidth = ref(32)
const autoRefreshEnabled = ref(false)
const fetchSize = ref(200)
const showExportDropdown = ref(false)
const selectedRecordIndex = ref(0)
const isDraggingSidebar = ref(false)
const isDraggingRRP = ref(false)

const sectionsExpanded = ref<Record<string, boolean>>({
  valueFields: true,
  metaInfo: true,
  metaCols: true,
  calcSum: true,
  insightStats: true,
  insightDist: true,
  insightQuality: true,
  groupPresets: false,
})
interface SelectedCell {
  column: string
  row: number
  value: unknown
}

interface AGGridSortAPI {
  applySortState(s: Array<{ colId: string; sort: string }>): void
}

const selectedCell = ref<SelectedCell | null>(null)

const viewModes = computed(() => [
  {
    key: 'grid' as ViewMode,
    icon: Database,
    label: t('workbench.gridView'),
    shortcut: 'Ctrl+1',
    badge: activeTab.value?.displayedRowCount ? String(activeTab.value.displayedRowCount) : '',
  },
  {
    key: 'text' as ViewMode,
    icon: AlignLeft,
    label: t('workbench.textView'),
    shortcut: 'Ctrl+2',
    badge: '',
  },
  {
    key: 'record' as ViewMode,
    icon: List,
    label: t('workbench.recordView'),
    shortcut: 'Ctrl+3',
    badge: '',
  },
  {
    key: 'chart' as ViewMode,
    icon: BarChart3,
    label: t('workbench.chartView'),
    shortcut: 'Ctrl+4',
    badge: '',
  },
])

const rightTabs = [
  { key: 'value', icon: PanelRight, label: '值' },
  { key: 'metadata', icon: BadgeInfo, label: '元数据' },
  { key: 'calc', icon: Sigma, label: '计算' },
  { key: 'group', icon: Group, label: '分组' },
  { key: 'insight', icon: ChartLine, label: '洞察' },
]

const selectedSumText = computed(() => {
  if (!activeTab.value || selectedRows.value.length === 0) return '-'
  const cols = activeTab.value.columns
  const firstNumericCol = cols.find(c => {
    const val = (selectedRows.value[0] as Record<string, unknown>)[c]
    return typeof val === 'number'
  })
  if (!firstNumericCol) return '-'
  const sum = selectedRows.value.reduce((acc: number, row) => {
    const val = (row as Record<string, unknown>)[firstNumericCol]
    return acc + (typeof val === 'number' ? val : 0)
  }, 0)
  return `${sum}`
})

function getColumnType(col: string): string {
  if (!activeTab.value) return '-'
  const rowData = activeTab.value.objectRows
  if (rowData.length === 0) return '-'
  const val = (rowData[0] as Record<string, unknown>)[col]
  if (val === null || val === undefined) return 'NULL'
  if (typeof val === 'number') return Number.isInteger(val) ? 'integer' : 'float'
  if (typeof val === 'boolean') return 'boolean'
  if (typeof val === 'string') return 'text'
  if (val instanceof Date) return 'datetime'
  return 'text'
}

const nullColumnCount = computed(() => {
  if (!activeTab.value) return 0
  const rowData = activeTab.value.objectRows
  if (rowData.length === 0) return 0
  return activeTab.value.columns.filter(col => {
    const sample = (rowData[0] as Record<string, unknown>)[col]
    return sample === null || sample === undefined
  }).length
})

const numericColumnCount = computed(() => {
  if (!activeTab.value) return 0
  const rowData = activeTab.value.objectRows
  if (rowData.length === 0) return 0
  return activeTab.value.columns.filter(col => {
    const sample = (rowData[0] as Record<string, unknown>)[col]
    return typeof sample === 'number'
  }).length
})

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

// ─── 结果集导航 ──────────────────────────────────────
const resultNavText = computed(() => {
  if (tabs.value.length === 0) return ''
  const activeIdx = tabs.value.findIndex(t => t.id === activeTabId.value)
  return `${activeIdx + 1}/${tabs.value.length}`
})

function navigateResult(direction: number) {
  const all = tabs.value
  if (all.length <= 1) return
  const currentIdx = all.findIndex(t => t.id === activeTabId.value)
  if (currentIdx === -1) return
  const newIdx = (currentIdx + direction + all.length) % all.length
  switchTab(all[newIdx].id)
}

function formatExecTime(ms: number): string {
  if (ms < 1000) return `${ms}ms`
  if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`
  return `${(ms / 60000).toFixed(1)}m`
}

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

// ─── 标签管理（委托到 store，带 editorId）───────────────
function tabHasDirty(tab: ResultTab | null): boolean {
  return tab ? tab.dirtyRows.size > 0 : false
}

function switchTab(id: string) {
  resultStore.switchTab(props.editorId, id)
}

function closeTab(id: string) {
  resultStore.closeTab(props.editorId, id)
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
    ? resultStore.getTabs(props.editorId).find(t => t.id === panelId && t.columns.length === 0)
    : null

  if (existingTab) {
    resultStore.setTabResult(props.editorId, existingTab.id, {
      columns,
      rows,
      rowCount: rows.length,
      elapsedMs,
    })
  } else {
    const newTab = resultStore.addTab(props.editorId, '', '')
    resultStore.setTabResult(props.editorId, newTab.id, {
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

  const newTab = resultStore.addTab(props.editorId, '', '')
  if (latest.title) {
    newTab.title = latest.title
  }
  resultStore.setTabResult(props.editorId, newTab.id, {
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

// ─── DuckDB 临时表（委托到 store，带 editorId）──────────
async function _ensureDuckdbTempTable(tabId: string) {
  await resultStore.ensureDuckdbTable(props.editorId, tabId)
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
  // 使用 AG Grid 事件对象中的 clientX/clientY
  const event = params.event as MouseEvent | null
  contextMenu.value = {
    visible: true,
    x: event?.clientX ?? 0,
    y: event?.clientY ?? 0,
    type: 'cell',
    value: params.value,
    column: params.colDef.field ?? '',
    sortDir: '',
  }
}

function handleGridContextMenu(event: MouseEvent) {
  const target = event.target as HTMLElement
  if (target.closest('.ag-header-cell')) {
    const api = gridApi.value as {
      getFocusedCell?: () => { column: { getId: () => string } }
      getSortModel?: () => Array<{ colId: string; sort: string }>
    } | null
    const colId = api?.getFocusedCell?.()?.column?.getId?.() ?? ''
    const sortModel = api?.getSortModel
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

function toggleSection(key: string) {
  sectionsExpanded.value[key] = !sectionsExpanded.value[key]
}

function startSidebarResize(e: MouseEvent) {
  e.preventDefault()
  isDraggingSidebar.value = true
  document.addEventListener('mousemove', onSidebarResize)
  document.addEventListener('mouseup', stopSidebarResize)
}

function onSidebarResize(e: MouseEvent) {
  if (!isDraggingSidebar.value) return
  sidebarWidth.value = Math.max(32, Math.min(160, e.clientX - 100))
}

function stopSidebarResize() {
  isDraggingSidebar.value = false
  document.removeEventListener('mousemove', onSidebarResize)
  document.removeEventListener('mouseup', stopSidebarResize)
}

function startRRPResize(e: MouseEvent) {
  e.preventDefault()
  isDraggingRRP.value = true
  document.addEventListener('mousemove', onRRPResize)
  document.addEventListener('mouseup', stopRRPResize)
}

function onRRPResize(e: MouseEvent) {
  if (!isDraggingRRP.value) return
  const container = (e.target as HTMLElement).closest('.result-body')
  if (!container) return
  const rect = container.getBoundingClientRect()
  rightPanelWidth.value = Math.max(160, Math.min(400, rect.right - e.clientX))
}

function stopRRPResize() {
  isDraggingRRP.value = false
  document.removeEventListener('mousemove', onRRPResize)
  document.removeEventListener('mouseup', stopRRPResize)
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

/* 左侧栏包裹 */
.view-sidebar-wrap {
  display: flex;
  flex-shrink: 0;
  position: relative;
}

/* 左侧栏 */
.view-sidebar {
  display: flex;
  flex-direction: column;
  gap: 1px;
  padding: 2px;
  flex-shrink: 0;
  background: #2d2d30;
  border-right: 1px solid #3e3e42;
  width: 32px;
  transition: width 0.18s cubic-bezier(0.4, 0, 0.2, 1);
  overflow: hidden;
}
.view-sidebar.expanded {
  width: 100px;
}

/* 左侧栏底部快捷操作 */
.view-sidebar-footer {
  margin-top: auto;
  display: flex;
  flex-direction: column;
  gap: 1px;
  padding-top: 4px;
  border-top: 1px solid #3e3e42;
}
.view-footer-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: none;
  border-radius: 3px;
  background: transparent;
  color: #6a6a6a;
  cursor: pointer;
  transition: all 0.12s;
}
.view-footer-btn:hover {
  background: #3c3c3c;
  color: #ccc;
}
.view-footer-btn.active {
  color: #4caf50;
}

/* 左侧栏折叠按钮 */
.view-sidebar-toggle {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 22px;
  border: none;
  border-radius: 3px;
  background: transparent;
  color: #6a6a6a;
  cursor: pointer;
  margin-bottom: 2px;
  transition: all 0.12s;
}
.view-sidebar-toggle:hover {
  background: #3c3c3c;
  color: #ccc;
}

.view-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 28px;
  height: 28px;
  border: none;
  border-radius: 3px;
  background: transparent;
  color: #888;
  cursor: pointer;
  transition: all 0.12s;
  overflow: hidden;
  white-space: nowrap;
}
.view-sidebar.expanded .view-btn {
  width: auto;
  padding: 0 6px;
  justify-content: flex-start;
}
.view-btn:hover {
  background: #3c3c3c;
  color: #ccc;
}
.view-btn.active {
  background: #0078d4;
  color: #fff;
}
.view-btn-label {
  font-size: 10px;
  display: none;
}
.view-sidebar.expanded .view-btn-label {
  display: inline;
}
.view-btn-badge {
  font-size: 9px;
  background: rgba(255, 255, 255, 0.15);
  padding: 0 4px;
  border-radius: 8px;
  margin-left: auto;
  display: none;
  min-width: 16px;
  text-align: center;
}
.view-sidebar.expanded .view-btn-badge {
  display: inline;
}
.view-btn-badge.badge-visible {
  display: inline;
}
.view-btn-shortcut {
  font-size: 8px;
  color: #555;
  display: none;
}
.view-sidebar.expanded .view-btn-shortcut {
  display: inline;
}
.view-btn.active .view-btn-shortcut {
  color: rgba(255, 255, 255, 0.5);
}

/* 左侧栏拖拽调整 */
.view-sidebar-resize {
  width: 3px;
  cursor: col-resize;
  background: transparent;
  flex-shrink: 0;
  transition: background 0.15s;
}
.view-sidebar-resize:hover {
  background: #0078d4;
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

/* 右侧多Tab面板 */
.right-panel-wrap {
  display: flex;
  flex-shrink: 0;
  position: relative;
}
.right-panel {
  display: flex;
  flex-direction: column;
  border-left: 1px solid #3e3e42;
  background: #252526;
  min-width: 160px;
  max-width: 400px;
}
.rrp-tabs {
  display: flex;
  align-items: center;
  height: 28px;
  background: #2d2d30;
  border-bottom: 1px solid #3e3e42;
  overflow-x: auto;
  flex-shrink: 0;
}
.rrp-tab {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 0 8px;
  font-size: 10px;
  cursor: pointer;
  color: #888;
  white-space: nowrap;
  height: 100%;
  border-bottom: 2px solid transparent;
  transition: all 0.12s;
}
.rrp-tab:hover {
  color: #ccc;
}
.rrp-tab.active {
  color: #fff;
  border-bottom-color: #0078d4;
}
.rrp-header-actions {
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: 2px;
  padding-right: 4px;
}
.rrp-pin-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  border: none;
  border-radius: 3px;
  background: transparent;
  color: #888;
  cursor: pointer;
  transition: all 0.12s;
}
.rrp-pin-btn:hover {
  color: #ccc;
}
.rrp-pin-btn.pinned {
  color: #4fc3f7;
}
.rrp-collapse-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  border: none;
  border-radius: 3px;
  background: transparent;
  color: #888;
  cursor: pointer;
  transition: all 0.12s;
}
.rrp-collapse-btn:hover {
  color: #ccc;
}

.rrp-content {
  flex: 1;
  overflow-y: auto;
  padding: 4px;
}
.rrp-panel-content {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

/* 可折叠区域 */
.rrp-section {
  border: 1px solid #3e3e42;
  border-radius: 4px;
  overflow: hidden;
}
.rrp-section-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 4px 8px;
  font-size: 10px;
  font-weight: 600;
  color: #ccc;
  background: #2d2d30;
  user-select: none;
}
.rrp-section-header.collapsible {
  cursor: pointer;
}
.rrp-section-header.collapsible:hover {
  background: #333;
}
.rrp-chevron {
  transition: transform 0.15s;
  color: #6a6a6a;
}
.rrp-chevron.rotated {
  transform: rotate(-90deg);
}
.rrp-section-body {
  padding: 4px 8px;
  display: flex;
  flex-direction: column;
  gap: 3px;
  background: #1e1e1e;
}
.field-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-size: 10px;
}
.field-lbl {
  color: #888;
  flex-shrink: 0;
}
.field-val {
  font-family: 'Cascadia Code', 'Consolas', monospace;
  color: #ccc;
  text-align: right;
  max-width: 120px;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* 迷你图表 */
.rrp-mini-chart {
  display: flex;
  align-items: flex-end;
  gap: 3px;
  height: 40px;
  padding: 2px 0;
}
.mini-bar {
  flex: 1;
  background: #0078d4;
  border-radius: 2px 2px 0 0;
  opacity: 0.6;
  min-width: 4px;
  transition: opacity 0.15s;
}
.mini-bar:hover {
  opacity: 1;
}
.rrp-mini-hint {
  font-size: 9px;
  color: #6a6a6a;
  text-align: center;
}
.rrp-empty-hint {
  font-size: 11px;
  color: #6a6a6a;
  text-align: center;
  padding: 20px 8px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
}
.rrp-hint-sub {
  font-size: 9px;
  color: #555;
}
.rrp-more-hint {
  font-size: 9px;
  color: #555;
  text-align: center;
  padding: 4px 0;
}

/* 快速操作按钮 */
.rrp-quick-actions {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 3px;
}
.rrp-quick-btn {
  padding: 3px 6px;
  font-size: 9px;
  font-weight: 600;
  border: 1px solid #3e3e42;
  border-radius: 3px;
  background: #2d2d30;
  color: #888;
  cursor: pointer;
  transition: all 0.12s;
  font-family: 'Consolas', monospace;
}
.rrp-quick-btn:hover {
  background: #094771;
  color: #fff;
  border-color: #007acc;
}

/* 列值类型 */
.field-val-col {
  font-family: 'Consolas', monospace;
  font-size: 9px;
  color: #007acc;
  background: rgba(0, 120, 212, 0.1);
  padding: 0 4px;
  border-radius: 2px;
  max-width: 80px;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* 值查看器文本 */
.viewer-text {
  width: 100%;
  min-height: 60px;
  border: 1px solid #3e3e42;
  background: #1e1e1e;
  color: #ccc;
  font-family: 'Cascadia Code', 'Consolas', monospace;
  font-size: 10px;
  padding: 4px;
  resize: none;
  border-radius: 3px;
}

/* 右侧面板拖拽 */
.rrp-resize {
  width: 3px;
  cursor: col-resize;
  background: transparent;
  flex-shrink: 0;
  transition: background 0.15s;
}
.rrp-resize:hover {
  background: #0078d4;
}

.viewer-toggle {
  position: absolute;
  top: 4px;
  right: 2px;
  z-index: 10;
}

/* ─── 底部状态栏（优化版） ──────────────── */
.result-statusbar {
  display: flex;
  align-items: center;
  height: 24px;
  padding: 0 4px;
  gap: 2px;
  flex-shrink: 0;
  font-size: 10px;
  color: #999;
  background: #2d2d30;
  border-top: 1px solid #3e3e42;
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
  color: #00b894;
}
.mode-badge.sql {
  background: rgba(26, 90, 138, 0.2);
  color: #1890ff;
}
.mode-badge.duckdb {
  background: rgba(97, 58, 138, 0.2);
  color: #b37feb;
}

/* 状态栏分隔符 */
.rsb-sep {
  width: 1px;
  height: 14px;
  background: #3e3e42;
  margin: 0 2px;
}

/* 自动刷新 */
.rsb-auto-refresh {
  display: flex;
  align-items: center;
  gap: 3px;
  padding: 0 4px;
  border-radius: 2px;
  cursor: pointer;
  font-size: 9px;
  color: #888;
}
.rsb-auto-refresh:hover {
  color: #ccc;
}
.rsb-auto-refresh.active {
  color: #4caf50;
}
.refresh-dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: #4caf50;
  animation: pulse 1.5s ease-in-out infinite;
}

/* 抓取大小 */
.rsb-fetch-size {
  font-family: 'Consolas', monospace;
  font-size: 9px;
  color: #888;
  cursor: pointer;
  padding: 0 3px;
}
.rsb-fetch-size:hover {
  color: #ccc;
}

/* 选中行信息 */
.rsb-selected-info {
  font-size: 9px;
  color: #4fc3f7;
}

/* 结果集导航 */
.result-nav-text {
  font-family: 'Consolas', monospace;
  font-size: 9px;
  color: #888;
  min-width: 30px;
  text-align: center;
}

/* 连接信息 */
.rsb-conn-info {
  display: flex;
  align-items: center;
  gap: 3px;
  font-size: 9px;
  color: #6a6a6a;
  max-width: 120px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

@keyframes pulse {
  0%,
  100% {
    opacity: 1;
  }
  50% {
    opacity: 0.3;
  }
}

.row-info {
  font-family: 'Consolas', monospace;
}
.exec-time {
  color: #0078d4;
  font-family: 'Consolas', monospace;
}
.page-indicator {
  font-family: 'Consolas', monospace;
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
