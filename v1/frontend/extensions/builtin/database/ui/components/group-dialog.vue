<template>
  <div v-if="visible" class="group-dialog-overlay" @click="handleClose">
    <div class="group-dialog" @click.stop>
      <div class="dialog-header">
        <h3>{{ isEdit ? t('navigator.editGroup') : t('navigator.newGroupTitle') }}</h3>
        <button class="close-btn" :aria-label="t('common.cancel')" @click="handleClose">
          <X :size="16" aria-hidden="true" />
        </button>
      </div>

      <div class="dialog-body">
        <div class="form-group">
          <label>{{ t('navigator.groupName') }}</label>
          <input
            v-model="form.name"
            type="text"
            class="form-input"
            :placeholder="t('navigator.groupNamePlaceholder')"
            autofocus
            @keyup.enter="handleSubmit"
          />
        </div>

        <div class="form-group">
          <label>{{ t('navigator.groupDescription') }}</label>
          <input
            v-model="form.description"
            type="text"
            class="form-input"
            :placeholder="t('navigator.groupDescriptionPlaceholder')"
          />
        </div>

        <div class="form-group">
          <label>{{ t('navigator.groupColor') }}</label>
          <div class="color-picker">
            <button
              v-for="color in colors"
              :key="color"
              class="color-btn"
              :class="{ active: form.color === color }"
              :style="{ backgroundColor: color }"
              :title="color"
              @click="form.color = color"
            />
          </div>
        </div>
      </div>

      <div class="dialog-footer">
        <button class="btn btn-secondary" @click="handleClose">{{ t('common.cancel') }}</button>
        <button class="btn btn-primary" :disabled="!form.name.trim()" @click="handleSubmit">
          {{ isEdit ? t('common.save') : t('navigator.create') }}
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { X } from 'lucide-vue-next'
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import { GROUP_COLORS } from '../types/group'

const { t } = useI18n()

interface Props {
  visible: boolean
  isEdit?: boolean
  initialData?: {
    name: string
    description?: string
    color?: string
  }
}

const props = withDefaults(defineProps<Props>(), {
  isEdit: false,
})

const emit = defineEmits<{
  close: []
  submit: [data: { name: string; description?: string; color?: string }]
}>()

const colors = GROUP_COLORS

const form = ref({
  name: '',
  description: '',
  color: colors[0],
})

watch(
  () => props.visible,
  newVal => {
    if (newVal) {
      if (props.isEdit && props.initialData) {
        form.value = {
          name: props.initialData.name,
          description: props.initialData.description || '',
          color: props.initialData.color || colors[0],
        }
      } else {
        form.value = {
          name: '',
          description: '',
          color: colors[0],
        }
      }
    }
  }
)

function handleClose() {
  emit('close')
}

function handleSubmit() {
  if (!form.value.name.trim()) return

  emit('submit', {
    name: form.value.name.trim(),
    description: form.value.description.trim() || undefined,
    color: form.value.color,
  })

  form.value = {
    name: '',
    description: '',
    color: colors[0],
  }
}
</script>

<style scoped>
.group-dialog-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}

.group-dialog {
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: 8px;
  width: 360px;
  max-width: 90%;
  box-shadow: 0 10px 40px rgba(0, 0, 0, 0.2);
}

.dialog-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 16px;
  border-bottom: 1px solid var(--border-color);
}

.dialog-header h3 {
  margin: 0;
  font-size: 16px;
  font-weight: 600;
  color: var(--text-primary);
}

.close-btn {
  width: 24px;
  height: 24px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: none;
  border-radius: 4px;
  cursor: pointer;
  color: var(--text-secondary);
  transition: all 0.15s;
}

.close-btn:hover {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}

.dialog-body {
  padding: 16px;
}

.form-group {
  margin-bottom: 16px;
}

.form-group label {
  display: block;
  font-size: 12px;
  font-weight: 500;
  color: var(--text-secondary);
  margin-bottom: 6px;
}

.form-input {
  width: 100%;
  padding: 8px 12px;
  border: 1px solid var(--border-color);
  border-radius: 4px;
  font-size: 13px;
  background: var(--bg-secondary);
  color: var(--text-primary);
  outline: none;
  transition: border-color 0.15s;
  box-sizing: border-box;
}

.form-input:focus {
  border-color: var(--primary-color);
}

.form-input::placeholder {
  color: var(--text-tertiary);
}

.color-picker {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}

.color-btn {
  width: 24px;
  height: 24px;
  border-radius: 50%;
  border: 2px solid transparent;
  cursor: pointer;
  transition: all 0.15s;
}

.color-btn:hover {
  transform: scale(1.1);
}

.color-btn.active {
  border-color: var(--text-primary);
  transform: scale(1.15);
}

.dialog-footer {
  display: flex;
  gap: 8px;
  justify-content: flex-end;
  padding: 16px;
  border-top: 1px solid var(--border-color);
}

.btn {
  padding: 6px 16px;
  border: none;
  border-radius: 4px;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.15s;
}

.btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.btn-primary {
  background: var(--primary-color);
  color: white;
}

.btn-primary:hover:not(:disabled) {
  background: var(--primary-dark);
}

.btn-secondary {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}

.btn-secondary:hover {
  background: var(--bg-secondary);
}
</style>
