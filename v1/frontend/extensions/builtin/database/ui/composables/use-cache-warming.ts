/**
 * 智能缓存预热策略
 *
 * 实现智能缓存预热，根据用户行为动态调整预热深度
 * 支持并发控制、取消机制和进度追踪
 * 遵循架构规范：前端不实现业务逻辑，只负责调度后端 API
 */

import { ref } from 'vue'

import { cacheStateManager } from './use-cache-state'
import {
  refreshMetadataCache,
  getTablesFromCache,
  getColumnsFromCache,
} from '../services/metadata-cache-service'

/**
 * 预热深度
 */
export type WarmingDepth = 'databases' | 'schemas' | 'tables' | 'columns'

/**
 * 缓存预热配置
 */
export interface CacheWarmingConfig {
  /** 是否启用自动预热 */
  enabled: boolean
  /** 预热深度 */
  depth: WarmingDepth
  /** 预热延迟（毫秒） */
  delay: number
  /** 最大预热数据库数量 */
  maxDatabases: number
  /** 最大预热 Schema 数量 */
  maxSchemas: number
  /** 最大预热表数量 */
  maxTables: number
  /** 是否启用智能预热 */
  smartWarming: boolean
  /** 并发预热数量 */
  concurrency: number
}

/**
 * 用户行为追踪
 */
interface UserBehavior {
  /** 展开的数据库列表 */
  expandedDatabases: Set<string>
  /** 展开的 Schema 列表 */
  expandedSchemas: Set<string>
  /** 点击的表列表 */
  clickedTables: Set<string>
  /** 展开次数 */
  expandCount: number
  /** 最后活跃时间 */
  lastActive: number
}

const defaultConfig: CacheWarmingConfig = {
  enabled: true,
  depth: 'schemas',
  delay: 100,
  maxDatabases: 5,
  maxSchemas: 10,
  maxTables: 50,
  smartWarming: true,
  concurrency: 2,
}

/**
 * 缓存预热状态
 */
export interface CacheWarmingState {
  isWarming: boolean
  warmedConnections: Set<string>
  progress: number
  currentDepth: WarmingDepth
  warmedDatabases: number
  warmedSchemas: number
  warmedTables: number
  /** 是否已取消 */
  isCancelled: boolean
  /** 当前正在预热的连接 ID */
  currentConnectionId: string | null
}

/**
 * 并发任务控制器
 */
interface ConcurrencyController {
  /** 当前运行任务数 */
  running: number
  /** 最大并发数 */
  maxConcurrency: number
  /** 是否已取消 */
  cancelled: boolean
  /** AbortController 用于取消 */
  abortController: AbortController
}

/**
 * 智能缓存预热 Composable
 */
export function useCacheWarming() {
  const state = ref<CacheWarmingState>({
    isWarming: false,
    warmedConnections: new Set(),
    progress: 0,
    currentDepth: 'databases',
    warmedDatabases: 0,
    warmedSchemas: 0,
    warmedTables: 0,
    isCancelled: false,
    currentConnectionId: null,
  })

  const config = ref<CacheWarmingConfig>({ ...defaultConfig })

  const userBehaviors = ref<Map<string, UserBehavior>>(new Map())

  /**
   * 记录用户行为
   */
  function recordBehavior(
    connectionId: string,
    action: 'expand_db' | 'expand_schema' | 'click_table',
    target: string
  ): void {
    let behavior = userBehaviors.value.get(connectionId)
    if (!behavior) {
      behavior = {
        expandedDatabases: new Set(),
        expandedSchemas: new Set(),
        clickedTables: new Set(),
        expandCount: 0,
        lastActive: Date.now(),
      }
      userBehaviors.value.set(connectionId, behavior)
    }

    behavior.lastActive = Date.now()
    behavior.expandCount++

    switch (action) {
      case 'expand_db':
        behavior.expandedDatabases.add(target)
        break
      case 'expand_schema':
        behavior.expandedSchemas.add(target)
        break
      case 'click_table':
        behavior.clickedTables.add(target)
        break
    }
  }

  /**
   * 根据用户行为计算预热深度
   */
  function calculateSmartDepth(connectionId: string): WarmingDepth {
    const behavior = userBehaviors.value.get(connectionId)
    if (!behavior || !config.value.smartWarming) {
      return config.value.depth
    }

    const now = Date.now()
    const timeSinceLastActive = now - behavior.lastActive

    if (behavior.expandCount < 3) {
      return 'databases'
    }

    if (behavior.expandedDatabases.size > 2) {
      return 'schemas'
    }

    if (behavior.expandedSchemas.size > 1) {
      return 'tables'
    }

    if (behavior.clickedTables.size > 3) {
      return 'columns'
    }

    if (timeSinceLastActive > 5 * 60 * 1000) {
      return 'schemas'
    }

    return 'tables'
  }

  /**
   * 创建并发控制器
   */
  function createConcurrencyController(): ConcurrencyController {
    return {
      running: 0,
      maxConcurrency: config.value.concurrency,
      cancelled: false,
      abortController: new AbortController(),
    }
  }

  /**
   * 并发执行任务
   */
  async function runWithConcurrency<T>(
    controller: ConcurrencyController,
    task: () => Promise<T>
  ): Promise<T | null> {
    if (controller.cancelled || controller.abortController.signal.aborted) {
      return null
    }

    while (controller.running >= controller.maxConcurrency) {
      if (controller.cancelled || controller.abortController.signal.aborted) {
        return null
      }
      await new Promise(resolve => setTimeout(resolve, 50))
    }

    controller.running++
    try {
      const result = await task()
      return result
    } finally {
      controller.running--
    }
  }

  /**
   * 取消当前预热
   */
  function cancelWarming(): void {
    state.value.isCancelled = true
    state.value.isWarming = false
    state.value.currentConnectionId = null
  }

  /**
   * 预热单个数据库（支持并发和取消）
   */
  async function warmDatabaseConcurrent(
    connectionId: string,
    connectionType: 'global' | 'project',
    dbName: string,
    projectPath: string | undefined,
    controller: ConcurrencyController,
    smartDepth: WarmingDepth
  ): Promise<boolean> {
    if (controller.cancelled || controller.abortController.signal.aborted) {
      return false
    }

    return runWithConcurrency(controller, async () => {
      try {
        const cacheState = cacheStateManager.getState({
          connectionId,
          databaseName: dbName,
        })

        if (
          !cacheState?.isValid ||
          cacheStateManager.isExpired({
            connectionId,
            databaseName: dbName,
          })
        ) {
          await refreshMetadataCache({
            connection_id: connectionId,
            connection_type: connectionType,
            database_name: dbName,
            schema_name: undefined,
            project_path: projectPath,
          }).catch(() => {})
        }

        if (smartDepth === 'schemas' || smartDepth === 'tables' || smartDepth === 'columns') {
          const tables = await getTablesFromCache(
            connectionId,
            connectionType,
            dbName,
            undefined,
            projectPath
          ).catch(() => [])

          if ((smartDepth === 'tables' || smartDepth === 'columns') && tables.length > 0) {
            const tablesToWarm = tables.slice(0, config.value.maxTables)

            for (const table of tablesToWarm) {
              if (controller.cancelled || controller.abortController.signal.aborted) {
                return false
              }

              if (smartDepth === 'columns') {
                await getColumnsFromCache(
                  connectionId,
                  connectionType,
                  dbName,
                  table.schema_name || 'public',
                  table.name,
                  projectPath
                ).catch(() => {})
              }

              if (config.value.delay > 0) {
                await new Promise(resolve => setTimeout(resolve, config.value.delay))
              }
            }
          }
        }

        cacheStateManager.markValid({ connectionId, databaseName: dbName }, 0, 0)

        return true
      } catch (error) {
        console.error(`预热数据库 ${dbName} 失败:`, error)
        return false
      }
    }).then(result => result ?? false)
  }

  /**
   * 预热单个连接的缓存（并发版本）
   */
  async function warmConnection(
    connectionId: string,
    connectionType: 'global' | 'project',
    databases: string[],
    projectPath?: string
  ): Promise<void> {
    if (!config.value.enabled) return

    const controller = createConcurrencyController()
    const smartDepth = calculateSmartDepth(connectionId)

    state.value.isWarming = true
    state.value.progress = 0
    state.value.warmedDatabases = 0
    state.value.warmedSchemas = 0
    state.value.warmedTables = 0
    state.value.currentDepth = smartDepth
    state.value.isCancelled = false
    state.value.currentConnectionId = connectionId

    const dbsToWarm = databases.slice(0, config.value.maxDatabases)
    let warmed = 0

    const warmingPromises = dbsToWarm.map(async dbName => {
      if (controller.cancelled || controller.abortController.signal.aborted) {
        return
      }

      const success = await warmDatabaseConcurrent(
        connectionId,
        connectionType,
        dbName,
        projectPath,
        controller,
        smartDepth
      )

      if (success) {
        warmed++
        state.value.warmedDatabases = warmed
        state.value.progress = (warmed / dbsToWarm.length) * 100
      }
    })

    await Promise.all(warmingPromises)

    if (!controller.cancelled && !controller.abortController.signal.aborted) {
      state.value.warmedConnections.add(connectionId)
    }

    state.value.isWarming = false
    state.value.currentConnectionId = null
  }

  /**
   * 预热单个数据库
   */
  async function warmDatabase(
    connectionId: string,
    connectionType: 'global' | 'project',
    dbName: string,
    projectPath?: string
  ): Promise<void> {
    if (!config.value.enabled) return

    const smartDepth = calculateSmartDepth(connectionId)

    try {
      const cacheState = cacheStateManager.getState({
        connectionId,
        databaseName: dbName,
      })

      if (
        !cacheState?.isValid ||
        cacheStateManager.isExpired({
          connectionId,
          databaseName: dbName,
        })
      ) {
        await refreshMetadataCache({
          connection_id: connectionId,
          connection_type: connectionType,
          database_name: dbName,
          schema_name: undefined,
          project_path: projectPath,
        }).catch(() => {})
      }

      if (smartDepth === 'schemas' || smartDepth === 'tables' || smartDepth === 'columns') {
        const tables = await getTablesFromCache(
          connectionId,
          connectionType,
          dbName,
          undefined,
          projectPath
        ).catch(() => [])

        cacheStateManager.markValid({ connectionId, databaseName: dbName }, tables.length, 0)
      }
    } catch (error) {
      console.error(`预热数据库 ${dbName} 失败:`, error)
    }
  }

  /**
   * 预热单个 Schema
   */
  async function warmSchema(
    connectionId: string,
    connectionType: 'global' | 'project',
    dbName: string,
    schemaName: string,
    projectPath?: string
  ): Promise<void> {
    if (!config.value.enabled) return

    const smartDepth = calculateSmartDepth(connectionId)

    try {
      const tables = await getTablesFromCache(
        connectionId,
        connectionType,
        dbName,
        schemaName,
        projectPath
      ).catch(() => [])

      cacheStateManager.markValid(
        { connectionId, databaseName: dbName, schemaName },
        tables.length,
        0
      )

      if ((smartDepth === 'tables' || smartDepth === 'columns') && tables.length > 0) {
        const tablesToWarm = tables.slice(0, config.value.maxTables)

        for (const table of tablesToWarm) {
          if (smartDepth === 'columns') {
            await getColumnsFromCache(
              connectionId,
              connectionType,
              dbName,
              schemaName,
              table.name,
              projectPath
            ).catch(() => {})
          }
        }
      }
    } catch (error) {
      console.error(`预热 Schema ${schemaName} 失败:`, error)
    }
  }

  /**
   * 清除预热状态
   */
  function clearWarmingState(connectionId?: string): void {
    if (connectionId) {
      state.value.warmedConnections.delete(connectionId)
      userBehaviors.value.delete(connectionId)
    } else {
      state.value.warmedConnections.clear()
      userBehaviors.value.clear()
      cancelWarming()
    }
  }

  /**
   * 更新配置
   */
  function updateConfig(newConfig: Partial<CacheWarmingConfig>): void {
    config.value = { ...config.value, ...newConfig }
  }

  return {
    state,
    config,
    warmConnection,
    warmDatabase,
    warmSchema,
    recordBehavior,
    cancelWarming,
    clearWarmingState,
    updateConfig,
  }
}
