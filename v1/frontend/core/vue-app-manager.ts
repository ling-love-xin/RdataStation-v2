/**
 * Vue 应用实例管理器
 *
 * 用于存储和访问 Vue 应用实例，以便在扩展系统中注册全局组件
 */

import type { App } from 'vue'

let vueAppInstance: App | null = null

/**
 * 设置 Vue 应用实例
 */
export function setVueApp(app: App): void {
  vueAppInstance = app
}

/**
 * 获取 Vue 应用实例
 */
export function getVueApp(): App | null {
  return vueAppInstance
}

/**
 * 注册全局组件
 */
export function registerGlobalComponent(name: string, component: unknown): void {
  if (vueAppInstance) {
    vueAppInstance.component(name, component as Parameters<typeof vueAppInstance.component>[1])
    // eslint-disable-next-line no-console
    console.log(`[VueAppManager] Registered global component: ${name}`)
  } else {
    console.warn('[VueAppManager] Vue app instance not set, cannot register component:', name)
  }
}
