import { ref } from 'vue'

export function useDebounceFn<T extends (...args: unknown[]) => unknown>(
  fn: T,
  delay: number
): (...args: Parameters<T>) => void {
  let timeoutId: ReturnType<typeof setTimeout> | null = null

  return (...args: Parameters<T>) => {
    if (timeoutId) {
      clearTimeout(timeoutId)
    }
    timeoutId = setTimeout(() => {
      fn(...args)
    }, delay)
  }
}

export function useDebounce<T>(initialValue: T, delay: number) {
  const value = ref<T>(initialValue)
  const debouncedValue = ref<T>(initialValue)
  let timeoutId: ReturnType<typeof setTimeout> | null = null

  function update(newValue: T) {
    value.value = newValue
    if (timeoutId) {
      clearTimeout(timeoutId)
    }
    timeoutId = setTimeout(() => {
      debouncedValue.value = newValue
    }, delay)
  }

  return {
    value,
    debouncedValue,
    update,
  }
}
