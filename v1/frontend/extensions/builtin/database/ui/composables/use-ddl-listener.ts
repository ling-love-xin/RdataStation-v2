/**
 * DDL 监听缓存失效
 *
 * 监听 SQL 执行中的 DDL 语句，自动失效相关缓存：
 * - CREATE TABLE/DATABASE/SCHEMA
 * - ALTER TABLE/COLUMN
 * - DROP TABLE/VIEW/INDEX
 * - TRUNCATE TABLE
 *
 * 遵循架构规范：前端只负责调度，不实现业务逻辑
 */

import { ref } from 'vue'

import { cacheStateManager } from './use-cache-state'

/**
 * DDL 语句类型
 */
export type DDLType =
  | 'CREATE_TABLE'
  | 'CREATE_DATABASE'
  | 'CREATE_SCHEMA'
  | 'CREATE_VIEW'
  | 'CREATE_INDEX'
  | 'ALTER_TABLE'
  | 'ALTER_COLUMN'
  | 'DROP_TABLE'
  | 'DROP_VIEW'
  | 'DROP_INDEX'
  | 'DROP_DATABASE'
  | 'DROP_SCHEMA'
  | 'TRUNCATE_TABLE'

/**
 * DDL 事件
 */
export interface DDLEvent {
  /** DDL 类型 */
  type: DDLType
  /** 连接 ID */
  connectionId: string
  /** 连接类型 */
  connectionType?: 'global' | 'project'
  /** 项目路径 */
  projectPath?: string
  /** 数据库名 */
  databaseName: string
  /** Schema 名 */
  schemaName?: string
  /** 表名 */
  tableName?: string
  /** 列名 */
  columnName?: string
  /** 执行时间 */
  executedAt: number
}

/**
 * DDL 监听器配置
 */
export interface DDLListenerConfig {
  /** 是否启用 DDL 监听 */
  enabled: boolean
  /** 是否自动失效缓存 */
  autoInvalidate: boolean
  /** 是否通知后端 */
  notifyBackend: boolean
}

const defaultConfig: DDLListenerConfig = {
  enabled: true,
  autoInvalidate: true,
  notifyBackend: true,
}

/**
 * DDL 监听状态
 */
export interface DDLListenerState {
  /** 是否正在监听 */
  isListening: boolean
  /** 捕获的 DDL 事件列表 */
  capturedEvents: DDLEvent[]
  /** 失效的缓存数量 */
  invalidatedCaches: number
}

/**
 * DDL 语句关键词
 */
const DDL_KEYWORDS: Record<string, DDLType> = {
  'CREATE TABLE': 'CREATE_TABLE',
  'CREATE DATABASE': 'CREATE_DATABASE',
  'CREATE SCHEMA': 'CREATE_SCHEMA',
  'CREATE VIEW': 'CREATE_VIEW',
  'CREATE INDEX': 'CREATE_INDEX',
  'ALTER TABLE': 'ALTER_TABLE',
  'ALTER COLUMN': 'ALTER_COLUMN',
  'DROP TABLE': 'DROP_TABLE',
  'DROP VIEW': 'DROP_VIEW',
  'DROP INDEX': 'DROP_INDEX',
  'DROP DATABASE': 'DROP_DATABASE',
  'DROP SCHEMA': 'DROP_SCHEMA',
  'TRUNCATE TABLE': 'TRUNCATE_TABLE',
}

/**
 * DDL 监听缓存失效 Composable
 */
export function useDDLListener(config?: Partial<DDLListenerConfig>) {
  const state = ref<DDLListenerState>({
    isListening: false,
    capturedEvents: [],
    invalidatedCaches: 0,
  })

  const cfg = ref<DDLListenerConfig>({ ...defaultConfig, ...config })

  /**
   * 解析 SQL 语句，检测是否为 DDL 语句
   */
  function detectDDLType(sql: string): DDLType | null {
    const normalizedSql = sql.trim().toUpperCase()

    for (const [keyword, ddlType] of Object.entries(DDL_KEYWORDS)) {
      if (normalizedSql.startsWith(keyword)) {
        return ddlType
      }
    }

    return null
  }

  /**
   * 从 SQL 语句中提取元数据信息
   */
  function extractMetadata(
    sql: string,
    ddlType: DDLType,
    connectionId: string,
    databaseName: string,
    schemaName?: string
  ): Partial<DDLEvent> {
    const normalizedSql = sql.trim().toUpperCase()
    const metadata: Partial<DDLEvent> = {
      type: ddlType,
      connectionId,
      databaseName,
      schemaName,
    }

    const parts = normalizedSql.split(/\s+/)

    if (ddlType.includes('TABLE') || ddlType.includes('VIEW')) {
      const nameIndex = parts.findIndex(p => p === 'TABLE' || p === 'VIEW' || p === 'INDEX')
      if (nameIndex !== -1 && nameIndex + 1 < parts.length) {
        let name = parts[nameIndex + 1]
        name = name.replace(/[`"'[\]]/g, '')

        const nameParts = name.split('.')
        if (nameParts.length >= 2) {
          metadata.schemaName = nameParts[0]
          metadata.tableName = nameParts[1]
        } else {
          metadata.tableName = name
        }
      }
    } else if (ddlType.includes('COLUMN')) {
      const nameIndex = parts.findIndex(p => p === 'COLUMN')
      if (nameIndex !== -1 && nameIndex + 1 < parts.length) {
        metadata.columnName = parts[nameIndex + 1].replace(/[`"'[\]]/g, '')
      }
    }

    return metadata
  }

  /**
   * 失效相关缓存
   */
  function invalidateRelatedCache(event: DDLEvent): number {
    let invalidated = 0

    switch (event.type) {
      case 'CREATE_TABLE':
      case 'DROP_TABLE':
      case 'ALTER_TABLE':
      case 'TRUNCATE_TABLE':
        cacheStateManager.markInvalid({
          connectionId: event.connectionId,
          databaseName: event.databaseName,
          schemaName: event.schemaName,
          tableName: event.tableName,
        })
        invalidated++
        break

      case 'ALTER_COLUMN':
        cacheStateManager.markInvalid({
          connectionId: event.connectionId,
          databaseName: event.databaseName,
          schemaName: event.schemaName,
          tableName: event.tableName,
          columnName: event.columnName,
        })
        invalidated++
        break

      case 'CREATE_DATABASE':
      case 'DROP_DATABASE':
        cacheStateManager.clearConnection(event.connectionId)
        invalidated++
        break

      case 'CREATE_SCHEMA':
      case 'DROP_SCHEMA':
        cacheStateManager.markInvalid({
          connectionId: event.connectionId,
          databaseName: event.databaseName,
        })
        invalidated++
        break

      case 'CREATE_VIEW':
      case 'DROP_VIEW':
        cacheStateManager.markInvalid({
          connectionId: event.connectionId,
          databaseName: event.databaseName,
          schemaName: event.schemaName,
        })
        invalidated++
        break

      case 'CREATE_INDEX':
      case 'DROP_INDEX':
        cacheStateManager.markInvalid({
          connectionId: event.connectionId,
          databaseName: event.databaseName,
          schemaName: event.schemaName,
          tableName: event.tableName,
        })
        invalidated++
        break
    }

    return invalidated
  }

  /**
   * 处理 SQL 执行事件
   */
  function handleSqlExecution(
    sql: string,
    connectionId: string,
    databaseName: string,
    schemaName?: string
  ): DDLEvent | null {
    if (!cfg.value.enabled) return null

    const ddlType = detectDDLType(sql)
    if (!ddlType) return null

    const metadata = extractMetadata(sql, ddlType, connectionId, databaseName, schemaName)

    const event: DDLEvent = {
      type: ddlType,
      connectionId,
      databaseName,
      schemaName,
      tableName: metadata.tableName,
      columnName: metadata.columnName,
      executedAt: Date.now(),
    }

    state.value.capturedEvents.push(event)

    if (cfg.value.autoInvalidate) {
      const invalidated = invalidateRelatedCache(event)
      state.value.invalidatedCaches += invalidated
    }

    if (cfg.value.notifyBackend) {
      notifyBackendDDL(event).catch(err => console.error('通知后端 DDL 事件失败:', err))
    }

    return event
  }

  /**
   * 通知后端 DDL 事件
   */
  async function notifyBackendDDL(event: DDLEvent): Promise<void> {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      await invoke('notify_ddl_event', {
        event: {
          type: event.type,
          connectionId: event.connectionId,
          connectionType: event.connectionType || 'global',
          projectPath: event.projectPath,
          databaseName: event.databaseName,
          schemaName: event.schemaName,
          tableName: event.tableName,
          columnName: event.columnName,
          executedAt: event.executedAt,
        },
      })
    } catch (error) {
      console.warn('通知后端 DDL 事件失败:', error)
    }
  }

  /**
   * 开始监听
   */
  function startListening(): void {
    state.value.isListening = true
  }

  /**
   * 停止监听
   */
  function stopListening(): void {
    state.value.isListening = false
  }

  /**
   * 清除监听状态
   */
  function clearState(): void {
    state.value.capturedEvents = []
    state.value.invalidatedCaches = 0
  }

  /**
   * 更新配置
   */
  function updateConfig(newConfig: Partial<DDLListenerConfig>): void {
    cfg.value = { ...cfg.value, ...newConfig }
  }

  return {
    state,
    config: cfg,
    detectDDLType,
    handleSqlExecution,
    startListening,
    stopListening,
    clearState,
    updateConfig,
  }
}
