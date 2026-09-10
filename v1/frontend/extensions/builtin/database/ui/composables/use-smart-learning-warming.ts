/**
 * 基于用户行为学习的智能缓存预热策略
 *
 * 实现高级预热策略：
 * - 学习用户访问模式（时间、频率、深度）
 * - 预测用户下一步操作
 * - 根据学习结果动态调整预热策略
 *
 * 遵循架构规范：前端只负责调度，不实现业务逻辑
 */

import { cacheStateManager } from './use-cache-state'
import { useCacheWarming, type WarmingDepth } from './use-cache-warming'
import { useDatabaseNavigatorStore } from '../stores/database-navigator-store'

/**
 * 用户访问模式
 */
export interface AccessPattern {
  /** 访问的数据库 */
  database: string
  /** 访问的 Schema */
  schema: string
  /** 访问的表 */
  table: string
  /** 访问时间戳 */
  timestamp: number
  /** 访问类型 */
  accessType: 'expand' | 'query' | 'edit' | 'browse'
  /** 停留时间（毫秒） */
  dwellTime: number
}

/**
 * 用户行为画像
 */
export interface UserProfile {
  /** 用户 ID（连接 ID） */
  connectionId: string
  /** 访问历史 */
  accessHistory: AccessPattern[]
  /** 常用数据库 */
  frequentDatabases: Map<string, number>
  /** 常用表 */
  frequentTables: Map<string, number>
  /** 活跃时间段 */
  activeTimeSlots: Map<number, number>
  /** 平均访问深度 */
  avgDepth: number
  /** 最后更新时间 */
  lastUpdated: number
}

/**
 * 学习配置
 */
export interface LearningConfig {
  /** 是否启用学习 */
  enabled: boolean
  /** 历史保留天数 */
  historyDays: number
  /** 最小学习样本数 */
  minSamples: number
  /** 学习权重（新数据权重） */
  learningRate: number
  /** 预测置信度阈值 */
  confidenceThreshold: number
  /** 最大历史记录数 */
  maxHistorySize: number
}

const defaultConfig: LearningConfig = {
  enabled: true,
  historyDays: 30,
  minSamples: 5,
  learningRate: 0.1,
  confidenceThreshold: 0.7,
  maxHistorySize: 1000,
}

/**
 * 预测结果
 */
export interface PredictionResult {
  /** 预测的数据库 */
  predictedDatabase?: string
  /** 预测的 Schema */
  predictedSchema?: string
  /** 预测的表 */
  predictedTables: string[]
  /** 预测置信度 */
  confidence: number
  /** 推荐的预热深度 */
  recommendedDepth: WarmingDepth
}

/**
 * 智能学习预热管理器
 */
class SmartLearningManager {
  /** 用户画像存储 */
  private profiles = new Map<string, UserProfile>()

  /** 配置 */
  private config: LearningConfig

  constructor(config?: Partial<LearningConfig>) {
    this.config = { ...defaultConfig, ...config }
  }

  /**
   * 记录用户访问
   */
  recordAccess(
    connectionId: string,
    database: string,
    schema: string,
    table: string,
    accessType: AccessPattern['accessType'],
    dwellTime: number
  ): void {
    if (!this.config.enabled) return

    let profile = this.profiles.get(connectionId)
    if (!profile) {
      profile = {
        connectionId,
        accessHistory: [],
        frequentDatabases: new Map(),
        frequentTables: new Map(),
        activeTimeSlots: new Map(),
        avgDepth: 0,
        lastUpdated: Date.now(),
      }
      this.profiles.set(connectionId, profile)
    }

    const pattern: AccessPattern = {
      database,
      schema,
      table,
      timestamp: Date.now(),
      accessType,
      dwellTime,
    }

    profile.accessHistory.push(pattern)

    if (profile.accessHistory.length > this.config.maxHistorySize) {
      profile.accessHistory = profile.accessHistory.slice(-this.config.maxHistorySize)
    }

    this.updateProfile(profile, pattern)
  }

  /**
   * 更新用户画像
   */
  private updateProfile(profile: UserProfile, pattern: AccessPattern): void {
    const dbKey = `${pattern.database}`
    const tableKey = `${pattern.database}.${pattern.schema}.${pattern.table}`

    profile.frequentDatabases.set(dbKey, (profile.frequentDatabases.get(dbKey) || 0) + 1)

    profile.frequentTables.set(tableKey, (profile.frequentTables.get(tableKey) || 0) + 1)

    const hour = new Date(pattern.timestamp).getHours()
    profile.activeTimeSlots.set(hour, (profile.activeTimeSlots.get(hour) || 0) + 1)

    const depth = pattern.table ? 3 : pattern.schema ? 2 : 1
    profile.avgDepth = (profile.avgDepth + depth) / 2

    profile.lastUpdated = Date.now()
  }

  /**
   * 预测用户下一步操作
   */
  predict(
    connectionId: string,
    currentContext: {
      database?: string
      schema?: string
      table?: string
    }
  ): PredictionResult {
    const profile = this.profiles.get(connectionId)
    if (!profile || profile.accessHistory.length < this.config.minSamples) {
      return {
        predictedTables: [],
        confidence: 0,
        recommendedDepth: 'tables',
      }
    }

    const recentHistory = profile.accessHistory.slice(-50)
    const predictions = this.analyzePatterns(recentHistory, currentContext)

    return predictions
  }

  /**
   * 分析访问模式
   */
  private analyzePatterns(
    history: AccessPattern[],
    currentContext: { database?: string; schema?: string; table?: string }
  ): PredictionResult {
    const tableFrequency = new Map<string, number>()
    const sequencePatterns: Map<string, number> = new Map()

    for (let i = 0; i < history.length - 1; i++) {
      const current = history[i]
      const next = history[i + 1]

      const currentKey = `${current.database}.${current.schema}.${current.table}`
      const nextKey = `${next.database}.${next.schema}.${next.table}`

      if (currentContext.table && currentKey.includes(currentContext.table)) {
        sequencePatterns.set(nextKey, (sequencePatterns.get(nextKey) || 0) + 1)
      }

      tableFrequency.set(
        `${current.database}.${current.schema}.${current.table}`,
        (tableFrequency.get(`${current.database}.${current.schema}.${current.table}`) || 0) + 1
      )
    }

    const sortedTables = Array.from(tableFrequency.entries())
      .sort((a, b) => b[1] - a[1])
      .slice(0, 5)
      .map(([key]) => key.split('.').pop() || '')

    const confidence = sortedTables.length > 0 ? Math.min(0.9, (sortedTables.length / 5) * 0.8) : 0

    const recommendedDepth = this.calculateRecommendedDepth(sortedTables.length, confidence)

    return {
      predictedTables: sortedTables,
      confidence,
      recommendedDepth,
    }
  }

  /**
   * 计算推荐的预热深度
   */
  private calculateRecommendedDepth(tableCount: number, confidence: number): WarmingDepth {
    if (confidence < 0.3) return 'databases'
    if (confidence < 0.5) return 'schemas'
    if (tableCount < 3) return 'tables'
    return 'columns'
  }

  /**
   * 获取用户画像
   */
  getProfile(connectionId: string): UserProfile | undefined {
    return this.profiles.get(connectionId)
  }

  /**
   * 清除用户画像
   */
  clearProfile(connectionId: string): void {
    this.profiles.delete(connectionId)
  }

  /**
   * 清除所有画像
   */
  clearAllProfiles(): void {
    this.profiles.clear()
  }

  /**
   * 更新配置
   */
  updateConfig(newConfig: Partial<LearningConfig>): void {
    this.config = { ...this.config, ...newConfig }
  }
}

export const smartLearningManager = new SmartLearningManager()

/**
 * 智能学习预热 Composable
 */
export function useSmartLearningWarming(config?: Partial<LearningConfig>) {
  const _navigatorStore = useDatabaseNavigatorStore()
  const cacheWarming = useCacheWarming()

  const learningManager = new SmartLearningManager(config)

  /**
   * 记录用户访问并触发学习
   */
  function recordAndLearn(
    connectionId: string,
    database: string,
    schema: string,
    table: string,
    accessType: AccessPattern['accessType'],
    dwellTime: number
  ): void {
    learningManager.recordAccess(connectionId, database, schema, table, accessType, dwellTime)

    cacheWarming.recordBehavior(
      connectionId,
      accessType === 'expand' ? 'expand_db' : 'click_table',
      table || schema || database
    )
  }

  /**
   * 基于学习结果预热
   */
  async function warmBasedOnLearning(
    connectionId: string,
    connectionType: 'global' | 'project',
    currentContext: {
      database?: string
      schema?: string
      table?: string
    },
    projectPath?: string
  ): Promise<void> {
    const prediction = learningManager.predict(connectionId, currentContext)

    if (prediction.confidence < learningManager['config'].confidenceThreshold) {
      return
    }

    for (const tableKey of prediction.predictedTables) {
      const [dbName, schemaName, tableName] = tableKey.split('.')

      try {
        const cacheState = cacheStateManager.getState({
          connectionId,
          databaseName: dbName,
          schemaName,
          tableName,
        })

        if (
          !cacheState?.isValid ||
          cacheStateManager.isExpired({
            connectionId,
            databaseName: dbName,
            schemaName,
            tableName,
          })
        ) {
          const { getTablesFromCache, getColumnsFromCache } =
            await import('../services/metadata-cache-service')

          await getTablesFromCache(connectionId, connectionType, dbName, schemaName, projectPath)

          if (prediction.recommendedDepth === 'columns') {
            await getColumnsFromCache(
              connectionId,
              connectionType,
              dbName,
              schemaName,
              tableName,
              projectPath
            )
          }

          cacheStateManager.markValid(
            { connectionId, databaseName: dbName, schemaName, tableName },
            1,
            0
          )
        }
      } catch (error) {
        console.error(`基于学习预热 ${tableKey} 失败:`, error)
      }
    }
  }

  /**
   * 获取学习统计
   */
  function getLearningStats(connectionId: string): {
    totalAccess: number
    frequentDatabases: Array<{ name: string; count: number }>
    frequentTables: Array<{ name: string; count: number }>
    activeHours: Array<{ hour: number; count: number }>
    avgDepth: number
  } | null {
    const profile = learningManager.getProfile(connectionId)
    if (!profile) return null

    return {
      totalAccess: profile.accessHistory.length,
      frequentDatabases: Array.from(profile.frequentDatabases.entries())
        .map(([name, count]) => ({ name, count }))
        .sort((a, b) => b.count - a.count)
        .slice(0, 10),
      frequentTables: Array.from(profile.frequentTables.entries())
        .map(([name, count]) => ({ name, count }))
        .sort((a, b) => b.count - a.count)
        .slice(0, 10),
      activeHours: Array.from(profile.activeTimeSlots.entries())
        .map(([hour, count]) => ({ hour, count }))
        .sort((a, b) => b.count - a.count),
      avgDepth: profile.avgDepth,
    }
  }

  return {
    learningManager,
    recordAndLearn,
    warmBasedOnLearning,
    getLearningStats,
  }
}
