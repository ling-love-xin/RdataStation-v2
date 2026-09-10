/**
 * 日志模块类型定义
 *
 * 与后端 core/logging/record.rs 保持同步
 */

/**
 * 日志级别
 */
export type LogLevel = 'TRACE' | 'DEBUG' | 'INFO' | 'WARN' | 'ERROR'

/**
 * 日志记录（与 Rust LogRecord 对应）
 */
export interface LogRecord {
  id: number
  timestamp: string
  level: LogLevel
  target: string
  message: string
  fields: string | null
  file: string | null
  line: number | null
  session_id: string
}

/**
 * 日志查询参数
 */
export interface LogQuery {
  page?: number
  page_size?: number
  level?: LogLevel
  target?: string
  keyword?: string
  start?: string
  end?: string
}

/**
 * 分页日志查询结果
 */
export interface LogPage {
  records: LogRecord[]
  total: number
  page: number
  page_size: number
  total_pages: number
}

/**
 * 各级别日志计数
 */
export interface LogLevelCounts {
  trace: number
  debug: number
  info: number
  warn: number
  error: number
}

/**
 * 模块日志统计
 */
export interface TargetStat {
  target: string
  count: number
}

/**
 * 日志统计
 */
export interface LogStats {
  total: number
  by_level: LogLevelCounts
  by_target: TargetStat[]
  first_timestamp: string | null
  last_timestamp: string | null
}
