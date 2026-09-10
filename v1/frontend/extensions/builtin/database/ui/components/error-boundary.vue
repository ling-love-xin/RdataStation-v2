<template>
  <div v-if="hasError" class="error-boundary">
    <div class="error-content">
      <div class="error-icon">
        <AlertTriangle :size="48" />
      </div>
      <h3 class="error-title">{{ t('navigator.componentError') }}</h3>
      <p class="error-message">{{ errorMessage }}</p>
      <button class="retry-btn" @click="handleRetry">
        <RefreshCw :size="16" />
        {{ t('navigator.retry') }}
      </button>
    </div>
  </div>
  <slot v-else />
</template>

<script setup lang="ts">
import { AlertTriangle, RefreshCw } from 'lucide-vue-next'
import { ref, onErrorCaptured, type ComponentPublicInstance } from 'vue'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

const emit = defineEmits<{
  error: [error: Error]
  retry: []
}>()

const hasError = ref(false)
const errorMessage = ref('')

onErrorCaptured((error: Error, instance: ComponentPublicInstance | null, info: string) => {
  hasError.value = true
  errorMessage.value = error.message || t('navigator.unknownError')

  console.error('Error Boundary caught error:', error)
  console.error('Component:', instance)
  console.error('Info:', info)

  emit('error', error)

  return false
})

function handleRetry() {
  hasError.value = false
  errorMessage.value = ''
  emit('retry')
}
</script>

<style scoped>
.error-boundary {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 200px;
  padding: 20px;
}

.error-content {
  display: flex;
  flex-direction: column;
  align-items: center;
  text-align: center;
  padding: 32px;
  background: var(--bg-secondary);
  border-radius: 12px;
  border: 1px solid var(--border-color);
}

.error-icon {
  color: var(--warning-color);
  margin-bottom: 16px;
}

.error-title {
  font-size: 16px;
  font-weight: 600;
  color: var(--text-primary);
  margin: 0 0 8px 0;
}

.error-message {
  font-size: 13px;
  color: var(--text-secondary);
  margin: 0 0 20px 0;
  max-width: 300px;
  word-break: break-word;
}

.retry-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 16px;
  background: var(--primary-color);
  border: none;
  border-radius: 6px;
  font-size: 13px;
  font-weight: 500;
  color: white;
  cursor: pointer;
  transition: all 0.15s;
}

.retry-btn:hover {
  background: var(--primary-dark);
}
</style>
