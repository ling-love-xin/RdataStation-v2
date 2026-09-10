<template>
  <div class="dep-editor">
    <div class="dep-header">
      <span class="dep-title">列依赖配置</span>
      <NButton size="tiny" quaternary @click="$emit('close')">
        <template #icon><X :size="14" /></template>
      </NButton>
    </div>

    <div v-if="!hasDependency" class="dep-empty">
      <NButton size="small" secondary @click="addDependency">
        <template #icon><Plus :size="14" /></template>
        添加依赖
      </NButton>
    </div>

    <div v-else class="dep-form">
      <div class="dep-field">
        <label class="dep-label">依赖类型</label>
        <NSelect
          v-model:value="localDepType"
          size="small"
          :options="depTypeOptions"
          @update:value="onTypeChange"
        />
      </div>

      <div class="dep-field">
        <label class="dep-label">源列</label>
        <NSelect
          v-model:value="localSourceColumns"
          size="small"
          multiple
          :options="columnOptions"
          placeholder="选择依赖的源列"
        />
      </div>

      <div
        v-if="localDepType === 'Expression' || localDepType === 'Template'"
        class="dep-field"
      >
        <label class="dep-label">{{ localDepType === 'Expression' ? '表达式' : '模板' }}</label>
        <NInput
          v-model:value="localExpression"
          size="small"
          :placeholder="
            localDepType === 'Expression'
              ? '例: price * quantity'
              : '例: {first_name} {last_name}'
          "
        />
      </div>

      <div class="dep-actions">
        <NButton size="small" type="primary" @click="applyDependency">应用</NButton>
        <NButton size="small" @click="removeDependency">移除依赖</NButton>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Plus, X } from 'lucide-vue-next'
import { NButton, NSelect, NInput } from 'naive-ui'
import { ref, computed, watch } from 'vue'

import type { ColumnDependency, DependencyType } from '@/shared/api/mock-api'

const props = defineProps<{
  dependency: ColumnDependency | null
  availableColumns: string[]
}>()

const emit = defineEmits<{
  close: []
  update: [dep: ColumnDependency | null]
}>()

const depTypeOptions = [
  { label: '计算表达式', value: 'Expression' },
  { label: '模板拼接', value: 'Template' },
  { label: '外键引用', value: 'ForeignKey' },
  { label: '序列依赖', value: 'Sequence' },
  { label: '加权随机', value: 'Weighted' },
]

const localDepType = ref<DependencyType>(props.dependency?.depType ?? 'Expression')
const localSourceColumns = ref<string[]>(props.dependency?.sourceColumns ?? [])
const localExpression = ref<string>(props.dependency?.expression ?? '')

const columnOptions = computed(() =>
  props.availableColumns.map(c => ({ label: c, value: c }))
)

const hasDependency = computed(() => props.dependency !== null)

watch(
  () => props.dependency,
  (dep) => {
    if (dep) {
      localDepType.value = dep.depType
      localSourceColumns.value = dep.sourceColumns
      localExpression.value = dep.expression ?? ''
    }
  }
)

function onTypeChange() {
  localExpression.value = ''
}

function addDependency() {
  localDepType.value = 'Expression'
  localSourceColumns.value = []
  localExpression.value = ''
  applyDependency()
}

function applyDependency() {
  const dep: ColumnDependency = {
    depType: localDepType.value,
    sourceColumns: localSourceColumns.value,
    expression: localExpression.value || null,
    refTable: null,
    refColumn: null,
    weights: null,
  }
  emit('update', dep)
}

function removeDependency() {
  emit('update', null)
}
</script>

<style scoped>
.dep-editor {
  padding: 8px;
  border: 1px solid var(--n-border-color);
  border-radius: 6px;
  background: var(--n-color-embedded);
}
.dep-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 8px;
}
.dep-title {
  font-size: 13px;
  font-weight: 600;
}
.dep-empty {
  text-align: center;
  padding: 8px 0;
}
.dep-form {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.dep-field {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.dep-label {
  font-size: 12px;
  color: var(--n-text-color-3);
}
.dep-actions {
  display: flex;
  gap: 8px;
}
</style>