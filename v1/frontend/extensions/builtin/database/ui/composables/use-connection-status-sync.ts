/**
 * 连接状态实时同步
 *
 * 实时监测连接健康状态
 * 自动重连机制
 * 状态变更通知
 * 集成缓存预热
 */

import { ref, computed } from 'vue'

import { useProjectConnectionStore } from '@/extensions/builtin/connection/ui/stores/project-connection-store'
import { useRuntimeConnectionStore } from '@/extensions/builtin/connection/ui/stores/runtime-connection-store'

import { cacheStateManager } from './use-cache-state'
import { useCacheWarming } from './use-cache-warming'
import { useDatabaseNavigatorStore } from '../stores/database-navigator-store'

export type ConnectionHealthStatus = 'healthy' | 'degraded' | 'unhealthy' | 'unknown'

export interface IConnectionHealthInfo {
  /** 连接 ID */
  connectionId: string
  /** 健康状态 */
  status: ConnectionHealthStatus
  /** 最后检查时间 */
  lastChecked: number
  /** 延迟（毫秒） */
  latency?: number
  /** 错误信息 */
  error?: string
  /** 连续失败次数 */
  consecutiveFailures: number
  /** 是否正在检查 */
  isChecking: boolean
  /** 是否已预热缓存 */
  isCacheWarmed?: boolean
}

export interface IConnectionStatusSyncOptions {
  /** 健康检查间隔（毫秒） */
  healthCheckInterval?: number
  /** 最大连续失败次数 */
  maxConsecutiveFailures?: number
  /** 是否启用自动重连 */
  enableAutoReconnect?: boolean
  /** 自动重连延迟（毫秒） */
  reconnectDelay?: number
  /** 最大重连次数 */
  maxReconnectAttempts?: number
  /** 是否启用缓存预热 */
  enableCacheWarming?: boolean
}

const DEFAULT_OPTIONS: IConnectionStatusSyncOptions = {
  healthCheckInterval: 30000,
  maxConsecutiveFailures: 3,
  enableAutoReconnect: true,
  reconnectDelay: 5000,
  maxReconnectAttempts: 5,
  enableCacheWarming: true,
}

export function useConnectionStatusSync(options?: IConnectionStatusSyncOptions) {
  const opts = { ...DEFAULT_OPTIONS, ...options }
  const runtimeConnectionStore = useRuntimeConnectionStore()
  const projectConnectionStore = useProjectConnectionStore()
  const navigatorStore = useDatabaseNavigatorStore()
  const { warmConnection, recordBehavior } = useCacheWarming()

  const healthInfoMap = ref<Map<string, IConnectionHealthInfo>>(new Map())
  const checkTimers = ref<Map<string, number>>(new Map())
  const reconnectTimers = ref<Map<string, number>>(new Map())

  /**
   * 获取连接健康状态
   */
  function getHealthInfo(connectionId: string): IConnectionHealthInfo | undefined {
    return healthInfoMap.value.get(connectionId)
  }

  /**
   * 获取所有连接健康状态
   */
  const allHealthInfo = computed(() => {
    return Array.from(healthInfoMap.value.values())
  })

  /**
   * 获取健康连接列表
   */
  const healthyConnections = computed(() => {
    return Array.from(healthInfoMap.value.values())
      .filter(info => info.status === 'healthy')
      .map(info => info.connectionId)
  })

  /**
   * 获取不健康连接列表
   */
  const unhealthyConnections = computed(() => {
    return Array.from(healthInfoMap.value.values())
      .filter(info => info.status === 'unhealthy' || info.status === 'degraded')
      .map(info => info.connectionId)
  })

  /**
   * 触发缓存预热
   */
  async function triggerCacheWarming(connectionId: string): Promise<void> {
    if (!opts.enableCacheWarming) return

    const healthInfo = healthInfoMap.value.get(connectionId)
    if (!healthInfo || healthInfo.isCacheWarmed) return

    try {
      const connType = navigatorStore.getConnectionType(connectionId) || 'global'
      const projectPath = navigatorStore.getProjectPath(connectionId)
      const catalogs = navigatorStore.getCatalogs(connectionId)

      if (catalogs.length > 0) {
        await warmConnection(
          connectionId,
          connType as 'global' | 'project',
          catalogs.map((d: { name: string }) => d.name),
          projectPath
        )

        healthInfo.isCacheWarmed = true
        healthInfoMap.value.set(connectionId, healthInfo)
      }
    } catch (error) {
      console.error('缓存预热失败:', connectionId, error)
    }
  }

  /**
   * 执行健康检查
   */
  async function checkConnectionHealth(connectionId: string): Promise<IConnectionHealthInfo> {
    const startTime = Date.now()
    const existingInfo = healthInfoMap.value.get(connectionId)
    const consecutiveFailures = existingInfo?.consecutiveFailures || 0

    const newInfo: IConnectionHealthInfo = {
      connectionId,
      status: 'unknown',
      lastChecked: Date.now(),
      consecutiveFailures,
      isChecking: true,
      isCacheWarmed: existingInfo?.isCacheWarmed || false,
    }

    healthInfoMap.value.set(connectionId, newInfo)

    try {
      const dbType = navigatorStore.getDbType(connectionId)
      // Oracle 需要 FROM DUAL，其余数据库 SELECT 1 均有效
      const pingSql = dbType === 'oracle' ? 'SELECT 1 FROM DUAL' : 'SELECT 1'

      await navigatorStore.executeSql(connectionId, 'master', pingSql)
      const latency = Date.now() - startTime

      newInfo.status = 'healthy'
      newInfo.latency = latency
      newInfo.consecutiveFailures = 0
      newInfo.isChecking = false

      healthInfoMap.value.set(connectionId, newInfo)

      if (latency < 100) {
        recordBehavior(connectionId, 'expand_db', 'auto')
      }

      if (!newInfo.isCacheWarmed) {
        triggerCacheWarming(connectionId)
      }

      return newInfo
    } catch (error) {
      // defensive: TypeScript 窄化，newInfo 在 try 块之前已初始化
      if (!newInfo) {
        return (
          existingInfo ?? {
            connectionId,
            status: 'unhealthy',
            lastChecked: Date.now(),
            consecutiveFailures: consecutiveFailures + 1,
            isChecking: false,
            isCacheWarmed: false,
          }
        )
      }
      const newFailures = consecutiveFailures + 1
      const maxFailures = (opts.maxConsecutiveFailures ??
        DEFAULT_OPTIONS.maxConsecutiveFailures) as number

      newInfo.status = newFailures >= maxFailures ? 'unhealthy' : 'degraded'
      newInfo.consecutiveFailures = newFailures
      newInfo.error = error instanceof Error ? error.message : '检查失败'
      newInfo.isChecking = false

      healthInfoMap.value.set(connectionId, newInfo)

      if (opts.enableAutoReconnect && newFailures >= maxFailures) {
        scheduleReconnect(connectionId)
      }

      return newInfo
    }
  }

  /**
   * 开始定期健康检查
   */
  function startHealthCheck(connectionId: string): void {
    // 清除已存在的定时器
    stopHealthCheck(connectionId)

    // 立即执行一次检查
    checkConnectionHealth(connectionId)

    // 设置定时器
    const timer = window.setInterval(() => {
      checkConnectionHealth(connectionId)
    }, opts.healthCheckInterval)

    checkTimers.value.set(connectionId, timer)
  }

  /**
   * 停止健康检查
   */
  function stopHealthCheck(connectionId: string): void {
    const timer = checkTimers.value.get(connectionId)
    if (timer) {
      clearInterval(timer)
      checkTimers.value.delete(connectionId)
    }

    // 清除重连定时器
    const reconnectTimer = reconnectTimers.value.get(connectionId)
    if (reconnectTimer) {
      clearTimeout(reconnectTimer)
      reconnectTimers.value.delete(connectionId)
    }
  }

  /**
   * 调度自动重连
   */
  function scheduleReconnect(connectionId: string): void {
    // 清除已存在的重连定时器
    const existingTimer = reconnectTimers.value.get(connectionId)
    if (existingTimer) {
      clearTimeout(existingTimer)
    }

    const timer = window.setTimeout(async () => {
      const healthInfo = healthInfoMap.value.get(connectionId)
      if (!healthInfo || healthInfo.status !== 'unhealthy') {
        return
      }

      try {
        const projectConn = projectConnectionStore.connections.find(c => c.id === connectionId)
        if (projectConn) {
          await runtimeConnectionStore.closeRuntimeConnection(connectionId)
          await runtimeConnectionStore.establishRuntimeConnection(projectConn)

          await checkConnectionHealth(connectionId)
        }
      } catch (error) {
        console.error('重连失败:', connectionId, error)

        const healthInfo = healthInfoMap.value.get(connectionId)
        if (healthInfo) {
          healthInfo.consecutiveFailures += 1
          healthInfoMap.value.set(connectionId, healthInfo)

          if (
            healthInfo.consecutiveFailures <
            (opts.maxReconnectAttempts ?? (DEFAULT_OPTIONS.maxReconnectAttempts as number))
          ) {
            scheduleReconnect(connectionId)
          }
        }
      }
    }, opts.reconnectDelay)

    reconnectTimers.value.set(connectionId, timer)
  }

  /**
   * 手动触发重连
   */
  async function triggerReconnect(connectionId: string): Promise<boolean> {
    try {
      const projectConn = projectConnectionStore.connections.find(c => c.id === connectionId)
      if (!projectConn) {
        console.error('未找到连接配置:', connectionId)
        return false
      }

      await runtimeConnectionStore.closeRuntimeConnection(connectionId)
      await runtimeConnectionStore.establishRuntimeConnection(projectConn)

      const healthInfo = healthInfoMap.value.get(connectionId)
      if (healthInfo) {
        healthInfo.consecutiveFailures = 0
        healthInfo.status = 'healthy'
        healthInfo.isCacheWarmed = false
        healthInfoMap.value.set(connectionId, healthInfo)
      }

      cacheStateManager.clearConnection(connectionId)

      return true
    } catch (error) {
      console.error('手动重连失败:', connectionId, error)
      return false
    }
  }

  /**
   * 清理所有定时器
   */
  function cleanup(): void {
    for (const [connectionId] of checkTimers.value) {
      stopHealthCheck(connectionId)
    }
    checkTimers.value.clear()
    reconnectTimers.value.clear()
  }

  return {
    healthInfoMap,
    allHealthInfo,
    healthyConnections,
    unhealthyConnections,
    getHealthInfo,
    checkConnectionHealth,
    startHealthCheck,
    stopHealthCheck,
    triggerReconnect,
    cleanup,
  }
}
