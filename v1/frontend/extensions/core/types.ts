/**
 * 扩展系统核心类型定义
 *
 * 定义扩展生命周期、上下文、API 等核心接口
 */

import type { EventBus } from './event-bus'

// ============================================================================
// 扩展基础类型
// ============================================================================

/** 扩展唯一标识 */
export type ExtensionId = string

/** 扩展版本 */
export type ExtensionVersion = string

/** 可释放资源 */
export interface Disposable {
  dispose(): void
}

// ============================================================================
// 扩展上下文
// ============================================================================

/** 项目信息 */
export interface ProjectInfo {
  id: string
  name: string
  path: string
  description?: string
  createdAt: string
  updatedAt: string
}

/** 命令注册表 */
export interface CommandRegistry {
  registerCommand(id: string, handler: (...args: unknown[]) => unknown): Disposable
  executeCommand(id: string, ...args: unknown[]): Promise<unknown>
}

/** 面板注册表 */
export interface PanelRegistry {
  register(panel: PanelDescriptor): Disposable
}

/** 面板描述符 */
export interface PanelDescriptor {
  id: string
  name: string
  component: unknown // Vue component
  location: 'left' | 'right' | 'bottom' | 'center'
  icon?: string
  order?: number
}

/** 窗口 API */
export interface WindowAPI {
  registerViewProvider(
    id: string,
    provider: {
      component: unknown
      title: string
      location: 'left' | 'right' | 'bottom' | 'center'
      icon?: string
      order?: number
    }
  ): Disposable
  showNotification(message: string, type?: 'info' | 'warning' | 'error'): void
}

/** 工作区 API */
export interface WorkspaceAPI {
  getWorkspacePath(): string | undefined
  openFile(path: string): Promise<void>
}

/** 数据库 API */
export interface DatabaseAPI {
  executeQuery(connId: string, sql: string): Promise<unknown>
  getConnection(connId: string): unknown
  registerConnectionProvider(provider: ConnectionProvider): Disposable
}

/** SQL 编辑器 API */
export interface SqlEditorAPI {
  openEditor(connId?: string): Promise<void>
  getCurrentEditor(): unknown
}

/** 配置 API */
export interface ConfigurationAPI {
  get<T>(key: string, defaultValue?: T): T
  set<T>(key: string, value: T): Promise<void>
}

/** 工具 API */
export interface UtilsAPI {
  formatDate(date: Date): string
  formatBytes(bytes: number): string
  debounce<T extends (...args: unknown[]) => unknown>(fn: T, delay: number): T
}

/** 数据库驱动贡献 */
export interface DatabaseDriverContribution {
  id: string
  name: string
  icon: string
  features: DatabaseFeature[]
  defaultPort?: number
  connectionSchema: ConnectionSchema
}

export type DatabaseFeature =
  | 'schemas'
  | 'tables'
  | 'views'
  | 'procedures'
  | 'functions'
  | 'triggers'
  | 'indexes'
  | 'foreignKeys'
  | 'ssl'
  | 'sshTunnel'
  | 'httpProxy'

export interface ConnectionSchema {
  fields: ConnectionField[]
}

export interface ConnectionField {
  name: string
  label: string
  type: 'text' | 'number' | 'password' | 'file' | 'select' | 'checkbox'
  required?: boolean
  default?: unknown
  placeholder?: string
  options?: { label: string; value: unknown }[]
}

/** 连接提供者 */
export interface ConnectionProvider {
  readonly driverId: string
  connect(config: unknown): Promise<Connection>
}

/** 连接 */
export interface Connection {
  readonly id: string
  readonly state: 'connecting' | 'connected' | 'error' | 'closed'
  disconnect(): Promise<void>
  execute(sql: string): Promise<QueryResult>
}

/** 查询结果 */
export interface QueryResult {
  columns: string[]
  rows: unknown[][]
  rowCount: number
  executionTime: number
}

/** 扩展上下文 */
export interface ExtensionContext {
  /** 当前项目信息 */
  project: ProjectInfo

  /** 事件总线（插件间通信） */
  events: EventBus

  /** 命令注册表 */
  commands: CommandRegistry

  /** 面板注册表 */
  window: WindowAPI

  /** 工作区 API */
  workspace: WorkspaceAPI

  /** 数据库 API */
  database: DatabaseAPI

  /** SQL 编辑器 API */
  sqlEditor: SqlEditorAPI

  /** 配置 API */
  configuration: ConfigurationAPI

  /** 工具 API */
  utils: UtilsAPI

  /** 扩展存储路径 */
  extensionPath: string

  /** 订阅资源释放 */
  subscribe(disposable: Disposable): void
}

// ============================================================================
// 扩展 API
// ============================================================================

/** 扩展 API 基础接口 */
export interface ExtensionAPI {
  /** API 版本 */
  version: ExtensionVersion

  /** 当前项目 */
  project: ProjectInfo

  /** 命令注册表 */
  commands: CommandRegistry

  /** 窗口 API */
  window: WindowAPI

  /** 工作区 API */
  workspace: WorkspaceAPI

  /** 数据库 API */
  database: DatabaseAPI

  /** SQL 编辑器 API */
  sqlEditor: SqlEditorAPI

  /** 事件总线 */
  events: EventBus

  /** 配置 API */
  configuration: ConfigurationAPI

  /** 工具 API */
  utils: UtilsAPI

  /** 释放资源 */
  dispose?(): void
}

// ============================================================================
// 扩展模块
// ============================================================================

/** 扩展模块定义 */
export interface ExtensionModule {
  /** 扩展激活 */
  activate(context: ExtensionContext): ExtensionAPI | Promise<ExtensionAPI>

  /** 扩展停用 */
  deactivate?(): void | Promise<void>
}

/** 扩展元数据 */
export interface ExtensionMetadata {
  /** 扩展 ID */
  id: ExtensionId

  /** 扩展名称 */
  name: string

  /** 扩展版本 */
  version: ExtensionVersion

  /** 扩展描述 */
  description?: string

  /** 作者 */
  author?: string

  /** 依赖的其他扩展 */
  extensionDependencies?: ExtensionId[]

  /** 激活事件 */
  activationEvents?: string[]
}

// ============================================================================
// 扩展注册表
// ============================================================================

/** 扩展注册表 */
export interface ExtensionRegistry {
  /** 注册扩展 */
  register(id: ExtensionId, module: ExtensionModule, metadata: ExtensionMetadata): void

  /** 获取扩展 */
  get(id: ExtensionId): ExtensionModule | undefined

  /** 激活扩展 */
  activate(id: ExtensionId, context: ExtensionContext): Promise<ExtensionAPI>

  /** 停用扩展 */
  deactivate(id: ExtensionId): Promise<void>

  /** 获取所有已激活的扩展 */
  getActivatedExtensions(): Map<ExtensionId, ExtensionAPI>
}

// ============================================================================
// 预定义事件名称
// ============================================================================

/** 连接相关事件 */
export const ConnectionEvents = {
  /** 连接状态变化 */
  CHANGED: 'connection:changed',
  /** 连接创建 */
  CREATED: 'connection:created',
  /** 连接删除 */
  DELETED: 'connection:deleted',
  /** 连接测试完成 */
  TESTED: 'connection:tested',
} as const

/** 查询相关事件 */
export const QueryEvents = {
  /** 查询执行 */
  EXECUTED: 'query:executed',
  /** 查询取消 */
  CANCELLED: 'query:cancelled',
  /** 查询错误 */
  ERROR: 'query:error',
} as const

/** 项目相关事件 */
export const ProjectEvents = {
  /** 项目打开 */
  OPENED: 'project:opened',
  /** 项目关闭 */
  CLOSED: 'project:closed',
  /** 项目切换 */
  SWITCHED: 'project:switched',
} as const

/** 导航器相关事件 */
export const NavigatorEvents = {
  /** 刷新导航器 */
  REFRESH: 'navigator:refresh',
  /** 节点展开 */
  NODE_EXPANDED: 'navigator:nodeExpanded',
  /** 节点折叠 */
  NODE_COLLAPSED: 'navigator:nodeCollapsed',
} as const

// ============================================================================
// 统一插件 Manifest 类型（镜像 Rust 侧 manifest.rs）
// ============================================================================

export interface PluginManifest {
  plugin: PluginMeta
  capabilities: PluginCapabilities
  permissions?: PluginPermissions
  contributes?: PluginContributes
  dependencies?: PluginDependency[]
}

export interface PluginMeta {
  id: string
  name: string
  version: string
  publisher: string
  description?: string
  icon?: string
  homepage?: string
  license?: string
  engines: { rdatastation: string }
}

export interface PluginCapabilities {
  frontend?: CapabilitiesFrontend
  wasm?: CapabilitiesWasm
}

export interface CapabilitiesFrontend {
  entry: string
  activation_events?: string[]
}

export interface CapabilitiesWasm {
  entry: string
  max_memory_mb?: number
  max_cpu_time_ms?: number
  allowed_host_functions?: string[]
}

export interface PluginPermissions {
  frontend?: string[]
  wasm?: string[]
}

export interface PluginContributes {
  commands?: ContributesCommand[]
  panels?: ContributesPanel[]
  drivers?: ContributesDriver[]
  settings?: ContributesSetting[]
  menus?: Record<string, unknown>
}

export interface ContributesCommand {
  id: string
  title: string
  category?: string
  icon?: string
  shortcut?: string
}

export interface ContributesPanel {
  id: string
  title: string
  location: 'left' | 'right' | 'bottom' | 'center'
  icon?: string
  order?: number
}

export interface ContributesDriver {
  id: string
  display_name: string
  default_port?: number
  connection_schema?: string
  features?: string[]
}

export interface ContributesSetting {
  key: string
  type: string
  default: unknown
  label?: string
  description?: string
}

export interface PluginDependency {
  id: string
  version: string
}

// ============================================================================
// 新版 PluginContext（Plugin 系统）—— 取代旧 ExtensionContext
// ============================================================================

/** 新版插件上下文 */
export interface PluginContext {
  /** 插件唯一 ID */
  readonly pluginId: string

  /** 解析后的 manifest */
  readonly manifest: PluginManifest

  /** 当前项目信息 */
  readonly project: ProjectInfo

  /** 插件文件根目录 */
  readonly extensionPath: string

  /** 日志 */
  logging: {
    info(msg: string, data?: unknown): void
    warn(msg: string, data?: unknown): void
    error(msg: string, data?: unknown): void
  }

  /** 命名空间隔离的存储 */
  storage: PluginStorage

  /** 事件总线（插件间通信） */
  events: EventBusInterface

  /** 面板注册 */
  panels: PanelRegistryInterface

  /** 命令注册 */
  commands: CommandRegistryInterface

  /** 数据库访问（需权限） */
  database: PluginDatabaseAPI

  /** 系统能力（需授权） */
  system: PluginSystemAPI

  /** 订阅资源释放 */
  subscribe(disposable: Disposable): void
}

/** 插件存储接口 */
export interface PluginStorage {
  get<T>(key: string): Promise<T | null>
  set<T>(key: string, value: T): Promise<void>
  delete(key: string): Promise<void>
  keys(): Promise<string[]>
}

/** 事件总线接口 */
export interface EventBusInterface {
  emit(event: string, data: unknown): void
  on(event: string, handler: (data: unknown) => void): Disposable
}

/** 面板注册接口 */
export interface PanelRegistryInterface {
  register(panel: PanelDescriptor): Disposable
}

/** 命令注册接口 */
export interface CommandRegistryInterface {
  registerCommand(id: string, handler: (...args: unknown[]) => unknown): Disposable
  executeCommand<T>(id: string, ...args: unknown[]): Promise<T>
}

/** 插件数据库 API */
export interface PluginDatabaseAPI {
  query(connId: string, sql: string, options?: { timeout?: number }): Promise<unknown>
  getActiveConnection(): Promise<ConnectionInfo | null>
  getMetadata(
    connId: string,
    path: { catalog: string; schema: string; kind: string }
  ): Promise<unknown>
  cancelQuery(queryId: string): Promise<void>
}

/** 连接信息 */
export interface ConnectionInfo {
  id: string
  name: string
  dbType: string
  state: 'connecting' | 'connected' | 'error' | 'closed'
}

/** 插件系统 API */
export interface PluginSystemAPI {
  fetch(url: string, options?: RequestInit): Promise<Response>
  fs: PluginFileSystem
}

/** 插件文件系统 */
export interface PluginFileSystem {
  readText(path: string): Promise<string>
  writeText(path: string, content: string): Promise<void>
  listDir(path: string): Promise<FileEntry[]>
}

/** 文件条目 */
export interface FileEntry {
  name: string
  path: string
  isDirectory: boolean
  size: number
}
