<template>
  <div
    class="group-node"
    :class="{
      'is-expanded': group.expanded,
      'is-selected': isSelected,
    }"
    @click="handleClick"
    @contextmenu.prevent="handleContextMenu"
  >
    <span class="expand-icon" @click.stop="handleExpand">
      <ChevronRight v-if="!group.expanded" :size="14" />
      <ChevronDown v-else :size="14" />
    </span>

    <span class="group-icon" :style="{ backgroundColor: group.color }">
      <Folder :size="12" />
    </span>

    <span class="group-label">{{ group.name }}</span>

    <span class="group-count">{{ group.connectionIds.length }}</span>

    <button class="group-actions" @click.stop="showActions = !showActions">
      <MoreHorizontal :size="12" />
    </button>

    <div v-if="showActions" class="actions-menu">
      <button @click="handleEdit">
        <Edit :size="12" />
        编辑
      </button>
      <button @click="handleDelete">
        <Trash2 :size="12" />
        删除
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ChevronRight, ChevronDown, Folder, MoreHorizontal, Edit, Trash2 } from 'lucide-vue-next'
import { ref } from 'vue'

import type { ConnectionGroup } from '../types/group'

interface Props {
  group: ConnectionGroup
  isSelected: boolean
}

const props = defineProps<Props>()

const emit = defineEmits<{
  expand: [groupId: string]
  select: [groupId: string]
  'context-menu': [group: ConnectionGroup, event: MouseEvent]
  edit: [groupId: string]
  delete: [groupId: string]
}>()

const showActions = ref(false)

function handleClick() {
  emit('select', props.group.id)
}

function handleExpand() {
  emit('expand', props.group.id)
}

function handleContextMenu(event: MouseEvent) {
  emit('context-menu', props.group, event)
}

function handleEdit() {
  showActions.value = false
  emit('edit', props.group.id)
}

function handleDelete() {
  showActions.value = false
  emit('delete', props.group.id)
}
</script>

<style scoped>
.group-node {
  display: flex;
  align-items: center;
  height: 28px;
  padding-left: 8px;
  cursor: pointer;
  user-select: none;
  font-size: 13px;
  color: var(--text-primary);
  transition: background-color 0.1s;
  position: relative;
}

.group-node:hover {
  background-color: var(--bg-tertiary);
}

.group-node.is-selected {
  background-color: var(--primary-color);
  color: white;
}

.expand-icon {
  width: 16px;
  height: 16px;
  display: flex;
  align-items: center;
  justify-content: center;
  margin-right: 4px;
  flex-shrink: 0;
}

.group-icon {
  width: 18px;
  height: 18px;
  border-radius: 4px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: white;
  margin-right: 6px;
  flex-shrink: 0;
}

.group-label {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  min-width: 0;
}

.group-count {
  font-size: 11px;
  color: var(--text-secondary);
  background: var(--bg-tertiary);
  padding: 2px 6px;
  border-radius: 10px;
  margin-right: 8px;
  flex-shrink: 0;
}

.group-node.is-selected .group-count {
  background: rgba(255, 255, 255, 0.2);
  color: white;
}

.group-actions {
  width: 20px;
  height: 20px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: none;
  border-radius: 4px;
  cursor: pointer;
  color: var(--text-secondary);
  opacity: 0;
  transition: all 0.15s;
}

.group-node:hover .group-actions {
  opacity: 1;
}

.group-actions:hover {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}

.actions-menu {
  position: absolute;
  right: 0;
  top: 100%;
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: 4px;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.15);
  z-index: 100;
  overflow: hidden;
  min-width: 100px;
}

.actions-menu button {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  background: transparent;
  border: none;
  cursor: pointer;
  font-size: 12px;
  color: var(--text-primary);
  text-align: left;
  transition: background-color 0.1s;
}

.actions-menu button:hover {
  background: var(--bg-tertiary);
}

.actions-menu button:last-child {
  color: var(--error-color);
}
</style>
