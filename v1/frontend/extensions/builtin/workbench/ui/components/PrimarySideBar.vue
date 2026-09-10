<template>
  <div
    :class="['primary-sidebar', position, { expanded: isExpanded, visible: isVisible }]"
    :style="sidebarStyle"
  >
    <div v-if="isExpanded && isVisible" class="sidebar-header">
      <span class="sidebar-title">{{ currentTitle }}</span>
      <button class="sidebar-close" :title="t('workbench.close')" @click="handleClose">
        <X :size="16" />
      </button>
    </div>

    <div v-if="isExpanded && isVisible" class="sidebar-content">
      <component :is="currentComponent" />
    </div>

    <div v-if="isVisible" class="sidebar-resizer" :class="position" @mousedown="startResize" />
  </div>
</template>

<script setup lang="ts">
import { X } from 'lucide-vue-next'
import { computed, ref, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'

import { panelRegistry } from '@/core/panel-registry'

import { useLayoutStore, ACTIVEBAR_TO_PANEL_ID } from '../stores/layout-store'

interface Props {
  position: 'left' | 'right'
}

const props = defineProps<Props>()

const { t } = useI18n()
const layoutStore = useLayoutStore()

const isResizing = ref(false)
const startX = ref(0)
const startWidth = ref(0)

const isVisible = computed(() => {
  return props.position === 'left'
    ? layoutStore.primarySideBarVisible
    : layoutStore.secondarySideBarVisible
})

const isExpanded = computed(() => {
  return props.position === 'left'
    ? layoutStore.primarySideBarExpanded
    : layoutStore.secondarySideBarExpanded
})

const sidebarWidth = computed(() => {
  return props.position === 'left'
    ? layoutStore.primarySideBarWidth
    : layoutStore.secondarySideBarWidth
})

const currentComponentId = computed(() => {
  return props.position === 'left'
    ? layoutStore.currentLeftComponentId
    : layoutStore.currentRightComponentId
})

const sidebarStyle = computed(() => {
  return {
    width: isExpanded.value ? `${sidebarWidth.value}px` : '0px',
  }
})

const currentComponent = computed(() => {
  const id = currentComponentId.value
  if (!id) return null

  const panelId = ACTIVEBAR_TO_PANEL_ID[id] || id
  const panel = panelRegistry.get(panelId)
  return panel?.component || null
})

const currentTitle = computed(() => {
  const id = currentComponentId.value
  if (!id) return ''

  const panelId = ACTIVEBAR_TO_PANEL_ID[id] || id
  const panel = panelRegistry.get(panelId)
  return panel?.name || ''
})

function handleClose() {
  if (props.position === 'left') {
    layoutStore.primarySideBarExpanded = false
  } else {
    layoutStore.secondarySideBarExpanded = false
  }
}

function startResize(e: MouseEvent) {
  isResizing.value = true
  startX.value = e.clientX
  startWidth.value = sidebarWidth.value

  document.addEventListener('mousemove', handleResize)
  document.addEventListener('mouseup', stopResize)
  e.preventDefault()
}

function handleResize(e: MouseEvent) {
  if (!isResizing.value) return

  const delta = props.position === 'left' ? e.clientX - startX.value : startX.value - e.clientX

  const newWidth = startWidth.value + delta

  if (props.position === 'left') {
    layoutStore.setPrimarySideBarWidth(newWidth)
  } else {
    layoutStore.setSecondarySideBarWidth(newWidth)
  }
}

function stopResize() {
  isResizing.value = false
  document.removeEventListener('mousemove', handleResize)
  document.removeEventListener('mouseup', stopResize)
}

onUnmounted(() => {
  stopResize()
})
</script>

<style scoped>
.primary-sidebar {
  display: flex;
  flex-direction: column;
  background-color: var(--color-bg-secondary);
  border-color: var(--color-border);
  overflow: hidden;
  transition: width 0.2s ease;
}

.primary-sidebar.left {
  border-right: 1px solid var(--color-border);
}

.primary-sidebar.right {
  border-left: 1px solid var(--color-border);
}

.primary-sidebar:not(.visible) {
  display: none;
}

.primary-sidebar:not(.expanded) {
  width: 0 !important;
}

.sidebar-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 12px;
  border-bottom: 1px solid var(--color-border);
  background-color: var(--color-bg-tertiary);
  min-height: 35px;
}

.sidebar-title {
  font-size: 11px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.5px;
  color: var(--color-text-secondary);
}

.sidebar-close {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  border: none;
  background: transparent;
  color: var(--color-text-secondary);
  border-radius: 4px;
  cursor: pointer;
  padding: 0;
}

.sidebar-close:hover {
  background-color: var(--color-bg-hover);
  color: var(--color-text-primary);
}

.sidebar-content {
  flex: 1;
  overflow: auto;
  min-width: 0;
}

.sidebar-resizer {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 5px;
  cursor: col-resize;
  z-index: 10;
}

.sidebar-resizer.left {
  right: -2px;
}

.sidebar-resizer.right {
  left: -2px;
}

.sidebar-resizer:hover {
  background-color: var(--color-accent);
  opacity: 0.3;
}
</style>
