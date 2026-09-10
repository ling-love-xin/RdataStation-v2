/**
 * 面板注册表实现
 *
 * 负责管理所有插件注册的面板描述符
 * 提供面板的注册、查询、按位置过滤等功能
 */

import type { PanelDescriptor, Disposable } from '@/extensions/core/types'

class PanelRegistryImpl {
  private panels = new Map<string, PanelDescriptor>()

  /**
   * 注册面板
   * @param panel 面板描述符
   * @returns Disposable 对象，用于注销面板
   */
  register(panel: PanelDescriptor): Disposable {
    if (this.panels.has(panel.id)) {
      console.warn(`[PanelRegistry] Panel '${panel.id}' already registered, overwriting`)
    }

    this.panels.set(panel.id, panel)
    // eslint-disable-next-line no-console
    console.log(`[PanelRegistry] Registered panel: ${panel.id} at ${panel.location}`)

    return {
      dispose: () => {
        this.panels.delete(panel.id)
        // eslint-disable-next-line no-console
        console.log(`[PanelRegistry] Unregistered panel: ${panel.id}`)
      },
    }
  }

  /**
   * 获取指定 ID 的面板
   * @param id 面板 ID
   * @returns 面板描述符或 undefined
   */
  get(id: string): PanelDescriptor | undefined {
    return this.panels.get(id)
  }

  /**
   * 获取所有已注册的面板
   * @returns 面板描述符数组
   */
  getAll(): PanelDescriptor[] {
    return Array.from(this.panels.values())
  }

  /**
   * 按位置获取面板列表
   * @param location 面板位置 (left/right/bottom/center)
   * @returns 指定位置的面板描述符数组
   */
  getByLocation(location: PanelDescriptor['location']): PanelDescriptor[] {
    return this.getAll()
      .filter(p => p.location === location)
      .sort((a, b) => (a.order ?? 0) - (b.order ?? 0))
  }

  /**
   * 检查面板是否已注册
   * @param id 面板 ID
   * @returns 是否已注册
   */
  has(id: string): boolean {
    return this.panels.has(id)
  }

  /**
   * 获取已注册面板数量
   * @returns 面板数量
   */
  count(): number {
    return this.panels.size
  }

  /**
   * 清空所有面板
   */
  clear(): void {
    this.panels.clear()
    // eslint-disable-next-line no-console
    console.log('[PanelRegistry] Cleared all panels')
  }
}

export const panelRegistry = new PanelRegistryImpl()
