<template>
  <div class="quick-filter-input">
    <div class="filter-row">
      <NInput
        :value="expression"
        size="tiny"
        :placeholder="t('resultPanel.quickFilterPlaceholder')"
        clearable
        @update:value="onInput"
        @keydown.enter="onApply"
      >
        <template #prefix>
          <Search :size="12" />
        </template>
      </NInput>
      <NButton size="tiny" quaternary :disabled="!expression" @click="onClear">
        <template #icon><X :size="12" /></template>
      </NButton>
    </div>
    <div class="filter-info">
      <span v-if="visibleCount !== null && totalCount !== null" class="info-text">
        {{ t('resultPanel.filterResult', { total: totalCount, filtered: visibleCount }) }}
      </span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Search, X } from 'lucide-vue-next'
import { NInput, NButton } from 'naive-ui'
import { onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

defineProps<{
  expression: string
  visibleCount: number | null
  totalCount: number | null
}>()

const emit = defineEmits<{
  'update:expression': [string]
  apply: [string]
  clear: []
}>()

let timer: ReturnType<typeof setTimeout> | null = null

function onInput(val: string | null) {
  const v = val || ''
  emit('update:expression', v)
  if (timer) clearTimeout(timer)
  timer = setTimeout(() => emit('apply', v), 300)
}

function onApply() {
  /* handled by debounce */
}
function onClear() {
  emit('clear')
}

onUnmounted(() => {
  if (timer) {
    clearTimeout(timer)
    timer = null
  }
})
</script>

<style scoped>
.quick-filter-input {
  padding: 4px 8px;
  border-bottom: 1px solid var(--border-color, #333);
  flex-shrink: 0;
}
.filter-row {
  display: flex;
  align-items: center;
  gap: 4px;
}
.filter-info {
  margin-top: 2px;
}
.info-text {
  font-size: 11px;
  color: var(--text-tertiary, #888);
}
</style>
