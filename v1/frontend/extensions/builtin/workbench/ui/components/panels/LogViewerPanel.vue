<template>
  <div class="log-viewer-panel">
    <!-- 工具栏 -->
    <div class="log-toolbar">
      <div class="log-toolbar-left">
        <NSelect
          v-model:value="selectedLevel"
          :options="levelOptions as any"
          placeholder="全部级别"
          size="small"
          clearable
          style="width: 120px"
          @update:value="onLevelChange"
        />
        <NInput
          v-model:value="keyword"
          placeholder="搜索日志..."
          size="small"
          clearable
          style="width: 200px"
          @keyup.enter="onSearch"
        >
          <template #suffix>
            <NButton size="tiny" quaternary @click="onSearch">
              <template #icon>
                <Search :size="14" />
              </template>
            </NButton>
          </template>
        </NInput>
      </div>
      <div class="log-toolbar-right">
        <NSpace :size="6">
          <NButton size="tiny" quaternary @click="toggleAutoRefresh">
            <template #icon>
              <RefreshCw :size="14" :class="{ 'auto-refresh-spin': autoRefreshActive }" />
            </template>
          </NButton>
          <NButton size="tiny" quaternary @click="onExport">
            <template #icon>
              <Download :size="14" />
            </template>
          </NButton>
          <NTag v-if="store.stats" type="info" size="small" :bordered="false">
            共 {{ store.stats.total.toLocaleString() }} 条
          </NTag>
          <NTag v-if="store.errorCount > 0" type="error" size="small" :bordered="false">
            ERR {{ store.errorCount }}
          </NTag>
          <NTag v-if="store.warnCount > 0" type="warning" size="small" :bordered="false">
            WRN {{ store.warnCount }}
          </NTag>
          <NButton size="tiny" quaternary @click="onClearLogs">
            <template #icon>
              <Trash2 :size="14" />
            </template>
          </NButton>
        </NSpace>
      </div>
    </div>

    <!-- AG Grid 表格 -->
    <div class="log-grid-wrapper">
      <AgGridVue
        class="ag-theme-alpine"
        :class="{ 'ag-theme-alpine-dark': isDark }"
        :column-defs="columnDefs"
        :row-data="rowData"
        :default-col-def="defaultColDef"
        :pagination="false"
        :enable-cell-text-selection="true"
        :suppress-row-click-selection="true"
        :overlay-no-rows-template="'暂无日志记录'"
        style="width: 100%; height: 100%"
        @row-double-clicked="onRowDoubleClicked"
      />
    </div>

    <!-- 底部分页栏 -->
    <div class="log-pagination-bar">
      <div class="log-pagination-left">
        <span class="log-pagination-info">
          {{
            store.total > 0
              ? `${(store.page - 1) * store.pageSize + 1}-${Math.min(store.page * store.pageSize, store.total)} / ${store.total}`
              : '暂无记录'
          }}
        </span>
        <NSelect
          v-model:value="currentPageSize"
          :options="pageSizeOptions"
          size="tiny"
          style="width: 80px"
          @update:value="onPageSizeChange"
        />
      </div>
      <NPagination
        v-model:page="currentPage"
        :page-count="store.totalPages"
        :page-slot="7"
        size="small"
        @update:page="onPageChange"
      />
    </div>

    <!-- 详情弹窗 -->
    <NModal v-model:show="showDetail" preset="card" title="日志详情" style="width: 600px">
      <div v-if="selectedRecord" class="log-detail">
        <div class="log-detail-row">
          <span class="log-detail-label">时间</span>
          <span class="log-detail-value">{{ selectedRecord.timestamp }}</span>
        </div>
        <div class="log-detail-row">
          <span class="log-detail-label">级别</span>
          <NTag :type="getLevelType(selectedRecord.level)" size="small">
            {{ selectedRecord.level }}
          </NTag>
        </div>
        <div class="log-detail-row">
          <span class="log-detail-label">模块</span>
          <span class="log-detail-value">{{ selectedRecord.target }}</span>
        </div>
        <div class="log-detail-row">
          <span class="log-detail-label">位置</span>
          <span class="log-detail-value">{{ selectedRecord.file }}:{{ selectedRecord.line }}</span>
        </div>
        <div class="log-detail-row">
          <span class="log-detail-label">会话</span>
          <span class="log-detail-value log-detail-mono">{{ selectedRecord.session_id }}</span>
        </div>
        <div class="log-detail-row">
          <span class="log-detail-label">消息</span>
          <pre class="log-detail-message">{{ selectedRecord.message }}</pre>
        </div>
      </div>
    </NModal>
  </div>
</template>

<script setup lang="ts">
import { ClientSideRowModelModule, ModuleRegistry } from 'ag-grid-community'
import { AgGridVue } from 'ag-grid-vue3'
import { Search, RefreshCw, Trash2, Download } from 'lucide-vue-next'
import {
  NSelect,
  NInput,
  NButton,
  NTag,
  NSpace,
  NModal,
  NPagination,
  createDiscreteApi,
} from 'naive-ui'
import { ref, computed, onMounted, onUnmounted, h, watch } from 'vue'

import { useLogStore } from '@/extensions/builtin/workbench/ui/stores/log-store'
import { useUiStore } from '@/shared/stores/ui'
import type { LogLevel, LogRecord } from '@/shared/types/logging'

import type { RowDoubleClickedEvent, ICellRendererParams } from 'ag-grid-community'

ModuleRegistry.registerModules([ClientSideRowModelModule])

const store = useLogStore()
const uiStore = useUiStore()
const { dialog, notification } = createDiscreteApi(['dialog', 'notification'])

const isDark = computed(() => uiStore.isDark)
const selectedLevel = ref<string | null>(null)
const keyword = ref('')
const selectedRecord = ref<LogRecord | null>(null)
const showDetail = ref(false)
const autoRefreshActive = ref(false)
let autoRefreshTimer: ReturnType<typeof setInterval> | null = null

const currentPage = ref(1)
const currentPageSize = ref(50)

const pageSizeOptions = [
  { label: '25条/页', value: 25 },
  { label: '50条/页', value: 50 },
  { label: '100条/页', value: 100 },
  { label: '200条/页', value: 200 },
]

const levelOptions: Array<{ label: string; value: string | null; type?: 'group' | 'ignored' }> = [
  { label: '全部级别', value: null },
  { label: 'TRACE', value: 'TRACE' },
  { label: 'DEBUG', value: 'DEBUG' },
  { label: 'INFO', value: 'INFO' },
  { label: 'WARN', value: 'WARN' },
  { label: 'ERROR', value: 'ERROR' },
]

function getLevelType(level: string): 'default' | 'info' | 'success' | 'warning' | 'error' {
  const map: Record<string, 'default' | 'info' | 'success' | 'warning' | 'error'> = {
    TRACE: 'default',
    DEBUG: 'info',
    INFO: 'success',
    WARN: 'warning',
    ERROR: 'error',
  }
  return map[level] || 'default'
}

const columnDefs = [
  {
    field: 'level',
    headerName: '级别',
    width: 72,
    sortable: true,
    filter: true,
    cellRenderer: (params: ICellRendererParams) => {
      const type = getLevelType(params.value)
      return h(NTag, { type, size: 'tiny', bordered: false }, { default: () => params.value })
    },
  },
  {
    field: 'timestamp',
    headerName: '时间',
    width: 185,
    sortable: true,
    filter: 'agTextColumnFilter',
  },
  {
    field: 'target',
    headerName: '模块',
    width: 220,
    sortable: true,
    filter: 'agTextColumnFilter',
  },
  {
    field: 'message',
    headerName: '消息',
    flex: 1,
    minWidth: 300,
    sortable: true,
    filter: 'agTextColumnFilter',
    cellStyle: { whiteSpace: 'normal', wordBreak: 'break-word' },
    autoHeight: true,
  },
]

const defaultColDef = {
  sortable: true,
  resizable: true,
  filter: true,
  suppressMenu: false,
}

const rowData = computed(() => store.records)

function onLevelChange(level: string | null) {
  currentPage.value = 1
  store.setLevelFilter(level as LogLevel | null)
}

function onSearch() {
  currentPage.value = 1
  if (keyword.value.trim()) {
    store.searchLogs(keyword.value.trim())
  } else {
    store.loadLogs()
  }
}

function onPageChange(page: number) {
  currentPage.value = page
  store.goToPage(page)
}

function onPageSizeChange(size: number) {
  currentPageSize.value = size
  currentPage.value = 1
  store.pageSize = size
  store.loadLogs()
}

function toggleAutoRefresh() {
  autoRefreshActive.value = !autoRefreshActive.value
  if (autoRefreshActive.value) {
    autoRefreshTimer = setInterval(() => {
      store.loadStats()
      store.loadLogs()
    }, 5000)
  } else {
    if (autoRefreshTimer) {
      clearInterval(autoRefreshTimer)
      autoRefreshTimer = null
    }
  }
}

function onClearLogs() {
  dialog.warning({
    title: '清理日志',
    content: '确认清理所有日志记录？此操作不可撤销。',
    positiveText: '确认清理',
    negativeText: '取消',
    onPositiveClick: async () => {
      await store.clearLogs()
    },
  })
}

function onRowDoubleClicked(event: RowDoubleClickedEvent) {
  selectedRecord.value = event.data as LogRecord
  showDetail.value = true
}

async function onExport() {
  try {
    const level = selectedLevel.value ?? undefined
    const records = await store.exportLogs(level as LogLevel | undefined)
    if (!records || records.length === 0) {
      notification.warning({ title: '导出', content: '没有可导出的日志记录' })
      return
    }
    const json = JSON.stringify(records, null, 2)
    const blob = new Blob([json], { type: 'application/json' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = `rdata-logs-${new Date().toISOString().slice(0, 19).replace(/:/g, '-')}.json`
    a.click()
    URL.revokeObjectURL(url)
    notification.success({ title: '导出成功', content: `已导出 ${records.length} 条日志` })
  } catch (e) {
    notification.error({ title: '导出失败', content: e instanceof Error ? e.message : '未知错误' })
  }
}

watch(
  () => store.page,
  p => {
    currentPage.value = p
  }
)
watch(
  () => store.pageSize,
  s => {
    currentPageSize.value = s
  }
)
watch(
  () => store.error,
  err => {
    if (err) {
      notification.error({ title: '日志操作失败', content: err, duration: 4000 })
    }
  }
)

onMounted(() => {
  store.initSession()
  store.loadStats()
  store.loadLogs()
  currentPage.value = store.page
  currentPageSize.value = store.pageSize
})

onUnmounted(() => {
  if (autoRefreshTimer) {
    clearInterval(autoRefreshTimer)
    autoRefreshTimer = null
  }
})
</script>

<style scoped>
.log-viewer-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg-primary);
}

.log-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 8px;
  border-bottom: 1px solid var(--border-color);
  flex-shrink: 0;
}

.log-toolbar-left {
  display: flex;
  align-items: center;
  gap: 8px;
}

.log-toolbar-right {
  display: flex;
  align-items: center;
}

.log-grid-wrapper {
  flex: 1;
  overflow: hidden;
}

.log-pagination-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 12px;
  border-top: 1px solid var(--border-color);
  flex-shrink: 0;
  background: var(--bg-primary);
}

.log-pagination-left {
  display: flex;
  align-items: center;
  gap: 10px;
}

.log-pagination-info {
  font-size: 12px;
  color: var(--text-secondary);
  white-space: nowrap;
}

.auto-refresh-spin {
  animation: spin 1s linear infinite;
}

@keyframes spin {
  from {
    transform: rotate(0deg);
  }
  to {
    transform: rotate(360deg);
  }
}

.log-detail {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.log-detail-row {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.log-detail-label {
  font-size: 12px;
  color: var(--text-secondary);
  font-weight: 500;
}

.log-detail-value {
  font-size: 13px;
  color: var(--text-primary);
}

.log-detail-mono {
  font-family: monospace;
  font-size: 11px;
  color: var(--text-secondary);
  word-break: break-all;
}

.log-detail-message {
  margin: 0;
  padding: 8px 12px;
  background: var(--bg-secondary);
  border-radius: 4px;
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-word;
  max-height: 200px;
  overflow-y: auto;
}
</style>
