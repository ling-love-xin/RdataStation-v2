<template>
  <div class="result-table">
    <NDataTable
      :columns="columns"
      :data="data"
      :bordered="false"
      size="small"
      striped
      :max-height="maxHeight"
      virtual-scroll
    />
  </div>
</template>

<script setup lang="ts">
import { NDataTable } from 'naive-ui'
import { computed } from 'vue'

import type { QueryResult } from '@/shared/types'

import type { DataTableColumns } from 'naive-ui'

interface Props {
  result?: QueryResult | null
  maxHeight?: number
}

const props = withDefaults(defineProps<Props>(), {
  result: null,
  maxHeight: 400,
})

const columns = computed((): DataTableColumns<Record<string, unknown>> => {
  if (!props.result?.columns) return []
  return props.result.columns.map(col => ({
    title: col,
    key: col,
    ellipsis: true,
    width: 150,
  }))
})

const data = computed(() => {
  if (!props.result?.rows) return []
  return props.result.rows.map((row, index) => {
    const obj: Record<string, unknown> = { key: index }
    const rowArray = row as unknown[]
    props.result?.columns.forEach((col, i) => {
      obj[col] = rowArray[i]
    })
    return obj
  })
})
</script>

<style scoped>
.result-table {
  height: 100%;
  overflow: auto;
}
</style>
