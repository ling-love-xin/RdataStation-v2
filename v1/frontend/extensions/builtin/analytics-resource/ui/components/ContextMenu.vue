<template>
  <Teleport to="body">
    <div v-if="visible" class="context-menu" :style="{ left: `${x}px`, top: `${y}px` }" @click.stop>
      <div
        v-for="item in items"
        :key="item.id"
        :class="['context-menu-item', { disabled: item.disabled, danger: item.danger }]"
        @click="handleClick(item)"
      >
        <span v-if="item.icon" class="item-icon">{{ item.icon }}</span>
        <span class="item-label">{{ item.label }}</span>
        <span v-if="item.shortcut" class="item-shortcut">{{ item.shortcut }}</span>
      </div>
    </div>
    <div v-if="visible" class="context-menu-overlay" @click="close" @contextmenu.prevent="close" />
  </Teleport>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'

export interface ContextMenuItem {
  id: string
  label: string
  icon?: string
  shortcut?: string
  disabled?: boolean
  danger?: boolean
  action?: () => void
}

defineProps<{
  items: ContextMenuItem[]
}>()

const emit = defineEmits<{
  select: [item: ContextMenuItem]
  close: []
}>()

const visible = ref(false)
const x = ref(0)
const y = ref(0)

function open(event: MouseEvent) {
  x.value = event.clientX
  y.value = event.clientY
  visible.value = true
}

function close() {
  visible.value = false
  emit('close')
}

function handleClick(item: ContextMenuItem) {
  if (item.disabled) return
  item.action?.()
  emit('select', item)
  close()
}

function handleKeyDown(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    close()
  }
}

onMounted(() => {
  document.addEventListener('keydown', handleKeyDown)
})

onUnmounted(() => {
  document.removeEventListener('keydown', handleKeyDown)
})

defineExpose({ open, close })
</script>

<style scoped>
.context-menu {
  position: fixed;
  z-index: 10000;
  min-width: 180px;
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-md);
  padding: 4px 0;
}

.context-menu-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 12px;
  cursor: pointer;
  transition: background 0.15s;
  font-size: var(--font-size-md);
  color: var(--text-primary);
}

.context-menu-item:hover:not(.disabled) {
  background: var(--primary-light);
}

.context-menu-item.disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.context-menu-item.danger {
  color: var(--danger-color);
}

.context-menu-item.danger:hover:not(.disabled) {
  background: var(--danger-light);
}

.item-icon {
  font-size: var(--font-size-lg);
  width: 20px;
  text-align: center;
}

.item-label {
  flex: 1;
  font-size: var(--font-size-md);
}

.item-shortcut {
  font-size: var(--font-size-xs);
  color: var(--text-tertiary);
}

.context-menu-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  z-index: 9999;
}
</style>
