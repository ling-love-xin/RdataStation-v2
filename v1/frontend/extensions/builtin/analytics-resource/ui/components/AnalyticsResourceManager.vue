<template>
  <div class="analytics-resource-manager">
    <div class="header">
      <h2>{{ t('analyticsResource.title') }}</h2>
      <div class="header-actions">
        <NButton type="primary" @click="handleOpenCreate">
          <template #icon>
            <Plus :size="16" />
          </template>
          {{ t('analyticsResource.create') }}
        </NButton>
      </div>
    </div>

    <div class="content">
      <div v-if="store.resources.length === 0 && !store.loading" class="empty-state">
        <Database :size="48" />
        <p>{{ t('analyticsResource.empty') }}</p>
      </div>

      <div v-else class="resource-list">
        <div
          v-for="resource in store.resources"
          :key="resource.id"
          class="resource-card"
          @click="handleResourceClick(resource)"
        >
          <div class="resource-icon">
            <Database :size="24" />
          </div>
          <div class="resource-info">
            <h3>{{ resource.name }}</h3>
            <p v-if="resource.alias" class="resource-alias">{{ resource.alias }}</p>
            <div class="resource-meta">
              <NTag :bordered="false" size="small">
                {{ resource.resource_type }}
              </NTag>
              <span class="resource-date">{{ formatDate(resource.updated_at) }}</span>
            </div>
          </div>
          <div class="resource-actions">
            <NButton quaternary size="small" @click.stop="handleEdit(resource)">
              <template #icon>
                <Edit :size="16" />
              </template>
            </NButton>
            <NButton quaternary size="small" @click.stop="handleDelete(resource)">
              <template #icon>
                <Trash2 :size="16" />
              </template>
            </NButton>
          </div>
        </div>
      </div>
    </div>

    <CreateResourceModal
      :show="showCreateModal"
      :edit-resource="editingResource"
      @close="handleModalClose"
      @create="handleCreate"
      @update="handleUpdate"
    />
  </div>
</template>

<script setup lang="ts">
import { Database, Edit, Plus, Trash2 } from 'lucide-vue-next'
import { NButton, NTag } from 'naive-ui'
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'

import CreateResourceModal from './CreateResourceModal.vue'
import { useAnalyticsResourceStore } from '../stores/analytics-resource-store'

import type { AnalyticsResource, CreateResourceRequest } from '../../types'

const { t } = useI18n()
const store = useAnalyticsResourceStore()

const showCreateModal = ref(false)
const editingResource = ref<AnalyticsResource | undefined>(undefined)

function formatDate(dateStr: string): string {
  return new Date(dateStr).toLocaleDateString()
}

function handleResourceClick(resource: AnalyticsResource) {
  store.selectResource(resource.id)
}

function handleOpenCreate() {
  editingResource.value = undefined
  showCreateModal.value = true
}

function handleEdit(resource: AnalyticsResource) {
  editingResource.value = resource
  showCreateModal.value = true
}

function handleDelete(resource: AnalyticsResource) {
  store.deleteResource(resource.id)
}

function handleModalClose() {
  showCreateModal.value = false
  editingResource.value = undefined
}

function handleCreate(input: CreateResourceRequest) {
  store.createResource(input)
  showCreateModal.value = false
  editingResource.value = undefined
}

function handleUpdate(id: string, input: CreateResourceRequest) {
  store.updateResource(id, input)
  showCreateModal.value = false
  editingResource.value = undefined
}
</script>

<style scoped>
.analytics-resource-manager {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: var(--spacing-md);
}

.header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: var(--spacing-lg);
}

.header h2 {
  font-size: var(--font-size-lg);
  font-weight: 600;
  color: var(--text-primary);
}

.content {
  flex: 1;
  overflow-y: auto;
}

.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: var(--text-tertiary);
  gap: var(--spacing-md);
}

.resource-list {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
  gap: var(--spacing-md);
}

.resource-card {
  display: flex;
  align-items: center;
  gap: var(--spacing-md);
  padding: var(--spacing-md);
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: var(--border-radius-md);
  cursor: pointer;
  transition: all 0.2s;
}

.resource-card:hover {
  border-color: var(--primary-color);
}

.resource-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 48px;
  height: 48px;
  background: var(--bg-tertiary);
  border-radius: var(--border-radius-sm);
  color: var(--primary-color);
}

.resource-info {
  flex: 1;
}

.resource-info h3 {
  font-size: var(--font-size-md);
  font-weight: 500;
  color: var(--text-primary);
  margin: 0 0 var(--spacing-xs) 0;
}

.resource-alias {
  font-size: var(--font-size-sm);
  color: var(--text-secondary);
  margin: 0 0 var(--spacing-xs) 0;
}

.resource-meta {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  font-size: var(--font-size-xs);
  color: var(--text-tertiary);
}

.resource-actions {
  display: flex;
  gap: var(--spacing-xs);
}
</style>
