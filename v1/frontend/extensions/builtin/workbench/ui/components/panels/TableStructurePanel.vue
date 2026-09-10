<template>
  <div class="table-structure-panel">
    <!-- 表信息头部 -->
    <div class="table-header">
      <div class="table-title">
        <Table2 :size="20" />
        <span class="table-name">{{ tableName }}</span>
        <NTag v-if="schemaName" size="small" type="info">{{ schemaName }}</NTag>
        <NTag v-if="databaseName" size="small">{{ databaseName }}</NTag>
      </div>
      <div class="table-actions">
        <NButtonGroup size="small">
          <NButton :title="t('workbench.viewData')" @click="handleViewData">
            <template #icon>
              <Eye :size="14" />
            </template>
            {{ t('workbench.viewData') }}
          </NButton>
          <NButton :title="t('workbench.generateQuery')" @click="handleGenerateSelect">
            <template #icon>
              <Code :size="14" />
            </template>
            {{ t('workbench.generateQuery') }}
          </NButton>
        </NButtonGroup>
      </div>
    </div>

    <!-- 标签页 -->
    <NTabs v-model:value="activeTab" type="line" size="small" class="structure-tabs">
      <NTabPane name="columns" :tab="t('workbench.columnsTab')">
        <div class="tab-content">
          <NDataTable
            :columns="columnColumns"
            :data="columns"
            :loading="loading"
            size="small"
            :pagination="false"
            :bordered="false"
          />
        </div>
      </NTabPane>

      <NTabPane name="indexes" :tab="t('workbench.indexesTab')">
        <div class="tab-content">
          <NEmpty v-if="indexes.length === 0" :description="t('workbench.noIndexes')" />
          <NDataTable
            v-else
            :columns="indexColumns"
            :data="indexes"
            size="small"
            :pagination="false"
            :bordered="false"
          />
        </div>
      </NTabPane>

      <NTabPane name="constraints" :tab="t('workbench.constraintsTab')">
        <div class="tab-content">
          <NEmpty v-if="constraints.length === 0" :description="t('workbench.noConstraints')" />
          <NDataTable
            v-else
            :columns="constraintColumns"
            :data="constraints"
            size="small"
            :pagination="false"
            :bordered="false"
          />
        </div>
      </NTabPane>

      <NTabPane name="ddl" :tab="t('workbench.ddlTab')">
        <div class="tab-content">
          <NCode :code="ddl" language="sql" show-line-numbers />
        </div>
      </NTabPane>
    </NTabs>
  </div>
</template>

<script setup lang="ts">
import { Table2, Eye, Code } from 'lucide-vue-next'
import {
  NTag,
  NTabs,
  NTabPane,
  NDataTable,
  NButton,
  NButtonGroup,
  NEmpty,
  NCode,
  createDiscreteApi,
} from 'naive-ui'
import { ref, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'

import { navigatorApi } from '@/shared/api'

interface Props {
  connectionId: string
  databaseName: string
  schemaName?: string
  tableName: string
}

const props = defineProps<Props>()

const { t } = useI18n()
const { message } = createDiscreteApi(['message'])

const emit = defineEmits<{
  viewData: [tableName: string]
  generateSelect: [tableName: string, columns: string[]]
}>()

interface ColumnInfo {
  name: string
  dataType: string
  nullable: string
  defaultValue: string
  comment: string
}

interface IndexInfo {
  name: string
  type: string
  columns: string
}

interface ConstraintInfo {
  name: string
  type: string
  columns: string
}

// 状态
const loading = ref(false)
const activeTab = ref('columns')
const columns = ref<ColumnInfo[]>([])
const indexes = ref<IndexInfo[]>([])
const constraints = ref<ConstraintInfo[]>([])
const ddl = ref('')

// 列定义
const columnColumns = [
  { title: t('workbench.columnName'), key: 'name', width: 150 },
  { title: t('workbench.dataType'), key: 'dataType', width: 120 },
  { title: t('workbench.nullable'), key: 'nullable', width: 60 },
  { title: t('workbench.defaultValue'), key: 'defaultValue', width: 120 },
  { title: t('workbench.comment'), key: 'comment', ellipsis: { tooltip: true } },
]

const indexColumns = [
  { title: t('workbench.indexName'), key: 'name', width: 150 },
  { title: t('workbench.indexType'), key: 'type', width: 100 },
  { title: t('workbench.indexColumns'), key: 'columns', ellipsis: { tooltip: true } },
  { title: t('workbench.unique'), key: 'unique', width: 60 },
]

const constraintColumns = [
  { title: t('workbench.constraintName'), key: 'name', width: 150 },
  { title: t('workbench.constraintType'), key: 'type', width: 100 },
  { title: t('workbench.constraintColumns'), key: 'columns', ellipsis: { tooltip: true } },
]

// 加载表结构
async function loadTableStructure() {
  loading.value = true
  try {
    const schema = props.schemaName || 'public'

    // 获取列信息
    const columnNodes = await navigatorApi.getColumns(
      props.connectionId,
      props.databaseName,
      schema,
      props.tableName
    )

    columns.value = columnNodes.map(col => ({
      name: col.name,
      dataType: String(col.metadata?.dataType || 'unknown'),
      nullable: col.metadata?.nullable ? t('workbench.yes') : t('workbench.no'),
      defaultValue: String(col.metadata?.defaultValue || ''),
      comment: String(col.metadata?.comment || ''),
    }))

    // 生成 DDL
    generateDDL()
  } catch (error) {
    console.error('Failed to load table structure:', error)
    message.error(t('workbench.loadTableStructureFailed'))
  } finally {
    loading.value = false
  }
}

// 生成 DDL
function generateDDL() {
  const columnDefs = columns.value
    .map(col => {
      let def = `  ${col.name} ${col.dataType}`
      if (!col.nullable) def += ' NOT NULL'
      if (col.defaultValue) def += ` DEFAULT ${col.defaultValue}`
      return def
    })
    .join(',\n')

  ddl.value = `CREATE TABLE ${props.tableName} (\n${columnDefs}\n);`
}

// 查看数据
function handleViewData() {
  emit('viewData', props.tableName)
}

// 生成查询
function handleGenerateSelect() {
  const columnNames = columns.value.map(col => col.name)
  emit('generateSelect', props.tableName, columnNames)
}

onMounted(() => {
  loadTableStructure()
})
</script>

<style scoped>
.table-structure-panel {
  height: 100%;
  display: flex;
  flex-direction: column;
  background-color: var(--bg-primary);
}

.table-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 16px;
  border-bottom: 1px solid var(--border-color);
  background-color: var(--bg-secondary);
}

.table-title {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 16px;
  font-weight: 500;
}

.table-name {
  color: var(--text-primary);
}

.table-actions {
  display: flex;
  gap: 8px;
}

.structure-tabs {
  flex: 1;
  overflow: hidden;
}

.structure-tabs :deep(.n-tabs-nav) {
  padding: 0 16px;
  background-color: var(--bg-secondary);
  border-bottom: 1px solid var(--border-color);
}

.structure-tabs :deep(.n-tabs-pane-wrapper) {
  height: calc(100% - 41px);
}

.tab-content {
  height: 100%;
  padding: 16px;
  overflow: auto;
}
</style>
