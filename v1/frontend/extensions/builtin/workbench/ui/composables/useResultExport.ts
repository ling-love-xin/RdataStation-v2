import { save } from '@tauri-apps/plugin-dialog'
import { type ComputedRef, type Ref } from 'vue'

import { copyToClipboard } from '@/shared/utils/clipboard'

import {
  createDuckdbTempTable as apiCreateTempTable,
  exportResultToFile as apiExportResult,
} from '../services/result-analysis'

import type { ResultTab } from '../types/result'
import type { GridApi } from 'ag-grid-community'

export function useResultExport(
  activeTab: ComputedRef<ResultTab | null>,
  gridApi: Ref<GridApi | null>,
  rowData: ComputedRef<Record<string, unknown>[]>,
  message: {
    success: (msg: string) => void
    error: (msg: string) => void
    loading: (msg: string, opts?: Record<string, unknown>) => { destroy: () => void }
    info: (msg: string) => void
  }
) {
  function generateCsv(): string {
    const tab = activeTab.value
    if (!tab) return ''
    const header = tab.columns.map(c => `"${c.replace(/"/g, '""')}"`).join(',')
    const body = tab.rows
      .map(r => r.map(v => (v === null ? '' : `"${String(v).replace(/"/g, '""')}"`)).join(','))
      .join('\n')
    return header + '\n' + body
  }

  function generateJson(): string {
    const tab = activeTab.value
    if (!tab) return ''
    const data = tab.rows.map(r => {
      const obj: Record<string, unknown> = {}
      tab.columns.forEach((c, i) => {
        obj[c] = r[i]
      })
      return obj
    })
    return JSON.stringify(data, null, 2)
  }

  function copyRowsAsInsert(): void {
    if (!activeTab.value) return
    const cols = activeTab.value.columns
    const tableName = activeTab.value.tableName || 'result'
    const rows = gridApi.value?.getSelectedRows() || rowData.value
    const inserts = rows.map((row: Record<string, unknown>) => {
      const vals = cols
        .map(c => {
          const v = row[c]
          if (v === null || v === undefined) return 'NULL'
          if (typeof v === 'number') return String(v)
          return `'${String(v).replace(/'/g, "''")}'`
        })
        .join(', ')
      return `INSERT INTO ${tableName} (${cols.join(', ')}) VALUES (${vals});`
    })
    copyToClipboard(inserts.join('\n'))
  }

  async function handleExport(format: string): Promise<void> {
    const tab = activeTab.value
    if (!tab) return

    if (format === 'csv') {
      if (!gridApi.value) return
      gridApi.value.exportDataAsCsv({ fileName: `result_${Date.now()}.csv`, columnSeparator: ',' })
      message.success('已导出 CSV')
      return
    }
    if (format === 'json') {
      const data = JSON.stringify(rowData.value, null, 2)
      const blob = new Blob([data], { type: 'application/json' })
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = `result_${Date.now()}.json`
      document.body.appendChild(a)
      a.click()
      document.body.removeChild(a)
      URL.revokeObjectURL(url)
      message.success('已导出 JSON')
      return
    }
    if (format === 'insert') {
      copyRowsAsInsert()
      message.info('INSERT SQL 已复制到剪贴板')
      return
    }

    const ext = format === 'parquet' ? 'parquet' : 'xlsx'
    const filterLabel = format === 'parquet' ? 'Parquet' : 'Excel'
    const filePath = await save({
      defaultPath: `result_${Date.now()}.${ext}`,
      filters: [{ name: `${filterLabel} 文件`, extensions: [ext] }],
    })
    if (!filePath) return

    const loadingMsg = message.loading(`正在导出 ${filterLabel}...`, { duration: 0 })
    let tempTable = tab.duckdbTempTable
    if (!tempTable) {
      try {
        tempTable = await apiCreateTempTable(tab.columns, tab.rows)
      } catch (err: unknown) {
        loadingMsg.destroy()
        const msg = err instanceof Error ? err.message : String(err)
        message.error(`创建临时表失败: ${msg}`)
        return
      }
    }

    try {
      await apiExportResult({
        temp_table: tempTable,
        file_path: filePath,
        format,
      })
      loadingMsg.destroy()
      message.success(`已导出到 ${filePath}`)
    } catch (err: unknown) {
      loadingMsg.destroy()
      const msg = err instanceof Error ? err.message : String(err)
      message.error(`导出失败: ${msg}`)
    }
  }

  return {
    generateCsv,
    generateJson,
    copyRowsAsInsert,
    handleExport,
  }
}
