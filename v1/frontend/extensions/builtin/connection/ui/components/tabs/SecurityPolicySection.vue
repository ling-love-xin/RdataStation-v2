<template>
  <NCollapse>
    <NCollapseItem name="sec">
      <template #header>
        <span class="sec-title" style="margin-bottom: 0">🔐 {{ title }}</span>
        <span class="collapse-summary">{{ summary }}</span>
      </template>
      <div class="policy-grid">
        <div class="policy-row">
          <span class="pol-item"
            >{{ readonlyLabel }}
            <NSwitch
              :value="readonly"
              size="small"
              @update:value="handleChange('update:readonly', $event)"
          /></span>
          <span class="pol-item"
            >{{ writeConfirmLabel }}
            <NSwitch
              :value="writeConfirm"
              size="small"
              @update:value="handleChange('update:writeConfirm', $event)"
          /></span>
          <span class="pol-item"
            >{{ ddlConfirmLabel }}
            <NSwitch
              :value="ddlConfirm"
              size="small"
              @update:value="handleChange('update:ddlConfirm', $event)"
          /></span>
        </div>
        <div class="policy-row">
          <span class="pol-item"
            >{{ dropOpLabel }}
            <NSelect
              :value="dropPolicy"
              size="small"
              :options="dropOptions"
              style="width: 80px"
              @update:value="handleChange('update:dropPolicy', $event)"
            />
          </span>
          <span class="pol-item"
            >{{ autocommitLabel }}
            <NSwitch
              :value="autocommit"
              size="small"
              @update:value="handleChange('update:autocommit', $event)"
          /></span>
          <span class="pol-item"
            >{{ rowLimitLabel }}
            <NInputNumber
              :value="rowLimit"
              size="small"
              :min="0"
              style="width: 80px"
              @update:value="handleChange('update:rowLimit', $event)"
          /></span>
          <span class="pol-item"
            >{{ sizeLimitLabel }}
            <NInputNumber
              :value="sizeLimit"
              size="small"
              :min="0"
              :max="10240"
              style="width: 80px"
              @update:value="handleChange('update:sizeLimit', $event)"
          /></span>
        </div>
      </div>
    </NCollapseItem>
  </NCollapse>
</template>

<script setup lang="ts">
import { NCollapse, NCollapseItem, NSelect, NSwitch, NInputNumber } from 'naive-ui'

import type { SelectOption } from 'naive-ui'

defineProps<{
  title: string
  summary: string
  readonly: boolean
  writeConfirm: boolean
  ddlConfirm: boolean
  autocommit: boolean
  dropPolicy: string
  rowLimit: number
  sizeLimit: number
  readonlyLabel: string
  writeConfirmLabel: string
  ddlConfirmLabel: string
  dropOpLabel: string
  autocommitLabel: string
  rowLimitLabel: string
  sizeLimitLabel: string
  dropOptions: SelectOption[]
}>()

const emit = defineEmits<{
  'update:readonly': [value: boolean]
  'update:writeConfirm': [value: boolean]
  'update:ddlConfirm': [value: boolean]
  'update:autocommit': [value: boolean]
  'update:dropPolicy': [value: string]
  'update:rowLimit': [value: number | null]
  'update:sizeLimit': [value: number | null]
  override: []
}>()

function handleChange(eventName: string, value: unknown) {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  ;(emit as any)(eventName, value)
  emit('override')
}
</script>

<style scoped>
.sec-title {
  font-size: 11px;
  font-weight: 700;
  text-transform: uppercase;
  color: var(--color-text-muted);
  letter-spacing: 0.5px;
  display: flex;
  align-items: center;
  gap: 6px;
}
.collapse-summary {
  font-size: var(--font-size-xs);
  color: var(--color-text-muted);
  opacity: 0.7;
  max-width: 240px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  margin-left: var(--spacing-sm);
}
.policy-grid {
  border: 1px solid var(--color-border-subtle);
  border-radius: 6px;
  padding: 10px;
  background: var(--color-bg-elevated);
}
.policy-row {
  display: flex;
  gap: 14px;
  flex-wrap: wrap;
  margin-bottom: 4px;
}
.policy-row:last-child {
  margin-bottom: 0;
}
.pol-item {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--color-text-secondary);
}
</style>
