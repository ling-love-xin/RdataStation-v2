/**
 * SQL 执行状态管理
 *
 * 使用 Pinia Store 管理 SQL 编辑器与结果面板之间的通信。
 * editor-runtime-store.ts 管理编辑器 UI 运行时状态（执行中/执行时间），
 * 本 Store 管理执行结果数据（结果集/面板绑定/新标签请求）。
 * 两者职责互补，非替代关系。
 */

import { defineStore } from 'pinia'
import { ref, computed } from 'vue'

export interface ExecutionResult {
  panelId: string
  result: {
    columns: string[]
    rows: unknown[][]
    rowCount: number
    executionTime: number
    affectedRows?: number
  } | null
  error: string | null
  timestamp: number
  title?: string
}

export const useSqlExecutionStore = defineStore('sqlExecution', () => {
  const executionResults = ref<Map<string, ExecutionResult>>(new Map())
  const resultVersion = ref(0)
  const activeEditorPanelId = ref<string | null>(null)
  const newTabRequests = ref<Map<string, ExecutionResult>>(new Map())
  const refreshRequests = ref<Map<string, number>>(new Map())
  const settingsPanelOpen = ref(false)

  // ==================== Getters ====================
  const getExecutionResult = computed(() => (panelId: string) => {
    return executionResults.value.get(panelId) || null
  })

  const latestResult = computed(() => {
    if (!activeEditorPanelId.value) return null
    return executionResults.value.get(activeEditorPanelId.value) || null
  })

  const latestNewTabRequest = computed(() => {
    const entries = Array.from(newTabRequests.value.entries())
    if (entries.length === 0) return null
    return entries[entries.length - 1][1]
  })

  // ==================== Actions ====================

  /**
   * 设置当前活动的编辑器面板 ID
   */
  function setActiveEditorPanelId(panelId: string | null) {
    activeEditorPanelId.value = panelId
  }

  /**
   * 执行 SQL 查询
   * 返回 Promise，调用者可以等待结果
   */
  function getResultForPanel(panelId: string): ExecutionResult | null {
    return executionResults.value.get(panelId) || null
  }

  /**
   * 清除指定面板的结果
   */
  function clearResult(panelId: string) {
    executionResults.value.delete(panelId)
  }

  /**
   * 清除所有结果
   */
  function clearAllResults() {
    executionResults.value.clear()
  }

  /**
   * 请求在新标签页打开结果
   */
  function requestNewTab(panelId: string, result: ExecutionResult) {
    newTabRequests.value.set(panelId, result)
    newTabRequests.value = new Map(newTabRequests.value)
  }

  function requestNewTabWithTitle(panelId: string, result: ExecutionResult, title: string) {
    newTabRequests.value.set(panelId, { ...result, title })
    newTabRequests.value = new Map(newTabRequests.value)
  }

  /**
   * 获取并消费最新的新标签请求
   */
  function consumeNewTabRequest(): ExecutionResult | null {
    const result = latestNewTabRequest.value
    if (result) {
      newTabRequests.value.clear()
    }
    return result
  }

  /**
   * 请求刷新结果面板
   */
  function requestRefresh(panelId: string) {
    refreshRequests.value.set(panelId, Date.now())
    refreshRequests.value = new Map(refreshRequests.value)
  }

  /**
   * 打开设置面板
   */
  function openSettings() {
    settingsPanelOpen.value = true
  }

  /**
   * 关闭设置面板
   */
  function closeSettings() {
    settingsPanelOpen.value = false
  }

  function incrementResultVersion() {
    resultVersion.value++
  }

  return {
    // State
    executionResults,
    resultVersion,
    activeEditorPanelId,
    newTabRequests,
    refreshRequests,
    settingsPanelOpen,
    // Getters
    getExecutionResult,
    latestResult,
    latestNewTabRequest,
    // Actions
    setActiveEditorPanelId,
    getResultForPanel,
    clearResult,
    clearAllResults,
    requestNewTab,
    requestNewTabWithTitle,
    consumeNewTabRequest,
    requestRefresh,
    openSettings,
    closeSettings,
    incrementResultVersion,
  }
})
