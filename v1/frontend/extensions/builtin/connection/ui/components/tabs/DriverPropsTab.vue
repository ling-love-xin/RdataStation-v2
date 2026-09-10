<template>
  <div class="props-tab">
    <NAlert v-if="driver" type="info" :bordered="false" class="props-banner">
      {{ $t('connection.driverPropsTab.desc') }}
    </NAlert>
    <div v-if="!driver" class="empty-hint">{{ $t('navigator.noDriver') }}</div>
    <template v-else>
      <div class="props-table">
        <div class="props-thead">
          <span class="props-th" style="width: 30%">{{
            $t('connection.driverPropsTab.property')
          }}</span>
          <span class="props-th" style="width: 45%">{{
            $t('connection.driverPropsTab.value')
          }}</span>
          <span class="props-th" style="width: 25%">{{
            $t('connection.driverPropsTab.description')
          }}</span>
        </div>
        <div v-if="rows.length === 0" class="props-empty">
          {{ $t('connection.driverPropsTab.noProperties') }}
        </div>
        <div v-for="(row, idx) in rows" :key="idx" class="props-row">
          <span class="props-td" style="width: 30%"
            ><NInput v-model:value="row.key" size="small"
          /></span>
          <span class="props-td" style="width: 45%"
            ><NInput v-model:value="row.value" size="small"
          /></span>
          <span class="props-td props-acts" style="width: 25%">
            <span class="props-desc">&mdash;</span>
            <NButton text size="tiny" class="props-del" @click="removeRow(idx)">
              <template #icon><X :size="14" /></template>
            </NButton>
          </span>
        </div>
      </div>
      <div class="props-add-row">
        <NButton text size="small" @click="addRow">+ {{ $t('navigator.addProperty') }}</NButton>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { X } from 'lucide-vue-next'
import { NAlert, NButton, NInput } from 'naive-ui'
import { ref, watch } from 'vue'

import type { Driver } from '../../../domain/types'

const props = defineProps<{
  driver: Driver | null
  /** 初始连接属性（编辑已有连接时传入，来自 connections.driver_properties JSON 字符串） */
  driverProperties?: string | null
}>()

const emit = defineEmits<{
  'extra-config': [config: Record<string, unknown>]
}>()

interface PropRow {
  key: string
  value: string
}
const rows = ref<PropRow[]>([])

/// 解析 driver_properties JSON 字符串 { key: value } → PropRow[]
function tryParseDriverProperties(raw: string | null | undefined): PropRow[] {
  if (!raw) return []
  try {
    const obj: unknown = JSON.parse(raw)
    if (typeof obj === 'object' && obj !== null && !Array.isArray(obj)) {
      return Object.entries(obj as Record<string, unknown>).map(([k, v]) => ({
        key: k,
        value: String(v),
      }))
    }
    return []
  } catch (err) {
    console.warn('[parseDriverProperties] 解析失败:', err)
    return []
  }
}

/// 当 driver 变化时重置（新驱动 → 新属性）
watch(
  () => props.driver,
  () => {
    rows.value = []
  },
  { immediate: true }
)

/// 当初始属性传入时（编辑已有连接），用 driver_properties 填充
watch(
  () => props.driverProperties,
  dp => {
    rows.value = tryParseDriverProperties(dp)
  },
  { immediate: true }
)

function addRow() {
  rows.value.push({ key: '', value: '' })
}
function removeRow(idx: number) {
  rows.value.splice(idx, 1)
}

// Emit extra config whenever props change
watch(
  rows,
  () => {
    const obj: Record<string, string> = {}
    rows.value.forEach(r => {
      if (r.key.trim()) obj[r.key.trim()] = r.value
    })
    emit('extra-config', {
      driverProperties: Object.keys(obj).length > 0 ? JSON.stringify(obj) : null,
    })
  },
  { deep: true }
)
</script>

<style scoped>
.props-tab {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 4px 0;
}
.props-banner {
  border-radius: 6px;
}
.empty-hint {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 120px;
  font-size: 13px;
  color: var(--color-text-muted);
}
.props-table {
  border: 1px solid var(--color-border-subtle);
  border-radius: 8px;
  overflow: hidden;
  font-size: 12px;
}
.props-thead {
  display: flex;
  background: var(--color-bg-elevated);
  padding: 10px 14px;
  border-bottom: 1px solid var(--color-border-subtle);
}
.props-th {
  font-weight: 600;
  color: var(--color-text-muted);
}
.props-row {
  display: flex;
  padding: 6px 14px;
  align-items: center;
  border-bottom: 1px solid var(--color-border-subtle);
  gap: 8px;
}
.props-row:last-child {
  border-bottom: none;
}
.props-td {
  display: flex;
  align-items: center;
}
.props-acts {
  gap: 6px;
}
.props-desc {
  font-size: 11px;
  color: var(--color-text-muted);
}
.props-del {
  opacity: 0;
  transition: opacity 0.12s;
}
.props-row:hover .props-del {
  opacity: 1;
}
.props-add-row {
  padding: 10px 14px;
}
.props-empty {
  padding: 24px 14px;
  text-align: center;
  font-size: 12px;
  color: var(--color-text-muted);
}
</style>
