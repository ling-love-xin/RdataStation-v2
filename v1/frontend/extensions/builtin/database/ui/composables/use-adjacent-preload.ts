/**
 * 相邻节点预加载器
 *
 * 实现智能预加载策略：
 * - 展开表 A 时，预加载相邻的表 B、C
 * - 展开列文件夹时，预加载相邻表的列
 * - 减少用户等待时间，提升体验
 *
 * 遵循架构规范：前端只负责调度，不实现业务逻辑
 */

import { ref } from 'vue'

import { cacheStateManager } from './use-cache-state'
import { useDatabaseNavigatorStore } from '../stores/database-navigator-store'

/**
 * 预加载配置
 */
export interface PreloadConfig {
  /** 是否启用预加载 */
  enabled: boolean
  /** 预加载相邻节点数量 */
  adjacentCount: number
  /** 预加载延迟（毫秒） */
  delay: number
  /** 最大并发预加载数 */
  maxConcurrency: number
}

const defaultConfig: PreloadConfig = {
  enabled: true,
  adjacentCount: 2,
  delay: 100,
  maxConcurrency: 3,
}

/**
 * 预加载状态
 */
export interface PreloadState {
  isPreloading: boolean
  preloadedNodes: Set<string>
  totalPreloaded: number
}

/**
 * 相邻节点预加载 Composable
 */
export function useAdjacentPreload(config?: Partial<PreloadConfig>) {
  const navigatorStore = useDatabaseNavigatorStore()

  const state = ref<PreloadState>({
    isPreloading: false,
    preloadedNodes: new Set(),
    totalPreloaded: 0,
  })

  const cfg = ref<PreloadConfig>({ ...defaultConfig, ...config })

  /**
   * 获取相邻节点列表
   */
  function getAdjacentNodes(
    connectionId: string,
    dbName: string,
    schemaName: string | undefined,
    currentNodeType: string,
    currentNodeName: string
  ): Array<{ type: string; name: string }> {
    const adjacent: Array<{ type: string; name: string }> = []

    if (currentNodeType === 'table') {
      const tables = navigatorStore.getSchemaTables(connectionId, dbName, schemaName || '')
      const currentIndex = tables.findIndex(t => t.name === currentNodeName)

      if (currentIndex === -1) return adjacent

      const start = Math.max(0, currentIndex - cfg.value.adjacentCount)
      const end = Math.min(tables.length, currentIndex + cfg.value.adjacentCount + 1)

      for (let i = start; i < end; i++) {
        if (i !== currentIndex) {
          adjacent.push({
            type: 'table',
            name: tables[i].name,
          })
        }
      }
    } else if (currentNodeType === 'columns-folder') {
      const tables = navigatorStore.getSchemaTables(connectionId, dbName, schemaName || '')
      const currentIndex = tables.findIndex(t => t.name === currentNodeName)

      if (currentIndex === -1) return adjacent

      const start = Math.max(0, currentIndex - cfg.value.adjacentCount)
      const end = Math.min(tables.length, currentIndex + cfg.value.adjacentCount + 1)

      for (let i = start; i < end; i++) {
        if (i !== currentIndex) {
          adjacent.push({
            type: 'columns',
            name: tables[i].name,
          })
        }
      }
    }

    return adjacent
  }

  /**
   * 预加载单个表
   */
  async function preloadTable(
    connectionId: string,
    connectionType: 'global' | 'project',
    dbName: string,
    schemaName: string | undefined,
    tableName: string,
    _projectPath?: string
  ): Promise<boolean> {
    const cacheKey = `${connectionId}:${dbName}:${schemaName || ''}:${tableName}`

    if (state.value.preloadedNodes.has(cacheKey)) {
      return true
    }

    const cacheState = cacheStateManager.getState({
      connectionId,
      databaseName: dbName,
      schemaName,
      tableName,
    })

    if (
      cacheState?.isValid &&
      !cacheStateManager.isExpired({
        connectionId,
        databaseName: dbName,
        schemaName,
        tableName,
      })
    ) {
      state.value.preloadedNodes.add(cacheKey)
      return true
    }

    try {
      await navigatorStore.loadTables(connectionId, dbName, schemaName || '')

      cacheStateManager.markValid(
        { connectionId, databaseName: dbName, schemaName, tableName },
        0,
        0
      )

      state.value.preloadedNodes.add(cacheKey)
      state.value.totalPreloaded++

      return true
    } catch (error) {
      console.error(`预加载表 ${tableName} 失败:`, error)
      return false
    }
  }

  /**
   * 预加载单个表的列
   */
  async function preloadColumns(
    connectionId: string,
    connectionType: 'global' | 'project',
    dbName: string,
    schemaName: string | undefined,
    tableName: string,
    _projectPath?: string
  ): Promise<boolean> {
    const cacheKey = `${connectionId}:${dbName}:${schemaName || ''}:${tableName}:columns`

    if (state.value.preloadedNodes.has(cacheKey)) {
      return true
    }

    try {
      await navigatorStore.loadColumns(connectionId, dbName, schemaName || '', tableName)

      state.value.preloadedNodes.add(cacheKey)
      state.value.totalPreloaded++

      return true
    } catch (error) {
      console.error(`预加载表 ${tableName} 的列失败:`, error)
      return false
    }
  }

  /**
   * 执行相邻节点预加载
   */
  async function preloadAdjacentNodes(
    connectionId: string,
    connectionType: 'global' | 'project',
    dbName: string,
    schemaName: string | undefined,
    currentNodeType: string,
    currentNodeName: string,
    projectPath?: string
  ): Promise<void> {
    if (!cfg.value.enabled) return

    const adjacentNodes = getAdjacentNodes(
      connectionId,
      dbName,
      schemaName,
      currentNodeType,
      currentNodeName
    )

    if (adjacentNodes.length === 0) return

    state.value.isPreloading = true

    const preloadTasks: Promise<void>[] = []
    let loaded = 0

    for (const node of adjacentNodes) {
      if (loaded >= cfg.value.maxConcurrency) break

      const task = async () => {
        if (node.type === 'table') {
          await preloadTable(
            connectionId,
            connectionType,
            dbName,
            schemaName,
            node.name,
            projectPath
          )
        } else if (node.type === 'columns') {
          await preloadColumns(
            connectionId,
            connectionType,
            dbName,
            schemaName,
            node.name,
            projectPath
          )
        }
      }

      preloadTasks.push(task())
      loaded++
    }

    await Promise.allSettled(preloadTasks)

    state.value.isPreloading = false
  }

  /**
   * 清除预加载状态
   */
  function clearPreloadState(connectionId?: string): void {
    if (connectionId) {
      const keysToRemove: string[] = []
      state.value.preloadedNodes.forEach(key => {
        if (key.startsWith(`${connectionId}:`)) {
          keysToRemove.push(key)
        }
      })
      keysToRemove.forEach(key => state.value.preloadedNodes.delete(key))
    } else {
      state.value.preloadedNodes.clear()
      state.value.totalPreloaded = 0
    }
  }

  /**
   * 更新配置
   */
  function updateConfig(newConfig: Partial<PreloadConfig>): void {
    cfg.value = { ...cfg.value, ...newConfig }
  }

  return {
    state,
    config: cfg,
    preloadAdjacentNodes,
    clearPreloadState,
    updateConfig,
  }
}
