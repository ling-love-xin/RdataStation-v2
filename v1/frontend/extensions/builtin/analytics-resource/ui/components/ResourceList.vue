<template>
  <div
    ref="containerRef"
    class="resource-list"
    @scroll="useVirtualScroll ? handleScroll : undefined"
    @contextmenu.prevent="handleContextMenu"
  >
    <template v-if="useVirtualScroll">
      <div class="virtual-spacer" :style="{ height: `${totalHeight}px` }">
        <div class="virtual-content" :style="{ transform: `translateY(${offsetY}px)` }">
          <div
            v-for="item in visibleItems"
            :key="item.id"
            :class="['resource-item', { selected: isSelected(item.id) }]"
            draggable="true"
            @click="handleClick(item, $event)"
            @dblclick="handleDoubleClick(item)"
            @dragstart="handleDragStart(item, $event)"
            @dragend="handleDragEnd"
          >
            <span v-if="showIcons" class="resource-icon">
              {{ getResourceIcon(item.resource_type) }}
            </span>

            <div class="resource-info">
              <div class="resource-name">{{ item.name }}</div>
              <div v-if="showMetadata" class="resource-meta">
                {{ getResourceMeta(item) }}
              </div>
            </div>

            <span v-if="showScopeTags" :class="['scope-tag', item.scope]">
              {{ getScopeLabel(item.scope) }}
            </span>

            <div v-if="getItemTags(item.id).length > 0" class="tag-badges">
              <span
                v-for="tag in getItemTags(item.id)"
                :key="tag.id"
                class="tag-badge"
                :style="{ background: tag.color + '20', color: tag.color, borderColor: tag.color }"
              >
                {{ tag.name }}
              </span>
            </div>
          </div>
        </div>
      </div>
    </template>

    <template v-else>
      <div
        v-for="item in items"
        :key="item.id"
        :class="['resource-item', { selected: isSelected(item.id) }]"
        draggable="true"
        @click="handleClick(item, $event)"
        @dblclick="handleDoubleClick(item)"
        @dragstart="handleDragStart(item, $event)"
        @dragend="handleDragEnd"
      >
        <span v-if="showIcons" class="resource-icon">
          {{ getResourceIcon(item.resource_type) }}
        </span>

        <div class="resource-info">
          <div class="resource-name">{{ item.name }}</div>
          <div v-if="showMetadata" class="resource-meta">
            {{ getResourceMeta(item) }}
          </div>
        </div>

        <span v-if="showScopeTags" :class="['scope-tag', item.scope]">
          {{ getScopeLabel(item.scope) }}
        </span>

        <div v-if="getItemTags(item.id).length > 0" class="tag-badges">
          <span
            v-for="tag in getItemTags(item.id)"
            :key="tag.id"
            class="tag-badge"
            :style="{ background: tag.color + '20', color: tag.color, borderColor: tag.color }"
          >
            {{ tag.name }}
          </span>
        </div>
      </div>
    </template>

    <div v-if="items.length === 0" class="empty-state">
      <span class="empty-icon">{{ emptyIcon }}</span>
      <p>{{ emptyText || t('analyticsResource.noResources') }}</p>
    </div>

    <ContextMenu ref="contextMenuRef" :items="contextMenuItems" @select="handleContextMenuSelect" />
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import { useI18n } from 'vue-i18n'

import ContextMenu, { type ContextMenuItem } from './ContextMenu.vue'
import { useVirtualScroll as useVirtualScrollComposable } from '../composables/use-virtual-scroll'

import type { AnalyticsResource, AnalyticsResourceDisplaySettings, AnalyticsTag } from '../../types'

const props = withDefaults(
  defineProps<{
    items: AnalyticsResource[]
    selectedIds: string[]
    itemHeight?: number
    emptyIcon?: string
    emptyText?: string
    displaySettings?: AnalyticsResourceDisplaySettings
    resourceTagMap?: Map<string, AnalyticsTag[]>
  }>(),
  {
    itemHeight: 72,
    emptyIcon: '📭',
    emptyText: '',
    displaySettings: () => ({
      showIcons: true,
      showScopeTags: true,
      showMetadata: true,
      enableVirtualScroll: true,
    }),
    resourceTagMap: () => new Map(),
  }
)

const showIcons = computed(() => props.displaySettings?.showIcons ?? true)
const showScopeTags = computed(() => props.displaySettings?.showScopeTags ?? true)
const showMetadata = computed(() => props.displaySettings?.showMetadata ?? true)
const useVirtualScroll = computed(() => props.displaySettings?.enableVirtualScroll ?? true)

const emit = defineEmits<{
  select: [id: string, multiSelect: boolean]
  open: [resource: AnalyticsResource]
  delete: [id: string]
  edit: [resource: AnalyticsResource]
  copy: [resource: AnalyticsResource]
  'view-versions': [resource: AnalyticsResource]
  dragstart: [resources: AnalyticsResource[]]
  dragend: []
}>()

const { t } = useI18n()

const selectedIds = computed(() => props.selectedIds)

const { totalHeight, offsetY, visibleItems, containerRef, handleScroll } =
  useVirtualScrollComposable<AnalyticsResource>({
    itemHeight: props.itemHeight,
    items: computed(() => props.items),
  })

const contextMenuRef = ref<InstanceType<typeof ContextMenu> | null>(null)

const contextMenuItems = computed<ContextMenuItem[]>(() => [
  {
    id: 'open',
    label: t('analyticsResource.open'),
    icon: '📖',
    action: () => {
      const item = props.items.find(i => i.id === selectedIds.value[0])
      if (item) emit('open', item)
    },
  },
  {
    id: 'edit',
    label: t('analyticsResource.edit'),
    icon: '✏️',
    action: () => {
      const item = props.items.find(i => i.id === selectedIds.value[0])
      if (item) emit('edit', item)
    },
  },
  {
    id: 'copy',
    label: t('analyticsResource.copy'),
    icon: '📋',
    action: () => {
      const item = props.items.find(i => i.id === selectedIds.value[0])
      if (item) emit('copy', item)
    },
  },
  {
    id: 'versions',
    label: t('analyticsResource.viewVersions'),
    icon: '📜',
    action: () => {
      const item = props.items.find(i => i.id === selectedIds.value[0])
      if (item) emit('view-versions', item)
    },
  },
  { id: 'separator', label: '---', disabled: true },
  {
    id: 'delete',
    label: t('analyticsResource.delete'),
    icon: '🗑️',
    danger: true,
    action: () => emit('delete', selectedIds.value[0]),
  },
])

function isSelected(id: string) {
  return selectedIds.value.includes(id)
}

function getItemTags(resourceId: string): AnalyticsTag[] {
  return props.resourceTagMap?.get(resourceId) ?? []
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

function getScopeLabel(scope: string) {
  switch (scope) {
    case 'global':
      return '🌍 ' + t('analyticsResource.global')
    case 'project':
      return '📂 ' + t('analyticsResource.project')
    case 'session':
      return '📌 ' + t('analyticsResource.session')
    default:
      return scope
  }
}

function getResourceMeta(resource: AnalyticsResource) {
  const meta: string[] = []
  if (resource.row_count !== undefined && resource.row_count !== null) {
    meta.push(`${resource.row_count.toLocaleString()} ${t('resultPanel.rows')}`)
  }
  if (resource.column_count !== undefined && resource.column_count !== null) {
    meta.push(`${resource.column_count} ${t('resultPanel.column')}`)
  }
  if (resource.file_size !== undefined && resource.file_size !== null) {
    meta.push(`${formatFileSize(resource.file_size)}`)
  }
  return meta.join(' · ')
}

function formatFileSize(bytes: number) {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`
}

function handleClick(item: AnalyticsResource, event: MouseEvent) {
  const multiSelect = event.shiftKey || event.ctrlKey || event.metaKey
  emit('select', item.id, multiSelect)
}

function handleDoubleClick(item: AnalyticsResource) {
  emit('open', item)
}

function handleContextMenu(event: MouseEvent) {
  contextMenuRef.value?.open(event)
}

function handleContextMenuSelect(_item: ContextMenuItem) {
  // ContextMenu handles the action
}

function handleDragStart(item: AnalyticsResource, event: DragEvent) {
  const draggedResources =
    selectedIds.value.length > 0
      ? props.items.filter(i => selectedIds.value.includes(i.id))
      : [item]

  event.dataTransfer?.setData('application/json', JSON.stringify(draggedResources))
  if (event.dataTransfer) {
    event.dataTransfer.effectAllowed = 'move'
  }

  if (event.target instanceof HTMLElement) {
    event.target.style.opacity = '0.5'
  }

  emit('dragstart', draggedResources)
}

function handleDragEnd(event: DragEvent) {
  if (event.target instanceof HTMLElement) {
    event.target.style.opacity = '1'
  }
  emit('dragend')
}
</script>

<style scoped>
.resource-list {
  flex: 1;
  overflow-y: auto;
  padding: var(--size-sm);
  position: relative;
}

.virtual-spacer {
  position: relative;
}

.virtual-content {
  position: absolute;
  left: 0;
  right: 0;
  top: 0;
}

.resource-item {
  display: flex;
  align-items: center;
  gap: var(--size-md);
  padding: var(--size-md);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  margin-bottom: var(--size-sm);
  background: var(--bg-primary);
  cursor: pointer;
  transition: all 0.2s;
  height: 56px;
  box-sizing: border-box;
}

.resource-item:hover {
  border-color: var(--primary-color);
}

.resource-item.selected {
  border-color: var(--primary-color);
  background: var(--primary-light);
}

.resource-icon {
  font-size: var(--font-size-title);
  flex-shrink: 0;
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
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.resource-meta {
  font-size: var(--font-size-xs);
  color: var(--text-tertiary);
}

.scope-tag {
  padding: 2px 6px;
  border-radius: var(--radius-sm);
  font-size: var(--font-size-xs);
  font-weight: 500;
  flex-shrink: 0;
}

.scope-tag.global {
  background: var(--primary-light);
  color: var(--primary-color);
}

.scope-tag.project {
  background: var(--success-light);
  color: var(--success-color);
}

.scope-tag.session {
  background: var(--warning-light);
  color: var(--warning-color);
}

.tag-badges {
  display: flex;
  flex-wrap: wrap;
  gap: var(--spacing-xs);
  margin-top: 4px;
}

.tag-badge {
  font-size: var(--font-size-xs);
  padding: 1px 8px;
  border-radius: var(--border-radius-xl);
  border: 1px solid;
  font-weight: 500;
  white-space: nowrap;
}

.resource-item:hover .tag-badges {
  display: flex;
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
</style>
