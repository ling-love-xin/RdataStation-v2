<template>
  <Teleport to="body">
    <div class="toast-container">
      <TransitionGroup name="toast">
        <div
          v-for="toast in toasts"
          :key="toast.id"
          :class="['toast', `toast-${toast.type}`]"
          @click="toggleDetail(toast.id)"
        >
          <span class="toast-icon">{{ getIcon(toast.type) }}</span>
          <div class="toast-content">
            <span class="toast-message">{{ toast.message }}</span>
            <Transition name="detail">
              <pre v-if="expandedIds.includes(toast.id) && toast.detail" class="toast-detail">{{
                toast.detail
              }}</pre>
            </Transition>
          </div>
          <button v-if="toast.detail" class="toast-toggle" @click.stop="toggleDetail(toast.id)">
            {{ expandedIds.includes(toast.id) ? '▼' : '▶' }}
          </button>
          <button class="toast-close" @click.stop="remove(toast.id)">✕</button>
        </div>
      </TransitionGroup>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { ref } from 'vue'

import { useToast } from '../composables/use-toast'

const { toasts, remove } = useToast()
const expandedIds = ref<number[]>([])

function getIcon(type: string) {
  switch (type) {
    case 'success':
      return '✅'
    case 'error':
      return '❌'
    case 'warning':
      return '⚠️'
    case 'info':
      return 'ℹ️'
    default:
      return 'ℹ️'
  }
}

function toggleDetail(id: number) {
  const index = expandedIds.value.indexOf(id)
  if (index !== -1) {
    expandedIds.value.splice(index, 1)
  } else {
    expandedIds.value.push(id)
  }
}
</script>

<style scoped>
.toast-container {
  position: fixed;
  top: 60px;
  right: 20px;
  z-index: 9999;
  display: flex;
  flex-direction: column;
  gap: var(--size-sm);
  pointer-events: none;
}

.toast {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: var(--size-md) var(--size-lg);
  border-radius: var(--radius-md);
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  box-shadow: var(--shadow-md);
  cursor: pointer;
  pointer-events: auto;
  max-width: 420px;
  transition: all 0.3s ease;
}

.toast:hover {
  transform: translateX(-4px);
}

.toast-icon {
  font-size: var(--font-size-xl);
  flex-shrink: 0;
  margin-top: 2px;
}

.toast-content {
  flex: 1;
  min-width: 0;
}

.toast-message {
  font-size: var(--font-size-md);
  color: var(--text-primary);
  line-height: 1.4;
}

.toast-detail {
  margin-top: 8px;
  padding: 8px;
  background: var(--bg-secondary);
  border-radius: var(--radius-sm);
  font-size: var(--font-size-xs);
  color: var(--text-tertiary);
  font-family: var(--font-mono);
  max-height: 150px;
  overflow-y: auto;
  white-space: pre-wrap;
  word-break: break-all;
}

.toast-toggle {
  background: none;
  border: none;
  font-size: var(--font-size-xs);
  color: var(--text-tertiary);
  cursor: pointer;
  padding: 2px;
  flex-shrink: 0;
  transition: color 0.15s;
}

.toast-toggle:hover {
  color: var(--text-primary);
}

.toast-close {
  background: none;
  border: none;
  font-size: var(--font-size-lg);
  color: var(--text-tertiary);
  cursor: pointer;
  padding: 2px 4px;
  flex-shrink: 0;
  transition: color 0.15s;
}

.toast-close:hover {
  color: var(--text-primary);
}

.toast-success {
  border-left: 3px solid var(--success-color);
}

.toast-error {
  border-left: 3px solid var(--danger-color);
}

.toast-warning {
  border-left: 3px solid var(--warning-color);
}

.toast-info {
  border-left: 3px solid var(--info-color);
}

.toast-enter-from {
  opacity: 0;
  transform: translateX(100%);
}

.toast-leave-to {
  opacity: 0;
  transform: translateX(100%);
}

.toast-move {
  transition: transform 0.3s ease;
}

.detail-enter-active,
.detail-leave-active {
  transition: all 0.2s ease;
  overflow: hidden;
}

.detail-enter-from,
.detail-leave-to {
  opacity: 0;
  max-height: 0;
}

.detail-enter-to,
.detail-leave-from {
  opacity: 1;
  max-height: 150px;
}
</style>
