/**
 * 项目级连接状态管理
 *
 * 管理项目级别的数据库连接配置
 * 与后端 SQLite 存储同步
 */

import { defineStore } from 'pinia'
import { ref, computed } from 'vue'

import { useProjectStore } from '@/core/project/stores/project'

import * as projectConnectionService from '../services/project-connection'

import type {
  ProjectConnection,
  CreateProjectConnectionInput,
  ConnectionStatus,
} from '../../types/connection'

export const useProjectConnectionStore = defineStore('projectConnection', () => {
  // ==================== State ====================
  const connections = ref<ProjectConnection[]>([])
  const currentConnection = ref<ProjectConnection | null>(null)
  const loading = ref(false)
  const error = ref<string | null>(null)

  // ==================== Getters ====================
  const connectionCount = computed(() => connections.value.length)
  const hasConnections = computed(() => connections.value.length > 0)
  const isConnected = computed(() => currentConnection.value !== null)

  // 按类型分组的连接
  const connectionsByType = computed(() => {
    const grouped: Record<string, ProjectConnection[]> = {}
    connections.value.forEach(conn => {
      if (!grouped[conn.driver]) {
        grouped[conn.driver] = []
      }
      grouped[conn.driver].push(conn)
    })
    return grouped
  })

  // ==================== Actions ====================

  /**
   * 加载项目所有连接
   */
  async function loadConnections(): Promise<void> {
    const projectStore = useProjectStore()
    if (!projectStore.currentProject?.path) {
      error.value = '没有打开的项目'
      return
    }

    loading.value = true
    error.value = null

    try {
      const result = await projectConnectionService.getProjectConnections(
        projectStore.currentProject.path
      )
      // 为每个连接添加 connection_type 和 project_path
      connections.value = result.map(conn => ({
        ...conn,
        connection_type: 'project' as const,
        project_path: projectStore.currentProject?.path,
      }))
    } catch (e) {
      error.value = e instanceof Error ? e.message : '加载连接失败'
      console.error('加载项目连接失败:', e)
    } finally {
      loading.value = false
    }
  }

  /**
   * 创建新连接
   */
  async function createConnection(
    input: Omit<CreateProjectConnectionInput, 'project_path'>
  ): Promise<ProjectConnection | null> {
    const projectStore = useProjectStore()
    if (!projectStore.currentProject?.path) {
      error.value = '没有打开的项目'
      return null
    }

    loading.value = true
    error.value = null

    try {
      const result = await projectConnectionService.createProjectConnection({
        ...input,
        project_path: projectStore.currentProject.path,
      })
      connections.value.push(result)
      return result
    } catch (e) {
      error.value = e instanceof Error ? e.message : '创建连接失败'
      console.error('创建项目连接失败:', e instanceof Error ? e.message : String(e))
      throw e
    } finally {
      loading.value = false
    }
  }

  /**
   * 更新连接
   */
  async function updateConnection(connection: ProjectConnection): Promise<void> {
    const projectStore = useProjectStore()
    if (!projectStore.currentProject?.path) {
      error.value = '没有打开的项目'
      return
    }

    loading.value = true
    error.value = null

    try {
      await projectConnectionService.updateProjectConnection(
        connection,
        projectStore.currentProject.path
      )

      // 更新本地状态
      const index = connections.value.findIndex(c => c.id === connection.id)
      if (index !== -1) {
        connections.value[index] = connection
      }

      // 如果当前连接被更新，也更新当前连接
      if (currentConnection.value?.id === connection.id) {
        currentConnection.value = connection
      }
    } catch (e) {
      error.value = e instanceof Error ? e.message : '更新连接失败'
      console.error('更新项目连接失败:', e)
      throw e
    } finally {
      loading.value = false
    }
  }

  /**
   * 删除连接
   */
  async function deleteConnection(connectionId: string): Promise<void> {
    const projectStore = useProjectStore()
    if (!projectStore.currentProject?.path) {
      error.value = '没有打开的项目'
      return
    }

    loading.value = true
    error.value = null

    try {
      await projectConnectionService.deleteProjectConnection(
        projectStore.currentProject.path,
        connectionId
      )

      // 更新本地状态
      connections.value = connections.value.filter(c => c.id !== connectionId)

      // 如果删除的是当前连接，清空当前连接
      if (currentConnection.value?.id === connectionId) {
        currentConnection.value = null
      }
    } catch (e) {
      error.value = e instanceof Error ? e.message : '删除连接失败'
      console.error('删除项目连接失败:', e)
      throw e
    } finally {
      loading.value = false
    }
  }

  /**
   * 更新连接状态
   */
  async function updateConnectionStatus(
    connectionId: string,
    status: ConnectionStatus,
    errorMessage?: string
  ): Promise<void> {
    const projectStore = useProjectStore()
    if (!projectStore.currentProject?.path) {
      error.value = '没有打开的项目'
      return
    }

    loading.value = true
    error.value = null

    try {
      await projectConnectionService.updateProjectConnectionStatus(
        projectStore.currentProject.path,
        connectionId,
        status,
        errorMessage
      )

      // 更新本地状态
      const index = connections.value.findIndex(c => c.id === connectionId)
      if (index !== -1) {
        connections.value[index] = {
          ...connections.value[index],
          status,
          error_message: errorMessage,
          last_connected_at:
            status === 'connected'
              ? new Date().toISOString()
              : connections.value[index].last_connected_at,
        }
      }

      // 如果当前连接被更新，也更新当前连接
      if (currentConnection.value?.id === connectionId) {
        currentConnection.value = {
          ...currentConnection.value,
          status,
          error_message: errorMessage,
          last_connected_at:
            status === 'connected'
              ? new Date().toISOString()
              : currentConnection.value.last_connected_at,
        }
      }
    } catch (e) {
      error.value = e instanceof Error ? e.message : '更新连接状态失败'
      console.error('更新连接状态失败:', e)
      throw e
    } finally {
      loading.value = false
    }
  }

  /**
   * 设置当前连接
   */
  function setCurrentConnection(connection: ProjectConnection | null): void {
    currentConnection.value = connection
  }

  /**
   * 搜索连接
   */
  async function searchConnections(query: string): Promise<ProjectConnection[]> {
    const projectStore = useProjectStore()
    if (!projectStore.currentProject?.path) {
      return []
    }

    try {
      return await projectConnectionService.searchProjectConnections(
        projectStore.currentProject.path,
        query
      )
    } catch (e) {
      console.error('搜索项目连接失败:', e)
      return []
    }
  }

  /**
   * 获取连接 URL
   */
  function getConnectionUrl(connection: ProjectConnection): string {
    return projectConnectionService.buildConnectionUrl(connection)
  }

  /**
   * 获取连接显示名称
   */
  function getConnectionDisplayName(connection: ProjectConnection): string {
    return projectConnectionService.getConnectionDisplayName(connection)
  }

  /**
   * 清除错误状态
   */
  function clearError(): void {
    error.value = null
  }

  /**
   * 重置状态
   */
  function reset(): void {
    connections.value = []
    currentConnection.value = null
    loading.value = false
    error.value = null
  }

  return {
    // State
    connections,
    currentConnection,
    loading,
    error,
    // Getters
    connectionCount,
    hasConnections,
    isConnected,
    connectionsByType,
    // Actions
    loadConnections,
    createConnection,
    updateConnection,
    deleteConnection,
    updateConnectionStatus,
    setCurrentConnection,
    searchConnections,
    getConnectionUrl,
    getConnectionDisplayName,
    clearError,
    reset,
  }
})
