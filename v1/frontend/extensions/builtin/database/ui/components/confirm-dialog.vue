<template>
  <div v-if="visible" class="confirm-dialog-overlay" @click.self="$emit('cancel')">
    <div class="confirm-dialog">
      <div class="dialog-header">
        <div class="dialog-icon" :class="type">
          <AlertTriangle v-if="type === 'warning'" :size="24" />
          <AlertCircle v-else-if="type === 'error'" :size="24" />
          <Info v-else-if="type === 'info'" :size="24" />
          <CheckCircle v-else-if="type === 'success'" :size="24" />
        </div>
        <h3>{{ title }}</h3>
        <button class="btn-close" @click="$emit('cancel')">
          <X :size="16" />
        </button>
      </div>

      <div class="dialog-body">
        <p>{{ message }}</p>

        <div v-if="details" class="dialog-details">
          <pre>{{ details }}</pre>
        </div>

        <div v-if="count" class="dialog-count">
          <span
            >将影响 <strong>{{ count }}</strong> 个项目</span
          >
        </div>
      </div>

      <div class="dialog-footer">
        <button v-if="showCancel" class="btn btn-cancel" @click="$emit('cancel')">
          {{ cancelText }}
        </button>
        <button v-if="showSecondary" class="btn btn-secondary" @click="$emit('secondary')">
          {{ secondaryText }}
        </button>
        <button
          class="btn btn-primary"
          :class="{ danger: type === 'error' }"
          @click="$emit('confirm')"
        >
          {{ confirmText }}
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { AlertTriangle, AlertCircle, Info, CheckCircle, X } from 'lucide-vue-next'

defineProps<{
  visible: boolean
  title?: string
  message?: string
  details?: string
  count?: number
  type?: 'warning' | 'error' | 'info' | 'success'
  confirmText?: string
  cancelText?: string
  secondaryText?: string
  showCancel?: boolean
  showSecondary?: boolean
}>()

defineEmits<{
  confirm: []
  cancel: []
  secondary: []
}>()
</script>

<style scoped>
.confirm-dialog-overlay {
  position: fixed;
  top: 0;
  right: 0;
  bottom: 0;
  left: 0;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
  animation: fadeIn 0.15s ease;
}

@keyframes fadeIn {
  from {
    opacity: 0;
  }
  to {
    opacity: 1;
  }
}

.confirm-dialog {
  width: 90%;
  max-width: 440px;
  background: var(--bg-primary);
  border-radius: 12px;
  box-shadow: 0 20px 40px rgba(0, 0, 0, 0.2);
  animation: slideUp 0.2s ease;
}

@keyframes slideUp {
  from {
    opacity: 0;
    transform: translateY(20px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

.dialog-header {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 20px;
  border-bottom: 1px solid var(--border-color);
}

.dialog-icon {
  width: 40px;
  height: 40px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 10px;
}

.dialog-icon.warning {
  background: rgba(255, 180, 0, 0.15);
  color: #ffb400;
}

.dialog-icon.error {
  background: rgba(255, 100, 100, 0.15);
  color: #ff6464;
}

.dialog-icon.info {
  background: rgba(100, 100, 255, 0.15);
  color: #6464ff;
}

.dialog-icon.success {
  background: rgba(0, 180, 100, 0.15);
  color: #00b464;
}

.dialog-header h3 {
  flex: 1;
  margin: 0;
  font-size: 16px;
  font-weight: 600;
  color: var(--text-primary);
}

.btn-close {
  padding: 4px;
  background: transparent;
  border: none;
  border-radius: 4px;
  color: var(--text-tertiary);
  cursor: pointer;
  transition: all 0.15s;
}

.btn-close:hover {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}

.dialog-body {
  padding: 20px;
}

.dialog-body p {
  margin: 0;
  font-size: 14px;
  color: var(--text-secondary);
  line-height: 1.6;
}

.dialog-details {
  margin-top: 12px;
  padding: 12px;
  background: var(--bg-secondary);
  border-radius: 6px;
  max-height: 120px;
  overflow-y: auto;
}

.dialog-details pre {
  margin: 0;
  font-size: 12px;
  color: var(--text-tertiary);
  white-space: pre-wrap;
  word-break: break-all;
}

.dialog-count {
  margin-top: 12px;
  padding: 10px 12px;
  background: rgba(255, 180, 0, 0.1);
  border-radius: 6px;
  font-size: 13px;
  color: #ffb400;
}

.dialog-footer {
  display: flex;
  gap: 8px;
  padding: 16px 20px;
  border-top: 1px solid var(--border-color);
  justify-content: flex-end;
}

.btn {
  padding: 10px 20px;
  border: none;
  border-radius: 6px;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.15s;
}

.btn-cancel {
  background: var(--bg-tertiary);
  color: var(--text-secondary);
}

.btn-cancel:hover {
  background: var(--border-color);
}

.btn-secondary {
  background: var(--bg-secondary);
  color: var(--text-primary);
}

.btn-secondary:hover {
  background: var(--bg-tertiary);
}

.btn-primary {
  background: var(--primary-color);
  color: white;
}

.btn-primary:hover {
  background: var(--primary-dark);
}

.btn-primary.danger {
  background: var(--error-color);
}

.btn-primary.danger:hover {
  background: rgba(255, 100, 100, 0.85);
}
</style>
