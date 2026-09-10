<template>
  <NTooltip :show-arrow="false" placement="right" :delay="500">
    <div
      class="virtual-tree-node"
      :class="{
        'is-expanded': node.isExpanded,
        'is-selected': isSelected,
        'is-loading': node.isLoading,
        'is-favorite': isFavorite,
      }"
      :style="{ paddingLeft: `${node.level * 16 + 8}px` }"
      role="treeitem"
      :aria-expanded="ariaExpanded"
      :aria-level="node.level + 1"
      :aria-selected="isSelected ? 'true' : 'false'"
      :aria-label="node.label"
      @click="handleClick"
      @dblclick="handleDblClick"
      @contextmenu.prevent="handleContextMenu"
    >
      <span
        class="expand-icon"
        role="button"
        :aria-label="node.isExpanded ? '折叠' : '展开'"
        @click.stop="handleExpand"
      >
        <ChevronRight v-if="!node.isExpanded && !node.isLeaf" :size="14" aria-hidden="true" />
        <ChevronDown v-if="node.isExpanded" :size="14" aria-hidden="true" />
        <span v-if="node.isLeaf" class="leaf-spacer" />
        <Loader2
          v-if="node.isLoading"
          :size="14"
          class="loading-icon"
          aria-label="加载中"
          role="img"
        />
      </span>

      <component
        :is="iconConfig.icon"
        :size="14"
        class="node-icon"
        :style="{ color: iconColor }"
        :aria-label="node.type"
        role="img"
      />

      <Star v-if="isFavorite" :size="12" class="favorite-icon" aria-label="已收藏" role="img" />

      <span class="node-label" :class="{ 'is-highlight': isHighlighted }">
        <template v-if="isHighlighted">
          <span
            v-for="(part, index) in labelParts"
            :key="index"
            :class="{ 'highlight-match': part.isMatch }"
          >
            {{ part.text }}
          </span>
        </template>
        <template v-else>
          {{ node.label }}
        </template>
      </span>

      <span
        v-if="node.connectionStatus === 'connected'"
        class="status-dot connected"
        title="已连接"
      >
        <span class="pulse-ring"></span>
      </span>
      <span
        v-else-if="node.connectionStatus === 'connecting'"
        class="status-dot connecting"
        title="连接中"
      ></span>
      <span
        v-else-if="node.type === 'connection'"
        class="status-dot disconnected"
        title="未连接"
      ></span>

      <span v-if="node.connectionTags?.length" class="connection-tags">
        <span v-for="tag in translatedTags" :key="tag" class="tag">{{ tag }}</span>
      </span>
    </div>

    <template #trigger>
      <div style="display: none"></div>
    </template>

    <template v-if="tooltipContent" #default>
      <div class="node-tooltip">{{ tooltipContent }}</div>
    </template>
  </NTooltip>
</template>

<script setup lang="ts">
import { ChevronRight, ChevronDown, Loader2, Star } from 'lucide-vue-next'
import { NTooltip } from 'naive-ui'
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

import { getNodeIcon } from '../config/node-icons'

import type { VirtualTreeNode } from '../types/virtual-tree'

interface Props {
  node: VirtualTreeNode
  isSelected: boolean
  searchQuery?: string
  favoriteKeys?: Set<string>
}

const props = withDefaults(defineProps<Props>(), {
  searchQuery: '',
  favoriteKeys: () => new Set(),
})

const { t } = useI18n()

const emit = defineEmits<{
  expand: [node: VirtualTreeNode]
  select: [node: VirtualTreeNode]
  'context-menu': [node: VirtualTreeNode, event: MouseEvent]
  dblclick: [node: VirtualTreeNode]
}>()

const iconConfig = computed(() => getNodeIcon(props.node.type))

const iconColor = computed(() => {
  if (props.node.connectionStatus === 'connected') {
    return '#00B42A'
  }
  return iconConfig.value.color || 'var(--text-secondary)'
})

const translatedTags = computed(() => {
  if (!props.node.connectionTags?.length) return []
  return props.node.connectionTags.map(tag => {
    switch (tag) {
      case 'Global':
        return t('workbench.scopeGlobal')
      case 'Project':
        return t('workbench.scopeProject')
      default:
        return tag
    }
  })
})

const isFavorite = computed(() => props.favoriteKeys.has(props.node.key))

const ariaExpanded = computed(() => {
  if (props.node.isLeaf) return undefined
  return props.node.isExpanded ? 'true' : 'false'
})

const isHighlighted = computed(() => {
  return (
    props.searchQuery && props.node.label.toLowerCase().includes(props.searchQuery.toLowerCase())
  )
})

const labelParts = computed(() => {
  if (!props.searchQuery) return [{ text: props.node.label, isMatch: false }]

  const label = props.node.label
  const query = props.searchQuery.toLowerCase()
  const labelLower = label.toLowerCase()
  const parts: Array<{ text: string; isMatch: boolean }> = []

  let lastIndex = 0
  let index = labelLower.indexOf(query)

  while (index !== -1) {
    if (index > lastIndex) {
      parts.push({ text: label.slice(lastIndex, index), isMatch: false })
    }
    parts.push({ text: label.slice(index, index + query.length), isMatch: true })
    lastIndex = index + query.length
    index = labelLower.indexOf(query, lastIndex)
  }

  if (lastIndex < label.length) {
    parts.push({ text: label.slice(lastIndex), isMatch: false })
  }

  return parts
})

function handleClick() {
  emit('select', props.node)
}

function handleDblClick() {
  emit('dblclick', props.node)
}

function handleExpand() {
  if (!props.node.isLeaf) {
    emit('expand', props.node)
  }
}

function handleContextMenu(event: MouseEvent) {
  emit('context-menu', props.node, event)
}

function formatNumber(n: number): string {
  if (n >= 1_000_000_000) return (n / 1_000_000_000).toFixed(1) + 'B'
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + 'M'
  if (n >= 1_000) return (n / 1_000).toFixed(1) + 'K'
  return n.toLocaleString()
}

function formatBytes(bytes: number): string {
  if (bytes >= 1_073_741_824) return (bytes / 1_073_741_824).toFixed(1) + ' GB'
  if (bytes >= 1_048_576) return (bytes / 1_048_576).toFixed(1) + ' MB'
  if (bytes >= 1_024) return (bytes / 1_024).toFixed(1) + ' KB'
  return bytes + ' B'
}

const tooltipContent = computed(() => {
  const { node } = props
  if (node.type === 'index') {
    const d = node.data
    const parts: string[] = []
    if (d.indexType) parts.push(`类型: ${d.indexType}`)
    if (d.isUnique) parts.push('唯一索引')
    if (d.isPrimary) parts.push('主键索引')
    if (d.indexColumnNames?.length) parts.push(`列: ${d.indexColumnNames.join(', ')}`)
    if (d.indexComment) parts.push(`注释: ${d.indexComment}`)
    return parts.length > 0 ? parts.join('\n') : undefined
  }
  if (node.type === 'constraint') {
    const d = node.data
    const parts: string[] = []
    if (d.constraintType) parts.push(`类型: ${d.constraintType}`)
    if (d.constraintColumnNames?.length) parts.push(`列: ${d.constraintColumnNames.join(', ')}`)
    if (d.referencedTable) {
      parts.push(`引用表: ${d.referencedTable}`)
      if (d.referencedColumns?.length) parts.push(`引用列: ${d.referencedColumns.join(', ')}`)
    }
    if (d.updateRule) parts.push(`更新规则: ${d.updateRule}`)
    if (d.deleteRule) parts.push(`删除规则: ${d.deleteRule}`)
    return parts.length > 0 ? parts.join('\n') : undefined
  }
  if (node.type === 'table') {
    const d = node.data
    const parts: string[] = []
    if (d.rowCount != null) parts.push(`行数: ${formatNumber(d.rowCount)}`)
    if (d.dataLength != null) parts.push(`数据大小: ${formatBytes(d.dataLength)}`)
    if (d.indexLength != null) parts.push(`索引大小: ${formatBytes(d.indexLength)}`)
    return parts.length > 0 ? parts.join('\n') : undefined
  }
  if (node.type === 'view') {
    const d = node.data
    const parts: string[] = ['类型: 视图']
    if (d.rowCount != null) parts.push(`行数: ${formatNumber(d.rowCount)}`)
    if (d.dataLength != null) parts.push(`数据大小: ${formatBytes(d.dataLength)}`)
    return parts.length > 0 ? parts.join('\n') : undefined
  }
  if (node.type === 'schema') {
    const d = node.data
    const parts: string[] = []
    if (d.tableCount != null) parts.push(`表数量: ${d.tableCount}`)
    if (d.viewCount != null) parts.push(`视图数量: ${d.viewCount}`)
    if (d.totalSizeBytes != null && d.totalSizeBytes > 0)
      parts.push(`总大小: ${formatBytes(d.totalSizeBytes)}`)
    if (d.rowCountTotal != null && d.rowCountTotal > 0)
      parts.push(`总行数: ${formatNumber(d.rowCountTotal)}`)
    return parts.length > 0 ? parts.join('\n') : undefined
  }
  return undefined
})
</script>

<style scoped>
.virtual-tree-node {
  display: flex;
  align-items: center;
  height: 28px;
  cursor: pointer;
  user-select: none;
  font-size: 13px;
  color: var(--text-primary);
  transition: background-color 0.1s;
}

.virtual-tree-node:hover {
  background-color: var(--bg-tertiary);
}

.virtual-tree-node.is-selected {
  background-color: var(--primary-color);
  color: white;
}

.virtual-tree-node.is-selected .node-icon {
  color: white;
}

.expand-icon {
  width: 16px;
  height: 16px;
  display: flex;
  align-items: center;
  justify-content: center;
  margin-right: 2px;
  flex-shrink: 0;
}

.leaf-spacer {
  width: 14px;
}

.node-icon {
  margin-right: 6px;
  flex-shrink: 0;
  color: var(--text-secondary);
}

.node-icon.icon-connected {
  color: #22c55e;
}

.virtual-tree-node.is-selected .node-icon {
  color: white;
}

.favorite-icon {
  color: #f59e0b;
  margin-right: 4px;
  flex-shrink: 0;
}

.highlight-match {
  background-color: rgba(255, 255, 0, 0.3);
  color: inherit;
  font-weight: 600;
}

.dark .highlight-match {
  background-color: rgba(255, 255, 0, 0.2);
}

.status-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  margin-left: 6px;
  flex-shrink: 0;
  position: relative;
}

.status-dot.connected {
  background-color: #22c55e;
  box-shadow: 0 0 4px rgba(34, 197, 94, 0.5);
}

.status-dot.connected .pulse-ring {
  position: absolute;
  top: 50%;
  left: 50%;
  width: 100%;
  height: 100%;
  border-radius: 50%;
  border: 2px solid #22c55e;
  transform: translate(-50%, -50%);
  animation: pulse-ring 2s ease-out infinite;
}

.status-dot.connecting {
  background-color: #f59e0b;
  animation: blink 1s ease-in-out infinite;
}

.status-dot.disconnected {
  background-color: var(--text-tertiary);
  opacity: 0.5;
}

.node-label {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  min-width: 0;
}

.connection-tags {
  display: flex;
  gap: 4px;
  margin-left: 8px;
  flex-shrink: 0;
}

.tag {
  font-size: 10px;
  padding: 1px 6px;
  background: var(--bg-tertiary);
  border-radius: 3px;
  color: var(--text-secondary);
}

.loading-icon {
  animation: spin 1s linear infinite;
}

@keyframes spin {
  from {
    transform: rotate(0deg);
  }
  to {
    transform: rotate(360deg);
  }
}

@keyframes pulse-ring {
  0% {
    width: 100%;
    height: 100%;
    opacity: 1;
  }
  100% {
    width: 200%;
    height: 200%;
    opacity: 0;
  }
}

@keyframes blink {
  0%,
  100% {
    opacity: 1;
  }
  50% {
    opacity: 0.3;
  }
}

.node-tooltip {
  font-size: 12px;
  line-height: 1.8;
  white-space: pre-line;
  max-width: 320px;
}
</style>
