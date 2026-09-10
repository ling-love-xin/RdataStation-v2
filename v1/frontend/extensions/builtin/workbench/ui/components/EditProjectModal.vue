<template>
  <NModal
    :show="visible"
    :mask-closable="false"
    :trap-focus="true"
    transform-origin="center"
    @update:show="handleCancel"
  >
    <div class="modal-container">
      <header class="modal-header">
        <h2>{{ t('workbench.editProjectInfo') }}</h2>
        <NButton size="tiny" quaternary @click="handleCancel">
          <template #icon>
            <X :size="18" />
          </template>
        </NButton>
      </header>
      <div class="modal-body">
        <div class="form-group">
          <label class="form-label">{{ t('workbench.projectName') }}</label>
          <NInput
            ref="nameInputRef"
            v-model:value="form.name"
            :placeholder="t('workbench.projectName')"
            size="small"
          />
        </div>
        <div class="form-group">
          <label class="form-label">{{ t('workbench.projectDescription') }}</label>
          <NInput
            v-model:value="form.description"
            type="textarea"
            :placeholder="t('workbench.projectDescriptionPlaceholder')"
            :rows="3"
            size="small"
          />
        </div>
        <div class="form-group">
          <label class="form-label">{{ t('workbench.projectLocation') }}</label>
          <div class="readonly-path">{{ project?.path }}</div>
        </div>
        <div class="form-row">
          <div class="form-group form-group-half">
            <label class="form-label">{{ t('workbench.projectCreatedAt') }}</label>
            <div class="readonly-text">{{ formatDateTime(project?.createdAt) }}</div>
          </div>
          <div class="form-group form-group-half">
            <label class="form-label">{{ t('workbench.projectLastOpened') }}</label>
            <div class="readonly-text">{{ formatDateTime(project?.updatedAt) }}</div>
          </div>
        </div>
      </div>
      <footer class="modal-footer">
        <NButton size="small" quaternary @click="handleCancel">
          {{ t('common.cancel') }}
        </NButton>
        <NButton
          size="small"
          type="primary"
          :disabled="!canSubmit"
          :loading="isSubmitting"
          @click="handleConfirm"
        >
          {{ t('common.save') }}
        </NButton>
      </footer>
    </div>
  </NModal>
</template>

<script setup lang="ts">
import { X } from 'lucide-vue-next'
import { NButton, NInput, NModal, useMessage } from 'naive-ui'
import { computed, ref, watch, nextTick } from 'vue'
import { useI18n } from 'vue-i18n'

import { useProjectStore } from '@/core/project/stores/project'
import type { Project } from '@/core/project/stores/project'

import { formatDateTime } from '../utils/format'

interface Props {
  visible: boolean
  project: Project | null
}

const props = defineProps<Props>()

const emit = defineEmits<{
  confirm: [id: string, name: string, description?: string]
  cancel: []
}>()

const { t } = useI18n()
const message = useMessage()
const projectStore = useProjectStore()

const isSubmitting = ref(false)
const nameInputRef = ref()
const form = ref({
  name: '',
  description: '',
})

const canSubmit = computed(() => form.value.name.trim().length > 0)

watch(
  () => props.visible,
  isVisible => {
    if (isVisible && props.project) {
      form.value.name = props.project.name
      form.value.description = props.project.description || ''
      nextTick(() => {
        nameInputRef.value?.focus()
      })
    }
  }
)

async function handleConfirm() {
  if (!props.project || !canSubmit.value) return

  isSubmitting.value = true
  try {
    await projectStore.updateProjectInfo(
      props.project.id,
      form.value.name.trim(),
      form.value.description.trim() || undefined
    )
    message.success(t('workbench.updateSuccess'))
    emit(
      'confirm',
      props.project.id,
      form.value.name.trim(),
      form.value.description.trim() || undefined
    )
  } catch {
    message.error(t('workbench.updateFailed'))
  } finally {
    isSubmitting.value = false
  }
}

function handleCancel() {
  emit('cancel')
}
</script>

<style scoped>
.modal-container {
  background: var(--color-bg-primary);
  border: 1px solid var(--color-border);
  border-radius: var(--border-radius-lg);
  width: 440px;
  max-width: 90vw;
}

.modal-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--spacing-md) var(--spacing-lg);
  border-bottom: 1px solid var(--color-border-subtle);
}

.modal-header h2 {
  margin: 0;
  font-size: var(--font-size-lg);
  font-weight: 600;
  color: var(--color-text-primary);
}

.modal-body {
  padding: var(--spacing-lg);
  display: flex;
  flex-direction: column;
  gap: var(--spacing-md);
}

.form-group {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
}

.form-row {
  display: flex;
  gap: var(--spacing-md);
}

.form-group-half {
  flex: 1;
}

.form-label {
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
  font-weight: 500;
}

.readonly-path {
  font-size: var(--font-size-sm);
  color: var(--color-text-muted);
  font-family: var(--font-mono);
  background: var(--color-bg-tertiary);
  padding: var(--spacing-xs) var(--spacing-sm);
  border-radius: var(--border-radius-sm);
  word-break: break-all;
}

.readonly-text {
  font-size: var(--font-size-sm);
  color: var(--color-text-muted);
}

.modal-footer {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: var(--spacing-sm);
  padding: var(--spacing-md) var(--spacing-lg);
  border-top: 1px solid var(--color-border-subtle);
}
</style>
