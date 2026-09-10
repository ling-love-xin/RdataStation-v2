/**
 * 扩展主机实现
 *
 * 负责扩展的生命周期管理：
 * - 加载和激活扩展
 * - 注入 ExtensionContext
 * - 管理扩展的停用和资源清理
 *
 * 遵循 VSCode 式扩展架构，确保插件隔离和生命周期管理
 */

import { eventBus } from '@/extensions/core/event-bus'
import type {
  ExtensionContext,
  ExtensionModule,
  ExtensionAPI,
  Disposable,
  ProjectInfo,
  PluginManifest,
  PluginContext,
  PanelDescriptor,
  ConnectionInfo,
  FileEntry,
} from '@/extensions/core/types'

import { commandRegistry } from './command-registry'
import { ScopedStorage } from './scoped-storage'
import { windowAPI } from './window-api'

/**
 * 简单的 Memento 实现（用于存储扩展状态）
 */
class MementoImpl {
  private data = new Map<string, unknown>()

  get<T>(key: string, defaultValue?: T): T | undefined {
    return (this.data.get(key) ?? defaultValue) as T | undefined
  }

  async update(key: string, value: unknown): Promise<void> {
    this.data.set(key, value)
  }

  keys(): readonly string[] {
    return Array.from(this.data.keys())
  }
}

/**
 * 扩展主机类
 */
class ExtensionHost {
  private activatedExtensions = new Map<string, ExtensionAPI>()
  private globalState = new MementoImpl()
  private workspaceState = new MementoImpl()

  /**
   * 激活扩展
   * @param extensions 扩展列表（ID + 模块）
   * @param projectInfo 项目信息
   */
  async activateExtensions(
    extensions: Array<{ id: string; module: ExtensionModule }>,
    projectInfo: ProjectInfo
  ): Promise<void> {
    // eslint-disable-next-line no-console
    console.log(
      `[ExtensionHost] Activating ${extensions.length} extensions for project: ${projectInfo.name}`
    )

    for (const { id, module } of extensions) {
      try {
        const context: ExtensionContext = {
          project: projectInfo,
          events: eventBus,
          commands: commandRegistry,
          window: windowAPI,
          workspace: this.createWorkspaceAPI(),
          database: this.createDatabaseAPI(),
          sqlEditor: this.createSqlEditorAPI(),
          configuration: this.createConfigurationAPI(),
          utils: this.createUtilsAPI(),
          extensionPath: `/extensions/${id}`,
          subscribe: (_disposable: Disposable) => {
            // 将 disposable 添加到订阅列表，扩展停用时自动清理
          },
        }

        const api = await module.activate(context)
        this.activatedExtensions.set(id, api)
        // eslint-disable-next-line no-console
        console.log(`[ExtensionHost] ✅ Activated: ${id}`)
      } catch (error) {
        console.error(`[ExtensionHost] ❌ Failed to activate ${id}:`, error)
      }
    }

    // eslint-disable-next-line no-console
    console.log(
      `[ExtensionHost] Successfully activated ${this.activatedExtensions.size}/${extensions.length} extensions`
    )
  }

  /**
   * 停用所有扩展
   */
  async deactivateExtensions(): Promise<void> {
    // eslint-disable-next-line no-console
    console.log(`[ExtensionHost] Deactivating ${this.activatedExtensions.size} extensions`)

    for (const [id, api] of this.activatedExtensions) {
      try {
        if (api.dispose) {
          api.dispose()
        }
        // eslint-disable-next-line no-console
        console.log(`[ExtensionHost] ✅ Deactivated: ${id}`)
      } catch (error) {
        console.error(`[ExtensionHost] ❌ Failed to deactivate ${id}:`, error)
      }
    }

    this.activatedExtensions.clear()
    // eslint-disable-next-line no-console
    console.log('[ExtensionHost] All extensions deactivated')
  }

  /**
   * 获取已激活的扩展 API
   * @param id 扩展 ID
   * @returns 扩展 API 或 undefined
   */
  getExtension(id: string): ExtensionAPI | undefined {
    return this.activatedExtensions.get(id)
  }

  /**
   * 获取所有已激活的扩展 ID
   * @returns 扩展 ID 数组
   */
  getActivatedExtensionIds(): string[] {
    return Array.from(this.activatedExtensions.keys())
  }

  /**
   * 检查扩展是否已激活
   * @param id 扩展 ID
   * @returns 是否已激活
   */
  isExtensionActivated(id: string): boolean {
    return this.activatedExtensions.has(id)
  }

  // ==================== 私有方法 ====================

  /**
   * 创建工作区 API
   */
  private createWorkspaceAPI() {
    return {
      getWorkspacePath(): string | undefined {
        return undefined
      },
      async openFile(_path: string): Promise<void> {
        // eslint-disable-next-line no-console
        console.log('[WorkspaceAPI] openFile not implemented')
      },
    }
  }

  /**
   * 创建数据库 API
   */
  private createDatabaseAPI() {
    const connectionProviders = new Map<string, unknown>()

    return {
      registerConnectionProvider(provider: { driverId: string }): Disposable {
        connectionProviders.set(provider.driverId, provider)
        // eslint-disable-next-line no-console
        console.log(`[DatabaseAPI] Registered connection provider: ${provider.driverId}`)

        return {
          dispose: () => {
            connectionProviders.delete(provider.driverId)
            // eslint-disable-next-line no-console
            console.log(`[DatabaseAPI] Unregistered connection provider: ${provider.driverId}`)
          },
        }
      },
      async executeQuery(_connId: string, _sql: string): Promise<unknown> {
        // eslint-disable-next-line no-console
        console.log('[DatabaseAPI] executeQuery not implemented')
        return null
      },
      getConnection(_connId: string): unknown {
        // eslint-disable-next-line no-console
        console.log('[DatabaseAPI] getConnection not implemented')
        return null
      },
      getConnectionProviders(): Map<string, unknown> {
        return connectionProviders
      },
    }
  }

  /**
   * 创建 SQL 编辑器 API
   */
  private createSqlEditorAPI() {
    return {
      async openEditor(_connId?: string): Promise<void> {
        // eslint-disable-next-line no-console
        console.log('[SqlEditorAPI] openEditor not implemented')
      },
      getCurrentEditor(): unknown {
        // eslint-disable-next-line no-console
        console.log('[SqlEditorAPI] getCurrentEditor not implemented')
        return null
      },
    }
  }

  /**
   * 创建配置 API
   */
  private createConfigurationAPI() {
    const config = new Map<string, unknown>()

    return {
      get<T>(key: string, defaultValue?: T): T {
        return (config.get(key) ?? defaultValue) as T
      },
      async set<T>(key: string, value: T): Promise<void> {
        config.set(key, value)
      },
    }
  }

  /**
   * 创建工具 API
   */
  private createUtilsAPI() {
    return {
      formatDate(date: Date): string {
        return date.toLocaleString()
      },
      formatBytes(bytes: number): string {
        if (bytes === 0) return '0 Bytes'
        const k = 1024
        const sizes = ['Bytes', 'KB', 'MB', 'GB']
        const i = Math.floor(Math.log(bytes) / Math.log(k))
        return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i]
      },
      debounce<T extends (...args: unknown[]) => unknown>(fn: T, delay: number): T {
        let timeoutId: ReturnType<typeof setTimeout> | null = null
        return ((...args: unknown[]) => {
          if (timeoutId) clearTimeout(timeoutId)
          timeoutId = setTimeout(() => fn(...args), delay)
        }) as T
      },
    }
  }
}

export const extensionHost = new ExtensionHost()

export function createPluginContext(
  pluginId: string,
  manifest: PluginManifest,
  project: ProjectInfo,
  extensionPath: string
): PluginContext {
  const disposables: Disposable[] = []

  return {
    pluginId,
    manifest,
    project,
    extensionPath,

    logging: {
      // eslint-disable-next-line no-console
      info: (msg, data) => console.info(`[Plugin:${pluginId}]`, msg, data ?? ''),
      warn: (msg, data) => console.warn(`[Plugin:${pluginId}]`, msg, data ?? ''),
      error: (msg, data) => console.error(`[Plugin:${pluginId}]`, msg, data ?? ''),
    },

    storage: new ScopedStorage(pluginId),

    events: {
      emit: (event, data) => eventBus.emit(event, data),
      on: (event, handler) => {
        const sub = eventBus.on(event, handler as (...args: unknown[]) => void)
        disposables.push(sub as unknown as Disposable)
        return sub as unknown as Disposable
      },
    },

    panels: {
      register: (panel: PanelDescriptor) => {
        const d = windowAPI.registerViewProvider(panel.id, {
          component: panel.component,
          title: panel.name,
          location: panel.location,
          icon: panel.icon,
          order: panel.order,
        })
        disposables.push(d)
        return d
      },
    },

    commands: {
      registerCommand: (id, handler) => {
        const d = commandRegistry.registerCommand(id, handler)
        disposables.push(d)
        return d
      },
      // @ts-expect-error: specta v2 generic constraint too strict for dynamic command registry
      executeCommand: (id, ...args) => commandRegistry.executeCommand(id, ...args),
    },

    database: {
      query: async (connId, sql, options) => {
        const { invoke } = await import('@tauri-apps/api/core')
        return invoke('plugin_db_query', { pluginId, connId, sql, timeout: options?.timeout })
      },
      getActiveConnection: async () => {
        const { invoke } = await import('@tauri-apps/api/core')
        return invoke('get_active_connection') as Promise<ConnectionInfo | null>
      },
      getMetadata: async (connId, path) => {
        const { invoke } = await import('@tauri-apps/api/core')
        return invoke('plugin_db_metadata', { pluginId, connId, ...path })
      },
      cancelQuery: async queryId => {
        const { invoke } = await import('@tauri-apps/api/core')
        return invoke('cancel_sql_query', { queryId })
      },
    },

    system: {
      fetch: async (url, options) => {
        return fetch(url, options)
      },
      fs: {
        readText: async path => {
          const { invoke } = await import('@tauri-apps/api/core')
          return invoke('plugin_fs_read_text', { pluginId, path }) as Promise<string>
        },
        writeText: async (path, content) => {
          const { invoke } = await import('@tauri-apps/api/core')
          return invoke('plugin_fs_write_text', { pluginId, path, content })
        },
        listDir: async path => {
          const { invoke } = await import('@tauri-apps/api/core')
          return invoke('plugin_fs_list_dir', { pluginId, path }) as Promise<FileEntry[]>
        },
      },
    },

    subscribe: disposable => {
      disposables.push(disposable)
    },
  }
}
