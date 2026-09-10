/**
 * 缓存预热取消机制
 *
 * 实现用户切换连接时自动取消当前预热：
 * - 监听连接切换事件
 * - 取消正在进行的预热任务
 * - 清理预热状态
 * - 防止资源浪费
 *
 * 遵循架构规范：前端只负责调度，不实现业务逻辑
 */

import { ref, watch } from 'vue'

import { useRuntimeConnectionStore } from '@/extensions/builtin/connection/ui/stores/runtime-connection-store'

import { cacheStateManager } from './use-cache-state'
import { useCacheWarming } from './use-cache-warming'
import { indexConstraintCacheManager } from './use-index-constraint-cache'
import { useDatabaseNavigatorStore } from '../stores/database-navigator-store'

/**
 * 预热取消配置
 */
export interface WarmingCancellationConfig {
  /** 是否启用自动取消 */
  enabled: boolean
  /** 取消后延迟清理时间（毫秒） */
  cleanupDelay: number
  /** 是否显示取消通知 */
  showNotification: boolean
}

const defaultConfig: WarmingCancellationConfig = {
  enabled: true,
  cleanupDelay: 1000,
  showNotification: true,
}

/**
 * 预热取消状态
 */
export interface WarmingCancellationState {
  /** 是否正在取消 */
  isCancelling: boolean
  /** 取消的连接 ID */
  cancelledConnectionId: string | null
  /** 取消原因 */
  reason: string | null
  /** 取消次数统计 */
  cancellationCount: number
}

/**
 * 缓存预热取消 Composable
 */
export function useWarmingCancellation(config?: Partial<WarmingCancellationConfig>) {
  const cfg = ref<WarmingCancellationConfig>({ ...defaultConfig, ...config })
  const runtimeConnectionStore = useRuntimeConnectionStore()
  const navigatorStore = useDatabaseNavigatorStore()
  const cacheWarming = useCacheWarming()

  const state = ref<WarmingCancellationState>({
    isCancelling: false,
    cancelledConnectionId: null,
    reason: null,
    cancellationCount: 0,
  })

  /**
   * 取消指定连接的预热
   */
  function cancelWarmingForConnection(connectionId: string, reason: string = '用户切换连接'): void {
    if (!cfg.value.enabled) return

    const warmingState = cacheWarming.state
    if (!warmingState.value.isWarming) return

    if (warmingState.value.currentConnectionId !== connectionId) return

    state.value.isCancelling = true
    state.value.cancelledConnectionId = connectionId
    state.value.reason = reason

    cacheWarming.cancelWarming()

    state.value.cancellationCount++
    state.value.isCancelling = false

    if (cfg.value.showNotification) {
      // eslint-disable-next-line no-console
      console.debug(`已取消连接 ${connectionId} 的缓存预热：${reason}`)
    }
  }

  /**
   * 取消所有预热
   */
  function cancelAllWarming(reason: string = '用户操作'): void {
    if (!cfg.value.enabled) return

    const warmingState = cacheWarming.state
    if (!warmingState.value.isWarming) return

    state.value.isCancelling = true
    state.value.reason = reason

    cacheWarming.cancelWarming()

    state.value.cancellationCount++
    state.value.isCancelling = false

    if (cfg.value.showNotification) {
      // eslint-disable-next-line no-console
      console.debug(`已取消所有缓存预热：${reason}`)
    }
  }

  /**
   * 清理预热相关状态
   */
  function cleanupWarmingState(connectionId: string): void {
    setTimeout(() => {
      cacheWarming.clearWarmingState(connectionId)
      cacheStateManager.clearConnection(connectionId)
      indexConstraintCacheManager.clearConnection(connectionId)
    }, cfg.value.cleanupDelay)
  }

  /**
   * 监听连接切换事件
   */
  function watchConnectionSwitch(): void {
    watch(
      () => runtimeConnectionStore.currentRuntimeConnId,
      (newConnectionId, oldConnectionId) => {
        if (oldConnectionId && oldConnectionId !== newConnectionId) {
          cancelWarmingForConnection(oldConnectionId, '用户切换连接')
          cleanupWarmingState(oldConnectionId)
        }
      }
    )
  }

  /**
   * 监听数据库导航器中的连接切换
   */
  function watchNavigatorConnectionSwitch(): void {
    watch(
      () => navigatorStore.selectedObject?.connectionId,
      (newConnectionId, oldConnectionId) => {
        if (oldConnectionId && oldConnectionId !== newConnectionId) {
          cancelWarmingForConnection(oldConnectionId, '用户切换数据库导航')
          cleanupWarmingState(oldConnectionId)
        }
      }
    )
  }

  /**
   * 启动监听
   */
  function startWatching(): void {
    watchConnectionSwitch()
    watchNavigatorConnectionSwitch()
  }

  /**
   * 更新配置
   */
  function updateConfig(newConfig: Partial<WarmingCancellationConfig>): void {
    cfg.value = { ...cfg.value, ...newConfig }
  }

  /**
   * 获取取消统计
   */
  function getCancellationStats(): {
    totalCancellations: number
    lastCancelledConnection: string | null
    lastReason: string | null
  } {
    return {
      totalCancellations: state.value.cancellationCount,
      lastCancelledConnection: state.value.cancelledConnectionId,
      lastReason: state.value.reason,
    }
  }

  return {
    state,
    config: cfg,
    cancelWarmingForConnection,
    cancelAllWarming,
    cleanupWarmingState,
    startWatching,
    updateConfig,
    getCancellationStats,
  }
}
