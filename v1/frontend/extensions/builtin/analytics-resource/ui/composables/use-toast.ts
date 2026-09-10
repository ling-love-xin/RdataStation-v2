import { reactive } from 'vue'

export type ToastType = 'success' | 'error' | 'warning' | 'info'

export interface Toast {
  id: number
  message: string
  type: ToastType
  duration: number
  detail?: string
}

const toasts = reactive<Toast[]>([])
let toastId = 0

export interface ErrorInfo {
  message: string
  code?: string
  detail?: string
}

export function parseError(error: unknown): ErrorInfo {
  if (error instanceof Error) {
    return {
      message: error.message,
      detail: error.stack,
    }
  }
  if (typeof error === 'string') {
    try {
      const parsed = JSON.parse(error)
      return {
        message: parsed.message || parsed.error || error,
        code: parsed.code,
        detail: parsed.detail,
      }
    } catch {
      return { message: error }
    }
  }
  return { message: String(error) }
}

export function useToast() {
  function show(
    message: string,
    type: ToastType = 'info',
    duration: number = 3000,
    detail?: string
  ) {
    const id = ++toastId
    const toast: Toast = { id, message, type, duration, detail }
    toasts.push(toast)

    if (duration > 0) {
      setTimeout(() => {
        remove(id)
      }, duration)
    }

    return id
  }

  function success(message: string, duration?: number) {
    return show(message, 'success', duration)
  }

  function error(message: string, duration?: number, detail?: string) {
    return show(message, 'error', duration, detail)
  }

  function warning(message: string, duration?: number) {
    return show(message, 'warning', duration)
  }

  function info(message: string, duration?: number) {
    return show(message, 'info', duration)
  }

  function showError(error: unknown) {
    const info = parseError(error)
    return show(info.message, 'error', 5000, info.detail)
  }

  function remove(id: number) {
    const index = toasts.findIndex(t => t.id === id)
    if (index !== -1) {
      toasts.splice(index, 1)
    }
  }

  function clear() {
    toasts.splice(0, toasts.length)
  }

  return {
    toasts,
    show,
    success,
    error,
    warning,
    info,
    showError,
    remove,
    clear,
  }
}
