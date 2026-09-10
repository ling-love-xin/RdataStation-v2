/**
 * 常量定义入口
 *
 * 应用级别的常量集中管理
 *
 * 使用方式：
 * ```ts
 * import { APP_NAME, DEFAULT_TIMEOUT } from '@/constants'
 * ```
 */

/** 应用信息 */
export const APP_NAME = 'RdataStation'
export const APP_VERSION = '0.1.0'

/** 默认配置 */
export const DEFAULT_TIMEOUT = 30000 // 30秒
export const DEFAULT_PAGE_SIZE = 100
export const MAX_HISTORY_ITEMS = 1000

/** 数据库类型 */
export const DB_TYPES = {
  MYSQL: 'mysql',
  POSTGRES: 'postgres',
  SQLITE: 'sqlite',
  DUCKDB: 'duckdb',
} as const

/** 路由名称 */
export const ROUTES = {
  CONNECTION: 'Connection',
  WORKBENCH: 'Workbench',
} as const

/** 存储键名 */
export const STORAGE_KEYS = {
  THEME: 'rdata:theme',
  RECENT_CONNECTIONS: 'rdata:recent_connections',
  SQL_HISTORY: 'rdata:sql_history',
  USER_PREFERENCES: 'rdata:preferences',
} as const

/** 事件名称 */
export const EVENTS = {
  CONNECTION_CHANGED: 'connection:changed',
  QUERY_EXECUTED: 'query:executed',
  THEME_CHANGED: 'theme:changed',
} as const

/** 错误代码 */
export const ERROR_CODES = {
  CONNECTION_FAILED: 'CONN_001',
  QUERY_TIMEOUT: 'QUERY_001',
  QUERY_ERROR: 'QUERY_002',
  TRANSACTION_FAILED: 'TRANS_001',
  NOT_CONNECTED: 'CONN_002',
} as const
