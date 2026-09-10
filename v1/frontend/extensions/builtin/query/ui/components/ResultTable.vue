<template>
  <div class="result-table-container">
    <AgGridVue
      class="ag-theme-alpine"
      :class="{ 'ag-theme-alpine-dark': uiStore.isDark }"
      :column-defs="columnDefs"
      :row-data="rowData"
      :default-col-def="defaultColDef"
      :pagination="true"
      :pagination-page-size="100"
      :pagination-page-size-selector="[50, 100, 200, 500]"
      :enable-cell-text-selection="true"
      :suppress-row-click-selection="true"
      style="width: 100%; height: 100%"
      @grid-ready="onGridReady"
    />
  </div>
</template>

<script setup lang="ts">
import { ClientSideRowModelModule, ModuleRegistry } from 'ag-grid-community'
import { AgGridVue } from 'ag-grid-vue3'
import { ref, computed, watch } from 'vue'

import { useUiStore } from '@/shared/stores/ui'

// 注册 ag-Grid 模块
ModuleRegistry.registerModules([ClientSideRowModelModule])

const props = defineProps<{
  columns: string[]
  rows: unknown[][]
}>()

const uiStore = useUiStore()
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const gridApi = ref<any>(null)

// 转换列为 ag-Grid 格式
const columnDefs = computed(() => {
  return props.columns.map(col => ({
    field: col,
    headerName: col,
    sortable: true,
    filter: true,
    resizable: true,
    minWidth: 100,
    flex: 1,
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    valueFormatter: (params: any) => {
      if (params.value === null || params.value === undefined) {
        return 'NULL'
      }
      if (typeof params.value === 'object') {
        return JSON.stringify(params.value)
      }
      return String(params.value)
    },
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    cellClass: (params: any) => {
      if (params.value === null || params.value === undefined) {
        return 'cell-null'
      }
      return ''
    },
  }))
})

// 转换行为 ag-Grid 格式
const rowData = computed(() => {
  return props.rows.map(row => {
    const obj: Record<string, unknown> = {}
    props.columns.forEach((col, index) => {
      obj[col] = row[index]
    })
    return obj
  })
})

const defaultColDef = {
  sortable: true,
  filter: true,
  resizable: true,
  minWidth: 80,
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function onGridReady(params: any) {
  gridApi.value = params.api
  // 自动调整列宽
  params.api.sizeColumnsToFit()
}

// 监听数据变化，自动调整列宽
watch(
  () => props.rows,
  () => {
    if (gridApi.value) {
      setTimeout(() => {
        gridApi.value.sizeColumnsToFit()
      }, 0)
    }
  },
  { deep: true }
)
</script>

<style scoped>
.result-table-container {
  width: 100%;
  height: 100%;
}

:deep(.cell-null) {
  color: var(--text-tertiary);
  font-style: italic;
}

/* Light Theme Overrides */
:deep(.ag-theme-alpine) {
  --ag-background-color: var(--bg-primary);
  --ag-foreground-color: var(--text-primary);
  --ag-header-background-color: var(--bg-secondary);
  --ag-header-foreground-color: var(--text-primary);
  --ag-border-color: var(--border-color);
  --ag-row-hover-color: var(--bg-hover);
  --ag-selected-row-background-color: var(--primary-light);
  --ag-font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
  --ag-font-size: 13px;
}

/* Dark Theme Overrides */
:deep(.ag-theme-alpine-dark) {
  --ag-background-color: var(--bg-primary);
  --ag-foreground-color: var(--text-primary);
  --ag-header-background-color: var(--bg-secondary);
  --ag-header-foreground-color: var(--text-primary);
  --ag-border-color: var(--border-color);
  --ag-row-hover-color: var(--bg-hover);
  --ag-selected-row-background-color: var(--primary-light);
  --ag-font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
  --ag-font-size: 13px;
}

:deep(.ag-root-wrapper) {
  border: none;
  border-radius: 0;
}

:deep(.ag-header) {
  border-bottom: 1px solid var(--border-color);
}

:deep(.ag-header-cell) {
  font-weight: 600;
}

:deep(.ag-row) {
  border-bottom: 1px solid var(--border-color);
}

:deep(.ag-cell) {
  display: flex;
  align-items: center;
  padding: 0 12px;
}

:deep(.ag-pagination) {
  border-top: 1px solid var(--border-color);
  background: var(--bg-secondary);
}

:deep(.ag-paging-button) {
  color: var(--text-secondary);
}

:deep(.ag-paging-button:hover) {
  color: var(--text-primary);
}

:deep(.ag-paging-button.ag-disabled) {
  opacity: 0.3;
}
</style>
