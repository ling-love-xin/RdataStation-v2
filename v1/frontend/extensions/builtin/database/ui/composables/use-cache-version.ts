/**
 * 缓存版本控制
 *
 * 支持缓存版本升级和迁移
 * 当缓存结构发生变化时，自动迁移或清除旧缓存
 */

import { ref } from 'vue'

/**
 * 缓存版本信息
 */
export interface CacheVersionInfo {
  /** 当前版本号 */
  currentVersion: number
  /** 最后升级时间 */
  lastUpgrade: number | null
  /** 升级历史 */
  upgradeHistory: VersionUpgradeRecord[]
}

/**
 * 版本升级记录
 */
export interface VersionUpgradeRecord {
  /** 从版本 */
  fromVersion: number
  /** 到版本 */
  toVersion: number
  /** 升级时间 */
  timestamp: number
  /** 升级原因 */
  reason: string
}

/**
 * 版本迁移策略
 */
export interface MigrationStrategy {
  /** 目标版本 */
  targetVersion: number
  /** 迁移函数 */
  migrate: (connectionId: string) => Promise<void>
  /** 是否可回滚 */
  canRollback: boolean
  /** 回滚函数 */
  rollback?: (connectionId: string) => Promise<void>
}

/**
 * 当前缓存版本
 *
 * 每次缓存结构变化时递增此版本号
 */
export const CURRENT_CACHE_VERSION = 1

/**
 * 缓存版本管理器
 */
class CacheVersionManager {
  private versionInfo = ref<Map<string, CacheVersionInfo>>(new Map())
  private migrationStrategies = ref<Map<number, MigrationStrategy>>(new Map())

  /**
   * 获取连接的缓存版本信息
   */
  getVersionInfo(connectionId: string): CacheVersionInfo {
    return (
      this.versionInfo.value.get(connectionId) || {
        currentVersion: 0,
        lastUpgrade: null,
        upgradeHistory: [],
      }
    )
  }

  /**
   * 设置连接的缓存版本
   */
  setVersion(connectionId: string, version: number): void {
    const info = this.getVersionInfo(connectionId)
    const oldVersion = info.currentVersion

    if (oldVersion !== version) {
      const record: VersionUpgradeRecord = {
        fromVersion: oldVersion,
        toVersion: version,
        timestamp: Date.now(),
        reason: '手动设置版本',
      }

      this.versionInfo.value.set(connectionId, {
        currentVersion: version,
        lastUpgrade: Date.now(),
        upgradeHistory: [...info.upgradeHistory, record],
      })
    }
  }

  /**
   * 注册迁移策略
   */
  registerMigration(strategy: MigrationStrategy): void {
    this.migrationStrategies.value.set(strategy.targetVersion, strategy)
  }

  /**
   * 检查是否需要升级
   */
  needsUpgrade(connectionId: string): boolean {
    const info = this.getVersionInfo(connectionId)
    return info.currentVersion < CURRENT_CACHE_VERSION
  }

  /**
   * 执行升级
   */
  async upgrade(connectionId: string): Promise<boolean> {
    const info = this.getVersionInfo(connectionId)
    const currentVersion = info.currentVersion

    if (currentVersion >= CURRENT_CACHE_VERSION) {
      return false
    }

    for (let version = currentVersion + 1; version <= CURRENT_CACHE_VERSION; version++) {
      const strategy = this.migrationStrategies.value.get(version)

      if (strategy) {
        try {
          await strategy.migrate(connectionId)

          const record: VersionUpgradeRecord = {
            fromVersion: version - 1,
            toVersion: version,
            timestamp: Date.now(),
            reason: `自动升级到版本 ${version}`,
          }

          this.versionInfo.value.set(connectionId, {
            currentVersion: version,
            lastUpgrade: Date.now(),
            upgradeHistory: [...info.upgradeHistory, record],
          })
        } catch (error) {
          console.error(`升级到版本 ${version} 失败:`, error)

          if (strategy.canRollback && strategy.rollback) {
            try {
              await strategy.rollback(connectionId)
              // eslint-disable-next-line no-console
              console.debug(`已回滚到版本 ${version - 1}`)
            } catch (rollbackError) {
              console.error(`回滚失败:`, rollbackError)
            }
          }

          return false
        }
      } else {
        console.warn(`未找到版本 ${version} 的迁移策略，清除缓存`)
        this.setVersion(connectionId, version)
      }
    }

    return true
  }

  /**
   * 清除指定连接的版本信息
   */
  clearVersion(connectionId: string): void {
    this.versionInfo.value.delete(connectionId)
  }

  /**
   * 清除所有版本信息
   */
  clearAll(): void {
    this.versionInfo.value.clear()
  }

  /**
   * 获取所有连接的版本统计
   */
  getVersionStats(): {
    total: number
    byVersion: Map<number, number>
    needsUpgrade: number
  } {
    const byVersion = new Map<number, number>()
    let needsUpgradeCount = 0
    let total = 0

    this.versionInfo.value.forEach((info, connectionId) => {
      total++
      const count = byVersion.get(info.currentVersion) || 0
      byVersion.set(info.currentVersion, count + 1)

      if (this.needsUpgrade(connectionId)) {
        needsUpgradeCount++
      }
    })

    return {
      total,
      byVersion,
      needsUpgrade: needsUpgradeCount,
    }
  }
}

/**
 * 单例实例
 */
export const cacheVersionManager = new CacheVersionManager()

/**
 * Composable 函数
 */
export function useCacheVersion() {
  const manager = cacheVersionManager

  function getVersionInfo(connectionId: string) {
    return manager.getVersionInfo(connectionId)
  }

  function setVersion(connectionId: string, version: number) {
    manager.setVersion(connectionId, version)
  }

  function registerMigration(strategy: MigrationStrategy) {
    manager.registerMigration(strategy)
  }

  function needsUpgrade(connectionId: string) {
    return manager.needsUpgrade(connectionId)
  }

  async function upgrade(connectionId: string) {
    return manager.upgrade(connectionId)
  }

  function clearVersion(connectionId: string) {
    manager.clearVersion(connectionId)
  }

  function clearAll() {
    manager.clearAll()
  }

  function getVersionStats() {
    return manager.getVersionStats()
  }

  return {
    getVersionInfo,
    setVersion,
    registerMigration,
    needsUpgrade,
    upgrade,
    clearVersion,
    clearAll,
    getVersionStats,
    CURRENT_CACHE_VERSION,
  }
}
