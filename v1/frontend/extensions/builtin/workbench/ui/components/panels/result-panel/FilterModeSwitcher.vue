<template>
  <div class="filter-mode-switcher">
    <button
      v-for="mode in modes"
      :key="mode.key"
      :class="['mode-btn', { active: modelValue === mode.key }]"
      @click="emit('update:modelValue', mode.key)"
    >
      {{ mode.icon }} {{ mode.label }}
    </button>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from 'vue-i18n'

import type { FilterMode } from '../../../types/result'

interface ModeItem {
  key: FilterMode
  icon: string
  label: string
}

const { t } = useI18n()

defineProps<{ modelValue: FilterMode }>()
const emit = defineEmits<{ 'update:modelValue': [FilterMode] }>()

const modes: ModeItem[] = [
  { key: 'quick', icon: '🔍', label: t('resultPanel.instantFilter') },
  { key: 'sql', icon: '🗄️', label: t('resultPanel.sqlFilter') },
  { key: 'duckdb', icon: '🧠', label: t('resultPanel.duckdbAnalysis') },
]
</script>

<style scoped>
.filter-mode-switcher {
  display: flex;
  align-items: center;
  height: 30px;
  padding: 0 4px;
  gap: 2px;
  background: var(--bg-secondary, #252526);
  border-bottom: 1px solid var(--border-color, #3e3e42);
  flex-shrink: 0;
}
.mode-btn {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 3px 10px;
  font-size: 12px;
  border: none;
  background: transparent;
  color: var(--text-secondary, #858585);
  cursor: pointer;
  border-radius: 3px;
  transition: all 0.15s;
}
.mode-btn:hover {
  background: var(--bg-hover, #3c3c3c);
  color: var(--text-primary);
}
.mode-btn.active {
  background: var(--primary-color, #165dff);
  color: #fff;
}
</style>
