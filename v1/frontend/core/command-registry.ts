/**
 * 命令注册表实现
 *
 * 负责管理所有插件注册的命令
 * 提供命令的注册、执行、注销等功能
 */

import type { CommandRegistry, Disposable } from '@/extensions/core/types'

class CommandRegistryImpl implements CommandRegistry {
  private commands = new Map<string, (...args: unknown[]) => unknown>()

  /**
   * 注册命令
   * @param id 命令 ID (格式: extension.commandName)
   * @param handler 命令处理函数
   * @returns Disposable 对象，用于注销命令
   */
  registerCommand(id: string, handler: (...args: unknown[]) => unknown): Disposable {
    if (this.commands.has(id)) {
      console.warn(`[CommandRegistry] Command '${id}' already registered, overwriting`)
    }

    this.commands.set(id, handler)
    // eslint-disable-next-line no-console
    console.log(`[CommandRegistry] Registered command: ${id}`)

    return {
      dispose: () => {
        this.commands.delete(id)
        // eslint-disable-next-line no-console
        console.log(`[CommandRegistry] Unregistered command: ${id}`)
      },
    }
  }

  /**
   * 执行命令
   * @param id 命令 ID
   * @param args 命令参数
   * @returns 命令执行结果
   * @throws Error 如果命令不存在
   */
  async executeCommand(id: string, ...args: unknown[]): Promise<unknown> {
    const handler = this.commands.get(id)
    if (!handler) {
      throw new Error(`Command '${id}' not found`)
    }

    // eslint-disable-next-line no-console
    console.log(`[CommandRegistry] Executing command: ${id}`)
    return handler(...args)
  }

  /**
   * 检查命令是否存在
   * @param id 命令 ID
   * @returns 是否存在
   */
  hasCommand(id: string): boolean {
    return this.commands.has(id)
  }

  /**
   * 获取所有已注册的命令 ID
   * @returns 命令 ID 数组
   */
  getCommandIds(): string[] {
    return Array.from(this.commands.keys())
  }

  /**
   * 获取已注册命令数量
   * @returns 命令数量
   */
  count(): number {
    return this.commands.size
  }

  /**
   * 清空所有命令
   */
  clear(): void {
    this.commands.clear()
    // eslint-disable-next-line no-console
    console.log('[CommandRegistry] Cleared all commands')
  }
}

export const commandRegistry = new CommandRegistryImpl()
