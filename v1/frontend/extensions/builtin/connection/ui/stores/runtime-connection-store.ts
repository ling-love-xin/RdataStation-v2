/**
 * 运行时连接状态管理
 *
 * 管理实际的数据库连接（运行时），与项目级连接配置分离
 * 项目级连接配置存储在 SQLite 中，运行时连接是实际的数据库连接
 */

import { defineStore } from 'pinia'
import { ref, computed } from 'vue'

import type { Connection } from '@/shared/types'

import * as connectionService from '../services/connection'

import type { ProjectConnection } from '../../types/connection'

export const useRuntimeConnectionStore = defineStore('runtimeConnection', () => {
  // ==================== State ====================

  // 运行时连接映射: projectConnectionId -> runtimeConnId
  const runtimeConnectionIds = ref<Map<string, string>>(new Map())

  // 当前激活的运行时连接 ID
  const currentRuntimeConnId = ref<string | null>(null)

  // DuckDB 本地加速偏好: connectionId -> enabled
  const duckdbEnabled = ref<Map<string, boolean>>(new Map())

  // 加载状态
  const loading = ref(false)
  const error = ref<string | null>(null)

  // ==================== Getters ====================

  const getRuntimeConnId = computed(() => {
    return (projectConnId: string): string | undefined => {
      return runtimeConnectionIds.value.get(projectConnId)
    }
  })

  const hasRuntimeConnection = computed(() => {
    return (projectConnId: string): boolean => {
      return runtimeConnectionIds.value.has(projectConnId)
    }
  })

  // ==================== Actions ====================

  /**
   * 为项目连接建立运行时连接
   */
  async function establishRuntimeConnection(
    projectConn: ProjectConnection
  ): Promise<string | null> {
    // 检查是否已有运行时连接
    const existingConnId = runtimeConnectionIds.value.get(projectConn.id)
    if (existingConnId) {
      currentRuntimeConnId.value = existingConnId
      return existingConnId
    }

    loading.value = true
    error.value = null

    try {
      // 构建连接 URL
      const url = buildConnectionUrl(projectConn)
      // 兼容 driver 和 db_type 字段
      const dbType = projectConn.driver
      if (!dbType) {
        throw new Error('数据库类型未定义')
      }
      // Debug: runtime connection establishment

      // 确定连接类型和项目 ID
      const connectionType = projectConn.connection_type || 'global'
      const projectId = connectionType === 'project' ? projectConn.project_path : undefined

      // 建立运行时连接
      const result = await connectionService.connectDatabase(
        dbType,
        url,
        projectConn.name,
        connectionType,
        projectId
      )

      // Debug: runtime connection established
      // 保存映射关系 - 创建新 Map 触发响应式更新
      const newMap = new Map(runtimeConnectionIds.value)
      newMap.set(projectConn.id, result.conn_id)
      runtimeConnectionIds.value = newMap
      currentRuntimeConnId.value = result.conn_id

      return result.conn_id
    } catch (e) {
      error.value = e instanceof Error ? e.message : '建立连接失败'
      return null
    } finally {
      loading.value = false
    }
  }

  /**
   * 关闭运行时连接
   */
  async function closeRuntimeConnection(projectConnId: string): Promise<void> {
    const runtimeConnId = runtimeConnectionIds.value.get(projectConnId)
    if (!runtimeConnId) return

    try {
      await connectionService.closeConnection(runtimeConnId)
      // 创建新 Map 触发响应式更新
      const newMap = new Map(runtimeConnectionIds.value)
      newMap.delete(projectConnId)
      runtimeConnectionIds.value = newMap

      if (currentRuntimeConnId.value === runtimeConnId) {
        currentRuntimeConnId.value = null
      }
    } catch {
      /* ignore close error */
    }
  }

  /**
   * 切换到指定连接
   */
  async function switchToConnection(projectConn: ProjectConnection): Promise<string | null> {
    const connId = await establishRuntimeConnection(projectConn)
    return connId
  }

  /**
   * 获取当前运行时连接 ID
   */
  function getCurrentRuntimeConnId(): string | null {
    return currentRuntimeConnId.value
  }

  /**
   * 清除所有运行时连接
   */
  async function clearAllRuntimeConnections(): Promise<void> {
    for (const [, runtimeConnId] of runtimeConnectionIds.value) {
      try {
        await connectionService.closeConnection(runtimeConnId)
      } catch {
        /* ignore close error */
      }
    }
    // 创建新 Map 触发响应式更新
    runtimeConnectionIds.value = new Map()
    currentRuntimeConnId.value = null
  }

  /**
   * 从 Connection（connectionStore 类型）建立运行时连接
   * 供 SQL 编辑器自动建连使用，不需要 ProjectConnection 对象
   */
  async function establishFromConnection(conn: Connection): Promise<string | null> {
    const existingConnId = runtimeConnectionIds.value.get(conn.connId)
    if (existingConnId) {
      currentRuntimeConnId.value = existingConnId
      return existingConnId
    }

    loading.value = true
    error.value = null

    try {
      const result = await connectionService.connectDatabase(
        conn.dbType,
        conn.url,
        conn.name || conn.connId,
        conn.connectionType as 'global' | 'project',
        conn.projectId || undefined
      )

      if (result && result.conn_id) {
        const newMap = new Map(runtimeConnectionIds.value)
        newMap.set(conn.connId, result.conn_id)
        runtimeConnectionIds.value = newMap
        currentRuntimeConnId.value = result.conn_id
        return result.conn_id
      }
      return null
    } catch (e) {
      error.value = e instanceof Error ? e.message : '建立连接失败'
      return null
    } finally {
      loading.value = false
    }
  }

  // ==================== DuckDB 本地加速 ====================

  /**
   * 获取指定连接的 DuckDB 加速开关状态
   */
  function isDuckDbEnabled(connectionId: string): boolean {
    if (duckdbEnabled.value.size === 0) {
      loadDuckDbPrefs()
    }
    return duckdbEnabled.value.get(connectionId) || false
  }

  /**
   * 切换指定连接的 DuckDB 加速
   */
  function toggleDuckDbEnabled(connectionId: string): boolean {
    if (duckdbEnabled.value.size === 0) {
      loadDuckDbPrefs()
    }
    const current = duckdbEnabled.value.get(connectionId) || false
    const newVal = !current
    const newMap = new Map(duckdbEnabled.value)
    newMap.set(connectionId, newVal)
    duckdbEnabled.value = newMap
    // 持久化到 localStorage
    try {
      const obj: Record<string, boolean> = {}
      for (const [k, v] of newMap) {
        obj[k] = v
      }
      localStorage.setItem('duckdb-enabled-connections', JSON.stringify(obj))
    } catch {
      /* ignore */
    }
    return newVal
  }

  /**
   * 从 localStorage 加载 DuckDB 偏好
   */
  function loadDuckDbPrefs() {
    try {
      const raw = localStorage.getItem('duckdb-enabled-connections')
      if (raw) {
        const obj = JSON.parse(raw)
        const newMap = new Map<string, boolean>()
        for (const [k, v] of Object.entries(obj)) {
          newMap.set(k, v as boolean)
        }
        duckdbEnabled.value = newMap
      }
    } catch {
      /* ignore */
    }
  }

  /**
   * 构建连接 URL
   */
  function buildConnectionUrl(projectConn: ProjectConnection): string {
    // 兼容 driver 和 db_type 字段
    const dbType = projectConn.driver
    if (!dbType) {
      throw new Error('数据库类型未定义')
    }

    const { host, port, database, username, password } = projectConn

    // 根据数据库类型构建 URL
    switch (dbType.toLowerCase()) {
      case 'mysql':
        if (username && password) {
          return `mysql://${username}:${password}@${host}:${port}/${database}`
        }
        return `mysql://${host}:${port}/${database}`

      case 'postgresql':
      case 'postgres':
        if (username && password) {
          return `postgresql://${username}:${password}@${host}:${port}/${database}`
        }
        return `postgresql://${host}:${port}/${database}`

      case 'sqlite': {
        // SQLite 使用 host 字段存储文件路径（如果 host 为空，使用 database）
        const sqlitePath = host || database || ''
        return `sqlite://${sqlitePath}`
      }

      case 'duckdb': {
        // DuckDB 使用 host 字段存储文件路径（如果 host 为空，使用 database）
        const duckdbPath = host || database || ''
        return `duckdb://${duckdbPath}`
      }

      default:
        throw new Error(`不支持的数据库类型: ${dbType}`)
    }
  }

  return {
    // State
    runtimeConnectionIds,
    currentRuntimeConnId,
    duckdbEnabled,
    loading,
    error,
    // Getters
    getRuntimeConnId,
    hasRuntimeConnection,
    isDuckDbEnabled,
    // Actions
    establishRuntimeConnection,
    establishFromConnection,
    closeRuntimeConnection,
    switchToConnection,
    getCurrentRuntimeConnId,
    clearAllRuntimeConnections,
    toggleDuckDbEnabled,
    loadDuckDbPrefs,
  }
})
