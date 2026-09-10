/**
 * Database Extension
 *
 * 提供数据库对象浏览能力
 */

import { navigatorApi } from '@/shared/api'
import type { NavigatorNodeResponse } from '@/shared/api'

import DatabaseNavigator from './ui/components/database-navigator.vue'

import type { ExtensionContext, ExtensionAPI, ExtensionModule, Disposable } from '../../core/types'

interface DatabaseExtensionAPI extends ExtensionAPI {
  databaseBrowser: {
    getCatalogs(connectionId: string): Promise<NavigatorNodeResponse[]>
    getSchemas(connectionId: string, catalog: string): Promise<NavigatorNodeResponse[]>
    getTables(
      connectionId: string,
      catalog: string,
      schema: string
    ): Promise<NavigatorNodeResponse[]>
    getViews(
      connectionId: string,
      catalog: string,
      schema: string
    ): Promise<NavigatorNodeResponse[]>
    getColumns(
      connectionId: string,
      catalog: string,
      schema: string,
      table: string
    ): Promise<NavigatorNodeResponse[]>
    refresh(connectionId: string): Promise<void>
  }
}

const activate = (context: ExtensionContext): DatabaseExtensionAPI => {
  // eslint-disable-next-line no-console
  console.log('[Database] Activating for project:', context.project.name)

  const getCatalogs = async (connectionId: string): Promise<NavigatorNodeResponse[]> => {
    return navigatorApi.getCatalogs(connectionId)
  }

  const getSchemas = async (
    connectionId: string,
    catalog: string
  ): Promise<NavigatorNodeResponse[]> => {
    return navigatorApi.getSchemas(connectionId, catalog)
  }

  const getTables = async (
    connectionId: string,
    catalog: string,
    schema: string
  ): Promise<NavigatorNodeResponse[]> => {
    return navigatorApi.getTables(connectionId, catalog, schema)
  }

  const getViews = async (
    connectionId: string,
    catalog: string,
    schema: string
  ): Promise<NavigatorNodeResponse[]> => {
    return navigatorApi.getViews(connectionId, catalog, schema)
  }

  const getColumns = async (
    connectionId: string,
    catalog: string,
    schema: string,
    table: string
  ): Promise<NavigatorNodeResponse[]> => {
    return navigatorApi.getColumns(connectionId, catalog, schema, table)
  }

  const refresh = async (connectionId: string): Promise<void> => {
    // eslint-disable-next-line no-console
    console.log('[Database] Refreshing connection:', connectionId)
  }

  // 注册数据库导航面板
  const panelDisposable = context.window.registerViewProvider('databaseNavigator', {
    component: DatabaseNavigator,
    title: '数据库导航',
    location: 'left',
    icon: 'database',
    order: 1,
  })

  const disposables: Disposable[] = [
    panelDisposable,
    context.commands.registerCommand('database.getCatalogs', (...args: unknown[]) =>
      getCatalogs(args[0] as string)
    ),
    context.commands.registerCommand('database.getSchemas', (...args: unknown[]) =>
      getSchemas(args[0] as string, args[1] as string)
    ),
    context.commands.registerCommand('database.getTables', (...args: unknown[]) =>
      getTables(args[0] as string, args[1] as string, args[2] as string)
    ),
    context.commands.registerCommand('database.getViews', (...args: unknown[]) =>
      getViews(args[0] as string, args[1] as string, args[2] as string)
    ),
    context.commands.registerCommand('database.getColumns', (...args: unknown[]) =>
      getColumns(args[0] as string, args[1] as string, args[2] as string, args[3] as string)
    ),
    context.commands.registerCommand('database.refresh', (...args: unknown[]) =>
      refresh(args[0] as string)
    ),
  ]

  return {
    version: '1.0.0',
    project: context.project,
    commands: context.commands,
    window: context.window,
    workspace: context.workspace,
    database: context.database,
    sqlEditor: context.sqlEditor,
    events: context.events,
    configuration: context.configuration,
    utils: context.utils,

    databaseBrowser: {
      getCatalogs,
      getSchemas,
      getTables,
      getViews,
      getColumns,
      refresh,
    },

    dispose: () => {
      disposables.forEach(d => d.dispose())
    },
  }
}

const deactivate = (): void => {
  // eslint-disable-next-line no-console
  console.log('[Database] Deactivated')
}

const extension: ExtensionModule = {
  activate: activate as (context: ExtensionContext) => ExtensionAPI,
  deactivate,
}

export default extension
