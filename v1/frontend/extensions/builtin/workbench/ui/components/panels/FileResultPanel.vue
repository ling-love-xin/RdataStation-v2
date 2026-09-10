<template>
  <div class="file-result-panel">
    <GridToolbar
      :row-count="metadata.totalRowCount"
      :elapsed-ms="metadata.elapsedMs"
      :is-duckdb-ready="metadata.duckdbTable !== null"
      @close="handleClose"
      @export-csv="handleExportCsv"
      @export-json="handleExportJson"
    />

    <div class="result-grid-wrapper">
      <div v-if="isGridReady" ref="gridContainerRef" class="ag-grid-container" />
      <div v-else class="grid-placeholder">
        <NSpin v-if="isLoading" size="medium" />
        <span v-else class="placeholder-text">点击结果 Tab 以加载数据</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { NSpin } from 'naive-ui'
import { ref, onMounted, onBeforeUnmount, nextTick, shallowRef, watch } from 'vue'

import { EditorManager } from '@/extensions/builtin/workbench/manager/EditorManager'
import type {
  ResultSetMetadata,
  GridStateSnapshot,
} from '@/extensions/builtin/workbench/types/editor-types'

import GridToolbar from './GridToolbar.vue'

interface GridInstance {
  destroy(): void
  setGridOption(key: string, value: unknown): void
  getColumnState(): unknown[]
  getFilterModel(): unknown
  getSortModel(): unknown[]
}

interface AGGridOptionAPI {
  setGridOption(key: string, value: unknown): void
}

interface AGGridExportAPI {
  exportDataAsCsv(opts?: { fileName?: string }): void
}

interface PanelApi {
  isActive: boolean
}

const props = defineProps<{
  params: Record<string, unknown>
}>()

const emit = defineEmits<{
  close: []
}>()

const panelApi = (props.params.api as PanelApi) ?? null

const gridContainerRef = ref<HTMLElement | null>(null)
const gridInstance = shallowRef<GridInstance | null>(null)
const isGridReady = ref(false)
const isLoading = ref(false)

const metadata = ref<ResultSetMetadata>({
  id: String(props.params.resultSetId ?? ''),
  title: 'Result',
  columns: (props.params.columns as string[]) ?? [],
  totalRowCount: 0,
  elapsedMs: 0,
  affectedRows: 0,
  messages: '',
  sql: '',
  timestamp: Date.now(),
  dataSource: 'memory',
  duckdbTable: null,
  rows: (props.params.rows as unknown[][]) ?? [],
})

const savedGridState = ref<GridStateSnapshot | null>(null)

function saveGridState(): GridStateSnapshot | null {
  const inst = gridInstance.value
  if (!inst) return null
  try {
    return {
      columnState: inst.getColumnState() as Record<string, unknown>[],
      filterModel: inst.getFilterModel() as Record<string, unknown>,
      sortModel: inst.getSortModel() as Record<string, unknown>[],
    }
  } catch {
    return null
  }
}

async function createGrid(): Promise<void> {
  if (isGridReady.value) return
  isLoading.value = true

  const startTs = performance.now()

  await nextTick()

  const el = gridContainerRef.value
  if (!el) {
    isLoading.value = false
    return
  }

  try {
    const { createGrid: createAgGrid } = await import('ag-grid-community')

    const instance = createAgGrid(el as HTMLElement, {
      columnDefs: metadata.value.columns.map(c => ({ field: c, headerName: c })),
      rowData: (metadata.value.rows ?? []).map(r => {
        const obj: Record<string, unknown> = {}
        metadata.value.columns.forEach((c, i) => {
          obj[c] = r[i]
        })
        return obj
      }),
    })

    if (instance) {
      gridInstance.value = instance as unknown as GridInstance
      isGridReady.value = true

      const elapsed = Math.round(performance.now() - startTs)
      if (elapsed > 200) {
        console.warn(
          `[Perf] AG Grid create: ${elapsed}ms (>200ms target) | cols=${metadata.value.columns.length} rows=${(metadata.value.rows ?? []).length}`
        )
      } else {
        // eslint-disable-next-line no-console
        console.debug(
          `[Perf] AG Grid create: ${elapsed}ms | cols=${metadata.value.columns.length} rows=${(metadata.value.rows ?? []).length}`
        )
      }

      if (savedGridState.value) {
        const state = savedGridState.value
        if (state.columnState) {
          const api = instance as unknown as AGGridOptionAPI
          api.setGridOption('columnState', state.columnState)
        }
        if (state.filterModel) {
          const api = instance as unknown as AGGridOptionAPI
          api.setGridOption('filterModel', state.filterModel)
        }
        if (state.sortModel) {
          const api = instance as unknown as AGGridOptionAPI
          api.setGridOption('sortModel', state.sortModel)
        }
      }
    }
  } catch (err) {
    console.error('[FileResultPanel] Failed to create grid:', err)
  } finally {
    isLoading.value = false
  }
}

function destroyGrid(): void {
  if (isGridReady.value) {
    savedGridState.value = saveGridState()
  }

  if (gridInstance.value) {
    try {
      gridInstance.value.destroy()
    } catch {
      console.warn('[FileResultPanel] grid.destroy failed, may already be destroyed')
    }
    gridInstance.value = null
  }
  isGridReady.value = false
}

onMounted(() => {
  if (panelApi?.isActive) {
    createGrid()
  }
})

watch(
  () => panelApi?.isActive,
  (active, prev) => {
    if (active === prev) return
    if (active) {
      const filePath = String(props.params.filePath ?? '')
      const resultSetId = String(props.params.resultSetId ?? '')
      if (filePath && resultSetId) {
        const info = EditorManager.openFiles.get(filePath)
        if (info) {
          const idx = info.resultSets.findIndex(rs => rs.id === resultSetId)
          if (idx >= 0) {
            EditorManager.setActiveResultIndex(filePath, idx)
          }
        }
      }
      createGrid()
    } else {
      destroyGrid()
    }
  }
)

onBeforeUnmount(() => {
  destroyGrid()
})

function handleClose(): void {
  emit('close')
}

async function handleExportCsv(): Promise<void> {
  const inst = gridInstance.value
  if (!inst) return
  const gridApi = inst as unknown as AGGridExportAPI
  gridApi.exportDataAsCsv?.({ fileName: `result_${Date.now()}.csv` })
}

async function handleExportJson(): Promise<void> {
  const cols = metadata.value.columns
  const rows = metadata.value.rows ?? []
  const data = rows.map(r => {
    const obj: Record<string, unknown> = {}
    cols.forEach((c, i) => {
      obj[c] = r[i]
    })
    return obj
  })
  const json = JSON.stringify(data, null, 2)
  const blob = new Blob([json], { type: 'application/json' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = `result_${Date.now()}.json`
  a.click()
  URL.revokeObjectURL(url)
}
</script>

<style scoped>
.file-result-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
}

.result-grid-wrapper {
  flex: 1;
  overflow: hidden;
  position: relative;
}

.ag-grid-container {
  width: 100%;
  height: 100%;
}

.grid-placeholder {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: var(--n-text-color-disabled);
  font-size: 13px;
}
</style>
