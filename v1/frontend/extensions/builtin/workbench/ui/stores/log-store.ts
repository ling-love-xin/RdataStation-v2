/**
 * 应用日志状态管理
 *
 * 管理日志查询、过滤、统计等功能。
 * 通过 Tauri invoke 调用后端 logging_commands。
 */

import { invoke } from '@tauri-apps/api/core'
import { defineStore } from 'pinia'
import { ref, computed } from 'vue'

import type { LogRecord, LogQuery, LogPage, LogStats, LogLevel } from '@/shared/types/logging'

const DEFAULT_PAGE_SIZE = 50

export const useLogStore = defineStore('log', () => {
  const records = ref<LogRecord[]>([])
  const stats = ref<LogStats | null>(null)
  const loading = ref(false)
  const error = ref<string | null>(null)
  const sessionId = ref<string>('')

  const page = ref(1)
  const pageSize = ref(DEFAULT_PAGE_SIZE)
  const total = ref(0)
  const totalPages = ref(0)

  const selectedLevel = ref<LogLevel | null>(null)
  const searchKeyword = ref('')
  const selectedTarget = ref<string | null>(null)

  const hasRecords = computed(() => records.value.length > 0)
  const errorCount = computed(() => stats.value?.by_level.error ?? 0)
  const warnCount = computed(() => stats.value?.by_level.warn ?? 0)

  async function initSession() {
    try {
      sessionId.value = await invoke<string>('get_log_session_id')
    } catch (e) {
      console.error('Failed to get log session ID:', e)
    }
  }

  async function loadLogs(query?: Partial<LogQuery>) {
    loading.value = true
    error.value = null
    try {
      const params: Record<string, unknown> = {
        page: query?.page ?? page.value,
        page_size: query?.page_size ?? pageSize.value,
      }
      if (query?.level || selectedLevel.value) {
        params.level = query?.level ?? selectedLevel.value ?? undefined
      }
      if (query?.keyword || searchKeyword.value) {
        params.keyword = query?.keyword ?? searchKeyword.value
      }
      if (query?.target || selectedTarget.value) {
        params.target = query?.target ?? selectedTarget.value ?? undefined
      }
      if (query?.start) params.start = query.start
      if (query?.end) params.end = query.end

      const result = await invoke<LogPage>('get_logs', params)
      records.value = result.records
      total.value = result.total
      page.value = result.page
      pageSize.value = result.page_size
      totalPages.value = result.total_pages
    } catch (e) {
      error.value = e instanceof Error ? e.message : '加载日志失败'
    } finally {
      loading.value = false
    }
  }

  async function searchLogs(keyword: string) {
    loading.value = true
    error.value = null
    searchKeyword.value = keyword
    try {
      const params: Record<string, unknown> = { keyword }
      if (selectedLevel.value) params.level = selectedLevel.value
      if (selectedTarget.value) params.target = selectedTarget.value

      const result = await invoke<LogPage>('search_logs', params)
      records.value = result.records
      total.value = result.total
      page.value = result.page
      pageSize.value = result.page_size
      totalPages.value = result.total_pages
    } catch (e) {
      error.value = e instanceof Error ? e.message : '搜索日志失败'
    } finally {
      loading.value = false
    }
  }

  async function loadStats() {
    try {
      stats.value = await invoke<LogStats>('get_log_stats')
    } catch (e) {
      console.error('Failed to load log stats:', e)
    }
  }

  async function clearLogs(before?: string) {
    loading.value = true
    try {
      const deleted = await invoke<number>('clear_logs', { before: before ?? null })
      await loadLogs()
      await loadStats()
      return deleted
    } catch (e) {
      error.value = e instanceof Error ? e.message : '清理日志失败'
      throw e
    } finally {
      loading.value = false
    }
  }

  async function exportLogs(level?: LogLevel, start?: string, end?: string, maxResults?: number) {
    try {
      const result = await invoke<LogRecord[]>('export_logs', {
        level: level ?? null,
        start: start ?? null,
        end: end ?? null,
        max_results: maxResults ?? null,
      })
      return result
    } catch (e) {
      error.value = e instanceof Error ? e.message : '导出日志失败'
      throw e
    }
  }

  function setLevelFilter(level: LogLevel | null) {
    selectedLevel.value = level
    page.value = 1
    loadLogs({ level: level ?? undefined })
  }

  function setTargetFilter(target: string | null) {
    selectedTarget.value = target
    page.value = 1
    loadLogs({ target: target ?? undefined })
  }

  function goToPage(p: number) {
    if (p < 1 || p > totalPages.value) return
    page.value = p
    loadLogs()
  }

  function clearError() {
    error.value = null
  }

  return {
    records,
    stats,
    loading,
    error,
    sessionId,
    page,
    pageSize,
    total,
    totalPages,
    selectedLevel,
    searchKeyword,
    selectedTarget,
    hasRecords,
    errorCount,
    warnCount,
    initSession,
    loadLogs,
    searchLogs,
    loadStats,
    clearLogs,
    exportLogs,
    setLevelFilter,
    setTargetFilter,
    goToPage,
    clearError,
  }
})
