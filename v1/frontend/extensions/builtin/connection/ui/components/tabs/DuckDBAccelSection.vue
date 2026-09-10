<template>
  <div v-if="isDuckDBSupported" :class="['accel-card', { on: enabled }]">
    <div class="accel-row">
      <span class="accel-icon">🦆</span>
      <div class="accel-body">
        <span class="accel-name">{{ title }}</span>
        <span class="accel-desc">{{ description }}</span>
      </div>
      <NSwitch :value="enabled" size="small" @update:value="$emit('update:enabled', $event)" />
    </div>
    <div v-if="enabled" class="accel-expand">
      <div class="accel-benefits">
        <span class="benefit-tag">🚀 大表分析</span>
        <span class="benefit-tag">🔗 跨库联邦</span>
        <span class="benefit-tag">📊 重复报表</span>
        <span class="benefit-tag">🗜 列式压缩</span>
      </div>
      <div class="accel-storage-info">
        <span class="storage-icon">📁</span>
        <span class="storage-path">存储路径: .rdata/duckdb/accel.duckdb</span>
        <span class="storage-hint">联邦查询中间结果缓存目录</span>
      </div>
      <div class="adv-grid" style="margin-top: 10px">
        <div class="adv-cell">
          <span class="adv-lbl">{{ syncLabel }}</span>
          <NSelect
            :value="sync"
            size="small"
            :options="syncOptions"
            @update:value="$emit('update:sync', $event)"
          />
        </div>
        <div class="adv-cell">
          <span class="adv-lbl">{{ intervalLabel }}</span>
          <NInputNumber
            :value="interval"
            size="small"
            :min="1"
            :max="1440"
            @update:value="$emit('update:interval', $event)"
          />
        </div>
        <div class="adv-cell">
          <span class="adv-lbl">{{ memoryLabel }}</span>
          <NInputNumber
            :value="memory"
            size="small"
            :min="64"
            :max="8192"
            @update:value="$emit('update:memory', $event)"
          />
        </div>
        <div class="adv-cell">
          <span class="adv-lbl">{{ threadsLabel }}</span>
          <NInputNumber
            :value="threads"
            size="small"
            :min="1"
            :max="16"
            @update:value="$emit('update:threads', $event)"
          />
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { NSelect, NInputNumber, NSwitch } from 'naive-ui'
import { computed } from 'vue'

import type { SelectOption } from 'naive-ui'

const props = defineProps<{
  enabled: boolean
  sync: string
  interval: number
  memory: number
  threads: number
  syncOptions: SelectOption[]
  title: string
  description: string
  syncLabel: string
  intervalLabel: string
  memoryLabel: string
  threadsLabel: string
  driverId?: string
}>()

const isDuckDBSupported = computed(() => {
  const driverId = (props.driverId || '').toLowerCase()
  return ['mysql', 'postgresql', 'postgres', 'sqlite', 'duckdb'].some(id => driverId.includes(id))
})

defineEmits<{
  'update:enabled': [value: boolean]
  'update:sync': [value: string]
  'update:interval': [value: number | null]
  'update:memory': [value: number | null]
  'update:threads': [value: number | null]
}>()
</script>

<style scoped>
.accel-card {
  padding: 12px;
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border-subtle);
  border-radius: var(--border-radius-lg);
  transition: border-color 0.2s;
}
.accel-card.on {
  border-color: rgba(166, 227, 161, 0.3);
  box-shadow: 0 0 0 1px rgba(166, 227, 161, 0.15);
}
.accel-row {
  display: flex;
  align-items: flex-start;
  gap: 12px;
}
.accel-icon {
  font-size: 26px;
  line-height: 1;
  flex-shrink: 0;
}
.accel-body {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.accel-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--color-text-primary);
}
.accel-desc {
  font-size: 11px;
  color: var(--color-text-muted);
}
.accel-expand {
  margin-top: 10px;
  padding-top: 10px;
  border-top: 1px solid var(--color-border-subtle);
}
.accel-benefits {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-bottom: 8px;
}
.benefit-tag {
  font-size: 11px;
  padding: 3px 8px;
  border-radius: var(--border-radius-sm);
  font-weight: 500;
  background: rgba(166, 227, 161, 0.1);
  color: var(--brand-success);
}
.accel-storage-info {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 6px;
  padding: 8px 10px;
  margin-bottom: 4px;
  background: rgba(166, 227, 161, 0.05);
  border: 1px solid rgba(166, 227, 161, 0.12);
  border-radius: var(--border-radius-sm);
}
.storage-icon {
  font-size: 12px;
  flex-shrink: 0;
}
.storage-path {
  font-size: 11px;
  font-family: var(--font-mono);
  color: var(--color-text-secondary);
}
.storage-hint {
  font-size: 10px;
  color: var(--color-text-muted);
}
.adv-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px;
}
.adv-cell {
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.adv-lbl {
  font-size: 11px;
  color: var(--color-text-secondary);
}
</style>
