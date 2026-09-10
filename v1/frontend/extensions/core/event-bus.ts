/**
 * 扩展系统事件总线
 *
 * 用于插件间通信，避免直接引用其他插件的 store
 * 遵循插件隔离原则
 */

type EventHandler = (...args: unknown[]) => void

interface EventSubscription {
  dispose(): void
}

export class EventBus {
  private handlers = new Map<string, Set<EventHandler>>()

  /**
   * 订阅事件
   *
   * @param event 事件名称
   * @param handler 事件处理函数
   * @returns 订阅对象，调用 dispose() 取消订阅
   */
  on(event: string, handler: EventHandler): EventSubscription {
    let handlers = this.handlers.get(event)
    if (!handlers) {
      handlers = new Set()
      this.handlers.set(event, handlers)
    }
    handlers.add(handler)

    return {
      dispose: () => this.off(event, handler),
    }
  }

  /**
   * 取消订阅
   *
   * @param event 事件名称
   * @param handler 事件处理函数
   */
  off(event: string, handler: EventHandler): void {
    this.handlers.get(event)?.delete(handler)
  }

  /**
   * 触发事件
   *
   * @param event 事件名称
   * @param args 事件参数
   */
  emit(event: string, ...args: unknown[]): void {
    const eventHandlers = this.handlers.get(event)
    if (eventHandlers) {
      for (const handler of eventHandlers) {
        try {
          handler(...args)
        } catch (error) {
          console.error(`[EventBus] Error in handler for event "${event}":`, error)
        }
      }
    }
  }

  /**
   * 一次性事件监听
   *
   * @param event 事件名称
   * @param handler 事件处理函数
   */
  once(event: string, handler: EventHandler): EventSubscription {
    const onceHandler = (...args: unknown[]) => {
      handler(...args)
      this.off(event, onceHandler)
    }
    return this.on(event, onceHandler)
  }

  /**
   * 移除事件的所有监听器
   *
   * @param event 事件名称
   */
  removeAllListeners(event?: string): void {
    if (event) {
      this.handlers.delete(event)
    } else {
      this.handlers.clear()
    }
  }

  /**
   * 获取事件的监听器数量
   *
   * @param event 事件名称
   */
  listenerCount(event: string): number {
    return this.handlers.get(event)?.size ?? 0
  }

  /**
   * 销毁事件总线
   */
  dispose(): void {
    this.removeAllListeners()
  }
}

/**
 * 创建全局事件总线实例
 */
export const eventBus = new EventBus()
