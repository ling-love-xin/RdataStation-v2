/**
 * 数据库适配器注册表
 *
 * 管理所有数据库类型的元数据适配器
 */

import type { DatabaseMetaAdapter } from '@/shared/types/databaseMeta'

export type { DatabaseMetaAdapter }

// 适配器注册表
const adapters = new Map<string, DatabaseMetaAdapter>()

/**
 * 注册适配器
 */
export function registerAdapter(dbType: string, adapter: DatabaseMetaAdapter): void {
  adapters.set(dbType.toLowerCase(), adapter)
}

/**
 * 获取适配器
 */
export function getAdapter(dbType: string): DatabaseMetaAdapter | undefined {
  return adapters.get(dbType.toLowerCase())
}

/**
 * 检查是否有适配器
 */
export function hasAdapter(dbType: string): boolean {
  return adapters.has(dbType.toLowerCase())
}

/**
 * 获取所有支持的类型
 */
export function getSupportedTypes(): string[] {
  return Array.from(adapters.keys())
}
