/**
 * 加载状态管理 Composable
 *
 * 统一管理异步操作的加载状态
 *
 * 使用示例：
 * ```ts
 * const { isLoading, withLoading } = useLoading()
 *
 * // 在方法中使用
 * async function fetchData() {
 *   return withLoading(async () => {
 *     const result = await api.getData()
 *     return result
 *   })
 * }
 * ```
 */

import { ref, type Ref } from 'vue'

export interface UseLoadingReturn {
  /** 是否正在加载 */
  isLoading: Ref<boolean>
  /** 包装异步函数，自动管理加载状态 */
  withLoading: <T>(fn: () => Promise<T>) => Promise<T>
  /** 手动设置加载状态 */
  setLoading: (value: boolean) => void
  /** 开始加载 */
  startLoading: () => void
  /** 结束加载 */
  stopLoading: () => void
}

export function useLoading(initialValue = false): UseLoadingReturn {
  const isLoading = ref(initialValue)

  function setLoading(value: boolean) {
    isLoading.value = value
  }

  function startLoading() {
    isLoading.value = true
  }

  function stopLoading() {
    isLoading.value = false
  }

  async function withLoading<T>(fn: () => Promise<T>): Promise<T> {
    startLoading()
    try {
      const result = await fn()
      return result
    } finally {
      stopLoading()
    }
  }

  return {
    isLoading,
    withLoading,
    setLoading,
    startLoading,
    stopLoading,
  }
}
