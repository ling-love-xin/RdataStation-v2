import { computed, readonly, ref, shallowRef, watch, type ComputedRef } from 'vue'

import type { ResultTab } from '../types/result'
import type { ColDef, GridApi, ValueGetterParams, ColumnState } from 'ag-grid-community'

export interface UseGridConfigOptions {
  activeTab: ComputedRef<ResultTab | null>
  editable?: boolean
}

const NUMERIC_HEURISTICS = [
  'id',
  '_id',
  'count',
  'num',
  'year',
  'age',
  'price',
  'amount',
  'total',
  'qty',
  'rate',
]

export function isLikelyNumeric(colName: string): boolean {
  const lower = colName.toLowerCase()
  return NUMERIC_HEURISTICS.some(p => lower.includes(p) || lower.endsWith(p))
}

function isLikelyDate(colName: string): boolean {
  const lower = colName.toLowerCase()
  return lower.includes('date') || lower.includes('time') || lower.endsWith('_at')
}

function isLikelyLongText(colName: string): boolean {
  const lower = colName.toLowerCase()
  return (
    lower.includes('description') ||
    lower.includes('content') ||
    lower.includes('comment') ||
    lower.includes('note') ||
    lower.includes('text')
  )
}

function nullCellRenderer() {
  const span = document.createElement('span')
  span.className = 'null-value'
  span.textContent = 'NULL'
  return span
}

function buildColumnDefs(tab: ResultTab, editable: boolean): ColDef[] {
  if (tab.columns.length === 0) {
    return [{ field: '__placeholder', headerName: '', hide: true }]
  }

  const dataCols: ColDef[] = tab.columns.map(col => {
    const numeric = isLikelyNumeric(col)
    const isDate = isLikelyDate(col)
    const longText = isLikelyLongText(col)

    return {
      field: col,
      headerName: col,
      headerTooltip: col,
      sortable: true,
      filter: true,
      resizable: true,
      minWidth: 80,
      width: numeric ? 110 : isDate ? 140 : longText ? 200 : 130,
      flex: longText ? 2 : numeric ? 0 : 1,
      cellClass: numeric ? 'text-right' : undefined,
      editable,
      cellRenderer: (params: { value: unknown }) => {
        const v = params.value
        if (v === null || v === undefined) return '<span class="null-value">NULL</span>'
        if (typeof v === 'object') {
          try {
            return JSON.stringify(v)
          } catch {
            return String(v)
          }
        }
        const str = String(v)
        if (str.length > 500) return str.substring(0, 200) + '...'
        return str
      },
      cellRendererSelector: (params: { value: unknown }) => {
        if (params.value === null || params.value === undefined) {
          return { component: nullCellRenderer }
        }
        return undefined
      },
      comparator: (a: unknown, b: unknown) => {
        if (a === null && b === null) return 0
        if (a === null) return 1
        if (b === null) return -1
        if (typeof a === 'number' && typeof b === 'number') return a - b
        return String(a).localeCompare(String(b))
      },
    }
  })

  return [
    {
      field: '__rowNumber',
      headerName: '#',
      width: 55,
      pinned: 'left',
      sortable: false,
      filter: false,
      resizable: false,
      valueGetter: (p: ValueGetterParams) => {
        const rowIndex = p.node?.rowIndex ?? 0
        if (!p.api?.paginationGetCurrentPage) return rowIndex + 1
        return p.api.paginationGetCurrentPage() * p.api.paginationGetPageSize() + rowIndex + 1
      },
      cellStyle: {
        textAlign: 'center',
        color: 'var(--text-tertiary)',
        fontSize: '11px',
        background: 'var(--bg-secondary)',
      },
    },
    ...dataCols,
  ]
}

export function useGridConfig(options: UseGridConfigOptions) {
  const { activeTab, editable = true } = options

  const gridApi = ref<GridApi | null>(null)
  const columnDefs = shallowRef<ColDef[]>([])
  let columnsKey = ''

  function refreshColumns(tab: ResultTab | null): void {
    if (!tab || tab.columns.length === 0) {
      const key = '__empty'
      if (columnsKey !== key) {
        columnsKey = key
        columnDefs.value = [{ field: '__placeholder', headerName: '', hide: true }]
      }
      return
    }

    const key = tab.columns.join('|')
    if (columnsKey !== key) {
      columnsKey = key
      columnDefs.value = buildColumnDefs(tab, editable)
    }
  }

  watch(
    () => {
      const tab = activeTab.value
      if (!tab) return '__none'
      return tab.columns.join('|')
    },
    key => {
      if (key !== columnsKey && key !== '__none') {
        refreshColumns(activeTab.value)
      }
    },
    { immediate: true }
  )

  function onGridReady(params: { api: GridApi }): void {
    gridApi.value = params.api
  }

  const defaultColDef: ColDef = {
    sortable: true,
    resizable: true,
  }

  const paginationEnabled = ref(true)
  const paginationAutoThreshold = 100
  const paginationPageSelector = [25, 50, 100, 200, 500, 1000]

  const pagination = computed(() => {
    const tab = activeTab.value
    if (!tab) return false
    if (!paginationEnabled.value) return false
    return tab.displayedRowCount > paginationAutoThreshold ? ('bottom' as const) : false
  })

  const rowData = computed<Record<string, unknown>[]>(() => {
    const tab = activeTab.value
    if (!tab) return []
    return tab.objectRows
  })

  const PAGE_SIZE_KEY_PREFIX = 'rds_grid_psize_'

  const paginationPageSize = computed(() => {
    const tab = activeTab.value
    if (!tab) return 100
    try {
      const saved = localStorage.getItem(`${PAGE_SIZE_KEY_PREFIX}${tab.id}`)
      const parsed = saved ? parseInt(saved, 10) : NaN
      if (!isNaN(parsed) && parsed > 0 && paginationPageSelector.includes(parsed)) return parsed
    } catch {
      //
    }
    return 100
  })

  function savePageSize(): void {
    const tab = activeTab.value
    if (!tab || !gridApi.value) return
    try {
      const currentSize = gridApi.value.paginationGetPageSize()
      if (currentSize) {
        localStorage.setItem(`${PAGE_SIZE_KEY_PREFIX}${tab.id}`, String(currentSize))
      }
    } catch {
      //
    }
  }

  const COLUMN_STATE_PREFIX = 'rds_grid_cols_'

  function saveColumnState(tabId: string): void {
    try {
      const state = gridApi.value?.getColumnState()
      if (!state) return
      localStorage.setItem(`${COLUMN_STATE_PREFIX}${tabId}`, JSON.stringify(state))
    } catch {
      //
    }
  }

  function restoreColumnState(tabId: string): void {
    try {
      const raw = localStorage.getItem(`${COLUMN_STATE_PREFIX}${tabId}`)
      if (!raw) return
      const state = JSON.parse(raw) as ColumnState[]
      if (!Array.isArray(state) || state.length === 0) return
      gridApi.value?.applyColumnState({ state, applyOrder: true })
    } catch {
      //
    }
  }

  function clearColumnState(tabId: string): void {
    try {
      localStorage.removeItem(`${COLUMN_STATE_PREFIX}${tabId}`)
    } catch {
      //
    }
  }

  return {
    columnDefs: readonly(columnDefs),
    defaultColDef,
    pagination,
    paginationEnabled,
    paginationPageSelector,
    paginationPageSize,
    rowData,
    gridApi,
    onGridReady,
    savePageSize,
    saveColumnState,
    restoreColumnState,
    clearColumnState,
  }
}
