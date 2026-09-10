<template>
  <div class="navigator-error" :class="{ visible: visible }" role="alert">
    <div class="error-icon">
      <AlertCircle :size="20" aria-hidden="true" />
    </div>
    <div class="error-content">
      <div class="error-title">{{ error?.title || '操作失败' }}</div>
      <div class="error-message">{{ error?.message || '发生未知错误' }}</div>
    </div>
    <div class="error-actions">
      <button class="retry-btn" aria-label="重试" @click="$emit('retry')">
        <RefreshCw :size="14" aria-hidden="true" />
        重试
      </button>
      <button class="close-btn" aria-label="关闭" @click="$emit('close')">
        <X :size="14" aria-hidden="true" />
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { AlertCircle, RefreshCw, X } from 'lucide-vue-next'

export interface NavigatorError {
  title?: string
  message: string
  code?: string
}

defineProps<{
  visible: boolean
  error?: NavigatorError
}>()

defineEmits<{
  retry: []
  close: []
}>()
</script>

<style scoped>
.navigator-error {
  display: none;
  align-items: center;
  padding: 8px 12px;
  background: var(--error-light);
  border-top: 1px solid var(--error-color);
  gap: 10px;
}

.navigator-error.visible {
  display: flex;
}

.error-icon {
  color: var(--error-color);
  flex-shrink: 0;
}

.error-content {
  flex: 1;
  min-width: 0;
}

.error-title {
  font-size: 12px;
  font-weight: 500;
  color: var(--error-color);
  margin-bottom: 2px;
}

.error-message {
  font-size: 11px;
  color: var(--text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.error-actions {
  display: flex;
  gap: 6px;
  flex-shrink: 0;
}

.retry-btn,
.close-btn {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px 8px;
  border: none;
  border-radius: 4px;
  font-size: 11px;
  cursor: pointer;
  transition: all 0.15s;
}

.retry-btn {
  background: var(--error-color);
  color: white;
}

.retry-btn:hover {
  background: var(--error-dark);
}

.close-btn {
  background: transparent;
  color: var(--text-secondary);
}

.close-btn:hover {
  background: rgba(0, 0, 0, 0.1);
  color: var(--text-primary);
}
</style>
