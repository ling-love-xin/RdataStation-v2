<template>
  <div class="result-grid-view">
    <div v-if="rowData.length === 0 && !loading" class="empty-state">
      <Database :size="32" class="empty-icon" />
      <span>{{ emptyText }}</span>
    </div>
    <div v-else ref="gridContainerRef" class="ag-grid-wrapper">
      <AgGridVue
        class="ag-theme-alpine grid-container"
        :class="themeClass"
        :column-defs="columnDefs"
        :default-col-def="localDefaultColDef"
        :row-data="rowData"
        :pagination="pagination"
        :pagination-page-size="pageSize"
        :pagination-page-selector="paginationPageSelector"
        :enable-cell-text-selection="true"
        :row-selection="rowSelection"
        :suppress-row-click-selection="true"
        :animate-rows="false"
        :header-height="24"
        :row-height="22"
        :column-virtualisation="true"
        :row-buffer="20"
        :block-load-debounce-ms="50"
        :single-click-edit="false"
        :stop-editing-when-cells-lose-focus="true"
        :dom-layout="'normal'"
        :overlay-no-rows-template="''"
        :tooltip-show-delay="300"
        @grid-ready="onGridReady"
        @cell-context-menu="onCellContextMenu"
        @row-clicked="onRowClicked"
        @selection-changed="onSelectionChanged"
        @cell-value-changed="onCellValueChanged"
        @first-data-rendered="onFirstDataRendered"
        @row-data-updated="onRowDataUpdated"
        @sort-changed="onSortChanged"
        @pagination-changed="onPaginationChanged"
        @keydown="onKeydown"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import { AgGridVue } from 'ag-grid-vue3'
import { Database } from 'lucide-vue-next'
import { ref, computed } from 'vue'

import type { ResultTab } from '@/extensions/builtin/workbench/ui/types/result'

import type { ColDef, GridApi } from 'ag-grid-community'

const props = withDefaults(
  defineProps<{
    tab: ResultTab
    columnDefs: ColDef[]
    defaultColDef: ColDef
    rowData: Record<string, unknown>[]
    pagination: boolean
    pageSize: number
    paginationPageSelector?: number[]
    isDark: boolean
    rowSelection?: 'single' | 'multiple'
    loading?: boolean
    emptyText?: string
  }>(),
  {
    paginationPageSelector: () => [50, 100, 200, 500],
    rowSelection: 'multiple',
    loading: false,
    emptyText: 'No data',
  }
)

const emit = defineEmits<{
  gridReady: [api: GridApi]
  cellContextMenu: [event: MouseEvent]
  rowClicked: [event: unknown]
  selectionChanged: [event: unknown]
  cellValueChanged: [event: unknown]
  firstDataRendered: [event: unknown]
  rowDataUpdated: [event: unknown]
  sortChanged: [event: unknown]
  paginationChanged: [event: unknown]
  keydown: [event: KeyboardEvent]
}>()

const gridApi = ref<GridApi | null>(null)
const gridContainerRef = ref<HTMLElement | null>(null)

const themeClass = computed(() => (props.isDark ? 'ag-theme-alpine-dark' : 'ag-theme-alpine'))

const localDefaultColDef = computed(() => ({
  sortable: true,
  resizable: true,
  ...props.defaultColDef,
}))

function onGridReady(params: { api: GridApi }) {
  gridApi.value = params.api
  emit('gridReady', params.api)
}

function onCellContextMenu(event: MouseEvent) {
  emit('cellContextMenu', event)
}

function onRowClicked(event: unknown) {
  emit('rowClicked', event)
}

function onSelectionChanged(event: unknown) {
  emit('selectionChanged', event)
}

function onCellValueChanged(event: unknown) {
  emit('cellValueChanged', event)
}

function onFirstDataRendered(event: unknown) {
  emit('firstDataRendered', event)
}

function onRowDataUpdated(event: unknown) {
  emit('rowDataUpdated', event)
}

function onSortChanged(event: unknown) {
  emit('sortChanged', event)
}

function onPaginationChanged(event: unknown) {
  emit('paginationChanged', event)
}

function onKeydown(event: KeyboardEvent) {
  emit('keydown', event)
}

defineExpose({ gridApi, gridContainerRef })
</script>

<style scoped>
.result-grid-view {
  flex: 1;
  overflow: hidden;
}
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  padding: 48px;
  color: var(--text-color-secondary);
}
.empty-icon {
  opacity: 0.3;
}
.ag-grid-wrapper {
  height: 100%;
  width: 100%;
}
.grid-container {
  height: 100%;
  width: 100%;
}
</style>
