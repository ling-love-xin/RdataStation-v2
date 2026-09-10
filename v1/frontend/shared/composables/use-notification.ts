/**
 * 通知管理 Composable
 *
 * 统一管理应用内的通知消息
 *
 * 使用示例：
 * ```ts
 * const { success, error, warning, info } = useNotification()
 *
 * // 显示成功消息
 * success('操作成功')
 *
 * // 显示错误消息
 * error('操作失败', '请检查网络连接')
 *
 * // 显示警告
 * warning('注意', '此操作不可撤销')
 * ```
 */

import { ref, type Ref } from 'vue'

/** 通知类型 */
export type NotificationType = 'success' | 'error' | 'warning' | 'info'

/** 通知对象 */
export interface Notification {
  id: string
  type: NotificationType
  title: string
  message?: string
  duration?: number
}

export interface UseNotificationReturn {
  /** 当前显示的通知列表 */
  notifications: Ref<Notification[]>
  /** 显示成功通知 */
  success: (title: string, message?: string, duration?: number) => void
  /** 显示错误通知 */
  error: (title: string, message?: string, duration?: number) => void
  /** 显示警告通知 */
  warning: (title: string, message?: string, duration?: number) => void
  /** 显示信息通知 */
  info: (title: string, message?: string, duration?: number) => void
  /** 关闭指定通知 */
  close: (id: string) => void
  /** 关闭所有通知 */
  closeAll: () => void
}

let notificationId = 0

export function useNotification(): UseNotificationReturn {
  const notifications = ref<Notification[]>([])

  function generateId(): string {
    return `notification-${++notificationId}-${Date.now()}`
  }

  function addNotification(
    type: NotificationType,
    title: string,
    message?: string,
    duration = 3000
  ) {
    const id = generateId()
    const notification: Notification = {
      id,
      type,
      title,
      message,
      duration,
    }

    notifications.value.push(notification)

    // 自动关闭
    if (duration > 0) {
      setTimeout(() => {
        close(id)
      }, duration)
    }

    return id
  }

  function success(title: string, message?: string, duration?: number) {
    return addNotification('success', title, message, duration)
  }

  function error(title: string, message?: string, duration?: number) {
    return addNotification('error', title, message, duration ?? 5000)
  }

  function warning(title: string, message?: string, duration?: number) {
    return addNotification('warning', title, message, duration)
  }

  function info(title: string, message?: string, duration?: number) {
    return addNotification('info', title, message, duration)
  }

  function close(id: string) {
    const index = notifications.value.findIndex(n => n.id === id)
    if (index > -1) {
      notifications.value.splice(index, 1)
    }
  }

  function closeAll() {
    notifications.value = []
  }

  return {
    notifications,
    success,
    error,
    warning,
    info,
    close,
    closeAll,
  }
}
