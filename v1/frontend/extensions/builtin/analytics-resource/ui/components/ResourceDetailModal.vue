<template>
  <div class="modal-overlay" @click.self="$emit('close')">
    <div class="modal large">
      <div class="modal-header">
        <div class="header-left">
          <span class="type-icon">{{ getResourceIcon(resource.resource_type) }}</span>
          <h3>{{ resource.alias || resource.name }}</h3>
          <span v-if="resource.alias" class="alias-hint">({{ resource.name }})</span>
        </div>
        <div class="header-right">
          <button
            class="icon-btn"
            :title="t('analyticsResource.edit')"
            @click="$emit('edit', resource)"
          >
            ✏️
          </button>
          <button
            class="icon-btn"
            :title="t('analyticsResource.copy')"
            @click="$emit('copy', resource)"
          >
            📋
          </button>
          <button class="close-btn" @click="$emit('close')">✕</button>
        </div>
      </div>

      <div class="modal-body">
        <div class="detail-grid">
          <div class="detail-section">
            <h4 class="section-title">{{ t('analyticsResource.basicInfo') }}</h4>
            <div class="detail-row">
              <span class="detail-label">{{ t('analyticsResource.id') }}</span>
              <code class="detail-value">{{ resource.id }}</code>
            </div>
            <div class="detail-row">
              <span class="detail-label">{{ t('analyticsResource.name') }}</span>
              <span class="detail-value">{{ resource.name }}</span>
            </div>
            <div class="detail-row">
              <span class="detail-label">{{ t('analyticsResource.type') }}</span>
              <span :class="['type-badge', resource.resource_type]">
                {{ getTypeLabel(resource.resource_type) }}
              </span>
            </div>
            <div class="detail-row">
              <span class="detail-label">{{ t('analyticsResource.scope') }}</span>
              <span :class="['scope-badge', resource.scope]">
                {{ getScopeLabel(resource.scope) }}
              </span>
            </div>
            <div class="detail-row">
              <span class="detail-label">{{ t('analyticsResource.version') }}</span>
              <span class="detail-value">v{{ resource.version }}</span>
            </div>
          </div>

          <div class="detail-section">
            <h4 class="section-title">{{ t('analyticsResource.statistics') }}</h4>
            <div v-if="resource.row_count != null" class="detail-row">
              <span class="detail-label">{{ t('analyticsResource.rowCount') }}</span>
              <span class="detail-value">{{ formatNumber(resource.row_count) }}</span>
            </div>
            <div v-if="resource.column_count != null" class="detail-row">
              <span class="detail-label">{{ t('analyticsResource.columnCount') }}</span>
              <span class="detail-value">{{ resource.column_count }}</span>
            </div>
            <div v-if="resource.file_size != null" class="detail-row">
              <span class="detail-label">{{ t('analyticsResource.fileSize') }}</span>
              <span class="detail-value">{{ formatFileSize(resource.file_size) }}</span>
            </div>
            <div class="detail-row">
              <span class="detail-label">{{ t('analyticsResource.created') }}</span>
              <span class="detail-value">{{ formatDate(resource.created_at) }}</span>
            </div>
            <div class="detail-row">
              <span class="detail-label">{{ t('analyticsResource.updated') }}</span>
              <span class="detail-value">{{ formatDate(resource.updated_at) }}</span>
            </div>
          </div>
        </div>

        <div v-if="tags.length > 0" class="detail-section">
          <h4 class="section-title">{{ t('analyticsResource.tagsLabel') }}</h4>
          <div class="tag-list-inline">
            <span
              v-for="tag in tags"
              :key="tag.id"
              class="tag-badge"
              :style="{ background: tag.color + '20', color: tag.color, borderColor: tag.color }"
            >
              {{ tag.name }}
            </span>
          </div>
        </div>

        <div v-if="folders.length > 0" class="detail-section">
          <h4 class="section-title">{{ t('analyticsResource.foldersLabel') }}</h4>
          <div class="folder-list-inline">
            <span v-for="f in folders" :key="f.id" class="folder-badge">📁 {{ f.name }}</span>
          </div>
        </div>

        <div v-if="resource.source_query" class="detail-section">
          <h4 class="section-title">{{ t('analyticsResource.sourceQuery') }}</h4>
          <pre class="sql-block">{{ resource.source_query }}</pre>
        </div>

        <div v-if="resource.parent_resource_id" class="detail-section">
          <h4 class="section-title">{{ t('analyticsResource.parentResource') }}</h4>
          <code class="detail-value">{{ resource.parent_resource_id }}</code>
        </div>

        <div class="detail-section">
          <h4 class="section-title">{{ t('analyticsResource.config') }}</h4>
          <pre class="json-block">{{ formatJson(resource.config) }}</pre>
        </div>
      </div>

      <div class="modal-footer">
        <button class="btn btn-secondary" @click="$emit('view-versions', resource)">
          📜 {{ t('analyticsResource.viewVersions') }}
        </button>
        <button class="btn btn-secondary" @click="$emit('close')">
          {{ t('analyticsResource.close') }}
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import { useAnalyticsResourceStore } from '../stores/analytics-resource-store'

import type { AnalyticsResource, AnalyticsTag, AnalyticsFolder } from '../../types'

const { t } = useI18n()

const props = defineProps<{
  resource: AnalyticsResource
}>()

defineEmits<{
  close: []
  edit: [resource: AnalyticsResource]
  copy: [resource: AnalyticsResource]
  'view-versions': [resource: AnalyticsResource]
}>()

const store = useAnalyticsResourceStore()
const tags = ref<AnalyticsTag[]>([])
const folders = ref<AnalyticsFolder[]>([])

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

function getTypeLabel(type: string) {
  const map: Record<string, string> = {
    connection: t('analyticsResource.connection'),
    table: t('analyticsResource.table'),
    file: t('analyticsResource.file'),
  }
  return map[type] || type
}

function getScopeLabel(scope: string) {
  const map: Record<string, string> = {
    global: t('analyticsResource.global'),
    project: t('analyticsResource.project'),
    session: t('analyticsResource.session'),
  }
  return map[scope] || scope
}

function formatDate(dateStr: string) {
  if (!dateStr) return '—'
  return new Date(dateStr).toLocaleString('zh-CN')
}

function formatNumber(n: number) {
  return n.toLocaleString('zh-CN')
}

function formatFileSize(bytes: number) {
  if (!bytes) return '—'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let i = 0
  let size = bytes
  while (size >= 1024 && i < units.length - 1) {
    size /= 1024
    i++
  }
  return `${size.toFixed(1)} ${units[i]}`
}

function formatJson(obj: Record<string, unknown>) {
  return JSON.stringify(obj, null, 2)
}

async function loadDetails() {
  try {
    const [resourceTags, resourceFolders] = await Promise.all([
      store.getTagsForResource(props.resource.id),
      Promise.resolve(
        store.folders.filter(f => store.getResourceFolders(props.resource.id).includes(f.id))
      ),
    ])
    tags.value = resourceTags
    folders.value = resourceFolders
  } catch (error) {
    console.error('Failed to load resource details:', error)
  }
}

watch(
  () => props.resource.id,
  () => {
    loadDetails()
  },
  { immediate: true }
)
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
  max-width: 680px;
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

.header-left {
  display: flex;
  align-items: center;
  gap: var(--size-md);
}

.header-left h3 {
  margin: 0;
  font-size: var(--font-size-xl);
  font-weight: 600;
  color: var(--text-primary);
}

.type-icon {
  font-size: var(--font-size-title);
}

.alias-hint {
  font-size: var(--font-size-md);
  color: var(--text-tertiary);
}

.header-right {
  display: flex;
  align-items: center;
  gap: var(--size-sm);
}

.icon-btn {
  background: none;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  font-size: var(--font-size-lg);
  cursor: pointer;
  color: var(--text-secondary);
  padding: 4px 8px;
  transition: all 0.15s;
}

.icon-btn:hover {
  background: var(--bg-secondary);
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

.detail-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--size-xl);
  margin-bottom: var(--size-xl);
}

.detail-section {
  margin-bottom: var(--size-xl);
}

.detail-section:last-child {
  margin-bottom: 0;
}

.section-title {
  margin: 0 0 var(--size-md);
  font-size: var(--font-size-sm);
  font-weight: 600;
  color: var(--text-tertiary);
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.detail-row {
  display: flex;
  align-items: flex-start;
  gap: var(--size-md);
  padding: var(--size-sm) 0;
}

.detail-label {
  min-width: 80px;
  font-size: var(--font-size-md);
  color: var(--text-tertiary);
  flex-shrink: 0;
}

.detail-value {
  font-size: var(--font-size-md);
  color: var(--text-primary);
  word-break: break-all;
}

code.detail-value {
  font-family: var(--font-mono);
  font-size: var(--font-size-xs);
  background: var(--bg-tertiary);
  padding: 2px 6px;
  border-radius: var(--radius-sm);
}

.type-badge,
.scope-badge {
  font-size: var(--font-size-sm);
  padding: 2px 8px;
  border-radius: var(--border-radius-xl);
  font-weight: 500;
}

.type-badge.connection {
  background: var(--resource-type-connection-soft);
  color: var(--resource-type-connection);
}

.type-badge.table {
  background: var(--resource-type-table-soft);
  color: var(--resource-type-table);
}

.type-badge.file {
  background: var(--resource-type-file-soft);
  color: var(--resource-type-file);
}

.scope-badge.global {
  background: var(--resource-scope-global-soft);
  color: var(--resource-scope-global);
}

.scope-badge.project {
  background: var(--resource-scope-project-soft);
  color: var(--resource-scope-project);
}

.scope-badge.session {
  background: var(--resource-scope-session-soft);
  color: var(--resource-scope-session);
}

.tag-list-inline {
  display: flex;
  flex-wrap: wrap;
  gap: var(--spacing-sm);
}

.tag-badge {
  font-size: var(--font-size-sm);
  padding: 2px 10px;
  border-radius: var(--border-radius-xl);
  border: 1px solid;
  font-weight: 500;
}

.folder-list-inline {
  display: flex;
  flex-wrap: wrap;
  gap: var(--spacing-sm);
}

.folder-badge {
  font-size: var(--font-size-sm);
  padding: 2px 10px;
  border-radius: var(--border-radius-xl);
  background: var(--bg-tertiary);
  color: var(--text-secondary);
  border: 1px solid var(--border-color);
}

.sql-block,
.json-block {
  margin: 0;
  font-family: var(--font-mono);
  font-size: var(--font-size-xs);
  color: var(--text-secondary);
  background: var(--bg-tertiary);
  padding: var(--size-md);
  border-radius: var(--radius-md);
  overflow-x: auto;
  max-height: 200px;
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
