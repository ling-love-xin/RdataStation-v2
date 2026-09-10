import { computed, type ComputedRef } from 'vue'

import type { ResultTab } from '../types/result'

export interface ColumnDiff {
  name: string
  inBoth: boolean
  onlyInA: boolean
  onlyInB: boolean
  typeA: string | null
  typeB: string | null
}

export interface RowDiff {
  status: 'unchanged' | 'added' | 'removed' | 'modified'
  rowA: Record<string, unknown> | null
  rowB: Record<string, unknown> | null
  key: string
}

export interface DiffResult {
  tabAName: string
  tabBName: string
  columns: ColumnDiff[]
  rows: RowDiff[]
  summary: DiffSummary
}

export interface DiffSummary {
  totalColumns: number
  commonColumns: number
  onlyAColumns: number
  onlyBColumns: number
  unchangedRows: number
  addedRows: number
  removedRows: number
  modifiedRows: number
}

function buildRowKey(row: Record<string, unknown>, columns: string[]): string {
  return columns.map(c => String(row[c.replace(/\./g, '_')] ?? '')).join('\x00')
}

export function useResultDiff(
  tabA: ComputedRef<ResultTab | null>,
  tabB: ComputedRef<ResultTab | null>,
  keyColumns: ComputedRef<string[]>
): ComputedRef<DiffResult | null> {
  return computed<DiffResult | null>(() => {
    const a = tabA.value
    const b = tabB.value
    if (!a || !b) return null
    if (a.objectRows.length === 0 && b.objectRows.length === 0) return null

    const aColSet = new Set(a.columns)
    const bColSet = new Set(b.columns)
    const allCols = [...new Set([...a.columns, ...b.columns])]

    const columnDiff: ColumnDiff[] = allCols.map(name => ({
      name,
      inBoth: aColSet.has(name) && bColSet.has(name),
      onlyInA: aColSet.has(name) && !bColSet.has(name),
      onlyInB: !aColSet.has(name) && bColSet.has(name),
      typeA: a.columns.includes(name) ? 'data' : null,
      typeB: b.columns.includes(name) ? 'data' : null,
    }))

    const commonCols = a.columns.filter(c => bColSet.has(c))
    const keys =
      keyColumns.value.length > 0
        ? keyColumns.value.filter(k => commonCols.includes(k))
        : commonCols.slice(0, Math.min(2, commonCols.length))

    const aKeyMap = new Map<string, Record<string, unknown>>()
    for (const row of a.objectRows) {
      const key = buildRowKey(row, keys)
      if (!aKeyMap.has(key)) {
        aKeyMap.set(key, row)
      }
    }

    const bKeyMap = new Map<string, Record<string, unknown>>()
    for (const row of b.objectRows) {
      const key = buildRowKey(row, keys)
      if (!bKeyMap.has(key)) {
        bKeyMap.set(key, row)
      }
    }

    const allKeys = new Set([...aKeyMap.keys(), ...bKeyMap.keys()])

    const rowDiff: RowDiff[] = []
    let unchanged = 0
    let added = 0
    let removed = 0
    let modified = 0

    for (const key of allKeys) {
      const rowInA = aKeyMap.get(key) ?? null
      const rowInB = bKeyMap.get(key) ?? null

      if (rowInA && rowInB) {
        const isModified = commonCols.some(col => {
          const field = col.replace(/\./g, '_')
          const valA = rowInA[field]
          const valB = rowInB[field]
          return JSON.stringify(valA) !== JSON.stringify(valB)
        })
        if (isModified) {
          rowDiff.push({ status: 'modified', rowA: rowInA, rowB: rowInB, key })
          modified++
        } else {
          rowDiff.push({ status: 'unchanged', rowA: rowInA, rowB: rowInB, key })
          unchanged++
        }
      } else if (rowInA) {
        rowDiff.push({ status: 'removed', rowA: rowInA, rowB: null, key })
        removed++
      } else if (rowInB) {
        rowDiff.push({ status: 'added', rowA: null, rowB: rowInB, key })
        added++
      }
    }

    return {
      tabAName: a.title,
      tabBName: b.title,
      columns: columnDiff,
      rows: rowDiff,
      summary: {
        totalColumns: allCols.length,
        commonColumns: commonCols.length,
        onlyAColumns: a.columns.filter(c => !bColSet.has(c)).length,
        onlyBColumns: b.columns.filter(c => !aColSet.has(c)).length,
        unchangedRows: unchanged,
        addedRows: added,
        removedRows: removed,
        modifiedRows: modified,
      },
    }
  })
}
