<template>
  <div class="mock-panel">
    <div class="panel-header">
      <h3 class="panel-title">Mock 数据生成器</h3>
      <div class="header-actions">
        <NButton size="small" quaternary title="场景模板" @click="showTemplateDialog = true">
          <template #icon><LayoutTemplate :size="14" /></template>
        </NButton>
        <NButton size="small" quaternary title="导入数据库结构" @click="showImportDialog = true">
          <template #icon><Database :size="14" /></template>
        </NButton>
        <NButton size="small" quaternary title="生成历史" @click="loadHistory">
          <template #icon><Clock :size="14" /></template>
        </NButton>
        <NButton size="small" quaternary title="保存模板" @click="showSaveTemplateModal = true">
          <template #icon><Save :size="14" /></template>
        </NButton>
      </div>
    </div>

    <div class="panel-body">
      <div class="config-section">
        <div class="config-row">
          <div class="config-item">
            <label class="config-label">表名</label>
            <NInput v-model:value="store.tableName" size="small" placeholder="table_name" />
          </div>
          <div class="config-item" style="width: 100px">
            <label class="config-label">行数</label>
            <NInputNumber v-model:value="store.rowCount" size="small" :min="1" :max="1000000" />
          </div>
          <div class="config-item">
            <label class="config-label">种子</label>
            <NInput
              v-model:value="seedInput"
              size="small"
              placeholder="可选"
              @update:value="onSeedChange"
            />
          </div>
          <div class="config-item">
            <label class="config-label">地区</label>
            <NSelect
              v-model:value="store.locale"
              size="small"
              :options="localeOptions"
              style="width: 140px"
            />
          </div>
          <div class="config-item" style="justify-content: flex-end">
            <NButton size="small" quaternary style="margin-top: 14px" @click="onReset">
              <template #icon><RotateCcw :size="14" /></template>
              重置
            </NButton>
          </div>
        </div>
      </div>

      <div class="columns-section">
        <div class="section-header">
          <span class="section-title">列配置</span>
          <NButton size="small" quaternary @click="store.addColumn()">
            <template #icon><Plus :size="14" /></template>
            添加列
          </NButton>
        </div>

        <div class="columns-list">
          <div v-for="(col, idx) in store.columns" :key="`col-${idx}`" class="column-row">
            <NInput
              :value="col.name"
              size="small"
              placeholder="列名"
              class="col-name"
              @update:value="(v: string) => store.updateColumn(idx, { name: v })"
            />
            <NSelect
              :value="col.dataType"
              size="small"
              :options="dataTypeOptions"
              class="col-type"
              @update:value="v => store.updateColumn(idx, { dataType: v })"
            />
            <NInputNumber
              v-if="col.dataType === 'varchar'"
              :value="col.varcharLength ?? 255"
              size="small"
              :min="1"
              :max="65535"
              class="col-dt-param"
              placeholder="长度"
              @update:value="(v: number | null) => store.updateColumn(idx, { varcharLength: v ?? undefined })"
            />
            <template v-if="col.dataType === 'decimal'">
              <NInputNumber
                :value="col.decimalPrecision ?? 18"
                size="small"
                :min="1"
                :max="38"
                class="col-dt-param-sm"
                placeholder="精度"
                @update:value="(v: number | null) => store.updateColumn(idx, { decimalPrecision: v ?? undefined })"
              />
              <NInputNumber
                :value="col.decimalScale ?? 2"
                size="small"
                :min="0"
                :max="38"
                class="col-dt-param-sm"
                placeholder="标度"
                @update:value="(v: number | null) => store.updateColumn(idx, { decimalScale: v ?? undefined })"
              />
            </template>
            <NSelect
              :value="col.generator.type"
              size="small"
              :options="generatorOptions"
              filterable
              class="col-gen"
              @update:value="v => store.setColumnType(idx, v)"
            />
            <NInputNumber
              :value="col.nullableRatio"
              size="small"
              :min="0"
              :max="1"
              :step="0.1"
              class="col-null"
              placeholder="NULL"
              @update:value="(v: number | null) => store.updateColumn(idx, { nullableRatio: v ?? 0 })"
            />
            <NTag
              v-if="col.unique"
              type="info"
              size="small"
              :bordered="false"
              closable
              @close="store.updateColumn(idx, { unique: false })"
            >
              UNIQUE
            </NTag>
            <NButton
              v-else
              size="small"
              quaternary
              class="col-fx"
              title="设为唯一"
              @click="store.updateColumn(idx, { unique: true })"
            >
              <template #icon><Fingerprint :size="13" /></template>
            </NButton>
            <NButton
              size="small"
              quaternary
              class="col-fx"
              title="高级配置"
              @click="openAdvanced(idx)"
            >
              <template #icon><Settings :size="13" /></template>
            </NButton>
            <NButton
              size="small"
              quaternary
              class="col-fx"
              title="智能映射生成器"
              :loading="mappingIdx === idx"
              @click="onAutoMapColumn(idx)"
            >
              <template #icon><Sparkles :size="13" /></template>
            </NButton>
            <NButton
              size="small"
              quaternary
              type="error"
              :disabled="store.columns.length <= 1"
              @click="store.removeColumn(idx)"
            >
              <template #icon><Trash2 :size="14" /></template>
            </NButton>
          </div>
        </div>
      </div>

      <div class="generate-section">
        <NButton
          type="primary"
          :loading="store.generateLoading"
          :disabled="!store.tableName || store.columns.length === 0"
          @click="onGenerate"
        >
          <template #icon><Sparkles :size="16" /></template>
          生成 {{ store.rowCount }} 行
        </NButton>
        <span v-if="store.lastResult" class="generate-info">
          已生成 {{ store.lastResult.rowCount }} 行，耗时 {{ store.lastResult.elapsedMs }}ms
        </span>
        <span
          v-if="store.generateLoading && store.generateProgressTotal > 0"
          class="generate-info progress-info"
        >
          {{ store.generateProgress }} / {{ store.generateProgressTotal }} 批次 ({{
            Math.round((store.generateProgress / store.generateProgressTotal) * 100)
          }}%)
        </span>
      </div>

      <div v-if="store.previewData.length > 0" class="preview-section">
        <div class="section-header">
          <span class="section-title"> 预览 (前 {{ store.previewData.length }} 行) </span>
          <div class="preview-actions">
            <NButton size="small" quaternary @click="onExportFile('csv')">
              <template #icon><FileDown :size="14" /></template>
              CSV
            </NButton>
            <NButton size="small" quaternary @click="onExportFile('xlsx')">
              <template #icon><FileSpreadsheet :size="14" /></template>
              XLSX
            </NButton>
            <NButton size="small" quaternary @click="onExportFile('parquet')">
              <template #icon><FileArchive :size="14" /></template>
              Parquet
            </NButton>
            <NButton size="small" quaternary @click="onExportFile('sql')">
              <template #icon><FileCode :size="14" /></template>
              SQL
            </NButton>
            <NDropdown trigger="click" :options="scratchpadOptions" @select="onSaveToScratchpad">
              <NButton size="small" quaternary>
                <template #icon><Save :size="14" /></template>
                草稿
              </NButton>
            </NDropdown>
            <NButton size="small" quaternary @click="onPersistAsset">
              <template #icon><HardDrive :size="14" /></template>
              持久化
            </NButton>
            <NButton
              size="small"
              quaternary
              :loading="store.previewLoading"
              @click="onRefreshPreview"
            >
              <template #icon><RotateCw :size="14" /></template>
              加载更多
            </NButton>
          </div>
        </div>

        <div class="preview-table-wrap">
          <NDataTable
            :columns="previewTableColumns"
            :data="previewTableData"
            size="small"
            :bordered="true"
            :single-line="false"
            :max-height="320"
            virtual-scroll
          />
        </div>

        <NCollapse v-if="store.previewData.length > 0" class="dist-collapse">
          <NCollapseItem title="数据分布" name="distribution">
            <DistributionChart
              :columns="store.generatedColumns.length > 0 ? store.generatedColumns : store.columns.map(c => c.name)"
              :column-types="store.generatedColumns.length > 0
                ? store.generatedColumns.map((_, i) => store.columns[i]?.dataType ?? 'varchar')
                : store.columns.map(c => c.dataType)"
              :rows="store.previewData"
              :bin-count="10"
              :top-n="10"
            />
          </NCollapseItem>
        </NCollapse>
      </div>

      <div v-if="store.persistenceHistory.length > 0" class="history-section">
        <div class="section-header">
          <span class="section-title">生成历史</span>
        </div>
        <div class="history-list">
          <div
            v-for="item in store.persistenceHistory"
            :key="item.id"
            class="history-item"
            @click="onReGenerateV2(item.id)"
          >
            <div class="history-item-left">
              <span class="history-table">{{ item.tableName }}</span>
              <span class="history-rows">{{ item.rowCount }} 行</span>
              <span class="history-status">{{ item.status }}</span>
            </div>
            <div class="history-item-right">
              <span class="history-time">{{ formatTimeStr(item.createdAt ?? '') }}</span>
              <NButton
                size="tiny"
                quaternary
                class="history-del"
                @click.stop="onDeleteHistory(item.id)"
              >
                <template #icon><Trash2 :size="12" /></template>
              </NButton>
            </div>
          </div>
        </div>
      </div>
    </div>

    <div v-if="store.userTemplates.length > 0" class="history-section">
      <div class="section-header">
        <span class="section-title">我的模板</span>
        <NButton
          size="small"
          quaternary
          :loading="store.templatesLoading"
          @click="onRefreshTemplates"
        >
          <template #icon><RotateCw :size="14" /></template>
        </NButton>
      </div>
      <div class="history-list">
        <div
          v-for="tpl in store.userTemplates"
          :key="tpl.id"
          class="history-item"
          @click="onApplyUserTemplate(tpl.id)"
        >
          <div class="history-item-left">
            <span class="history-table">{{ tpl.name }}</span>
            <span class="history-rows">{{ tpl.rowCount }} 行</span>
            <span class="history-status">{{ tpl.locale }}</span>
          </div>
          <div class="history-item-right">
            <span class="history-time">{{
              tpl.createdAt ? formatTimeStr(tpl.createdAt) : '-'
            }}</span>
            <NButton
              size="tiny"
              quaternary
              class="history-del"
              @click.stop="onDeleteUserTemplate(tpl.id)"
            >
              <template #icon><Trash2 :size="12" /></template>
            </NButton>
          </div>
        </div>
      </div>
    </div>

    <NModal
      v-model:show="showSaveTemplateModal"
      preset="card"
      title="保存为模板"
      style="width: 380px"
    >
      <div class="save-template-form">
        <div class="form-group">
          <label class="form-label">模板名称</label>
          <NInput v-model:value="saveTemplateName" size="small" placeholder="例如: 电商用户表" />
        </div>
        <div class="dialog-footer">
          <NButton size="small" @click="showSaveTemplateModal = false">取消</NButton>
          <NButton
            type="primary"
            size="small"
            :disabled="!saveTemplateName.trim()"
            :loading="saveTemplateLoading"
            @click="onSaveTemplate"
          >
            保存
          </NButton>
        </div>
      </div>
    </NModal>
    <MockTemplateSelectDialog
      :show="showTemplateDialog"
      @update:show="showTemplateDialog = $event"
      @apply="onTemplateApply"
    />
    <MockImportSchemaDialog
      :show="showImportDialog"
      @update:show="showImportDialog = $event"
      @apply="onImportApply"
    />
    <MockAdvancedDrawer
      v-if="advancedDrawerIndex >= 0"
      :show="advancedDrawerVisible"
      :generator-type="store.columns[advancedDrawerIndex]?.generator.type ?? 'words'"
      :current-params="
        (store.columns[advancedDrawerIndex]?.generator.params ?? {}) as Record<string, unknown>
      "
      :column-name="store.columns[advancedDrawerIndex]?.name ?? ''"
      :column-index="advancedDrawerIndex"
      :column-data-type="store.columns[advancedDrawerIndex]?.dataType ?? 'varchar'"
      :column-nullable-ratio="store.columns[advancedDrawerIndex]?.nullableRatio ?? 0"
      :column-unique="store.columns[advancedDrawerIndex]?.unique ?? false"
      :all-columns="allColumnsForDrawer"
      @update:show="advancedDrawerVisible = $event"
      @apply="onAdvancedApply"
    />
  </div>
</template>

<script setup lang="ts">
import { save } from '@tauri-apps/plugin-dialog'
import {
  Clock,
  Plus,
  Trash2,
  Sparkles,
  FileDown,
  FileCode,
  FileSpreadsheet,
  HardDrive,
  LayoutTemplate,
  Database,
  Save,
  RotateCcw,
  Fingerprint,
  FileArchive,
  RotateCw,
  Settings,
} from 'lucide-vue-next'
import {
  NButton,
  NInput,
  NInputNumber,
  NSelect,
  NDataTable,
  NDropdown,
  NTag,
  NModal,
  NCollapse,
  NCollapseItem,
  createDiscreteApi,
} from 'naive-ui'
import { ref, computed, onMounted } from 'vue'

import { useProjectStore } from '@/core/project'
import type {
  ColumnDef,
  MockExportFormat,
  ScenarioTemplate,
  GeneratorType,
  ColumnDataType,
} from '@/shared/api/mock-api'
import { useMockStore } from '@/stores/useMockStore'

import DistributionChart from './DistributionChart.vue'
import MockAdvancedDrawer from './MockAdvancedDrawer.vue'
import MockImportSchemaDialog from './MockImportSchemaDialog.vue'
import MockTemplateSelectDialog from './MockTemplateSelectDialog.vue'
import { useMockOptions } from './useMockOptions'

const store = useMockStore()
const projectStore = useProjectStore()
const { message } = createDiscreteApi(['message'])

const {
  localeOptions,
  dataTypeOptions,
  generatorOptions,
  formatExtMap,
  scratchpadOptions,
} = useMockOptions()

const seedInput = ref(store.seed !== null ? String(store.seed) : '')
const showTemplateDialog = ref(false)
const showImportDialog = ref(false)
const showSaveTemplateModal = ref(false)
const saveTemplateName = ref('')
const saveTemplateLoading = ref(false)

const advancedDrawerIndex = ref(-1)
const advancedDrawerVisible = ref(false)
const mappingIdx = ref(-1)

function openAdvanced(idx: number) {
  advancedDrawerIndex.value = idx
  advancedDrawerVisible.value = true
}

function onAdvancedApply(
  idx: number,
  type: string,
  params: Record<string, unknown>,
  fieldName: string,
  dataType: string,
  nullableRatio: number,
  unique: boolean
) {
  store.updateColumn(idx, {
    name: fieldName,
    dataType: dataType as ColumnDataType,
    generator: { type: type as GeneratorType, params },
    nullableRatio,
    unique,
  })
}

const allColumnsForDrawer = computed(() =>
  store.columns.map(c => ({ name: c.name, dataType: c.dataType }))
)

async function onAutoMapColumn(idx: number) {
  mappingIdx.value = idx
  await store.autoMapColumn(idx)
  mappingIdx.value = -1
}


const previewTableColumns = computed(() => {
  if (store.generatedColumns.length > 0) {
    return store.generatedColumns.map(name => ({
      title: name,
      key: name,
      minWidth: 120,
      ellipsis: { tooltip: true },
    }))
  }
  if (store.previewData.length > 0 && store.previewData[0]) {
    return store.previewData[0].map((_, i) => ({
      title: store.columns[i]?.name ?? `col_${i + 1}`,
      key: `col_${i}`,
      minWidth: 120,
      ellipsis: { tooltip: true },
    }))
  }
  return []
})

const previewTableData = computed(() => {
  if (store.generatedColumns.length > 0) {
    return store.previewData.map(row => {
      const record: Record<string, unknown> = {}
      store.generatedColumns.forEach((col, i) => {
        record[col] = row[i]
      })
      return record
    })
  }
  return store.previewData.map(row => {
    const record: Record<string, unknown> = {}
    row.forEach((val, i) => {
      record[`col_${i}`] = val
    })
    return record
  })
})

function onSeedChange(value: string) {
  seedInput.value = value
  const v = value.trim()
  store.seed = v === '' ? null : isNaN(parseInt(v, 10)) ? null : parseInt(v, 10)
}

function onReset() {
  store.reset()
  seedInput.value = ''
}

async function onGenerate() {
  try {
    const projectPath = projectStore.projectPath
    if (projectPath) {
      await store.generateAndSave(projectPath)
    } else {
      await store.generate()
    }
    message.success(`成功生成 ${store.lastResult?.rowCount ?? 0} 行`)
  } catch (e) {
    message.error(`生成失败: ${String(e)}`)
  }
}

async function onExportFile(format: string) {
  try {
    const fmt = formatExtMap[format]
    if (!fmt) return
    const filePath = await save({
      defaultPath: `${store.tableName}_${Date.now()}.${fmt.ext}`,
      filters: [{ name: `${fmt.label} 文件`, extensions: [fmt.ext] }],
    })
    if (!filePath) return
    const backendFormat =
      format === 'sql' ? 'SqlInsert' : format.charAt(0).toUpperCase() + format.slice(1)
    await store.doExport(backendFormat as MockExportFormat, filePath)
    message.success(`已导出到: ${filePath}`)
  } catch (e) {
    message.error(`导出失败: ${String(e)}`)
  }
}

async function onSaveToScratchpad(formatKey: string) {
  try {
    const path = await store.saveToScratchpad(formatKey as MockExportFormat)
    message.success(`已保存到草稿箱: ${path}`)
  } catch (e) {
    message.error(`保存失败: ${String(e)}`)
  }
}

async function onPersistAsset() {
  try {
    const result = await store.persistAsAsset(store.tableName)
    if (result) message.success(`已持久化为分析资产: ${result.tableName}`)
  } catch (e) {
    message.error(`持久化失败: ${String(e)}`)
  }
}

async function onRefreshPreview() {
  if (!store.generatedTableName) {
    message.warning('请先生成数据')
    return
  }
  try {
    await store.preview(store.generatedTableName, 200)
    message.success(`已加载 ${store.previewData.length} 行`)
  } catch (e) {
    message.error(`刷新预览失败: ${String(e)}`)
  }
}

async function onReGenerateV2(historyId: string) {
  const projectPath = projectStore.projectPath
  if (!projectPath) {
    message.warning('请先打开项目')
    return
  }
  try {
    const detail = await store.loadDetail(projectPath, historyId)
    if (detail.columns.length === 0) {
      message.warning('该历史记录无字段配置')
      return
    }
    message.success(
      `已从历史恢复 ${detail.task.tableName}（${detail.columns.length} 列），请点击生成按钮`
    )
  } catch (e) {
    message.error(`加载历史详情失败: ${String(e)}`)
  }
}

async function onDeleteHistory(historyId: string) {
  const projectPath = projectStore.projectPath
  if (!projectPath) return
  try {
    await store.deletePersistenceTask(projectPath, historyId)
    message.success('已删除历史记录')
  } catch (e) {
    message.error(`删除失败: ${String(e)}`)
  }
}

function onTemplateApply(_templateId: string, _template: ScenarioTemplate) {
  const firstTable = _template.tables[0]
  if (firstTable?.columns && firstTable.columns.length > 0) {
    store.columns = firstTable.columns.map(c => ({ ...c }))
    store.tableName = firstTable.name
    store.rowCount = firstTable.rowCount
    message.success(`已应用模板: ${_template.name}`)
  } else {
    message.warning('模板没有表定义')
  }
}

function onImportApply(columns: ColumnDef[]) {
  store.columns = columns.map(c => ({ ...c }))
  message.success(`已导入 ${columns.length} 列`)
}

function formatTimeStr(timestamp: string): string {
  try {
    const d = new Date(timestamp)
    const pad = (n: number) => String(n).padStart(2, '0')
    return `${d.getMonth() + 1}/${d.getDate()} ${pad(d.getHours())}:${pad(d.getMinutes())}`
  } catch {
    return timestamp
  }
}

async function loadHistory() {
  const projectPath = projectStore.projectPath
  if (!projectPath) return
  try {
    await store.loadHistoryV2(projectPath, 20)
  } catch {
    /* history load is non-critical */
  }
}

async function onSaveTemplate() {
  const projectPath = projectStore.projectPath
  if (!projectPath) return
  saveTemplateLoading.value = true
  try {
    await store.saveCurrentAsTemplate(projectPath, saveTemplateName.value.trim())
    showSaveTemplateModal.value = false
    saveTemplateName.value = ''
    message.success('模板已保存')
  } catch (e) {
    message.error(`保存模板失败: ${String(e)}`)
  } finally {
    saveTemplateLoading.value = false
  }
}

async function onRefreshTemplates() {
  const projectPath = projectStore.projectPath
  if (!projectPath) return
  try {
    await store.loadUserTemplates(projectPath)
  } catch {
    /* template load is non-critical */
  }
}

async function onApplyUserTemplate(templateId: string) {
  const projectPath = projectStore.projectPath
  if (!projectPath) return
  try {
    await store.applyUserTemplate(projectPath, templateId)
    message.success('模板已应用')
  } catch (e) {
    message.error(`应用模板失败: ${String(e)}`)
  }
}

async function onDeleteUserTemplate(templateId: string) {
  const projectPath = projectStore.projectPath
  if (!projectPath) return
  try {
    await store.deleteUserTemplate(projectPath, templateId)
    message.success('模板已删除')
  } catch (e) {
    message.error(`删除模板失败: ${String(e)}`)
  }
}

onMounted(() => {
  loadHistory()
  onRefreshTemplates()
})
</script>

<style scoped>
.mock-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--color-bg-primary);
}

.panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--spacing-xs) var(--spacing-md);
  border-bottom: 1px solid var(--color-border-subtle);
  min-height: 36px;
}

.panel-title {
  margin: 0;
  font-size: var(--font-size-md);
  font-weight: 600;
  color: var(--color-text-primary);
}

.header-actions {
  display: flex;
  gap: 2px;
}

.panel-body {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow-y: auto;
  padding: var(--spacing-sm) var(--spacing-md);
  gap: var(--spacing-sm);
}

.config-section {
  padding: var(--spacing-sm) var(--spacing-md);
  background: var(--color-bg-secondary);
  border-radius: var(--border-radius-md);
  border: 1px solid var(--color-border-subtle);
}

.config-row {
  display: flex;
  gap: var(--spacing-sm);
  align-items: flex-end;
  flex-wrap: wrap;
}

.config-item {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
}

.config-label {
  font-size: var(--font-size-xs);
  font-weight: 500;
  color: var(--color-text-muted);
  text-transform: uppercase;
}

.columns-section {
  border: 1px solid var(--color-border-subtle);
  border-radius: var(--border-radius-md);
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.section-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--spacing-xs) var(--spacing-sm);
  background: var(--color-bg-secondary);
  border-bottom: 1px solid var(--color-border-subtle);
  flex-shrink: 0;
}

.section-title {
  font-size: var(--font-size-xs);
  font-weight: 600;
  color: var(--color-text-muted);
  text-transform: uppercase;
}

.columns-list {
  padding: var(--spacing-xs) var(--spacing-sm);
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
  overflow-y: auto;
  max-height: 220px;
}

.column-row {
  display: flex;
  gap: var(--spacing-xs);
  align-items: center;
}

.col-name {
  flex: 0 0 110px;
}
.col-type {
  width: 100px;
  flex-shrink: 0;
}
.col-dt-param {
  width: 64px;
  flex-shrink: 0;
}
.col-dt-param-sm {
  width: 52px;
  flex-shrink: 0;
}
.col-gen {
  flex: 1;
  min-width: 120px;
}
.col-null {
  width: 72px;
  flex-shrink: 0;
}
.col-fx {
  flex-shrink: 0;
}

.generate-section {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  padding: var(--spacing-xs) 0;
}

.generate-info {
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
}

.preview-section {
  border: 1px solid var(--color-border-subtle);
  border-radius: var(--border-radius-md);
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.preview-actions {
  display: flex;
  gap: 2px;
  flex-wrap: wrap;
}

.preview-table-wrap {
  overflow: auto;
  max-height: 360px;
}

.history-section {
  border: 1px solid var(--color-border-subtle);
  border-radius: var(--border-radius-md);
  display: flex;
  flex-direction: column;
}

.history-list {
  padding: var(--spacing-xs) var(--spacing-sm);
  max-height: 140px;
  overflow-y: auto;
}

.history-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: var(--spacing-xs);
  cursor: pointer;
  border-radius: var(--border-radius-sm);
  transition: background 0.15s;
}

.history-item:hover {
  background: var(--color-hover);
}

.history-item-left {
  display: flex;
  gap: var(--spacing-xs);
  align-items: center;
  font-size: var(--font-size-sm);
}

.history-table {
  color: var(--color-text-primary);
  font-weight: 500;
}
.history-rows {
  color: var(--color-text-secondary);
}
.history-status {
  color: var(--brand-success);
  font-size: var(--font-size-xs);
}
.history-item-right {
  font-size: var(--font-size-xs);
  color: var(--color-text-muted);
}

.save-template-form {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-lg);
}

.form-group {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
}

.form-label {
  font-size: var(--font-size-sm);
  font-weight: 500;
  color: var(--color-text-secondary);
}

.dialog-footer {
  display: flex;
  justify-content: flex-end;
  gap: var(--spacing-sm);
}

.dist-collapse {
  margin-top: var(--spacing-xs);
}
</style>
