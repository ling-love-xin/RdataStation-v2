/**
 * RdataStation Core - Project
 *
 * Project 是 RdataStation 的核心，所有业务都运行在 Project 上下文中
 */

// 类型
export type {
  ExtensionIdentifier,
  ExtensionManifest,
  ExtensionModule,
  ExtensionAPI,
  ExtensionContext,
  ProjectContext,
  ProjectStateChangeEvent,
  Memento,
  Event,
  Disposable,
  ProjectAPI,
  CommandsAPI,
  WindowAPI,
  WorkspaceAPI,
  DatabaseAPI,
  SQLEditorAPI,
  EventsAPI,
  UtilsAPI,
  ConfigurationAPI,
  ViewProvider,
  QuickPickItem,
  QuickPickOptions,
  InputBoxOptions,
  StatusBarAlignment,
  StatusBarItem,
  Configuration,
  ConnectionProvider,
  Connection,
  MetadataObject,
  QueryResult,
  SQLEditor,
  SQLDocument,
  SQLDialectContribution,
  DatabaseDriverContribution,
  CommandContribution,
  ViewContribution,
  ExtensionContributes,
  ConfigurationProperty,
  ConnectionSchema,
  ConnectionField,
  ExtensionActivateFunc,
  ExtensionDeactivateFunc,
} from './types'

// Project Store
export { useProjectStore, type Project } from './stores/project'
