/**
 * 事件总线实现
 *
 * 提供组件间松耦合的通信机制
 */

export type EventHandler<T = unknown> = (data: T) => void

export interface EventSubscription {
  eventName: string
  handler: EventHandler
  once: boolean
}

export class EventBus {
  private subscriptions = new Map<string, EventSubscription[]>()

  /**
   * 订阅事件
   */
  on<T = unknown>(eventName: string, handler: EventHandler<T>): () => void {
    if (!this.subscriptions.has(eventName)) {
      this.subscriptions.set(eventName, [])
    }

    const subscription: EventSubscription = {
      eventName,
      handler: handler as EventHandler,
      once: false,
    }

    this.subscriptions.get(eventName)?.push(subscription)

    return () => {
      this.off(eventName, handler)
    }
  }

  /**
   * 订阅单次事件
   */
  once<T = unknown>(eventName: string, handler: EventHandler<T>): () => void {
    if (!this.subscriptions.has(eventName)) {
      this.subscriptions.set(eventName, [])
    }

    const subscription: EventSubscription = {
      eventName,
      handler: handler as EventHandler,
      once: true,
    }

    this.subscriptions.get(eventName)?.push(subscription)

    return () => {
      this.off(eventName, handler)
    }
  }

  /**
   * 取消订阅
   */
  off<T = unknown>(eventName: string, handler: EventHandler<T>): void {
    const handlers = this.subscriptions.get(eventName)
    if (handlers) {
      const index = handlers.findIndex(h => h.handler === handler)
      if (index !== -1) {
        handlers.splice(index, 1)
      }
    }
  }

  /**
   * 发布事件
   */
  emit<T = unknown>(eventName: string, data?: T): void {
    const subscriptions = this.subscriptions.get(eventName)
    if (!subscriptions) return

    const toRemove: number[] = []

    subscriptions.forEach((subscription, index) => {
      try {
        subscription.handler(data as T)
      } catch (error) {
        console.error(`Error handling event "${eventName}":`, error)
      }

      if (subscription.once) {
        toRemove.push(index)
      }
    })

    for (let i = toRemove.length - 1; i >= 0; i--) {
      subscriptions.splice(toRemove[i], 1)
    }
  }

  /**
   * 获取事件订阅数量
   */
  getSubscriptionCount(eventName: string): number {
    return this.subscriptions.get(eventName)?.length ?? 0
  }

  /**
   * 清除所有订阅
   */
  clear(): void {
    this.subscriptions.clear()
  }

  /**
   * 清除指定事件的所有订阅
   */
  clearEvent(eventName: string): void {
    this.subscriptions.delete(eventName)
  }
}

export const eventBus = new EventBus()

export type NavigatorEvent =
  | 'connection-connected'
  | 'connection-disconnected'
  | 'connection-error'
  | 'node-expanded'
  | 'node-collapsed'
  | 'node-selected'
  | 'transaction-started'
  | 'transaction-committed'
  | 'transaction-rolled-back'
  | 'group-created'
  | 'group-updated'
  | 'group-deleted'
  | 'search-query-change'
  | 'filters-change'
  | 'refresh-requested'
