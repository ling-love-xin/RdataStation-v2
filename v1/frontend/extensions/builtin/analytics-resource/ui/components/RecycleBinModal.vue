<template>
  <div class="modal-overlay" @click.self="$emit('close')">
    <div class="modal large">
      <div class="modal-header">
        <h3>🗑️ {{ t('analyticsResource.recycleBinTitle') }}</h3>
        <button class="close-btn" @click="$emit('close')">✕</button>
      </div>

      <div class="modal-body">
        <div v-if="recycleBin.length === 0" class="empty-state">
          <span class="empty-icon">🗑️</span>
          <p>{{ t('analyticsResource.recycleBinEmpty') }}</p>
        </div>

        <div v-else class="recycle-list">
          <div v-for="item in recycleBin" :key="item.id" class="recycle-item">
            <span class="resource-icon">
              {{ getResourceIcon(item.resource_type) }}
            </span>

            <div class="resource-info">
              <div class="resource-name">{{ item.resource_name }}</div>
              <div class="resource-meta">
                {{ t('analyticsResource.deletedAt') }}: {{ formatDate(item.deleted_at) }}
              </div>
            </div>

            <div class="item-actions">
              <button class="btn btn-sm btn-primary" @click="restoreItem(item)">
                ↩️ {{ t('analyticsResource.restore') }}
              </button>
              <button class="btn btn-sm btn-danger" @click="permanentDeleteItem(item)">
                🗑️ {{ t('analyticsResource.permanentDelete') }}
              </button>
            </div>
          </div>
        </div>
      </div>

      <div class="modal-footer">
        <button class="btn btn-secondary" @click="$emit('close')">
          {{ t('analyticsResource.close') }}
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'

import { useAnalyticsResourceStore } from '../stores/analytics-resource-store'

import type { AnalyticsRecycleItem } from '../../types'

defineEmits<{
  close: []
}>()

const { t } = useI18n()

const store = useAnalyticsResourceStore()
const recycleBin = ref<AnalyticsRecycleItem[]>([])

async function loadRecycleBin() {
  await store.loadRecycleBin()
  recycleBin.value = store.recycleBin
}

function getResourceIcon(type: string) {
  switch (type) {
    case 'connection':
      return '🔌'
    case 'table':
      return '📊'
    case 'file':
      return '📄'
    default:
      return '📦'
  }
}

function formatDate(dateStr: string) {
  return new Date(dateStr).toLocaleString('zh-CN')
}

async function restoreItem(item: AnalyticsRecycleItem) {
  if (!confirm(t('analyticsResource.restoreConfirm', { name: item.resource_name }))) {
    return
  }

  await store.restoreResource(item.id)
  await loadRecycleBin()
}

async function permanentDeleteItem(item: AnalyticsRecycleItem) {
  if (!confirm(t('analyticsResource.permanentDeleteConfirm', { name: item.resource_name }))) {
    return
  }

  await store.permanentDeleteResource(item.id)
  await loadRecycleBin()
}

onMounted(async () => {
  await loadRecycleBin()
})
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

.modal.large {
  max-width: 700px;
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
  flex: 1;
}

.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 48px;
  color: var(--text-tertiary);
}

.empty-icon {
  font-size: var(--font-size-display);
  margin-bottom: var(--size-lg);
  opacity: 0.5;
}

.recycle-list {
  display: flex;
  flex-direction: column;
  gap: var(--size-sm);
}

.recycle-item {
  display: flex;
  align-items: center;
  gap: var(--size-md);
  padding: var(--size-md);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  background: var(--bg-secondary);
}

.resource-icon {
  font-size: var(--font-size-title);
}

.resource-info {
  flex: 1;
  min-width: 0;
}

.resource-name {
  font-size: var(--font-size-md);
  font-weight: 500;
  color: var(--text-primary);
  margin-bottom: 2px;
}

.resource-meta {
  font-size: var(--font-size-xs);
  color: var(--text-tertiary);
}

.item-actions {
  display: flex;
  gap: var(--size-sm);
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

.btn.btn-sm {
  padding: 4px 10px;
  font-size: var(--font-size-sm);
  height: 28px;
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

.btn.btn-danger {
  background: var(--danger-color);
  color: white;
}

.btn.btn-danger:hover {
  background: var(--brand-danger);
}
</style>
