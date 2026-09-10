/**
 * 防抖 Composable
 *
 * 用于延迟执行频繁触发的事件（如搜索输入）
 *
 * 使用示例：
 * ```ts
 * const { debouncedValue, isDebouncing } = useDebounce(searchText, 300)
 *
 * // 监听防抖后的值
 * watch(debouncedValue, (value) => {
 *   performSearch(value)
 * })
 * ```
 */

import { ref, watch, type Ref, type WatchSource } from 'vue'

export interface UseDebounceReturn<T> {
  /** 防抖后的值 */
  debouncedValue: Ref<T>
  /** 是否正在防抖等待中 */
  isDebouncing: Ref<boolean>
  /** 立即执行（取消等待） */
  flush: () => void
  /** 取消等待 */
  cancel: () => void
}

export function useDebounce<T>(source: WatchSource<T>, delay = 300): UseDebounceReturn<T> {
  const debouncedValue = ref<T>(
    typeof source === 'function' ? (source as () => T)() : (source as Ref<T>).value
  ) as Ref<T>
  const isDebouncing = ref(false)

  let timeoutId: ReturnType<typeof setTimeout> | null = null

  function cancel() {
    if (timeoutId) {
      clearTimeout(timeoutId)
      timeoutId = null
      isDebouncing.value = false
    }
  }

  function flush() {
    cancel()
    const value = typeof source === 'function' ? (source as () => T)() : (source as Ref<T>).value
    debouncedValue.value = value
  }

  watch(
    source,
    newValue => {
      cancel()
      isDebouncing.value = true

      timeoutId = setTimeout(() => {
        debouncedValue.value = newValue
        isDebouncing.value = false
        timeoutId = null
      }, delay)
    },
    { immediate: false }
  )

  return {
    debouncedValue,
    isDebouncing,
    flush,
    cancel,
  }
}

/**
 * 防抖函数 Composable
 *
 * 返回一个防抖包装的函数
 *
 * 使用示例：
 * ```ts
 * const debouncedSearch = useDebounceFn((query: string) => {
 *   return api.search(query)
 * }, 300)
 *
 * // 调用
 * debouncedSearch('keyword')
 * ```
 */
export function useDebounceFn<T extends (...args: unknown[]) => unknown>(
  fn: T,
  delay = 300
): {
  run: (...args: Parameters<T>) => void
  cancel: () => void
  flush: () => ReturnType<T> | undefined
  isPending: Ref<boolean>
} {
  const isPending = ref(false)
  let timeoutId: ReturnType<typeof setTimeout> | null = null
  let lastArgs: Parameters<T> | null = null

  function cancel() {
    if (timeoutId) {
      clearTimeout(timeoutId)
      timeoutId = null
      isPending.value = false
    }
  }

  function flush(): ReturnType<T> | undefined {
    cancel()
    if (lastArgs) {
      return fn(...lastArgs)
    }
  }

  function run(...args: Parameters<T>) {
    lastArgs = args
    cancel()
    isPending.value = true

    timeoutId = setTimeout(() => {
      fn(...args)
      isPending.value = false
      timeoutId = null
    }, delay)
  }

  return {
    run,
    cancel,
    flush,
    isPending,
  }
}
