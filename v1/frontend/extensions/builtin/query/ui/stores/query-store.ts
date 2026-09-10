import { defineStore } from 'pinia'
import { ref, computed } from 'vue'

import type { QueryTab } from '@/shared/types'

import * as queryService from '../services/query'

/**
 * 查询状态管理
 *
 * 管理 SQL 查询相关的状态，包括：
 * - 查询标签页
 * - SQL 历史记录
 * - 查询执行状态
 */
export const useQueryStore = defineStore('query', () => {
  // ==================== State ====================
  const tabs = ref<QueryTab[]>([
    {
      id: '1',
      title: '查询 1',
      name: '查询 1',
      sql: '',
      result: null,
      status: 'idle',
      isExecuting: false,
      loading: false,
      error: null,
      elapsedMs: 0,
    },
  ])
  const activeTabId = ref('1')

  // ==================== Getters ====================
  const activeTab = computed(
    () => tabs.value.find(t => t.id === activeTabId.value) || tabs.value[0]
  )
  const tabCount = computed(() => tabs.value.length)
  const hasResults = computed(() => activeTab.value?.result !== null)

  // ==================== Actions ====================

  /**
   * 添加新标签页
   */
  function addTab() {
    const newId = String(Date.now())
    const newTab: QueryTab = {
      id: newId,
      title: `查询 ${tabs.value.length + 1}`,
      name: `查询 ${tabs.value.length + 1}`,
      sql: '',
      result: null,
      status: 'idle',
      isExecuting: false,
      loading: false,
      error: null,
      elapsedMs: 0,
    }
    tabs.value.push(newTab)
    activeTabId.value = newId
    return newTab
  }

  /**
   * 移除标签页
   */
  function removeTab(tabId: string) {
    if (tabs.value.length <= 1) {
      // 至少保留一个标签页
      const tab = tabs.value[0]
      tab.sql = ''
      tab.result = null
      tab.error = null
      tab.elapsedMs = 0
      return
    }

    const index = tabs.value.findIndex(t => t.id === tabId)
    if (index === -1) return

    tabs.value.splice(index, 1)

    // 如果关闭的是当前活动标签，切换到相邻标签
    if (activeTabId.value === tabId) {
      const newIndex = Math.min(index, tabs.value.length - 1)
      activeTabId.value = tabs.value[newIndex].id
    }
  }

  /**
   * 切换活动标签页
   */
  function switchTab(tabId: string) {
    if (tabs.value.find(t => t.id === tabId)) {
      activeTabId.value = tabId
    }
  }

  /**
   * 更新标签页 SQL
   */
  function updateTabSql(tabId: string, sql: string) {
    const tab = tabs.value.find(t => t.id === tabId)
    if (tab) {
      tab.sql = sql
    }
  }

  /**
   * 重命名标签页
   */
  function renameTab(tabId: string, name: string) {
    const tab = tabs.value.find(t => t.id === tabId)
    if (tab) {
      tab.name = name
    }
  }

  /**
   * 执行 SQL 查询
   */
  async function executeSql(connId?: string, sql?: string, timeoutMs?: number) {
    const tab = activeTab.value
    if (!tab) return

    // 使用传入的 SQL 或当前标签页的 SQL
    const executeSql = sql || tab.sql
    if (!executeSql.trim()) return

    tab.loading = true
    tab.error = null
    tab.result = null
    tab.elapsedMs = 0

    try {
      const response = await queryService.executeSql(executeSql, connId, timeoutMs)
      if (response.result) {
        tab.result = {
          columns: response.result.columns,
          rows: response.result.rows,
          rowCount: response.result.total_rows,
          executionTime: response.elapsed_ms,
        }
      }
      tab.elapsedMs = response.elapsed_ms ?? 0
      tab.affectedRows = response.affected_rows ?? undefined
    } catch (e) {
      tab.error = e instanceof Error ? e.message : '执行失败'
    } finally {
      tab.loading = false
    }
  }

  /**
   * 执行事务
   */
  async function executeTransaction(sqls: string[], connId?: string) {
    const tab = activeTab.value
    if (!tab) return

    tab.loading = true
    tab.error = null
    tab.result = null

    try {
      const response = await queryService.executeTransaction(sqls, connId)
      if (response.results && response.results.length > 0) {
        const lastResult = response.results[response.results.length - 1]
        if (lastResult && lastResult.result) {
          tab.result = {
            columns: lastResult.result.columns,
            rows: lastResult.result.rows,
            rowCount: lastResult.result.total_rows,
            executionTime: lastResult.elapsed_ms,
          }
        }
      }
      const totalElapsed = response.results.reduce((sum, r) => sum + r.elapsed_ms, 0)
      tab.elapsedMs = totalElapsed
    } catch (e) {
      tab.error = e instanceof Error ? e.message : '事务执行失败'
    } finally {
      tab.loading = false
    }
  }

  /**
   * 清除标签页错误
   */
  function clearTabError(tabId?: string) {
    const targetTab = tabId ? tabs.value.find(t => t.id === tabId) : activeTab.value
    if (targetTab) {
      targetTab.error = null
    }
  }

  return {
    // State
    tabs,
    activeTabId,
    // Getters
    activeTab,
    tabCount,
    hasResults,
    // Actions
    addTab,
    removeTab,
    switchTab,
    updateTabSql,
    renameTab,
    executeSql,
    executeTransaction,
    clearTabError,
  }
})
