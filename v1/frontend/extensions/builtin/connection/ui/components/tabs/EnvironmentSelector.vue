<template>
  <div class="env-row">
    <span class="sec-title">🏷 {{ label }}</span>
    <NSelect
      :value="modelValue"
      :options="options"
      size="small"
      class="env-select"
      @update:value="$emit('update:modelValue', $event)"
    />
    <NButton size="tiny" text @click="$emit('manage')">{{ manageLabel }}</NButton>
  </div>
</template>

<script setup lang="ts">
import { NSelect, NButton } from 'naive-ui'

import type { SelectOption } from 'naive-ui'

defineProps<{
  modelValue: string | null
  options: SelectOption[]
  label: string
  manageLabel: string
}>()

defineEmits<{
  'update:modelValue': [value: string]
  manage: []
}>()
</script>

<style scoped>
.env-row {
  display: flex;
  align-items: center;
  gap: 10px;
}
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
.env-select {
  min-width: 180px;
}
</style>
