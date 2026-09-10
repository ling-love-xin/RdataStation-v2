<template>
  <div class="modal-overlay" @click.self="$emit('close')">
    <div class="modal">
      <div class="modal-header">
        <h3>{{ t('analyticsResource.createFolder') }}</h3>
        <button class="close-btn" @click="$emit('close')">✕</button>
      </div>

      <div class="modal-body">
        <div class="form-group">
          <label>{{ t('analyticsResource.folderName') }} *</label>
          <input
            v-model="form.name"
            type="text"
            class="form-input"
            :placeholder="t('analyticsResource.folderNamePlaceholder')"
          />
        </div>

        <div class="form-group">
          <label>{{ t('analyticsResource.scope') }} *</label>
          <select v-model="form.scope" class="form-input">
            <option value="global">🌍 {{ t('analyticsResource.global') }}</option>
            <option value="project">📂 {{ t('analyticsResource.project') }}</option>
            <option value="session">📌 {{ t('analyticsResource.session') }}</option>
          </select>
        </div>

        <div class="form-group">
          <label>{{ t('analyticsResource.color') }}</label>
          <div class="color-picker">
            <input v-model="form.color" type="color" class="color-input" />
            <span
              class="color-preview"
              :style="{ backgroundColor: form.color || '#6366f1' }"
            ></span>
          </div>
        </div>

        <div class="form-group">
          <label>{{ t('analyticsResource.icon') }}</label>
          <div class="icon-selector">
            <button
              v-for="icon in icons"
              :key="icon"
              :class="['icon-btn', { active: form.icon === icon }]"
              type="button"
              @click="form.icon = icon"
            >
              {{ icon }}
            </button>
          </div>
        </div>
      </div>

      <div class="modal-footer">
        <button class="btn btn-secondary" @click="$emit('close')">
          {{ t('analyticsResource.cancel') }}
        </button>
        <button class="btn btn-primary" :disabled="!isValid" @click="handleCreate">
          {{ t('analyticsResource.create') }}
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import { useI18n } from 'vue-i18n'

import type { CreateFolderRequest, ResourceScope } from '../../types'

const { t } = useI18n()

const emit = defineEmits<{
  close: []
  create: [input: CreateFolderRequest]
}>()

const form = ref({
  name: '',
  scope: 'project' as ResourceScope,
  parent_folder_id: undefined as string | undefined,
  color: '#6366f1',
  icon: '📁',
})

const icons = ['📁', '📂', '🗂️', '📦', '💼', '📋', '📊', '📈', '🎯', '⭐']

const isValid = computed(() => {
  return form.value.name.trim() !== ''
})

function handleCreate() {
  const input: CreateFolderRequest = {
    name: form.value.name.trim(),
    scope: form.value.scope,
    parent_folder_id: form.value.parent_folder_id,
    color: form.value.color,
    icon: form.value.icon,
  }

  emit('create', input)
}
</script>

<style scoped>
.modal-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: var(--color-bg-overlay);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}

.modal {
  background: var(--bg-primary);
  border-radius: var(--radius-xl);
  width: 90%;
  max-width: 500px;
  max-height: 90vh;
  overflow: hidden;
  display: flex;
  flex-direction: column;
  box-shadow: var(--shadow-lg);
}

.modal-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: var(--size-lg) var(--size-xl);
  border-bottom: 1px solid var(--border-color);
}

.modal-header h3 {
  margin: 0;
  font-size: var(--font-size-xl);
  font-weight: 600;
  color: var(--text-primary);
}

.close-btn {
  background: none;
  border: none;
  font-size: var(--font-size-xxl);
  cursor: pointer;
  color: var(--text-tertiary);
  transition: color 0.15s;
}

.close-btn:hover {
  color: var(--text-primary);
}

.modal-body {
  padding: var(--size-xl);
  overflow-y: auto;
}

.form-group {
  margin-bottom: var(--size-lg);
}

.form-group label {
  display: block;
  margin-bottom: 6px;
  font-size: var(--font-size-sm);
  font-weight: 500;
  color: var(--text-secondary);
}

.form-input {
  width: 100%;
  padding: 6px 12px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  background: var(--bg-secondary);
  color: var(--text-primary);
  font-size: var(--font-size-md);
  box-sizing: border-box;
  height: var(--height-input);
}

.form-input:focus {
  outline: none;
  border-color: var(--primary-color);
}

.color-picker {
  display: flex;
  align-items: center;
  gap: var(--size-md);
}

.color-input {
  width: 36px;
  height: 36px;
  padding: 0;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  cursor: pointer;
}

.color-preview {
  width: 36px;
  height: 36px;
  border-radius: var(--radius-md);
  border: 1px solid var(--border-color);
}

.icon-selector {
  display: flex;
  flex-wrap: wrap;
  gap: var(--size-sm);
}

.icon-btn {
  width: 36px;
  height: 36px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  background: var(--bg-secondary);
  font-size: var(--font-size-xxl);
  cursor: pointer;
  transition: all 0.2s;
}

.icon-btn:hover {
  border-color: var(--primary-color);
}

.icon-btn.active {
  border-color: var(--primary-color);
  background: var(--primary-light);
}

.modal-footer {
  display: flex;
  justify-content: flex-end;
  gap: var(--size-md);
  padding: var(--size-lg) var(--size-xl);
  border-top: 1px solid var(--border-color);
}

.btn {
  padding: 6px 16px;
  border: none;
  border-radius: var(--radius-md);
  font-size: var(--font-size-md);
  cursor: pointer;
  transition: all 0.2s;
  height: var(--height-btn);
}

.btn.btn-secondary {
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  color: var(--text-secondary);
}

.btn.btn-secondary:hover {
  border-color: var(--text-secondary);
}

.btn.btn-primary {
  background: var(--primary-color);
  color: white;
}

.btn.btn-primary:hover {
  background: var(--primary-dark);
}

.btn.btn-primary:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
</style>
