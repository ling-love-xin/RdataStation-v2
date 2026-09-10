<template>
  <div class="table-schema-panel">
    <NTabs type="line" size="small">
      <NTabPane name="columns" :tab="t('workbench.columnsTab')">
        <NDataTable :columns="columnColumns" :data="columns" :bordered="false" size="small" />
      </NTabPane>
      <NTabPane name="indexes" :tab="t('workbench.indexesTab')">
        <NEmpty :description="t('workbench.noIndexes')" />
      </NTabPane>
      <NTabPane name="constraints" :tab="t('workbench.constraintsTab')">
        <NEmpty :description="t('workbench.noConstraints')" />
      </NTabPane>
    </NTabs>
  </div>
</template>

<script setup lang="ts">
import { NTabs, NTabPane, NDataTable, NEmpty } from 'naive-ui'
import { useI18n } from 'vue-i18n'

import type { ColumnInfo } from '@/shared/types/databaseMeta'

import type { DataTableColumns } from 'naive-ui'

interface Props {
  columns?: ColumnInfo[]
}

const { t } = useI18n()

const _props = withDefaults(defineProps<Props>(), {
  columns: () => [],
})

const columnColumns: DataTableColumns<ColumnInfo> = [
  { title: t('workbench.columnName'), key: 'name' },
  { title: t('workbench.dataType'), key: 'type' },
  {
    title: t('workbench.nullable'),
    key: 'nullable',
    render: row => (row.isNullable ? t('workbench.yes') : t('workbench.no')),
  },
  { title: t('workbench.defaultValue'), key: 'defaultValue' },
]
</script>

<style scoped>
.table-schema-panel {
  padding: 16px;
  height: 100%;
  overflow: auto;
}
</style>
