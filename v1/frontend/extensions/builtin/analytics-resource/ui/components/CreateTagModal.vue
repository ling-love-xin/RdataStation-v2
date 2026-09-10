<template>
  <div class="modal-overlay" @click.self="$emit('close')">
    <div class="modal">
      <div class="modal-header">
        <h3>🏷️ {{ t('analyticsResource.createTag') }}</h3>
        <button class="close-btn" @click="$emit('close')">✕</button>
      </div>

      <div class="modal-body">
        <div class="form-group">
          <label class="form-label">{{ t('analyticsResource.tagName') }}</label>
          <input
            v-model="tagName"
            type="text"
            class="form-input"
            :placeholder="t('analyticsResource.tagNamePlaceholder')"
            @keyup.enter="handleCreate"
          />
        </div>
        <div class="form-row">
          <div class="form-group">
            <label class="form-label">{{ t('analyticsResource.color') }}</label>
            <div class="color-picker">
              <button
                v-for="c in presetColors"
                :key="c"
                :class="['color-swatch', { active: tagColor === c }]"
                :style="{ background: c }"
                @click="tagColor = c"
              />
            </div>
          </div>
          <div class="form-group">
            <label class="form-label">{{ t('analyticsResource.scope') }}</label>
            <select v-model="tagScope" class="form-input">
              <option value="project">📂 {{ t('analyticsResource.project') }}</option>
              <option value="global">🌍 {{ t('analyticsResource.global') }}</option>
            </select>
          </div>
        </div>
      </div>

      <div class="modal-footer">
        <button class="btn btn-secondary" @click="$emit('close')">
          {{ t('analyticsResource.cancel') }}
        </button>
        <button class="btn btn-primary" :disabled="!tagName.trim()" @click="handleCreate">
          {{ t('analyticsResource.create') }}
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

const emit = defineEmits<{
  close: []
  create: [name: string, color: string, scope: string]
}>()

const tagName = ref('')
const tagColor = ref('#165DFF')
const tagScope = ref('project')

const presetColors = [
  '#165DFF',
  '#00B42A',
  '#FF7D00',
  '#F53F3F',
  '#722ED1',
  '#14C9C9',
  '#F77234',
  '#3491FA',
]

function handleCreate() {
  if (!tagName.value.trim()) return
  emit('create', tagName.value.trim(), tagColor.value, tagScope.value)
  tagName.value = ''
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
  max-width: 420px;
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
}

.form-group {
  margin-bottom: var(--size-lg);
}

.form-label {
  display: block;
  font-size: var(--font-size-md);
  font-weight: 500;
  color: var(--text-primary);
  margin-bottom: var(--size-sm);
}

.form-input {
  width: 100%;
  padding: 8px 12px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  background: var(--bg-secondary);
  color: var(--text-primary);
  font-size: var(--font-size-md);
  outline: none;
  transition: border-color 0.2s;
  box-sizing: border-box;
}

.form-input:focus {
  border-color: var(--primary-color);
}

.form-row {
  display: flex;
  gap: var(--size-lg);
}

.form-row .form-group {
  flex: 1;
}

.color-picker {
  display: flex;
  gap: var(--size-sm);
  flex-wrap: wrap;
}

.color-swatch {
  width: 28px;
  height: 28px;
  border-radius: var(--radius-sm);
  border: 2px solid transparent;
  cursor: pointer;
  transition: all 0.2s;
}

.color-swatch:hover {
  transform: scale(1.15);
}

.color-swatch.active {
  border-color: var(--text-primary);
  box-shadow:
    0 0 0 2px var(--bg-primary),
    0 0 0 4px currentColor;
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

.btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.btn-secondary {
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  color: var(--text-secondary);
}

.btn-secondary:hover {
  border-color: var(--text-secondary);
}

.btn-primary {
  background: var(--primary-color);
  color: white;
}

.btn-primary:hover {
  background: var(--primary-dark);
}
</style>
