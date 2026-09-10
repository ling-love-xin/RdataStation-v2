<template>
  <div class="data-preview">
    <PreviewToolbar
      :connection-id="connectionId"
      :schema-name="schemaName"
      :table-name="tableName"
      :is-loading="isLoading"
      :row-count="rowCount"
      :current-page="currentPage"
      :total-pages="totalPages"
      @refresh="handleRefresh"
      @export="handleExport"
      @copy="handleCopy"
      @page-change="handlePageChange"
      @close="$emit('close')"
    />

    <div v-if="isLoading" class="preview-loading">
      <Loader2 :size="24" class="spinning" />
      <span>{{ t('dataPreview.loading') }}</span>
    </div>

    <div v-else-if="error" class="preview-error">
      <AlertCircle :size="24" />
      <span>{{ error }}</span>
      <button @click="handleRefresh">{{ t('dataPreview.retry') }}</button>
    </div>

    <PreviewTable
      v-else
      :columns="columns"
      :data="tableData"
      :sort-field="sortField"
      :sort-order="sortOrder"
      @sort="handleSort"
      @filter="handleFilter"
    />

    <PreviewPagination
      v-if="totalPages > 1"
      :current-page="currentPage"
      :total-pages="totalPages"
      :page-size="pageSize"
      @change="handlePageChange"
    />
  </div>
</template>

<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { Loader2, AlertCircle } from 'lucide-vue-next'
import { ref, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

import PreviewPagination from './PreviewPagination.vue'
import PreviewTable from './PreviewTable.vue'
import PreviewToolbar from './PreviewToolbar.vue'

interface QueryResult {
  columns: Array<{ name: string; dataType: string }>
  rows: Record<string, unknown>[]
  rowCount?: number
}

interface Props {
  connectionId: string
  schemaName: string
  tableName: string
  objectType?: 'table' | 'view'
}

const props = withDefaults(defineProps<Props>(), {
  objectType: 'table',
})

defineEmits<{
  close: []
}>()

const isLoading = ref(false)
const error = ref<string | null>(null)
const tableData = ref<Record<string, unknown>[]>([])
const columns = ref<Array<{ key: string; title: string; dataType: string }>>([])
const rowCount = ref(0)
const currentPage = ref(1)
const totalPages = ref(1)
const pageSize = ref(100)
const sortField = ref<string | null>(null)
const sortOrder = ref<'asc' | 'desc' | null>(null)
const filterCondition = ref<string | null>(null)

async function loadData() {
  isLoading.value = true
  error.value = null

  try {
    const offset = (currentPage.value - 1) * pageSize.value
    let sql = `SELECT * FROM ${props.schemaName}.${props.tableName}`

    if (filterCondition.value) {
      sql += ` WHERE ${filterCondition.value}`
    }

    if (sortField.value && sortOrder.value) {
      sql += ` ORDER BY ${sortField.value} ${sortOrder.value.toUpperCase()}`
    }

    sql += ` LIMIT ${pageSize.value} OFFSET ${offset};`

    const result = await invoke<QueryResult>('execute_query', {
      connectionId: props.connectionId,
      sql,
    })

    if (result.columns && result.rows) {
      columns.value = result.columns.map((col: { name: string; dataType: string }) => ({
        key: col.name,
        title: col.name,
        dataType: col.dataType,
      }))

      tableData.value = result.rows
      rowCount.value = result.rowCount || result.rows.length
      totalPages.value = Math.ceil(rowCount.value / pageSize.value)
    }
  } catch (e) {
    error.value = e instanceof Error ? e.message : t('dataPreview.loadFailed')
    console.error('加载预览数据失败:', e)
  } finally {
    isLoading.value = false
  }
}

function handleRefresh() {
  currentPage.value = 1
  loadData()
}

function handlePageChange(page: number) {
  currentPage.value = page
  loadData()
}

function handleSort(field: string, order: 'asc' | 'desc') {
  sortField.value = field
  sortOrder.value = order
  currentPage.value = 1
  loadData()
}

function handleFilter(condition: string) {
  filterCondition.value = condition
  currentPage.value = 1
  loadData()
}

function handleExport() {}

function handleCopy() {
  const text = JSON.stringify(tableData.value, null, 2)
  navigator.clipboard.writeText(text)
}

onMounted(() => {
  loadData()
})
</script>

<style scoped>
.data-preview {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg-primary);
}

.preview-loading,
.preview-error {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  padding: 48px;
  color: var(--text-secondary);
}

.preview-error {
  color: var(--danger-color);
}

.preview-error button {
  padding: 6px 16px;
  border: 1px solid var(--border-color);
  border-radius: 4px;
  background: var(--bg-secondary);
  color: var(--text-primary);
  cursor: pointer;
}

.preview-error button:hover {
  background: var(--bg-tertiary);
}

.spinning {
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
</style>
