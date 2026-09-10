import { AlertTriangle, RefreshCw } from 'lucide-vue-next'
import { ref, h, defineComponent } from 'vue'

interface ErrorBoundaryState {
  hasError: boolean
  error: Error | null
  errorInfo: string | null
}

export function useErrorBoundary() {
  const state = ref<ErrorBoundaryState>({
    hasError: false,
    error: null,
    errorInfo: null,
  })

  function handleError(error: Error, info?: string) {
    state.value = {
      hasError: true,
      error,
      errorInfo: info || null,
    }

    console.error('[ErrorBoundary]', error, info)
  }

  function reset() {
    state.value = {
      hasError: false,
      error: null,
      errorInfo: null,
    }
  }

  function withErrorHandling<T>(fn: () => Promise<T>, fallback?: T): Promise<T> {
    return fn().catch(error => {
      handleError(error)
      if (fallback !== undefined) {
        return fallback
      }
      throw error
    })
  }

  function withRetry<T>(fn: () => Promise<T>, maxRetries = 3, delayMs = 1000): Promise<T> {
    return fn().catch(async error => {
      for (let i = 0; i < maxRetries; i++) {
        try {
          await new Promise(resolve => setTimeout(resolve, delayMs * (i + 1)))
          return await fn()
        } catch (retryError) {
          if (i === maxRetries - 1) {
            handleError(retryError as Error, `Failed after ${maxRetries} retries`)
            throw retryError
          }
        }
      }
      throw error
    })
  }

  return {
    state,
    handleError,
    reset,
    withErrorHandling,
    withRetry,
  }
}

export const ErrorBoundaryFallback = defineComponent({
  props: {
    error: {
      type: Error,
      default: null,
    },
    errorInfo: {
      type: String,
      default: null,
    },
    onRetry: {
      type: Function,
      default: () => {},
    },
  },
  setup(props) {
    return () =>
      h('div', { class: 'error-boundary-fallback' }, [
        h(AlertTriangle, { size: 32, class: 'error-icon' }),
        h('h3', { class: 'error-title' }, '加载失败'),
        h('p', { class: 'error-message' }, props.error?.message || '未知错误'),
        props.errorInfo && h('p', { class: 'error-info' }, props.errorInfo),
        h(
          'button',
          {
            class: 'retry-btn',
            onClick: props.onRetry,
          },
          [h(RefreshCw, { size: 14 }), ' 重试']
        ),
      ])
  },
})
