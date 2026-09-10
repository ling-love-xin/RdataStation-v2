/**
 * VS Code 式 Extension Platform 类型定义
 *
 * 设计原则：
 * 1. Project 是第一公民 - 所有扩展都运行在 Project 上下文中
 * 2. 声明式贡献 - 通过 package.json 声明贡献点
 * 3. 生命周期管理 - 严格的激活/停用生命周期
 * 4. 隔离性 - 扩展之间通过 API 通信，不直接访问内部状态
 */

import type { Component } from 'vue'

// ============================================================================
// 扩展标识
// ============================================================================

export interface ExtensionIdentifier {
  id: string
  publisher: string
  name: string
  version: string
}

// ============================================================================
// 扩展清单 (package.json 中的 contributes 部分)
// ============================================================================

export interface ExtensionManifest {
  name: string
  publisher: string
  version: string
  engines: {
    rdatastation: string
  }
  categories?: string[]
  keywords?: string[]
  activationEvents: string[]
  contributes?: ExtensionContributes
  main?: string
  browser?: string
}

export interface ExtensionContributes {
  // 命令贡献
  commands?: CommandContribution[]

  // 菜单贡献
  menus?: MenuContributions

  // 视图容器贡献 (左侧/右侧/底部面板)
  viewsContainers?: ViewsContainerContribution[]

  // 视图贡献
  views?: ViewsContribution

  // 配置贡献
  configuration?: ConfigurationContribution

  // 数据库驱动贡献
  databaseDrivers?: DatabaseDriverContribution[]

  // SQL 方言贡献
  sqlDialects?: SQLDialectContribution[]

  // 主题贡献
  themes?: ThemeContribution[]

  // 图标主题贡献
  iconThemes?: IconThemeContribution[]

  // 快捷键贡献
  keybindings?: KeybindingContribution[]
}

// ============================================================================
// 命令贡献
// ============================================================================

export interface CommandContribution {
  command: string
  title: string
  category?: string
  icon?: string
  when?: string // 条件表达式
}

export interface MenuContributions {
  commandPalette?: MenuItem[]
  'editor/context'?: MenuItem[]
  'editor/title'?: MenuItem[]
  'editor/title/context'?: MenuItem[]
  'explorer/context'?: MenuItem[]
  'view/title'?: MenuItem[]
  'view/item/context'?: MenuItem[]
  statusBar?: MenuItem[]
}

export interface MenuItem {
  command: string
  when?: string
  group?: string
  alt?: string
}

// ============================================================================
// 视图贡献 (类似 VS Code 的侧边栏视图)
// ============================================================================

export interface ViewsContainerContribution {
  id: string
  title: string
  icon: string
  location: 'left' | 'right' | 'bottom'
}

export interface ViewsContribution {
  [containerId: string]: ViewContribution[]
}

export interface ViewContribution {
  id: string
  name: string
  when?: string
  icon?: string
  contextualTitle?: string
  visibility?: 'visible' | 'collapsed'
}

// ============================================================================
// 配置贡献
// ============================================================================

export interface ConfigurationContribution {
  title?: string
  properties: {
    [key: string]: ConfigurationProperty
  }
}

export interface ConfigurationProperty {
  type: 'string' | 'number' | 'boolean' | 'array' | 'object'
  default?: unknown
  description?: string
  enum?: unknown[]
  enumDescriptions?: string[]
}

// ============================================================================
// 数据库驱动贡献
// ============================================================================

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

// ============================================================================
// SQL 方言贡献
// ============================================================================

export interface SQLDialectContribution {
  id: string
  name: string
  fileExtensions: string[]
  keywords: string[]
  functions: string[]
  operators: string[]
  formatter?: SQLFormatterConfig
}

export interface SQLFormatterConfig {
  indentSize: number
  keywordCase: 'upper' | 'lower' | 'preserve'
  identifierCase: 'upper' | 'lower' | 'preserve'
}

// ============================================================================
// 主题贡献
// ============================================================================

export interface ThemeContribution {
  id: string
  label: string
  uiTheme: 'vs' | 'vs-dark' | 'hc-black'
  path: string
}

export interface IconThemeContribution {
  id: string
  label: string
  path: string
}

// ============================================================================
// 快捷键贡献
// ============================================================================

export interface KeybindingContribution {
  command: string
  key: string
  mac?: string
  linux?: string
  win?: string
  when?: string
}

// ============================================================================
// 扩展上下文 (Project 作为第一公民)
// ============================================================================

export interface ExtensionContext {
  // 当前项目上下文 - 这是核心！
  readonly project: ProjectContext

  // 扩展信息
  readonly extension: ExtensionIdentifier

  // 全局状态 (跨项目)
  readonly globalState: Memento

  // 工作区状态 (项目级别)
  readonly workspaceState: Memento

  // 存储路径
  readonly storagePath: string
  readonly globalStoragePath: string

  // 日志通道
  readonly logPath: string

  // 订阅列表 (用于资源清理)
  readonly subscriptions: Disposable[]

  // ===== API 访问 =====
  // 这些 API 由 Extension Host 注入，供扩展使用

  // 命令 API
  readonly commands: CommandsAPI

  // 窗口/视图 API
  readonly window: WindowAPI

  // 工作区 API
  readonly workspace: WorkspaceAPI

  // 配置 API
  readonly configuration: ConfigurationAPI

  // 数据库 API
  readonly database: DatabaseAPI

  // SQL 编辑器 API
  readonly sqlEditor: SQLEditorAPI

  // 事件 API
  readonly events: EventsAPI

  // 工具 API
  readonly utils: UtilsAPI
}

export interface ProjectContext {
  readonly id: string
  readonly name: string
  readonly path: string
  readonly isActive: boolean

  // 项目级别的事件
  onDidChangeState: Event<ProjectStateChangeEvent>
  onDidClose: Event<void>
}

export interface ProjectStateChangeEvent {
  key: string
  value: unknown
}

export interface Memento {
  get<T>(key: string, defaultValue?: T): T | undefined
  update(key: string, value: unknown): Promise<void>
}

// ============================================================================
// 事件系统
// ============================================================================

export interface Event<T> {
  (listener: (e: T) => unknown, thisArgs?: unknown, disposables?: Disposable[]): Disposable
}

export interface Disposable {
  dispose(): void
}

// ============================================================================
// 扩展 API (提供给扩展使用)
// ============================================================================

export interface ExtensionAPI {
  // 版本
  readonly version: string

  // 项目上下文 (使用 ProjectContext 而不是 ProjectAPI)
  readonly project: ProjectContext

  // 命令
  readonly commands: CommandsAPI

  // 窗口/视图
  readonly window: WindowAPI

  // 工作区
  readonly workspace: WorkspaceAPI

  // 配置
  readonly configuration: ConfigurationAPI

  // 数据库
  readonly database: DatabaseAPI

  // SQL 编辑器
  readonly sqlEditor: SQLEditorAPI

  // 事件
  readonly events: EventsAPI

  // 工具
  readonly utils: UtilsAPI

  // 资源清理方法
  dispose?(): void
}

// ============================================================================
// 项目 API
// ============================================================================

export interface ProjectAPI {
  // 当前项目
  readonly current: ProjectContext | undefined

  // 所有打开的项目
  readonly all: readonly ProjectContext[]

  // 事件
  readonly onDidChangeProject: Event<ProjectContext | undefined>
  readonly onDidOpenProject: Event<ProjectContext>
  readonly onDidCloseProject: Event<ProjectContext>

  // 方法
  open(path: string): Promise<ProjectContext>
  close(projectId: string): Promise<void>
  getProjectData<T>(key: string): Promise<T | undefined>
  setProjectData<T>(key: string, value: T): Promise<void>
}

// ============================================================================
// 命令 API
// ============================================================================

export interface CommandsAPI {
  registerCommand(
    command: string,
    callback: (...args: unknown[]) => unknown | Promise<unknown>
  ): Disposable
  executeCommand<T>(command: string, ...args: unknown[]): Promise<T | undefined>
}

// ============================================================================
// 窗口 API
// ============================================================================

export interface WindowAPI {
  // 视图
  registerViewProvider(viewId: string, provider: ViewProvider): Disposable

  // 通知
  showInformationMessage(message: string, ...items: string[]): Promise<string | undefined>
  showWarningMessage(message: string, ...items: string[]): Promise<string | undefined>
  showErrorMessage(message: string, ...items: string[]): Promise<string | undefined>

  // 输入
  showInputBox(options?: InputBoxOptions): Promise<string | undefined>
  showQuickPick<T extends QuickPickItem>(
    items: T[],
    options?: QuickPickOptions
  ): Promise<T | undefined>

  // 状态栏
  createStatusBarItem(alignment?: StatusBarAlignment, priority?: number): StatusBarItem
}

export interface ViewProvider {
  resolveView(): Component | Promise<Component>
}

export interface QuickPickItem {
  label: string
  description?: string
  detail?: string
  picked?: boolean
}

export interface QuickPickOptions {
  placeHolder?: string
  canPickMany?: boolean
}

export interface InputBoxOptions {
  prompt?: string
  placeHolder?: string
  value?: string
  password?: boolean
  validateInput?(value: string): string | undefined | Promise<string | undefined>
}

export enum StatusBarAlignment {
  Left = 1,
  Right = 2,
}

export interface StatusBarItem {
  text: string
  tooltip: string | undefined
  command: string | undefined
  show(): void
  hide(): void
  dispose(): void
}

// ============================================================================
// 工作区 API
// ============================================================================

export interface WorkspaceAPI {
  // 文件系统
  readFile(path: string): Promise<Uint8Array>
  writeFile(path: string, content: Uint8Array): Promise<void>
  exists(path: string): Promise<boolean>

  // 项目配置
  getConfiguration(section?: string): Configuration
}

export interface Configuration {
  get<T>(key: string, defaultValue?: T): T
  update(key: string, value: unknown): Promise<void>
}

// ============================================================================
// 配置 API
// ============================================================================

export interface ConfigurationAPI {
  get<T>(key: string, defaultValue?: T): T
  update(key: string, value: unknown): Promise<void>
}

// ============================================================================
// 数据库 API
// ============================================================================

export interface DatabaseAPI {
  // 连接管理
  registerConnectionProvider(provider: ConnectionProvider): Disposable

  // 元数据
  getMetadata(connectionId: string, objectPath: string[]): Promise<MetadataObject | undefined>

  // 查询
  executeQuery(connectionId: string, sql: string): Promise<QueryResult>
}

export interface ConnectionProvider {
  readonly driverId: string
  connect(config: unknown): Promise<Connection>
}

export interface Connection {
  readonly id: string
  readonly state: 'connecting' | 'connected' | 'error' | 'closed'
  disconnect(): Promise<void>
  execute(sql: string): Promise<QueryResult>
}

export interface MetadataObject {
  name: string
  type: string
  children?: MetadataObject[]
  properties?: Record<string, unknown>
}

export interface QueryResult {
  columns: string[]
  rows: unknown[][]
  rowCount: number
  executionTime: number
}

// ============================================================================
// SQL 编辑器 API
// ============================================================================

export interface SQLEditorAPI {
  // 注册 SQL 方言
  registerSQLDialect(dialect: SQLDialectContribution): Disposable

  // 编辑器操作
  getActiveEditor(): SQLEditor | undefined
  openQuery(connectionId: string, sql?: string): Promise<SQLEditor>

  // 事件
  readonly onDidChangeActiveEditor: Event<SQLEditor | undefined>
}

export interface SQLEditor {
  readonly id: string
  readonly connectionId: string | undefined
  readonly document: SQLDocument

  execute(): Promise<QueryResult>
  format(): Promise<void>
}

export interface SQLDocument {
  getText(): string
  setText(text: string): void
  getSelection(): { start: number; end: number }
}

// ============================================================================
// 事件 API
// ============================================================================

export interface EventsAPI {
  // 项目事件
  readonly onProjectOpen: Event<ProjectContext>
  readonly onProjectClose: Event<ProjectContext>
  readonly onProjectDataChange: Event<{ projectId: string; key: string; value: unknown }>

  // 数据库事件
  readonly onConnectionConnect: Event<{ connectionId: string; projectId: string }>
  readonly onConnectionDisconnect: Event<{ connectionId: string; projectId: string }>

  // SQL 编辑器事件
  readonly onQueryExecute: Event<{ editorId: string; sql: string; connectionId: string }>
  readonly onQueryComplete: Event<{ editorId: string; result: QueryResult }>
}

// ============================================================================
// 工具 API
// ============================================================================

export interface UtilsAPI {
  // URI 处理
  uri: {
    file(path: string): { scheme: string; path: string }
    parse(uri: string): { scheme: string; path: string }
  }

  // 事件辅助
  Event: {
    readonly None: Event<unknown>
    map<T, U>(event: Event<T>, mapFunc: (e: T) => U): Event<U>
    filter<T>(event: Event<T>, filterFunc: (e: T) => boolean): Event<T>
  }

  // Disposable 辅助
  Disposable: {
    from(...disposables: Disposable[]): Disposable
    create(func: () => void): Disposable
  }
}

// ============================================================================
// 扩展激活/停用
// ============================================================================

export type ExtensionActivateFunc = (
  context: ExtensionContext
) => ExtensionAPI | Promise<ExtensionAPI>
export type ExtensionDeactivateFunc = () => void | Promise<void>

export interface ExtensionModule {
  activate: ExtensionActivateFunc
  deactivate?: ExtensionDeactivateFunc
}
