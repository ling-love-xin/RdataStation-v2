/**
 * 窗口 API 实现
 *
 * 提供视图提供者注册、通知显示等窗口相关功能
 * 作为插件与面板注册表之间的桥梁
 */

import type { WindowAPI, Disposable } from '@/extensions/core/types'

import { panelRegistry } from './panel-registry'

/**
 * 视图提供者配置接口
 */
interface ViewProviderConfig {
  component: unknown
  title: string
  location: 'left' | 'right' | 'bottom' | 'center'
  icon?: string
  order?: number
}

/**
 * 窗口 API 实现
 */
export const windowAPI: WindowAPI = {
  /**
   * 注册视图提供者
   * @param id 视图唯一标识
   * @param provider 视图提供者配置或组件
   * @returns Disposable 对象，用于注销视图
   */
  registerViewProvider(id: string, provider: unknown): Disposable {
    // 支持两种注册方式：
    // 1. 直接传入 Vue 组件（向后兼容）
    // 2. 传入 ViewProviderConfig 对象（推荐）

    let config: ViewProviderConfig

    if (typeof provider === 'object' && provider !== null && 'component' in provider) {
      config = provider as ViewProviderConfig
    } else {
      // 向后兼容：直接传入组件
      config = {
        component: provider,
        title: id,
        location: 'center',
      }
    }

    return panelRegistry.register({
      id,
      name: config.title,
      component: config.component,
      location: config.location,
      icon: config.icon,
      order: config.order,
    })
  },

  /**
   * 显示通知
   * @param message 通知消息
   * @param type 通知类型
   */
  showNotification(message: string, type: 'info' | 'warning' | 'error' = 'info'): void {
    // eslint-disable-next-line no-console
    console.log(`[WindowAPI] Notification [${type}]: ${message}`)
    // TODO: 集成 Naive UI 的 notification 组件
    // 可以使用 window.dispatchEvent 触发自定义事件
    window.dispatchEvent(
      new CustomEvent('show-notification', {
        detail: { message, type },
      })
    )
  },
}
