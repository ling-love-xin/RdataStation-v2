/**
 * SQL 执行历史服务
 *
 * v2.0: 历史记录统一由后端 SQLite 驱动，前端不再维护独立的 localStorage 副本。
 * 收藏/标签/备注等 UI 偏好存储在独立的 localStorage key 中。
 *
 * 迁移策略：
 * - 首次加载时检测旧 localStorage 数据，自动导入到后端
 * - 导入成功后清除旧 localStorage key
 */

import { sqlApi } from '@/shared/api'
import type { SqlHistoryResponse } from '@/shared/api'

const STORAGE_KEY_LEGACY = 'sql-execution-history'
const STORAGE_KEY_META = 'sql-history-meta'
const MIGRATION_FLAG = 'sql-history-migrated'

/** 历史记录元数据（收藏/标签/备注） */
interface HistoryMeta {
  [id: string]: {
    isFavorite: boolean
    tags: string[]
    note: string
  }
}

/** 前端历史记录项（合并后端数据 + 前端元数据） */
export interface SqlHistoryItem {
  id: string
  sql: string
  connId: string | null
  dbType: string | null
  executedAt: string
  durationMs: number | null
  success: boolean | null
  errorMessage: string | null
  rowsAffected: number | null
  rowsReturned: number | null
  /** UI 元数据 */
  isFavorite: boolean
  tags: string[]
  note: string
}

// ==================== 元数据管理（localStorage） ====================

function loadMeta(): HistoryMeta {
  try {
    const stored = localStorage.getItem(STORAGE_KEY_META)
    return stored ? JSON.parse(stored) : {}
  } catch {
    return {}
  }
}

function saveMeta(meta: HistoryMeta): void {
  try {
    localStorage.setItem(STORAGE_KEY_META, JSON.stringify(meta))
  } catch {
    console.warn('[sql-history] Failed to save meta')
  }
}

function getMeta(id: string): { isFavorite: boolean; tags: string[]; note: string } {
  const meta = loadMeta()
  return meta[id] ?? { isFavorite: false, tags: [], note: '' }
}

function setMeta(
  id: string,
  updates: Partial<{ isFavorite: boolean; tags: string[]; note: string }>
): void {
  const meta = loadMeta()
  meta[id] = { ...getMeta(id), ...updates }
  saveMeta(meta)
}

function removeMeta(id: string): void {
  const meta = loadMeta()
  delete meta[id]
  saveMeta(meta)
}

/** 将后端响应映射为前端历史项 */
function mapToItem(h: SqlHistoryResponse): SqlHistoryItem {
  const meta = getMeta(h.id)
  return {
    id: h.id,
    sql: h.sql,
    connId: h.conn_id,
    dbType: h.db_type,
    executedAt: h.executed_at,
    durationMs: h.duration_ms,
    success: h.success,
    errorMessage: h.error_message,
    rowsAffected: h.rows_affected,
    rowsReturned: h.rows_returned,
    isFavorite: meta.isFavorite,
    tags: meta.tags,
    note: meta.note,
  }
}

// ==================== 迁移：localStorage → 后端 ====================

/** 检查是否已迁移 */
export function isMigrated(): boolean {
  return localStorage.getItem(MIGRATION_FLAG) === 'true'
}

/** 将旧 localStorage 数据导入后端 */
export async function migrateFromLocalStorage(): Promise<number> {
  if (isMigrated()) return 0

  try {
    const stored = localStorage.getItem(STORAGE_KEY_LEGACY)
    if (!stored) {
      localStorage.setItem(MIGRATION_FLAG, 'true')
      return 0
    }

    const oldItems: Array<{
      id: string
      sql: string
      isFavorite: boolean
      tags?: string[]
      note?: string
    }> = JSON.parse(stored)

    if (!Array.isArray(oldItems) || oldItems.length === 0) {
      localStorage.setItem(MIGRATION_FLAG, 'true')
      return 0
    }

    // 保存元数据（收藏/标签/备注）
    const meta: HistoryMeta = {}
    for (const item of oldItems) {
      if (item.isFavorite || item.tags?.length || item.note) {
        meta[item.id] = {
          isFavorite: item.isFavorite ?? false,
          tags: item.tags ?? [],
          note: item.note ?? '',
        }
      }
    }
    if (Object.keys(meta).length > 0) {
      saveMeta(meta)
    }

    // 旧数据中的 SQL 已由后端在 execute_sql 时自动记录，
    // 此处只需迁移元数据，无需重复导入历史记录

    // 清除旧 localStorage
    localStorage.removeItem(STORAGE_KEY_LEGACY)
    localStorage.setItem(MIGRATION_FLAG, 'true')

    return oldItems.length
  } catch (error) {
    console.warn('[sql-history] Migration failed:', error)
    return 0
  }
}

// ==================== 历史 CRUD（后端驱动） ====================

/**
 * 获取所有历史记录
 */
export async function getHistory(limit: number = 100): Promise<SqlHistoryItem[]> {
  try {
    const records = await sqlApi.getSqlHistory(limit)
    return records.map(mapToItem)
  } catch (error) {
    console.error('[sql-history] Failed to load history:', error)
    return []
  }
}

/**
 * 删除历史记录
 */
export async function deleteHistory(id: string): Promise<boolean> {
  try {
    await sqlApi.removeSqlHistory(id)
    removeMeta(id)
    return true
  } catch (error) {
    console.error('[sql-history] Failed to delete history:', error)
    return false
  }
}

/**
 * 清空历史记录
 */
export async function clearHistory(): Promise<void> {
  try {
    await sqlApi.clearSqlHistory()
    localStorage.removeItem(STORAGE_KEY_META)
  } catch (error) {
    console.error('[sql-history] Failed to clear history:', error)
  }
}

/**
 * 搜索历史记录
 */
export async function searchHistory(query: string, limit: number = 100): Promise<SqlHistoryItem[]> {
  try {
    const records = await sqlApi.searchSqlHistory(query, limit)
    return records.map(mapToItem)
  } catch (error) {
    console.error('[sql-history] Failed to search history:', error)
    return []
  }
}

// ==================== 收藏/标签/备注（前端元数据） ====================

/**
 * 切换收藏状态
 */
export function toggleFavorite(id: string): boolean {
  try {
    const meta = getMeta(id)
    setMeta(id, { isFavorite: !meta.isFavorite })
    return true
  } catch {
    return false
  }
}

/**
 * 获取收藏的 SQL
 */
export async function getFavorites(): Promise<SqlHistoryItem[]> {
  const history = await getHistory(200)
  return history.filter(item => item.isFavorite)
}

/**
 * 添加标签
 */
export function addTag(id: string, tag: string): boolean {
  try {
    const meta = getMeta(id)
    if (!meta.tags.includes(tag)) {
      setMeta(id, { tags: [...meta.tags, tag] })
    }
    return true
  } catch {
    return false
  }
}

/**
 * 移除标签
 */
export function removeTag(id: string, tag: string): boolean {
  try {
    const meta = getMeta(id)
    setMeta(id, { tags: meta.tags.filter(t => t !== tag) })
    return true
  } catch {
    return false
  }
}

/**
 * 添加备注
 */
export function addNote(id: string, note: string): boolean {
  try {
    setMeta(id, { note })
    return true
  } catch {
    return false
  }
}

/**
 * 导出历史记录
 */
export async function exportHistory(): Promise<string> {
  const history = await getHistory(500)
  return JSON.stringify(history, null, 2)
}

/**
 * 导入历史记录（已废弃，历史由后端自动记录）
 * @deprecated 历史记录由后端 execute_sql 自动记录，无需手动导入
 */
export function importHistory(_json: string): boolean {
  console.warn('[sql-history] importHistory is deprecated: history is auto-recorded by backend')
  return false
}

/**
 * 获取常用 SQL（执行次数最多的）
 */
export async function getFrequentSql(limit: number = 10): Promise<Array<{ sql: string; count: number }>> {
  const history = await getHistory(500)
  const sqlCounts = new Map<string, number>()

  for (const item of history) {
    const normalizedSql = item.sql.trim().toLowerCase()
    sqlCounts.set(normalizedSql, (sqlCounts.get(normalizedSql) || 0) + 1)
  }

  return Array.from(sqlCounts.entries())
    .map(([sql, count]) => ({ sql, count }))
    .sort((a, b) => b.count - a.count)
    .slice(0, limit)
}

/**
 * 获取最近执行的 SQL
 */
export function getRecentSql(limit: number = 10): Promise<SqlHistoryItem[]> {
  return getHistory(limit)
}