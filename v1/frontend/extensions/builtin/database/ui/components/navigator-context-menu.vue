<template>
  <Teleport to="body">
    <Transition name="context-menu">
      <div
        v-if="visible"
        class="navigator-context-menu"
        :style="{ left: `${position.x}px`, top: `${position.y}px` }"
        @click.stop
      >
        <template v-for="item in menuItems" :key="item.key">
          <div
            v-if="item.visible !== false"
            class="context-menu-item"
            :class="{ disabled: item.disabled }"
            @click="handleItemClick(item)"
          >
            <component :is="item.icon" :size="14" class="context-menu-icon" />
            <span class="context-menu-label">{{ item.label }}</span>
          </div>
          <div v-if="item.divider" class="context-menu-divider" />
        </template>
      </div>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
import { RefreshCw, Copy, Table, Eye, FolderPlus, FolderMinus } from 'lucide-vue-next'
import { ref, computed, onMounted, onUnmounted } from 'vue'

import type { Component } from 'vue'

interface MenuItem {
  key: string
  label: string
  icon: Component
  action: () => void
  visible?: boolean
  disabled?: boolean
  divider?: boolean
}

interface ContextMenuProps {
  nodeType: string
  nodeData: Record<string, unknown>
}

const props = defineProps<ContextMenuProps>()
const emit = defineEmits<{
  refresh: []
  copyName: []
  openTable: []
  openView: []
  dropTable: []
  dropView: []
  expandAll: []
  collapseAll: []
  refreshSchema: []
  refreshDatabase: []
}>()

const visible = ref(false)
const position = ref({ x: 0, y: 0 })

const menuItems = computed((): MenuItem[] => {
  const items: MenuItem[] = []

  if (props.nodeType === 'connection') {
    items.push(
      { key: 'refresh', label: '刷新连接', icon: RefreshCw, action: () => emit('refresh') },
      {
        key: 'expandAll',
        label: '全部展开',
        icon: FolderPlus,
        action: () => emit('expandAll'),
        divider: true,
      },
      {
        key: 'collapseAll',
        label: '全部折叠',
        icon: FolderMinus,
        action: () => emit('collapseAll'),
      }
    )
  } else if (props.nodeType === 'catalog') {
    items.push(
      {
        key: 'refresh',
        label: '刷新数据库',
        icon: RefreshCw,
        action: () => emit('refreshDatabase'),
      },
      {
        key: 'copyName',
        label: '复制名称',
        icon: Copy,
        action: () => emit('copyName'),
        divider: true,
      },
      { key: 'expandAll', label: '全部展开', icon: FolderPlus, action: () => emit('expandAll') },
      {
        key: 'collapseAll',
        label: '全部折叠',
        icon: FolderMinus,
        action: () => emit('collapseAll'),
      }
    )
  } else if (props.nodeType === 'schema') {
    items.push(
      {
        key: 'refresh',
        label: '刷新 Schema',
        icon: RefreshCw,
        action: () => emit('refreshSchema'),
      },
      {
        key: 'copyName',
        label: '复制名称',
        icon: Copy,
        action: () => emit('copyName'),
        divider: true,
      },
      { key: 'expandAll', label: '全部展开', icon: FolderPlus, action: () => emit('expandAll') },
      {
        key: 'collapseAll',
        label: '全部折叠',
        icon: FolderMinus,
        action: () => emit('collapseAll'),
      }
    )
  } else if (props.nodeType === 'table') {
    items.push(
      { key: 'openTable', label: '打开表数据', icon: Table, action: () => emit('openTable') },
      {
        key: 'refresh',
        label: '刷新表',
        icon: RefreshCw,
        action: () => emit('refresh'),
        divider: true,
      },
      { key: 'copyName', label: '复制名称', icon: Copy, action: () => emit('copyName') }
    )
  } else if (props.nodeType === 'view') {
    items.push(
      { key: 'openView', label: '打开视图数据', icon: Eye, action: () => emit('openView') },
      {
        key: 'refresh',
        label: '刷新视图',
        icon: RefreshCw,
        action: () => emit('refresh'),
        divider: true,
      },
      { key: 'copyName', label: '复制名称', icon: Copy, action: () => emit('copyName') }
    )
  } else if (props.nodeType === 'column') {
    items.push({ key: 'copyName', label: '复制列名', icon: Copy, action: () => emit('copyName') })
  }

  return items
})

function show(event: MouseEvent) {
  event.preventDefault()
  position.value = { x: event.clientX, y: event.clientY }
  visible.value = true
}

function hide() {
  visible.value = false
}

function handleItemClick(item: MenuItem) {
  if (!item.disabled) {
    item.action()
    hide()
  }
}

function handleClickOutside(_event: MouseEvent) {
  hide()
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
  min-width: 180px;
  padding: 4px 0;
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: 4px;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.15);
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
