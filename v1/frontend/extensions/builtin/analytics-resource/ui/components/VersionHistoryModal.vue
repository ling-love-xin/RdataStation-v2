<template>
  <div class="modal-overlay" @click.self="$emit('close')">
    <div class="modal large">
      <div class="modal-header">
        <h3>📜 {{ t('analyticsResource.versionHistory') }}</h3>
        <button class="close-btn" @click="$emit('close')">✕</button>
      </div>

      <div class="modal-body">
        <div v-if="loading" class="loading-state">
          <p>{{ t('analyticsResource.loading') }}</p>
        </div>

        <div v-else-if="versions.length === 0" class="empty-state">
          <span class="empty-icon">📜</span>
          <p>{{ t('analyticsResource.noVersionHistory') }}</p>
        </div>

        <div v-else class="version-list">
          <div
            v-for="(v, index) in versions"
            :key="v.id"
            :class="['version-item', { current: index === 0 }]"
          >
            <div class="version-badge">
              <span v-if="index === 0" class="current-label">
                {{ t('analyticsResource.currentVersion') }}
              </span>
              v{{ v.version }}
            </div>

            <div class="version-info">
              <div class="version-meta">
                {{ formatDate(v.created_at) }}
              </div>
              <div class="version-snapshot">
                <pre>{{ formatSnapshot(v.snapshot) }}</pre>
              </div>
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

import type { ResourceVersion } from '../../types'

const { t } = useI18n()

const props = defineProps<{
  resourceId: string
}>()

defineEmits<{
  close: []
}>()

const store = useAnalyticsResourceStore()
const versions = ref<ResourceVersion[]>([])
const loading = ref(false)

function formatDate(dateStr: string) {
  return new Date(dateStr).toLocaleString('zh-CN')
}

function formatSnapshot(snapshot: Record<string, unknown>) {
  return JSON.stringify(snapshot, null, 2)
}

onMounted(async () => {
  try {
    loading.value = true
    versions.value = await store.getResourceVersions(props.resourceId)
  } catch (error) {
    console.error('Failed to load versions:', error)
  } finally {
    loading.value = false
  }
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
  max-width: 600px;
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

.loading-state,
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

.version-list {
  display: flex;
  flex-direction: column;
  gap: var(--size-md);
}

.version-item {
  display: flex;
  gap: var(--size-lg);
  padding: var(--size-lg);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-lg);
  background: var(--bg-secondary);
}

.version-item.current {
  border-color: var(--primary-color);
  background: var(--primary-light, rgba(22, 93, 255, 0.05));
}

.version-badge {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--spacing-xs);
  min-width: 60px;
  font-size: var(--font-size-lg);
  font-weight: 600;
  color: var(--text-primary);
}

.current-label {
  font-size: var(--font-size-xs);
  font-weight: 500;
  color: var(--primary-color);
  background: var(--primary-light, rgba(22, 93, 255, 0.1));
  padding: 2px 6px;
  border-radius: var(--border-radius-lg);
}

.version-info {
  flex: 1;
  min-width: 0;
}

.version-meta {
  font-size: var(--font-size-sm);
  color: var(--text-tertiary);
  margin-bottom: var(--size-sm);
}

.version-snapshot pre {
  margin: 0;
  font-family: var(--font-mono);
  font-size: var(--font-size-xs);
  color: var(--text-secondary);
  background: var(--bg-tertiary);
  padding: var(--size-md);
  border-radius: var(--radius-md);
  overflow-x: auto;
  max-height: 160px;
  white-space: pre-wrap;
  word-break: break-all;
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

.btn-secondary {
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  color: var(--text-secondary);
}

.btn-secondary:hover {
  border-color: var(--text-secondary);
}
</style>
