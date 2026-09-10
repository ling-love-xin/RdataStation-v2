import { invoke } from '@tauri-apps/api/core'
import { defineStore } from 'pinia'
import { ref, computed } from 'vue'

import type { DatabaseType, Connection, RecentConnection, ConnectionType } from '@/shared/types'
import type { SqlDialect } from '@/shared/types/sql'

import { useRuntimeConnectionStore } from './runtime-connection-store'
import * as connectionService from '../services/connection'

/**
 * 连接状态管理
 *
 * 管理所有数据库连接的状态，包括：
 * - 连接列表
 * - 当前活动连接
 * - 最近连接记录
 * - 加载和错误状态
 */
export const useConnectionStore = defineStore('connection', () => {
  // ==================== State ====================
  const connections = ref<Connection[]>([])
  const currentConnection = ref<Connection | null>(null)
  const recentConnections = ref<RecentConnection[]>([])
  const loading = ref(false)
  const error = ref<string | null>(null)

  // ==================== Getters ====================
  const connectionCount = computed(() => connections.value.length)
  const hasConnections = computed(() => connections.value.length > 0)
  const isConnected = computed(() => currentConnection.value !== null)

  // ==================== Actions ====================

  /**
   * 加载所有连接
   *
   * 从已保存的全局连接配置和项目连接中加载，
   * 并合并运行时连接状态
   */
  async function loadConnections() {
    loading.value = true
    error.value = null
    try {
      // 获取项目 store
      const projectStore = await import('@/core/project/stores/project').then(m =>
        m.useProjectStore()
      )

      // 使用 runtimeConnectionStore 判断运行时连接状态
      const runtimeIds = useRuntimeConnectionStore().runtimeConnectionIds
      const hasRuntime = (connId: string) => runtimeIds.has(connId)

      if (projectStore.currentProject?.path) {
        // 加载项目级连接
        const projectConnections = await connectionService.getProjectConnections(
          projectStore.currentProject.path
        )

        // getProjectConnections 返回 unknown[]（该命令有 State 参数未纳入 specta）
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        connections.value = (projectConnections as any[]).map((r: any) => ({
          connId: r.id,
          name: r.name,
          dbType: r.db_type || r.driver,
          url: `${r.db_type || r.driver}://${r.host}:${r.port}/${r.database}`,
          connectionType: 'project' as const,
          projectId: projectStore.currentProject?.id || null,
          status: hasRuntime(r.id) ? 'connected' : 'disconnected',
          isActive: hasRuntime(r.id),
          meta: {
            supportsTransaction: true,
            supportsStreaming: false,
            supportsArrow: false,
            supportsFederated: false,
            supportsConcurrentWrite: false,
            isInMemory: false,
          },
        }))
      }

      // 加载全局连接配置（已保存的连接，无论是否连接）
      const globalResult = await connectionService.getGlobalConnections().catch(() => [])
      const globalConnections = globalResult.map(r => ({
        connId: r.id,
        name: r.name,
        dbType: r.driver,
        url: `${r.driver}://${r.host || 'localhost'}:${r.port || 0}/${r.database || ''}`,
        connectionType: 'global' as const,
        projectId: null,
        status: hasRuntime(r.id) ? 'connected' : 'disconnected',
        isActive: hasRuntime(r.id),
        meta: {
          supportsTransaction: true,
          supportsStreaming: false,
          supportsArrow: false,
          supportsFederated: false,
          supportsConcurrentWrite: false,
          isInMemory: false,
        },
      }))

      // 合并：项目连接优先，然后是全局连接
      const existingIds = new Set(connections.value.map(c => c.connId))
      for (const gc of globalConnections) {
        if (!existingIds.has(gc.connId)) {
          connections.value.push(gc as Connection)
        }
      }
    } catch (e) {
      error.value = e instanceof Error ? e.message : '加载连接失败'
    } finally {
      loading.value = false
    }
  }

  /**
   * 同步运行时连接状态（由数据库导航栏调用，在连接建立/关闭后更新状态）
   */
  function syncConnectionStatus(connId: string, connected: boolean) {
    const conn = connections.value.find(c => c.connId === connId)
    if (conn) {
      conn.status = connected ? 'connected' : 'disconnected'
      conn.isActive = connected
      if (connected) {
        currentConnection.value = conn
      } else if (currentConnection.value?.connId === connId) {
        currentConnection.value = null
      }
    }
  }

  /**
   * 创建新连接
   */
  async function connect(dbType: string, url: string, name?: string) {
    loading.value = true
    error.value = null
    try {
      const result = await connectionService.connectDatabase(dbType, url, name)
      const newConn: Connection = {
        connId: result.conn_id,
        name: result.name,
        dbType: result.db_type as SqlDialect,
        url: result.url,
        connectionType: (result.connection_type || 'global') as ConnectionType,
        projectId: result.project_id || null,
        status: result.status,
        isActive: true,
        meta: {
          supportsTransaction: result.meta?.supports_transaction ?? false,
          supportsStreaming: result.meta?.supports_streaming ?? false,
          supportsArrow: result.meta?.supports_arrow ?? false,
          supportsFederated: result.meta?.supports_federated ?? false,
          supportsConcurrentWrite: result.meta?.supports_concurrent_write ?? false,
          isInMemory: result.meta?.is_in_memory ?? false,
        },
      }
      connections.value.push(newConn)
      currentConnection.value = newConn
      return newConn
    } catch (e) {
      error.value = e instanceof Error ? e.message : '连接失败'
      throw e
    } finally {
      loading.value = false
    }
  }

  /**
   * 切换活动连接
   */
  async function switchConnection(connId: string) {
    loading.value = true
    error.value = null
    try {
      await connectionService.switchConnection(connId)
      const conn = connections.value.find(c => c.connId === connId)
      if (conn) {
        currentConnection.value = conn
      }
    } catch (e) {
      error.value = e instanceof Error ? e.message : '切换连接失败'
      throw e
    } finally {
      loading.value = false
    }
  }

  /**
   * 断开连接
   */
  async function disconnect(connId?: string) {
    loading.value = true
    error.value = null
    try {
      if (connId) {
        await connectionService.closeConnection(connId)
        connections.value = connections.value.filter(c => c.connId !== connId)
        if (currentConnection.value?.connId === connId) {
          currentConnection.value = connections.value[0] || null
        }
      } else {
        await connectionService.closeAllConnections()
        connections.value = []
        currentConnection.value = null
      }
    } catch (e) {
      error.value = e instanceof Error ? e.message : '断开连接失败'
      throw e
    } finally {
      loading.value = false
    }
  }

  /**
   * 删除连接（别名，用于语义化）
   */
  async function deleteConnection(connId: string) {
    return disconnect(connId)
  }

  /**
   * 测试连接
   */
  async function testConnection(connId: string): Promise<boolean> {
    const conn = connections.value.find(c => c.connId === connId)
    if (!conn) {
      throw new Error('连接不存在')
    }

    loading.value = true
    error.value = null
    try {
      const result = await connectionService.testConnection(conn.dbType, conn.url)
      return result.success
    } catch (e) {
      error.value = e instanceof Error ? e.message : '测试连接失败'
      throw e
    } finally {
      loading.value = false
    }
  }

  /**
   * 更新连接
   */
  async function updateConnection(connId: string, dbType: string, url: string, name: string) {
    loading.value = true
    error.value = null
    try {
      // 断开旧连接
      await connectionService.closeConnection(connId)

      // 创建新连接
      const result = await connectionService.connectDatabase(dbType, url, name)

      // 更新连接列表
      const newConn: Connection = {
        connId: result.conn_id,
        name: result.name,
        dbType: result.db_type as SqlDialect,
        url: result.url,
        connectionType: (result.connection_type || 'global') as ConnectionType,
        projectId: result.project_id || null,
        status: result.status,
        isActive: true,
        meta: {
          supportsTransaction: result.meta?.supports_transaction ?? false,
          supportsStreaming: result.meta?.supports_streaming ?? false,
          supportsArrow: result.meta?.supports_arrow ?? false,
          supportsFederated: result.meta?.supports_federated ?? false,
          supportsConcurrentWrite: result.meta?.supports_concurrent_write ?? false,
          isInMemory: result.meta?.is_in_memory ?? false,
        },
      }

      // 替换旧连接
      const index = connections.value.findIndex(c => c.connId === connId)
      if (index !== -1) {
        connections.value[index] = newConn
      } else {
        connections.value.push(newConn)
      }

      // 如果是当前连接，更新当前连接
      if (currentConnection.value?.connId === connId) {
        currentConnection.value = newConn
      }

      return newConn
    } catch (e) {
      error.value = e instanceof Error ? e.message : '更新连接失败'
      throw e
    } finally {
      loading.value = false
    }
  }

  /**
   * 加载最近连接列表
   */
  async function loadRecentConnections() {
    try {
      const result = await connectionService.getRecentConnections()
      recentConnections.value = result.map(r => ({
        id: r.id,
        name: r.name,
        dbType: r.db_type as DatabaseType,
        url: r.url,
        connectionType: (r.connection_type || 'global') as ConnectionType,
        connectedAt: r.connected_at,
      }))
    } catch (e) {
      console.error('加载最近连接失败:', e)
    }
  }

  /**
   * 删除最近连接记录
   */
  async function removeRecentConnection(name: string) {
    try {
      await connectionService.removeRecentConnection(name)
      recentConnections.value = recentConnections.value.filter(r => r.name !== name)
    } catch (e) {
      console.error('删除最近连接失败:', e)
    }
  }

  /**
   * 清除错误状态
   */
  function clearError() {
    error.value = null
  }

  /**
   * 重置状态
   */
  function reset() {
    connections.value = []
    currentConnection.value = null
    recentConnections.value = []
    loading.value = false
    error.value = null
  }

  // ==================== 事务管理 ====================

  /**
   * 开始事务
   */
  async function beginTransaction(connId?: string) {
    const targetConnId = connId || currentConnection.value?.connId
    if (!targetConnId) throw new Error('没有活动的连接')

    loading.value = true
    try {
      const result = await invoke<{ success: boolean; message?: string }>('begin_transaction', {
        connId: targetConnId,
      })
      return result
    } finally {
      loading.value = false
    }
  }

  /**
   * 提交事务
   */
  async function commitTransaction(connId?: string) {
    const targetConnId = connId || currentConnection.value?.connId
    if (!targetConnId) throw new Error('没有活动的连接')

    loading.value = true
    try {
      const result = await invoke<{ success: boolean; message?: string }>('commit_transaction', {
        connId: targetConnId,
      })
      return result
    } finally {
      loading.value = false
    }
  }

  /**
   * 回滚事务
   */
  async function rollbackTransaction(connId?: string) {
    const targetConnId = connId || currentConnection.value?.connId
    if (!targetConnId) throw new Error('没有活动的连接')

    loading.value = true
    try {
      const result = await invoke<{ success: boolean; message?: string }>('rollback_transaction', {
        connId: targetConnId,
      })
      return result
    } finally {
      loading.value = false
    }
  }

  /**
   * 获取事务状态
   */
  async function getTransactionStatus(connId?: string) {
    const targetConnId = connId || currentConnection.value?.connId
    if (!targetConnId) throw new Error('没有活动的连接')

    const result = await invoke<{ success: boolean; message?: string }>('get_transaction_status', {
      connId: targetConnId,
    })
    return result
  }

  return {
    // State
    connections,
    currentConnection,
    recentConnections,
    loading,
    error,
    // Getters
    connectionCount,
    hasConnections,
    isConnected,
    // Actions
    loadConnections,
    syncConnectionStatus,
    connect,
    switchConnection,
    disconnect,
    deleteConnection,
    testConnection,
    updateConnection,
    loadRecentConnections,
    removeRecentConnection,
    clearError,
    reset,
    // 事务管理
    beginTransaction,
    commitTransaction,
    rollbackTransaction,
    getTransactionStatus,
  }
})
