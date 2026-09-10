<template>
  <NModal v-model:show="visible" preset="card" title="导入数据库结构" style="width: 560px">
    <div class="import-dialog">
      <div class="form-group">
        <label class="form-label">连接</label>
        <NSelect
          v-model:value="connId"
          size="small"
          :options="connectionOptions"
          :loading="connLoading"
          placeholder="选择已有连接"
          filterable
        />
      </div>
      <div class="form-group">
        <label class="form-label">数据库</label>
        <NInput v-model:value="database" size="small" placeholder="database" />
      </div>
      <div class="form-group">
        <label class="form-label">Schema (可选)</label>
        <NInput v-model:value="schemaName" size="small" placeholder="e.g. public" />
      </div>
      <div class="form-group">
        <label class="form-label">表名 (逗号分隔)</label>
        <NInput v-model:value="tablesInput" size="small" placeholder="users, orders, products" />
      </div>

      <div v-if="importedColumns.length > 0" class="imported-columns">
        <div class="section-header">
          <span>已导入 {{ importedColumns.length }} 列（{{ mappedCount }} 已自动映射生成器）</span>
          <NButton size="small" quaternary @click="onApply">应用到面板</NButton>
        </div>
        <div class="column-preview">
          <div
            v-for="(col, idx) in importedColumns.slice(0, 20)"
            :key="idx"
            class="imported-col-row"
          >
            <span class="icol-name">{{ col.name }}</span>
            <span class="icol-type">{{ col.dataType }}</span>
            <span class="icol-gen">{{ col.generator?.type ?? '-' }}</span>
          </div>
        </div>
      </div>

      <div class="dialog-footer">
        <NButton size="small" @click="visible = false">取消</NButton>
        <NButton
          type="primary"
          size="small"
          :loading="importing"
          :disabled="!connId || !database || !tablesInput"
          @click="onImport"
        >
          导入结构
        </NButton>
      </div>
    </div>
  </NModal>
</template>

<script setup lang="ts">
import { NModal, NButton, NInput, NSelect, createDiscreteApi } from 'naive-ui'
import { ref, computed, watch } from 'vue'

import { useConnectionStore } from '@/extensions/builtin/connection/ui/stores/connection-store'
import { mockApi, type ColumnDef } from '@/shared/api/mock-api'

const props = defineProps<{
  show: boolean
}>()

const emit = defineEmits<{
  'update:show': [value: boolean]
  apply: [columns: ColumnDef[]]
}>()

const { message } = createDiscreteApi(['message'])
const visible = ref(props.show)
const connId = ref('')
const database = ref('')
const schemaName = ref('')
const tablesInput = ref('')
const importing = ref(false)
const importedColumns = ref<ColumnDef[]>([])
const connLoading = ref(false)

const genericGenerators = new Set(['words', 'sentence', 'sentences', 'paragraph', 'word'])

const mappedCount = computed(
  () =>
    importedColumns.value.filter(c => c.generator?.type && !genericGenerators.has(c.generator.type))
      .length
)

const connectionStore = useConnectionStore()

const connectionOptions = computed(() =>
  connectionStore.connections.map(c => ({
    label: `${c.name} (${c.dbType})`,
    value: c.connId,
  }))
)

watch(
  () => props.show,
  val => {
    visible.value = val
    if (val) {
      importedColumns.value = []
      loadConnections()
    }
  }
)

watch(visible, val => {
  emit('update:show', val)
})

async function loadConnections() {
  connLoading.value = true
  try {
    await connectionStore.loadConnections()
  } catch {
    message.warning('加载连接列表失败')
  } finally {
    connLoading.value = false
  }
}

async function onImport() {
  importing.value = true
  try {
    const tables = tablesInput.value
      .split(',')
      .map(t => t.trim())
      .filter(Boolean)

    const columns = await mockApi.importSchema({
      connId: connId.value,
      database: database.value,
      schema: schemaName.value || undefined,
      tables,
    })

    if (columns.length > 0) {
      const batchInput: Array<[string, string]> = columns.map(c => [c.name, c.dataType])
      const mappingResults = await mockApi.mapColumnsBatch(batchInput)
      const mappingMap = new Map(mappingResults.map(m => [m.columnName, m]))
      let mappedCount = 0
      const merged = columns.map(c => {
        const mapping = mappingMap.get(c.name)
        if (mapping && mapping.confidence !== 'low') {
          mappedCount++
          return { ...c, generator: mapping.generator }
        }
        return c
      })
      importedColumns.value = merged
      let msg = `成功导入 ${columns.length} 列，${mappedCount} 已智能映射`
      if (mappedCount < columns.length) {
        msg += `，${columns.length - mappedCount} 列需要手动配置`
      }
      message.success(msg)
    } else {
      importedColumns.value = columns
      message.success('导入完成')
    }
  } catch (e) {
    message.error(`导入失败: ${String(e)}`)
  } finally {
    importing.value = false
  }
}

function onApply() {
  emit('apply', importedColumns.value)
  visible.value = false
}
</script>

<style scoped>
.import-dialog {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-md);
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

.imported-columns {
  border: 1px solid var(--color-border-subtle);
  border-radius: var(--border-radius-md);
  overflow: hidden;
}

.section-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: var(--spacing-sm) var(--spacing-sm);
  background: var(--color-bg-secondary);
  border-bottom: 1px solid var(--color-border-subtle);
  font-size: var(--font-size-sm);
  font-weight: 600;
  color: var(--color-text-secondary);
}

.column-preview {
  padding: var(--spacing-xs) var(--spacing-sm);
  max-height: 200px;
  overflow-y: auto;
}

.imported-col-row {
  display: flex;
  gap: var(--spacing-sm);
  padding: var(--spacing-xs) 0;
  font-size: var(--font-size-sm);
  border-bottom: 1px solid var(--color-border-subtle);
}

.imported-col-row:last-child {
  border-bottom: none;
}

.icol-name {
  flex: 0 0 140px;
  color: var(--color-text-primary);
  font-weight: 500;
}
.icol-type {
  flex: 0 0 90px;
  color: var(--color-text-muted);
}
.icol-gen {
  flex: 1;
  color: var(--brand-accent);
}

.dialog-footer {
  display: flex;
  justify-content: flex-end;
  gap: var(--spacing-sm);
  padding-top: var(--spacing-sm);
  border-top: 1px solid var(--color-border-subtle);
}
</style>
