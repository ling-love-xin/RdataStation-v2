<template>
  <Teleport to="body">
    <Transition name="context-menu">
      <div
        v-if="visible"
        class="navigator-context-menu"
        :style="{ left: `${position.x}px`, top: `${position.y}px` }"
        @click.stop
      >
        <template v-for="(item, index) in menuItems" :key="'id' in item ? item.id : `sep-${index}`">
          <div v-if="item.separator" class="context-menu-divider" />
          <div
            v-else-if="item.hidden !== true"
            class="context-menu-item"
            :class="{ disabled: item.disabled }"
            @click="handleItemClick(item)"
          >
            <component
              :is="getIcon(item.icon)"
              v-if="item.icon"
              :size="14"
              class="context-menu-icon"
              aria-hidden="true"
            />
            <span class="context-menu-label">{{ item.label }}</span>
            <span v-if="item.shortcut" class="context-menu-shortcut">{{ item.shortcut }}</span>
          </div>
        </template>
      </div>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
import {
  Settings,
  Zap,
  LogOut,
  LogIn,
  RefreshCw,
  Copy,
  Trash2,
  Plus,
  Code,
  Table,
  BarChart3,
  Eye,
  FileText,
} from 'lucide-vue-next'
import { ref, computed, onMounted, onUnmounted } from 'vue'

import type { IContextMenuItem } from '../composables/use-context-menu-actions'
import type { Component } from 'vue'

interface ContextMenuProps {
  items: IContextMenuItem[]
}

const props = defineProps<ContextMenuProps>()

const visible = ref(false)
const position = ref({ x: 0, y: 0 })

const menuItems = computed(() => props.items)

function show(event: MouseEvent) {
  event.preventDefault()
  position.value = { x: event.clientX, y: event.clientY }
  visible.value = true
}

function hide() {
  visible.value = false
}

function handleItemClick(item: IContextMenuItem) {
  if (!('separator' in item && item.separator) && !item.disabled && item.action) {
    item.action()
    hide()
  }
}

function handleClickOutside() {
  hide()
}

function getIcon(iconName: string): Component {
  const iconMap: Record<string, Component> = {
    Settings,
    Zap,
    LogOut,
    LogIn,
    RefreshCw,
    Copy,
    Trash2,
    Plus,
    Code,
    Table,
    BarChart3,
    Eye,
    FileText,
  }
  return iconMap[iconName] || FileText
}

onMounted(() => {
  document.addEventListener('click', handleClickOutside)
})

onUnmounted(() => {
  document.removeEventListener('click', handleClickOutside)
})

defineExpose({ show, hide })
</script>

<style scoped>
.navigator-context-menu {
  position: fixed;
  z-index: 9999;
  min-width: 200px;
  padding: 4px 0;
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: 6px;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.2);
}

.context-menu-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 12px;
  cursor: pointer;
  font-size: 13px;
  color: var(--text-primary);
  transition: background 0.1s;
}

.context-menu-item:hover:not(.disabled) {
  background: var(--bg-hover);
}

.context-menu-item.disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.context-menu-icon {
  color: var(--text-secondary);
  flex-shrink: 0;
}

.context-menu-label {
  flex: 1;
}

.context-menu-shortcut {
  font-size: 11px;
  color: var(--text-secondary);
  margin-left: 16px;
}

.context-menu-divider {
  height: 1px;
  margin: 4px 0;
  background: var(--border-color);
}

.context-menu-enter-active,
.context-menu-leave-active {
  transition: opacity 0.15s ease;
}

.context-menu-enter-from,
.context-menu-leave-to {
  opacity: 0;
}
</style>
