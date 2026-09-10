<template>
  <div class="filter-preset-selector">
    <NSelect
      v-model:value="selectedPresetId"
      :options="presetOptions"
      :placeholder="t('resultPanel.filterPreset.placeholder')"
      clearable
      size="small"
      class="preset-select"
      @update:value="onSelectPreset"
    >
      <template #header>
        <div class="preset-header">
          <span>{{ t('resultPanel.filterPreset.header') }}</span>
          <NButton size="tiny" quaternary @click.stop="showSaveModal = true">
            <Save :size="14" />
          </NButton>
        </div>
      </template>
      <template #action>
        <div v-if="selectedPresetId" class="preset-action">
          <NButton size="tiny" type="error" quaternary @click.stop="showDeleteModal = true">
            <Trash2 :size="14" />
          </NButton>
        </div>
      </template>
    </NSelect>

    <NModal
      v-model:show="showSaveModal"
      preset="dialog"
      :title="t('resultPanel.filterPreset.saveTitle')"
      :show-icon="false"
    >
      <div class="save-form">
        <span class="form-label">{{ t('resultPanel.filterPreset.savePrompt') }}</span>
        <NInput
          v-model:value="saveName"
          :placeholder="t('resultPanel.filterPreset.namePlaceholder')"
          @keyup.enter="confirmSave"
        />
      </div>
      <template #action>
        <NButton size="small" @click="showSaveModal = false">取消</NButton>
        <NButton size="small" type="primary" :disabled="!saveName.trim()" @click="confirmSave"
          >保存</NButton
        >
      </template>
    </NModal>

    <NModal
      v-model:show="showDeleteModal"
      preset="dialog"
      :title="t('resultPanel.filterPreset.deleteTitle')"
      :show-icon="false"
    >
      <div class="delete-confirm">{{
        t('resultPanel.filterPreset.deleteConfirm', { name: selectedPresetName })
      }}</div>
      <template #action>
        <NButton size="small" @click="showDeleteModal = false">取消</NButton>
        <NButton size="small" type="error" @click="confirmDelete">删除</NButton>
      </template>
    </NModal>
  </div>
</template>

<script setup lang="ts">
import { Save, Trash2 } from 'lucide-vue-next'
import { NButton, NInput, NModal, NSelect } from 'naive-ui'
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import { useResultFilterPresets } from '@/extensions/builtin/workbench/ui/composables/useResultFilterPresets'
import type { FilterMode } from '@/extensions/builtin/workbench/ui/types/result'

import type { SelectOption } from 'naive-ui'

export interface PresetSelectEvent {
  id: string
  name: string
  filterMode: FilterMode
  expression: string
}

const props = defineProps<{
  filterMode: FilterMode
  currentExpression: string
}>()

const emit = defineEmits<{
  select: [event: PresetSelectEvent]
  save: [name: string, expression: string, mode: FilterMode]
  delete: [id: string]
}>()

const { t } = useI18n()
const { presets, getPresetsByMode, removePreset } = useResultFilterPresets()

const selectedPresetId = ref<string | null>(null)
const showSaveModal = ref(false)
const showDeleteModal = ref(false)
const saveName = ref('')

const filteredPresets = computed(() => {
  if (props.filterMode) {
    return getPresetsByMode(props.filterMode)
  }
  return presets.value
})

const presetOptions = computed<SelectOption[]>(() =>
  filteredPresets.value.map(p => ({
    label: p.name,
    value: p.id,
    disabled: false,
  }))
)

const selectedPresetName = computed(() => {
  const preset = presets.value.find(p => p.id === selectedPresetId.value)
  return preset?.name ?? ''
})

function onSelectPreset(value: string | null) {
  if (!value) return
  selectedPresetId.value = value
  const preset = presets.value.find(p => p.id === value)
  if (preset) {
    emit('select', {
      id: preset.id,
      name: preset.name,
      filterMode: preset.filterMode,
      expression: preset.expression,
    })
  }
}

function confirmSave() {
  const name = saveName.value.trim()
  if (!name) return
  emit('save', name, props.currentExpression, props.filterMode)
  saveName.value = ''
  showSaveModal.value = false
}

function confirmDelete() {
  if (!selectedPresetId.value) return
  removePreset(selectedPresetId.value)
  emit('delete', selectedPresetId.value)
  selectedPresetId.value = null
  showDeleteModal.value = false
}
</script>

<style scoped>
.filter-preset-selector {
  display: flex;
  align-items: center;
}

.preset-select {
  min-width: 150px;
  max-width: 240px;
}

.preset-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 4px 8px;
  font-size: 12px;
  color: var(--text-color-secondary);
}

.preset-action {
  display: flex;
  justify-content: flex-end;
  padding: 4px 8px;
}

.save-form {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 8px 0;
}

.form-label {
  font-size: 13px;
  color: var(--text-color-secondary);
}

.delete-confirm {
  padding: 12px 0;
  font-size: 14px;
}
</style>
