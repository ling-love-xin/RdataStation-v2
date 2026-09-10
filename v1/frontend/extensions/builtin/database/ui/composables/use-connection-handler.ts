/**
 * 数据库导航器连接处理逻辑
 *
 * 处理连接的建立、关闭、刷新等操作
 * 从 database-navigator.vue 中提取，实现业务逻辑与 UI 分离
 */

import type { GlobalConnectionInfo } from '@/extensions/builtin/connection/ui/services/connection'
import { useRuntimeConnectionStore } from '@/extensions/builtin/connection/ui/stores/runtime-connection-store'
import type { ProjectConnection } from '@/extensions/builtin/connection/ui/types/connection'

import { useDatabaseNavigatorStore } from '../stores/database-navigator-store'

import type { VirtualTreeNode } from '../types/virtual-tree'

export function useConnectionHandler() {
  const runtimeConnectionStore = useRuntimeConnectionStore()
  const navigatorStore = useDatabaseNavigatorStore()

  /**
   * 处理连接节点点击（建立连接）
   */
  async function handleConnectionClick(
    node: VirtualTreeNode,
    globalConnections: GlobalConnectionInfo[],
    projectConnections: ProjectConnection[],
    clearConnection: (connectionId: string) => void,
    initializeRootNodes: () => void
  ): Promise<ProjectConnection | null> {
    const connectionId = node.data?.connectionId
    const scopeRaw = node.data?.scope
    const scope: 'global' | 'project' =
      scopeRaw === 'global' || scopeRaw === 'project' ? scopeRaw : 'global'

    if (!connectionId) return null

    // 尝试建立运行时连接
    let conn: ProjectConnection | undefined

    if (scope === 'project') {
      conn = projectConnections.find(c => c.id === connectionId)
    } else {
      // 全局连接：从全局连接列表查找
      const globalConn = globalConnections.find(c => c.id === connectionId)
      if (globalConn) {
        conn = {
          id: globalConn.id,
          name: globalConn.name,
          driver: globalConn.driver,
          host: globalConn.host || undefined,
          port: globalConn.port || undefined,
          database: globalConn.database || undefined,
          username: globalConn.username || undefined,
          password: globalConn.password || undefined,
          connection_type: 'global',
          created_at: globalConn.created_at,
          updated_at: globalConn.updated_at,
        } as ProjectConnection
      }
    }

    if (conn) {
      await runtimeConnectionStore.establishRuntimeConnection(conn)
      initializeRootNodes()
      return conn
    }

    return null
  }

  /**
   * 处理断开连接
   */
  async function handleDisconnect(
    currentConnection: ProjectConnection | null,
    initializeRootNodes: () => void
  ): Promise<void> {
    if (currentConnection) {
      await navigatorStore.disconnectConnection(currentConnection.id)
      initializeRootNodes()
    }
  }

  /**
   * 处理刷新连接
   */
  async function handleRefresh(
    globalConnections: GlobalConnectionInfo[],
    projectConnections: ProjectConnection[],
    clearConnection: (connectionId: string) => void,
    initializeRootNodes: () => void,
    loadGlobalConnections: () => Promise<void>
  ): Promise<void> {
    await loadGlobalConnections()

    const allConnections = [...globalConnections, ...projectConnections]

    for (const conn of allConnections) {
      clearConnection(conn.id)
      await navigatorStore.loadCatalogs(conn.id)
    }

    initializeRootNodes()
  }

  /**
   * 处理表/视图双击打开
   */
  function handleOpenTableOrView(
    node: VirtualTreeNode,
    projectConnections: ProjectConnection[]
  ): {
    connection: ProjectConnection | null
    tableName: string
    connectionId?: string
    dbName?: string
    schemaName?: string
  } | null {
    const { connectionId, dbName, schemaName } = node.data
    const tableName = node.data.tableName || node.data.viewName

    if (typeof tableName !== 'string') return null

    const conn = projectConnections.find(c => c.id === connectionId)

    return {
      connection: conn || null,
      tableName: tableName as string,
      connectionId: connectionId as string | undefined,
      dbName: dbName as string | undefined,
      schemaName: schemaName as string | undefined,
    }
  }

  return {
    handleConnectionClick,
    handleDisconnect,
    handleRefresh,
    handleOpenTableOrView,
  }
}
