import { ref } from 'vue'

export type NotificationType = 'success' | 'error' | 'warning' | 'info'

export interface Notification {
  id: string
  type: NotificationType
  title: string
  message: string
  duration: number
  closable: boolean
  createdAt: Date
}

const notifications = ref<Notification[]>([])

const notificationIdCounter = ref(0)

export function useNotification() {
  function createNotification(
    type: NotificationType,
    title: string,
    message: string,
    duration = 3000
  ): string {
    const id = `notification_${++notificationIdCounter.value}`

    const notification: Notification = {
      id,
      type,
      title,
      message,
      duration,
      closable: true,
      createdAt: new Date(),
    }

    notifications.value.push(notification)

    if (duration > 0) {
      setTimeout(() => {
        removeNotification(id)
      }, duration)
    }

    return id
  }

  function removeNotification(id: string) {
    const index = notifications.value.findIndex(n => n.id === id)
    if (index !== -1) {
      notifications.value.splice(index, 1)
    }
  }

  function clearAll() {
    notifications.value = []
  }

  function success(title: string, message = '', duration = 3000): string {
    return createNotification('success', title, message, duration)
  }

  function error(title: string, message = '', duration = 5000): string {
    return createNotification('error', title, message, duration)
  }

  function warning(title: string, message = '', duration = 4000): string {
    return createNotification('warning', title, message, duration)
  }

  function info(title: string, message = '', duration = 3000): string {
    return createNotification('info', title, message, duration)
  }

  return {
    notifications,
    createNotification,
    removeNotification,
    clearAll,
    success,
    error,
    warning,
    info,
  }
}
