<template>
  <div v-if="hasError" class="error-boundary">
    <div class="error-content">
      <AlertCircle :size="48" class="error-icon" />
      <h3>组件加载失败</h3>
      <p>{{ errorMessage }}</p>
      <button class="retry-btn" @click="handleRetry">
        <RefreshCw :size="16" />
        重试
      </button>
    </div>
  </div>
  <component :is="component" v-else v-bind="$attrs" />
</template>

<script setup lang="ts">
import { AlertCircle, RefreshCw } from 'lucide-vue-next'
import { ref, onMounted, onUnmounted } from 'vue'

const _props = defineProps<{
  component: unknown
}>()

const hasError = ref(false)
const errorMessage = ref('')
const retryCount = ref(0)
const maxRetries = 3

const handleError = (error: unknown) => {
  hasError.value = true
  errorMessage.value = error instanceof Error ? error.message : '未知错误'
  console.error('[ErrorBoundary] Component error:', error)
}

const handleRetry = () => {
  if (retryCount.value < maxRetries) {
    retryCount.value++
    hasError.value = false
    errorMessage.value = ''
  } else {
    errorMessage.value = '已达到最大重试次数，请检查组件配置'
  }
}

let errorHandler: (event: ErrorEvent) => void

onMounted(() => {
  errorHandler = (event: ErrorEvent) => {
    handleError(event.error)
    event.preventDefault()
  }
  window.addEventListener('error', errorHandler)
})

onUnmounted(() => {
  window.removeEventListener('error', errorHandler)
})
</script>

<style scoped>
.error-boundary {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 100%;
  height: 100%;
  background: var(--color-bg-secondary);
}

.error-content {
  text-align: center;
  padding: 2rem;
  background: var(--color-bg-primary);
  border-radius: 8px;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.15);
}

.error-icon {
  color: var(--color-error);
  margin-bottom: 1rem;
}

.error-content h3 {
  margin: 0 0 0.5rem 0;
  color: var(--color-text-primary);
}

.error-content p {
  margin: 0 0 1.5rem 0;
  color: var(--color-text-secondary);
  font-size: 0.875rem;
}

.retry-btn {
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.5rem 1rem;
  background: var(--color-accent);
  color: white;
  border: none;
  border-radius: 4px;
  cursor: pointer;
  font-size: 0.875rem;
  transition: background-color 0.2s;
}

.retry-btn:hover {
  background: var(--color-accent-hover);
}
</style>
