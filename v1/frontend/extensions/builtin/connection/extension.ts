/**
 * Connection Extension
 *
 * 提供数据库连接管理能力
 */

import { connectionApi } from '@/shared/api'
import type {
  ConnectDatabaseInput,
  ConnectDatabaseResponse,
  ConnectionInfoResponse,
} from '@/shared/api'

import type { ExtensionContext, ExtensionAPI, ExtensionModule, Disposable } from '../../core/types'

interface ConnectionExtensionAPI extends ExtensionAPI {
  connection: {
    getConnections(): Promise<Connection[]>
    createConnection(input: ConnectDatabaseInput): Promise<Connection>
    testConnection(dbType: string, url: string): Promise<boolean>
    disconnectConnection(connectionId: string): Promise<void>
  }
}

interface Connection {
  id: string
  name: string
  dbType: string
  url: string
  status: string
  isActive: boolean
  meta: {
    supportsTransaction: boolean
    supportsStreaming: boolean
    supportsArrow: boolean
    supportsFederated: boolean
    supportsConcurrentWrite: boolean
    isInMemory: boolean
  }
}

const activate = (context: ExtensionContext): ConnectionExtensionAPI => {
  console.warn('[Connection] Activating for project:', context.project.name)

  const getConnections = async (): Promise<Connection[]> => {
    const connections: ConnectionInfoResponse[] = await connectionApi.getConnections()
    return connections.map(c => ({
      id: c.id,
      name: c.name,
      dbType: c.db_type,
      url: c.url,
      status: c.status,
      isActive: c.is_active,
      meta: {
        supportsTransaction: false,
        supportsStreaming: false,
        supportsArrow: false,
        supportsFederated: false,
        supportsConcurrentWrite: false,
        isInMemory: false,
      },
    }))
  }

  const createConnection = async (input: ConnectDatabaseInput): Promise<Connection> => {
    const response: ConnectDatabaseResponse = await connectionApi.connectDatabase(input)
    return {
      id: response.conn_id,
      name: response.name,
      dbType: response.db_type,
      url: response.url,
      status: response.status,
      isActive: true,
      meta: {
        supportsTransaction: response.meta.supports_transaction,
        supportsStreaming: response.meta.supports_streaming,
        supportsArrow: response.meta.supports_arrow,
        supportsFederated: response.meta.supports_federated,
        supportsConcurrentWrite: response.meta.supports_concurrent_write,
        isInMemory: response.meta.is_in_memory,
      },
    }
  }

  const testConnection = async (dbType: string, url: string): Promise<boolean> => {
    const result = await connectionApi.testConnection(dbType, url)
    return result.success
  }

  const disconnectConnection = async (connectionId: string): Promise<void> => {
    await connectionApi.disconnectDatabase(connectionId)
  }

  const disposables: Disposable[] = [
    context.commands.registerCommand('connection.getConnections', (..._args: unknown[]) =>
      getConnections()
    ),
    context.commands.registerCommand('connection.createConnection', (..._args: unknown[]) =>
      createConnection(_args[0] as ConnectDatabaseInput)
    ),
    context.commands.registerCommand('connection.testConnection', (..._args: unknown[]) =>
      testConnection(_args[0] as string, _args[1] as string)
    ),
    context.commands.registerCommand('connection.disconnect', (..._args: unknown[]) =>
      disconnectConnection(_args[0] as string)
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

    connection: {
      getConnections,
      createConnection,
      testConnection,
      disconnectConnection,
    },

    dispose: () => {
      disposables.forEach(d => d.dispose())
    },
  }
}

const deactivate = (): void => {
  console.warn('[Connection] Deactivated')
}

const extension: ExtensionModule = {
  activate: activate as (context: ExtensionContext) => ExtensionAPI,
  deactivate,
}

export default extension
