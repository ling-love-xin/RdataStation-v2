/**
 * 异步状态管理 Composable
 *
 * 统一管理异步操作的状态（加载中、成功、失败）
 *
 * 使用示例：
 * ```ts
 * const { state, isLoading, error, execute } = useAsyncState(
 *   () => fetchUserData(userId),
 *   null
 * )
 *
 * // 执行异步操作
 * await execute()
 *
 * // 在模板中使用
 * <div v-if="isLoading">加载中...</div>
 * <div v-else-if="error">错误: {{ error.message }}</div>
 * <div v-else>{{ state }}</div>
 * ```
 */

import { ref, type Ref } from 'vue'

/** 异步状态 */
export type AsyncStateStatus = 'idle' | 'loading' | 'success' | 'error'

export interface UseAsyncStateReturn<T> {
  /** 当前状态值 */
  state: Ref<T>
  /** 当前状态 */
  status: Ref<AsyncStateStatus>
  /** 是否正在加载 */
  isLoading: Ref<boolean>
  /** 是否加载成功 */
  isSuccess: Ref<boolean>
  /** 是否加载失败 */
  isError: Ref<boolean>
  /** 错误信息 */
  error: Ref<Error | null>
  /** 执行异步操作 */
  execute: () => Promise<T>
  /** 重置状态 */
  reset: () => void
}

export function useAsyncState<T>(
  promiseFn: () => Promise<T>,
  initialState: T
): UseAsyncStateReturn<T> {
  const state = ref<T>(initialState) as Ref<T>
  const status = ref<AsyncStateStatus>('idle')
  const error = ref<Error | null>(null)

  const isLoading = ref(false)
  const isSuccess = ref(false)
  const isError = ref(false)

  async function execute(): Promise<T> {
    status.value = 'loading'
    isLoading.value = true
    isSuccess.value = false
    isError.value = false
    error.value = null

    try {
      const result = await promiseFn()
      state.value = result
      status.value = 'success'
      isSuccess.value = true
      return result
    } catch (e) {
      const err = e instanceof Error ? e : new Error(String(e))
      error.value = err
      status.value = 'error'
      isError.value = true
      throw err
    } finally {
      isLoading.value = false
    }
  }

  function reset() {
    state.value = initialState
    status.value = 'idle'
    error.value = null
    isLoading.value = false
    isSuccess.value = false
    isError.value = false
  }

  return {
    state,
    status,
    isLoading,
    isSuccess,
    isError,
    error,
    execute,
    reset,
  }
}
