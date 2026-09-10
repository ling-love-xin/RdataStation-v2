import analyticsResourceExtension from '@/extensions/builtin/analytics-resource/extension'
import connectionExtension from '@/extensions/builtin/connection/extension'
import databaseExtension from '@/extensions/builtin/database/extension'
import mysqlDriverExtension from '@/extensions/builtin/mysql-driver/extension'
import queryExtension from '@/extensions/builtin/query/extension'
import scratchpadExtension from '@/extensions/builtin/scratchpad/extension'
import settingsExtension from '@/extensions/builtin/settings/extension'
import workbenchExtension from '@/extensions/builtin/workbench/extension'
import type { PluginManifest, ExtensionModule } from '@/extensions/core/types'

export interface BuiltinExtension {
  id: string
  module: ExtensionModule
  manifest: PluginManifest
}

const builtinManifests: Record<string, PluginManifest> = {
  'rdatastation.workbench': {
    plugin: {
      id: 'rdatastation.workbench',
      name: 'Workbench',
      version: '1.0.0',
      publisher: 'rdatastation',
      engines: { rdatastation: '^0.1.0' },
    },
    capabilities: { frontend: { entry: './extension.ts', activation_events: ['onProjectOpen'] } },
    contributes: {
      panels: [
        { id: 'emptyWorkbench', title: '欢迎', location: 'center', icon: 'Home', order: 0 },
        { id: 'sqlHistory', title: 'SQL历史', location: 'right', icon: 'Clock', order: 2 },
        { id: 'outputPanel', title: '输出', location: 'bottom', icon: 'Terminal', order: 1 },
        { id: 'plugins', title: '插件', location: 'left', icon: 'Puzzle', order: 3 },
        { id: 'mockPanel', title: 'Mock 数据', location: 'right', icon: 'Database', order: 1 },
        {
          id: 'dynamicObjectProperties',
          title: '对象属性',
          location: 'center',
          icon: 'Info',
          order: 10,
        },
      ],
      commands: [
        { id: 'workbench.openPanel', title: '打开面板', category: 'Workbench' },
        { id: 'workbench.closePanel', title: '关闭面板', category: 'Workbench' },
        { id: 'workbench.focusPanel', title: '聚焦面板', category: 'Workbench' },
      ],
    },
  },
  'rdatastation.connection': {
    plugin: {
      id: 'rdatastation.connection',
      name: 'Connection Manager',
      version: '1.0.0',
      publisher: 'rdatastation',
      description: 'Database connection management for RdataStation',
      engines: { rdatastation: '^0.1.0' },
    },
    capabilities: { frontend: { entry: './extension.ts', activation_events: ['onProjectOpen'] } },
    contributes: {
      commands: [
        {
          id: 'connection.create',
          title: 'Create Connection',
          category: 'Connection',
          icon: 'add',
        },
        {
          id: 'connection.test',
          title: 'Test Connection',
          category: 'Connection',
          icon: 'debug-start',
        },
        {
          id: 'connection.delete',
          title: 'Delete Connection',
          category: 'Connection',
          icon: 'trash',
        },
        {
          id: 'connection.refresh',
          title: 'Refresh Connections',
          category: 'Connection',
          icon: 'refresh',
        },
      ],
    },
  },
  'rdatastation.database': {
    plugin: {
      id: 'rdatastation.database',
      name: 'Database',
      version: '1.0.0',
      publisher: 'rdatastation',
      description: 'database module for RdataStation',
      engines: { rdatastation: '^0.1.0' },
    },
    capabilities: { frontend: { entry: './extension.ts', activation_events: ['onProjectOpen'] } },
  },
  'rdatastation.query': {
    plugin: {
      id: 'rdatastation.query',
      name: 'Query',
      version: '1.0.0',
      publisher: 'rdatastation',
      description: 'Query module for RdataStation',
      engines: { rdatastation: '^0.1.0' },
    },
    capabilities: { frontend: { entry: './extension.ts', activation_events: ['onProjectOpen'] } },
  },
  'rdatastation.analytics-resource': {
    plugin: {
      id: 'rdatastation.analytics-resource',
      name: 'Analytics Resource',
      version: '1.4.0',
      publisher: 'rdatastation',
      description: 'Analytics Resource Management',
      engines: { rdatastation: '^0.1.0' },
    },
    capabilities: { frontend: { entry: './extension.ts', activation_events: ['onProjectOpen'] } },
    contributes: {
      panels: [
        {
          id: 'analytics-resource-manager',
          title: '分析资源管理器',
          location: 'left',
          icon: 'BarChart3',
          order: 2,
        },
      ],
    },
  },
  'rdatastation.mysql-driver': {
    plugin: {
      id: 'rdatastation.mysql-driver',
      name: 'MySQL Driver',
      version: '1.0.0',
      publisher: 'rdatastation',
      description: 'MySQL Driver Extension',
      engines: { rdatastation: '^0.1.0' },
    },
    capabilities: { frontend: { entry: './extension.ts', activation_events: ['onProjectOpen'] } },
    contributes: { drivers: [{ id: 'mysql', display_name: 'MySQL', default_port: 3306 }] },
  },
  'rdatastation.scratchpad': {
    plugin: {
      id: 'rdatastation.scratchpad',
      name: 'Scratchpad',
      version: '1.0.0',
      publisher: 'rdatastation',
      description: 'Scratchpad',
      engines: { rdatastation: '^0.1.0' },
    },
    capabilities: { frontend: { entry: './extension.ts', activation_events: ['onProjectOpen'] } },
    contributes: {
      panels: [{ id: 'scratchpad', title: '草稿箱', location: 'left', icon: 'FileText', order: 4 }],
    },
  },
  'rdatastation.settings': {
    plugin: {
      id: 'rdatastation.settings',
      name: 'Settings',
      version: '1.0.0',
      publisher: 'rdatastation',
      description: 'Settings module',
      engines: { rdatastation: '^0.1.0' },
    },
    capabilities: { frontend: { entry: './extension.ts', activation_events: ['onProjectOpen'] } },
  },
}

export const builtinExtensions: BuiltinExtension[] = [
  {
    id: 'rdatastation.connection',
    module: connectionExtension,
    manifest: builtinManifests['rdatastation.connection'],
  },
  {
    id: 'rdatastation.database',
    module: databaseExtension,
    manifest: builtinManifests['rdatastation.database'],
  },
  {
    id: 'rdatastation.query',
    module: queryExtension,
    manifest: builtinManifests['rdatastation.query'],
  },
  {
    id: 'rdatastation.workbench',
    module: workbenchExtension,
    manifest: builtinManifests['rdatastation.workbench'],
  },
  {
    id: 'rdatastation.analytics-resource',
    module: analyticsResourceExtension,
    manifest: builtinManifests['rdatastation.analytics-resource'],
  },
  {
    id: 'rdatastation.mysql-driver',
    module: mysqlDriverExtension,
    manifest: builtinManifests['rdatastation.mysql-driver'],
  },
  {
    id: 'rdatastation.scratchpad',
    module: scratchpadExtension,
    manifest: builtinManifests['rdatastation.scratchpad'],
  },
  {
    id: 'rdatastation.settings',
    module: settingsExtension,
    manifest: builtinManifests['rdatastation.settings'],
  },
]

export function loadBuiltinManifests(): PluginManifest[] {
  return Object.values(builtinManifests)
}
