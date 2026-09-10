<template>
  <div class="folders-section">
    <div class="folders-header">
      <span class="folders-title">文件夹</span>
      <button class="btn btn-sm" @click="emit('createFolder')">
        <span class="icon">+</span>
        新建
      </button>
    </div>
    <div class="folders-list">
      <div
        :class="['folder-item', { active: selectedFolderId === null }]"
        @click="selectFolder(null)"
        @dragover.prevent="handleDragOver(null, $event)"
        @dragleave="handleDragLeave"
        @drop="handleDrop(null, $event)"
      >
        <span class="folder-icon">📁</span>
        <span class="folder-name">全部资源</span>
      </div>
      <div
        v-for="folder in folders"
        :key="folder.id"
        :class="[
          'folder-item',
          { active: selectedFolderId === folder.id, 'drag-over': dragOverFolder === folder.id },
        ]"
        @click="selectFolder(folder.id)"
        @dragover.prevent="handleDragOver(folder.id, $event)"
        @dragleave="handleDragLeave"
        @drop="handleDrop(folder.id, $event)"
      >
        <span class="folder-icon">{{ folder.icon || '📁' }}</span>
        <span class="folder-name">{{ folder.name }}</span>
        <span v-if="folder.color" class="folder-color" :style="{ backgroundColor: folder.color }" />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'

import type { AnalyticsFolder, AnalyticsResource } from '../../types'

defineProps<{
  folders: AnalyticsFolder[]
  selectedFolderId: string | null
}>()

const emit = defineEmits<{
  selectFolder: [id: string | null]
  createFolder: []
  dropResource: [folderId: string | null, resources: AnalyticsResource[]]
}>()

const dragOverFolder = ref<string | null>(null)

function selectFolder(id: string | null) {
  emit('selectFolder', id)
}

function handleDragOver(folderId: string | null, event: DragEvent) {
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = 'move'
  }
  dragOverFolder.value = folderId
}

function handleDragLeave() {
  dragOverFolder.value = null
}

function handleDrop(folderId: string | null, event: DragEvent) {
  event.preventDefault()
  dragOverFolder.value = null

  try {
    const data = event.dataTransfer?.getData('application/json')
    if (data) {
      const resources = JSON.parse(data) as AnalyticsResource[]
      emit('dropResource', folderId, resources)
    }
  } catch (e) {
    console.error('Failed to parse dropped data:', e)
  }
}
</script>

<style scoped>
.folders-section {
  padding: var(--size-md) var(--size-lg);
  border-bottom: 1px solid var(--border-color);
}

.folders-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: var(--size-sm);
}

.folders-title {
  font-size: var(--font-size-xs);
  font-weight: 600;
  color: var(--text-tertiary);
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.btn {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  padding: 4px 8px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--font-size-sm);
  cursor: pointer;
  transition: all 0.15s;
}

.btn:hover {
  border-color: var(--primary-color);
  color: var(--primary-color);
}

.btn-sm {
  padding: 4px 8px;
  font-size: var(--font-size-sm);
}

.icon {
  font-size: var(--font-size-lg);
}

.folders-list {
  display: flex;
  flex-wrap: wrap;
  gap: var(--size-sm);
}

.folder-item {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  padding: 6px 10px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  background: var(--bg-primary);
  cursor: pointer;
  transition: all 0.2s;
}

.folder-item:hover {
  border-color: var(--primary-color);
}

.folder-item.active {
  border-color: var(--primary-color);
  background: var(--primary-light);
}

.folder-item.drag-over {
  border-color: var(--success-color);
  background: var(--success-light);
  box-shadow: inset 0 0 0 1px var(--success-color);
}

.folder-icon {
  font-size: var(--font-size-lg);
}

.folder-name {
  font-size: var(--font-size-md);
  color: var(--text-primary);
}

.folder-color {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  margin-left: 4px;
}
</style>
