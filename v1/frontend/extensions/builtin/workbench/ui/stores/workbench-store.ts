/**
 * 工作台状态管理
 *
 * 管理动态面板布局、SQL编辑器状态、查询结果等
 */

import { invoke } from '@tauri-apps/api/core'
import { defineStore } from 'pinia'
import { ref, computed } from 'vue'

import type { QueryResult, QueryTab } from '@/shared/types'

// 面板类型
export type PanelType =
  | 'sql-editor'
  | 'table-data'
  | 'query-result'
  | 'properties'
  | 'database-nav'
  | 'history'
  | 'settings'

// 面板位置
export type PanelPosition = 'left' | 'center' | 'right' | 'bottom'

// 面板定义
export interface Panel {
  id: string
  type: PanelType
  title: string
  position: PanelPosition
  component: string // 组件名称
  props: Record<string, unknown>
  isActive: boolean
  isClosable: boolean
  isPinned?: boolean
  order: number
}

// 工作台状态
export interface WorkbenchState {
  panels: Panel[]
  activePanelId: string | null
  layout: {
    leftWidth: number
    rightWidth: number
    bottomHeight: number
  }
}

// SQL编辑器状态
export interface EditorState {
  tabs: QueryTab[]
  activeTabId: string | null
  cursorPosition: { line: number; column: number }
}

export const useWorkbenchStore = defineStore('workbench', () => {
  // State
  const panels = ref<Panel[]>([
    // 默认面板
    {
      id: 'database-nav',
      type: 'database-nav',
      title: '数据库导航',
      position: 'left',
      component: 'DatabaseNavigator',
      props: {},
      isActive: true,
      isClosable: false,
      order: 0,
    },
    {
      id: 'sql-editor',
      type: 'sql-editor',
      title: 'SQL 编辑器',
      position: 'center',
      component: 'editorPanel',
      props: {},
      isActive: true,
      isClosable: false,
      order: 0,
    },
  ])

  const activePanelId = ref<string | null>('sql-editor')
  const layout = ref({
    leftWidth: 280,
    rightWidth: 0,
    bottomHeight: 200,
  })

  // 编辑器状态
  const editorState = ref<EditorState>({
    tabs: [],
    activeTabId: null,
    cursorPosition: { line: 1, column: 1 },
  })

  // 查询结果
  const queryResults = ref<Map<string, QueryResult>>(new Map())

  // Getters
  const leftPanels = computed(() =>
    panels.value.filter(p => p.position === 'left').sort((a, b) => a.order - b.order)
  )

  const centerPanels = computed(() =>
    panels.value.filter(p => p.position === 'center').sort((a, b) => a.order - b.order)
  )

  const rightPanels = computed(() =>
    panels.value.filter(p => p.position === 'right').sort((a, b) => a.order - b.order)
  )

  const bottomPanels = computed(() =>
    panels.value.filter(p => p.position === 'bottom').sort((a, b) => a.order - b.order)
  )

  const activePanel = computed(() => panels.value.find(p => p.id === activePanelId.value))

  const activeTab = computed(() =>
    editorState.value.tabs.find(t => t.id === editorState.value.activeTabId)
  )

  // Actions

  /**
   * 添加面板
   */
  function addPanel(panel: Omit<Panel, 'id' | 'order'>): string {
    const id = `${panel.type}_${Date.now()}`
    const existingCount = panels.value.filter(p => p.position === panel.position).length

    panels.value.push({
      ...panel,
      id,
      order: existingCount,
    })

    // 激活新面板
    activePanelId.value = id

    return id
  }

  /**
   * 移除面板
   */
  function removePanel(id: string): void {
    const index = panels.value.findIndex(p => p.id === id)
    if (index === -1) return

    const panel = panels.value[index]
    if (!panel.isClosable) return

    panels.value.splice(index, 1)

    // 重新排序
    panels.value
      .filter(p => p.position === panel.position)
      .forEach((p, i) => {
        p.order = i
      })

    // 如果关闭的是当前激活面板，激活其他面板
    if (activePanelId.value === id) {
      const samePosition = panels.value.filter(p => p.position === panel.position)
      activePanelId.value = samePosition[0]?.id || panels.value[0]?.id || null
    }
  }

  /**
   * 激活面板
   */
  function activatePanel(id: string): void {
    const panel = panels.value.find(p => p.id === id)
    if (panel) {
      panel.isActive = true
      activePanelId.value = id
    }
  }

  /**
   * 移动面板
   */
  function movePanel(id: string, position: PanelPosition): void {
    const panel = panels.value.find(p => p.id === id)
    if (!panel) return

    panel.position = position

    // 重新排序
    panels.value
      .filter(p => p.position === position)
      .forEach((p, i) => {
        p.order = i
      })
  }

  /**
   * 打开表数据面板
   */
  function openTableData(
    connectionId: string,
    database: string,
    schema: string,
    table: string
  ): void {
    const _id = addPanel({
      type: 'table-data',
      title: `${database}.${schema}.${table}`,
      position: 'center',
      component: 'TableDataPanel',
      props: {
        connectionId,
        database,
        schema,
        table,
      },
      isActive: true,
      isClosable: true,
    })
  }

  /**
   * 打开查询结果面板
   */
  function openQueryResult(queryId: string, title: string, result: QueryResult): void {
    queryResults.value.set(queryId, result)

    addPanel({
      type: 'query-result',
      title: `结果: ${title}`,
      position: 'bottom',
      component: 'QueryResultPanel',
      props: {
        queryId,
        result,
      },
      isActive: true,
      isClosable: true,
    })
  }

  /**
   * 添加 SQL 编辑器标签页
   */
  function addEditorTab(connectionId?: string, sql?: string, title?: string): string {
    const id = `tab_${Date.now()}`
    const tab: QueryTab = {
      id,
      title: title || '未命名',
      name: title || '未命名',
      sql: sql || '',
      connectionId,
      result: null,
      status: 'idle',
      isExecuting: false,
      loading: false,
      error: null,
      elapsedMs: 0,
    }

    editorState.value.tabs.push(tab)
    editorState.value.activeTabId = id

    return id
  }

  /**
   * 关闭编辑器标签页
   */
  function closeEditorTab(id: string): void {
    const index = editorState.value.tabs.findIndex(t => t.id === id)
    if (index === -1) return

    editorState.value.tabs.splice(index, 1)

    // 激活其他标签页
    if (editorState.value.activeTabId === id) {
      editorState.value.activeTabId =
        editorState.value.tabs[Math.min(index, editorState.value.tabs.length - 1)]?.id || null
    }
  }

  /**
   * 更新编辑器标签页
   */
  function updateEditorTab(id: string, updates: Partial<QueryTab>): void {
    const tab = editorState.value.tabs.find(t => t.id === id)
    if (tab) {
      Object.assign(tab, updates)
    }
  }

  /**
   * 保存工作台状态
   *
   * Rust 端 WorkbenchState 结构:
   *   layout: Option<String>       - JSON string
   *   open_panels: Option<String>  - JSON string
   *   active_panel_id: Option<String>
   */
  async function saveState(): Promise<void> {
    try {
      await invoke('save_project_store_workbench_state', {
        stateData: {
          layout: JSON.stringify(layout.value),
          open_panels: JSON.stringify(panels.value),
          active_panel_id: activePanelId.value,
        },
      })
    } catch (e) {
      console.error('保存工作台状态失败:', e)
      localStorage.setItem(
        'workbench_state',
        JSON.stringify({
          panels: panels.value,
          activePanelId: activePanelId.value,
          layout: layout.value,
        })
      )
    }
  }

  /**
   * 加载工作台状态
   */
  async function loadState(): Promise<void> {
    try {
      const state = await invoke<{
        layout: string | null
        open_panels: string | null
        active_panel_id: string | null
      } | null>('get_project_store_workbench_state')

      if (state) {
        if (state.open_panels) {
          try {
            panels.value = JSON.parse(state.open_panels)
          } catch {
            console.warn('[WorkbenchStore] JSON.parse open_panels failed, keeping default')
          }
        }
        if (state.active_panel_id) {
          activePanelId.value = state.active_panel_id
        }
        if (state.layout) {
          try {
            layout.value = JSON.parse(state.layout)
          } catch {
            console.warn('[WorkbenchStore] JSON.parse layout failed, keeping default')
          }
        }
      }
    } catch (e) {
      console.error('加载工作台状态失败:', e)
      const saved = localStorage.getItem('workbench_state')
      if (saved) {
        const state = JSON.parse(saved)
        panels.value = state.panels || panels.value
        activePanelId.value = state.activePanelId
        layout.value = state.layout || layout.value
      }
    }
  }

  /**
   * 重置工作台
   */
  function resetWorkbench(): void {
    panels.value = [
      {
        id: 'database-nav',
        type: 'database-nav',
        title: '数据库导航',
        position: 'left',
        component: 'DatabaseNavigator',
        props: {},
        isActive: true,
        isClosable: false,
        order: 0,
      },
      {
        id: 'sql-editor',
        type: 'sql-editor',
        title: 'SQL 编辑器',
        position: 'center',
        component: 'editorPanel',
        props: {},
        isActive: true,
        isClosable: false,
        order: 0,
      },
    ]
    activePanelId.value = 'sql-editor'
    editorState.value.tabs = []
    editorState.value.activeTabId = null
    queryResults.value.clear()
  }

  return {
    // State
    panels,
    activePanelId,
    layout,
    editorState,
    queryResults,

    // Getters
    leftPanels,
    centerPanels,
    rightPanels,
    bottomPanels,
    activePanel,
    activeTab,

    // Actions
    addPanel,
    removePanel,
    activatePanel,
    movePanel,
    openTableData,
    openQueryResult,
    addEditorTab,
    closeEditorTab,
    updateEditorTab,
    saveState,
    loadState,
    resetWorkbench,
  }
})
