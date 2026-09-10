<template>
  <div class="table-data-panel">
    <div class="panel-header">
      <span class="panel-title">{{ t('workbench.tableData') }}</span>
    </div>
    <div class="panel-content">
      <div v-if="!tableName" class="empty-state">
        <Table :size="32" />
        <p>{{ t('workbench.selectTableToView') }}</p>
      </div>
      <div v-else class="table-container">
        <div class="table-info">
          <span class="table-name">{{ tableName }}</span>
          <span class="row-count">{{ rowCount }} {{ t('resultPanel.rows') }}</span>
        </div>
        <ResultTable :columns="columns" :rows="rows" :loading="loading" />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Table } from 'lucide-vue-next'
import { ref, computed } from 'vue'
import { useI18n } from 'vue-i18n'

import ResultTable from '@/extensions/builtin/query/ui/components/result/ResultTable.vue'

import type { IDockviewPanelProps } from 'dockview-vue'

interface Props {
  params?: IDockviewPanelProps & {
    tableName?: string
    connectionId?: string
  }
}

const { t } = useI18n()

const props = defineProps<Props>()

// 表数据状态
const tableName = computed(() => props.params?.tableName || '')
const loading = ref(false)
const rowCount = ref(0)
const columns = ref<string[]>([])
const rows = ref<unknown[][]>([])
</script>

<style scoped>
.table-data-panel {
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--bg-primary);
}

.panel-header {
  height: 36px;
  display: flex;
  align-items: center;
  padding: 0 12px;
  border-bottom: 1px solid var(--border-color);
  background: var(--bg-secondary);
}

.panel-title {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-primary);
}

.panel-content {
  flex: 1;
  overflow: hidden;
}

.empty-state {
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  color: var(--text-secondary);
}

.empty-state p {
  font-size: 13px;
  margin: 0;
}

.table-container {
  height: 100%;
  display: flex;
  flex-direction: column;
}

.table-info {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 12px;
  border-bottom: 1px solid var(--border-color);
  background: var(--bg-secondary);
}

.table-name {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-primary);
}

.row-count {
  font-size: 12px;
  color: var(--text-secondary);
}
</style>
