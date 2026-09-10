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
        <CircleAlert :size="20" class="warning-icon" />
        <h2>{{ t('workbench.confirmDeleteProject') }}</h2>
        <NButton size="tiny" quaternary @click="handleCancel">
          <template #icon>
            <X :size="18" />
          </template>
        </NButton>
      </header>
      <div class="modal-body">
        <p class="warning-text">{{ t('workbench.deleteProjectWarning') }}</p>
        <div class="deleted-path">{{ project?.path }}</div>
        <div class="form-group">
          <label class="form-label">{{ t('workbench.deleteProjectInputHint') }}</label>
          <NInput
            ref="confirmInputRef"
            v-model:value="confirmName"
            :placeholder="project?.name"
            size="small"
            :status="confirmStatus"
            @keyup.enter="handleConfirm"
          />
        </div>
      </div>
      <footer class="modal-footer">
        <NButton size="small" quaternary @click="handleCancel">
          {{ t('common.cancel') }}
        </NButton>
        <NButton
          size="small"
          type="error"
          :disabled="!canConfirm"
          :loading="isSubmitting"
          @click="handleConfirm"
        >
          {{ t('workbench.confirmDelete') }}
        </NButton>
      </footer>
    </div>
  </NModal>
</template>

<script setup lang="ts">
import { CircleAlert, X } from 'lucide-vue-next'
import { NButton, NInput, NModal, useMessage } from 'naive-ui'
import { computed, ref, watch, nextTick } from 'vue'
import { useI18n } from 'vue-i18n'

import { useProjectStore } from '@/core/project/stores/project'
import type { Project } from '@/core/project/stores/project'

interface Props {
  visible: boolean
  project: Project | null
}

const props = defineProps<Props>()

const emit = defineEmits<{
  confirm: [projectId: string]
  cancel: []
}>()

const { t } = useI18n()
const message = useMessage()
const projectStore = useProjectStore()

const confirmName = ref('')
const confirmInputRef = ref()
const isSubmitting = ref(false)

const canConfirm = computed(() => {
  if (!props.project) return false
  return confirmName.value.trim() === props.project.name
})

type InputStatus = 'success' | 'error' | undefined

const confirmStatus = computed<InputStatus>(() => {
  if (confirmName.value.length === 0) return undefined
  return canConfirm.value ? 'success' : 'error'
})

watch(
  () => props.visible,
  isVisible => {
    if (isVisible) {
      confirmName.value = ''
      nextTick(() => {
        confirmInputRef.value?.focus()
      })
    }
  }
)

async function handleConfirm() {
  if (!props.project || !canConfirm.value) return

  isSubmitting.value = true
  try {
    await projectStore.deleteProjectDisk(props.project.id)
    message.success(t('workbench.deleteSuccess'))
    emit('confirm', props.project.id)
  } catch {
    message.error(t('workbench.deleteFailed'))
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
  gap: var(--spacing-sm);
  padding: var(--spacing-md) var(--spacing-lg);
  border-bottom: 1px solid var(--color-border-subtle);
}

.modal-header h2 {
  margin: 0;
  font-size: var(--font-size-lg);
  font-weight: 600;
  color: var(--color-text-primary);
  flex: 1;
}

.warning-icon {
  color: var(--brand-warning);
  flex-shrink: 0;
}

.modal-body {
  padding: var(--spacing-lg);
  display: flex;
  flex-direction: column;
  gap: var(--spacing-md);
}

.warning-text {
  margin: 0;
  font-size: var(--font-size-md);
  color: var(--color-text-secondary);
  line-height: 1.5;
}

.deleted-path {
  font-size: var(--font-size-sm);
  color: var(--brand-danger);
  font-family: var(--font-mono);
  background: rgba(214, 48, 49, 0.08);
  padding: var(--spacing-sm) var(--spacing-md);
  border-radius: var(--border-radius-sm);
  word-break: break-all;
}

.form-group {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
}

.form-label {
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
  font-weight: 500;
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
