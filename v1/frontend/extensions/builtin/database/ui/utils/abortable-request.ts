/**
 * 可取消的请求工具
 *
 * 用于在用户快速操作时取消之前的异步请求，避免不必要的网络开销和状态污染
 */

export class AbortableRequest {
  private abortControllers = new Map<string, AbortController>()

  /**
   * 创建可取消的请求
   */
  create<T>(key: string, executor: (signal: AbortSignal) => Promise<T>): Promise<T> {
    // 取消之前的同名请求
    this.abort(key)

    const controller = new AbortController()
    this.abortControllers.set(key, controller)

    return executor(controller.signal).finally(() => {
      this.abortControllers.delete(key)
    })
  }

  /**
   * 取消指定请求
   */
  abort(key: string): void {
    const controller = this.abortControllers.get(key)
    if (controller) {
      controller.abort()
      this.abortControllers.delete(key)
    }
  }

  /**
   * 取消所有请求
   */
  abortAll(): void {
    this.abortControllers.forEach(controller => controller.abort())
    this.abortControllers.clear()
  }

  /**
   * 检查是否有进行中的请求
   */
  hasPending(key: string): boolean {
    return this.abortControllers.has(key)
  }
}

export const abortableRequest = new AbortableRequest()
