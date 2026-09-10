import { invoke } from '@tauri-apps/api/core'

import type { PluginStorage } from '@/extensions/core/types'

export class ScopedStorage implements PluginStorage {
  private prefix: string

  constructor(pluginId: string) {
    this.prefix = `${pluginId}:`
  }

  private fullKey(key: string): string {
    return `${this.prefix}${key}`
  }

  async get<T>(key: string): Promise<T | null> {
    try {
      return (await invoke('plugin_storage_get', { pluginId: this.prefix, key })) as T
    } catch {
      return null
    }
  }

  async set<T>(key: string, value: T): Promise<void> {
    await invoke('plugin_storage_set', { pluginId: this.prefix, key, value })
  }

  async delete(key: string): Promise<void> {
    await invoke('plugin_storage_delete', { pluginId: this.prefix, key })
  }

  async keys(): Promise<string[]> {
    const result = await invoke('plugin_storage_keys', { pluginId: this.prefix })
    return result as string[]
  }
}
