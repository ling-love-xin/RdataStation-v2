/**
 * 扩展系统核心模块
 */

export { EventBus, eventBus } from './event-bus'
export type {
  Disposable,
  ExtensionId,
  ExtensionVersion,
  ProjectInfo,
  CommandRegistry,
  PanelRegistry,
  PanelDescriptor,
  WindowAPI,
  WorkspaceAPI,
  DatabaseAPI,
  SqlEditorAPI,
  ConfigurationAPI,
  UtilsAPI,
  ExtensionContext,
  ExtensionAPI,
  ExtensionModule,
  ExtensionMetadata,
  ExtensionRegistry,
  PluginContext,
  PluginStorage,
  EventBusInterface,
  PanelRegistryInterface,
  CommandRegistryInterface,
  PluginDatabaseAPI,
  ConnectionInfo,
  PluginSystemAPI,
  PluginFileSystem,
  FileEntry,
  PluginManifest,
  PluginMeta,
  PluginCapabilities,
  CapabilitiesFrontend,
  CapabilitiesWasm,
  PluginPermissions,
  PluginContributes,
  ContributesCommand,
  ContributesPanel,
  ContributesDriver,
  ContributesSetting,
  PluginDependency,
} from './types'

export { ConnectionEvents, QueryEvents, ProjectEvents, NavigatorEvents } from './types'
