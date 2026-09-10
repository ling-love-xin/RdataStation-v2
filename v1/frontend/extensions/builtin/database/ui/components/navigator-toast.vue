<template>
  <div class="toast-container">
    <TransitionGroup name="toast">
      <div
        v-for="toast in toasts"
        :key="toast.id"
        class="toast"
        :class="toast.type"
        @mouseenter="pauseToast(toast.id)"
        @mouseleave="resumeToast(toast.id)"
      >
        <span class="toast-icon">
          <CheckCircle v-if="toast.type === 'success'" :size="16" />
          <AlertCircle v-else-if="toast.type === 'error'" :size="16" />
          <Info v-else-if="toast.type === 'info'" :size="16" />
          <AlertTriangle v-else-if="toast.type === 'warning'" :size="16" />
        </span>
        <span class="toast-message">{{ toast.message }}</span>
        <button class="toast-close" @click="removeToast(toast.id)">
          <X :size="14" />
        </button>
      </div>
    </TransitionGroup>
  </div>
</template>

<script setup lang="ts">
import { CheckCircle, AlertCircle, Info, AlertTriangle, X } from 'lucide-vue-next'
import { ref, onMounted, onUnmounted } from 'vue'

export interface Toast {
  id: string
  type: 'success' | 'error' | 'info' | 'warning'
  message: string
  duration: number
  createdAt: number
  timeoutId?: ReturnType<typeof setTimeout>
}

const toasts = ref<Toast[]>([])

function addToast(type: Toast['type'], message: string, duration = 3000) {
  const id = `toast_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`
  const toast: Toast = {
    id,
    type,
    message,
    duration,
    createdAt: Date.now(),
  }

  toasts.value.push(toast)

  if (duration > 0) {
    scheduleRemove(id, duration)
  }

  return id
}

function scheduleRemove(id: string, duration: number) {
  const toast = toasts.value.find(t => t.id === id)
  if (toast) {
    toast.timeoutId = setTimeout(() => {
      removeToast(id)
    }, duration)
  }
}

function pauseToast(id: string) {
  const toast = toasts.value.find(t => t.id === id)
  if (toast && toast.timeoutId) {
    clearTimeout(toast.timeoutId)
    toast.timeoutId = undefined
  }
}

function resumeToast(id: string) {
  const toast = toasts.value.find(t => t.id === id)
  if (toast && !toast.timeoutId) {
    const elapsed = Date.now() - toast.createdAt
    const remaining = Math.max(0, toast.duration - elapsed)
    if (remaining > 0) {
      scheduleRemove(id, remaining)
    }
  }
}

function removeToast(id: string) {
  const index = toasts.value.findIndex(t => t.id === id)
  if (index !== -1) {
    const toast = toasts.value[index]
    if (toast.timeoutId) {
      clearTimeout(toast.timeoutId)
    }
    toasts.value.splice(index, 1)
  }
}

function success(message: string, duration?: number) {
  return addToast('success', message, duration)
}

function error(message: string, duration?: number) {
  return addToast('error', message, duration)
}

function info(message: string, duration?: number) {
  return addToast('info', message, duration)
}

function warning(message: string, duration?: number) {
  return addToast('warning', message, duration)
}

defineExpose({
  success,
  error,
  info,
  warning,
  removeToast,
})

let observer: MutationObserver | null = null

onMounted(() => {
  observer = new MutationObserver(() => {})
})

onUnmounted(() => {
  if (observer) {
    observer.disconnect()
  }
  toasts.value.forEach(toast => {
    if (toast.timeoutId) {
      clearTimeout(toast.timeoutId)
    }
  })
})
</script>

<style scoped>
.toast-container {
  position: fixed;
  top: 20px;
  right: 20px;
  z-index: 9999;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.toast {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px 16px;
  border-radius: 8px;
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
  min-width: 280px;
  max-width: 400px;
}

.toast.success {
  border-color: var(--success-color);
  background: rgba(34, 197, 94, 0.1);
}

.toast.success .toast-icon {
  color: var(--success-color);
}

.toast.error {
  border-color: var(--error-color);
  background: rgba(239, 68, 68, 0.1);
}

.toast.error .toast-icon {
  color: var(--error-color);
}

.toast.info {
  border-color: var(--primary-color);
  background: rgba(59, 130, 246, 0.1);
}

.toast.info .toast-icon {
  color: var(--primary-color);
}

.toast.warning {
  border-color: var(--warning-color);
  background: rgba(245, 158, 11, 0.1);
}

.toast.warning .toast-icon {
  color: var(--warning-color);
}

.toast-icon {
  flex-shrink: 0;
}

.toast-message {
  flex: 1;
  font-size: 13px;
  color: var(--text-primary);
  line-height: 1.4;
}

.toast-close {
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
  transition: all 0.15s;
}

.toast-close:hover {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}

.toast-enter-active,
.toast-leave-active {
  transition: all 0.3s ease;
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
</style>
