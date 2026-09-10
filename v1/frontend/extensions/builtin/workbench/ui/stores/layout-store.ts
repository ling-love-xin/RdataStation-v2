import { Database, BarChart3, Puzzle, FileText, Sparkles, StickyNote, Dices } from 'lucide-vue-next'
import { defineStore } from 'pinia'
import { ref, shallowRef, computed, type Component } from 'vue'

import type { SerializedDockviewLayout, SerializedSidebarState } from '@/stores/config'
import { useAppStore } from '@/stores/useAppStore'

import type { IDockviewPanel } from 'dockview-core'
import type { DockviewApi } from 'dockview-vue'

// ============================================
// 类型定义
// ============================================

export interface LeftActivityItem {
  id: string
  icon: Component
  title: string
}

export interface RightActivityItem {
  id: string
  icon: Component
  title: string
}

export type PanelLocation = 'left' | 'right' | 'center' | 'bottom' | 'floating'

export interface PanelConfig {
  location: PanelLocation
  groupId?: string
  isVisible: boolean
  order: number
}

export interface LayoutVisibility {
  menuBar: boolean
  activityBar: boolean
  primarySideBar: boolean
  secondarySideBar: boolean
  panel: boolean
  statusBar: boolean
}

export interface LayoutSizes {
  primarySideBarWidth: number
  secondarySideBarWidth: number
  panelHeight: number
}

// ============================================
// 常量定义
// ============================================
const DEFAULT_PRIMARY_SIDEBAR_WIDTH = 300
const DEFAULT_SECONDARY_SIDEBAR_WIDTH = 300
const DEFAULT_PANEL_HEIGHT = 250
const MIN_SIDEBAR_WIDTH = 200
const MAX_SIDEBAR_WIDTH = 600
const MAX_HISTORY = 50
const TEMPLATES_STORAGE_KEY = 'rdata_station_layout_templates'

// 布局快照，用于 undo/redo
export interface LayoutSnapshot {
  leftEdgeGroupCollapsed: boolean
  rightEdgeGroupCollapsed: boolean
  primarySideBarWidth: number
  secondarySideBarWidth: number
  menuBarVisible: boolean
  statusBarVisible: boolean
}

// 用户自定义布局模板
export interface LayoutTemplate {
  name: string
  snapshot: LayoutSnapshot
  createdAt: number
}

// ============================================
// 活动栏项目
// ============================================
export const leftActivityItems: LeftActivityItem[] = [
  { id: 'scratchpad', icon: StickyNote, title: '草稿箱' },
  { id: 'database', icon: Database, title: '数据库导航' },
  { id: 'analytics', icon: BarChart3, title: '分析资源管理' },
  { id: 'plugins', icon: Puzzle, title: '插件管理' },
]

export const rightActivityItems: RightActivityItem[] = [
  { id: 'column-insights', icon: Sparkles, title: '列洞察' },
  { id: 'mock', icon: Dices, title: 'Mock 数据' },
  { id: 'sql-history', icon: FileText, title: 'SQL历史' },
]

// ActivityBar 到面板映射
export const ACTIVEBAR_TO_PANEL_ID: Record<string, string> = {
  scratchpad: 'scratchpad',
  database: 'databaseNavigator',
  analytics: 'analytics-resource-manager',
  plugins: 'plugins',
  'column-insights': 'columnInsights',
  mock: 'mockPanel',
  'sql-history': 'sqlHistory',
  output: 'outputPanel',
}

// 面板注册表 ID 到 ActivityBar ID 的反向映射
export const PANEL_ID_TO_ACTIVITYBAR: Record<string, string> = Object.fromEntries(
  Object.entries(ACTIVEBAR_TO_PANEL_ID).map(([key, value]) => [value, key])
)

// ============================================
// Layout Store
// ============================================
export const useLayoutStore = defineStore('layout', () => {
  // ============================================
  // 可见性状态
  // ============================================
  const menuBarVisible = ref(true)
  const leftActivityBarVisible = ref(true)
  const rightActivityBarVisible = ref(true)
  const primarySideBarVisible = ref(true)
  const secondarySideBarVisible = ref(true)
  const panelVisible = ref(true)
  const statusBarVisible = ref(true)

  // ============================================
  // 侧边栏锁定状态（VSCode 风格：侧边栏始终存在，不可关闭）
  // ============================================
  const primarySideBarLocked = ref(true)
  const secondarySideBarLocked = ref(true)

  // ============================================
  // 选中状态
  // ============================================
  const selectedLeftItem = ref<string | null>('scratchpad')
  const selectedRightItem = ref<string | null>('column-insights')

  // Bottom insight panel tab
  const bottomInsightTab = ref<'multi' | 'history' | 'schema' | 'table'>('multi')

  // ============================================
  // 展开状态
  // ============================================
  const primarySideBarExpanded = ref(true)
  const secondarySideBarExpanded = ref(true)

  // ============================================
  // 尺寸状态
  // ============================================
  const primarySideBarWidth = ref(DEFAULT_PRIMARY_SIDEBAR_WIDTH)
  const secondarySideBarWidth = ref(DEFAULT_SECONDARY_SIDEBAR_WIDTH)
  const panelHeight = ref(DEFAULT_PANEL_HEIGHT)

  // ============================================
  // Dockview API 引用
  // ============================================
  const dockviewApi = shallowRef<DockviewApi | null>(null)

  // ============================================
  // Edge Group 折叠状态
  // ============================================
  const leftEdgeGroupCollapsed = ref(false)
  const rightEdgeGroupCollapsed = ref(false)

  // ============================================
  // 设置页面显示状态
  // ============================================
  const showCustomizeLayoutDialog = ref(false)

  // ============================================
  // Undo / Redo 历史栈
  // ============================================
  const historyStack = ref<LayoutSnapshot[]>([])
  const historyIndex = ref(-1)

  const canUndo = computed(() => historyIndex.value > 0)
  const canRedo = computed(() => historyIndex.value < historyStack.value.length - 1)

  function captureSnapshot(): LayoutSnapshot {
    return {
      leftEdgeGroupCollapsed: leftEdgeGroupCollapsed.value,
      rightEdgeGroupCollapsed: rightEdgeGroupCollapsed.value,
      primarySideBarWidth: primarySideBarWidth.value,
      secondarySideBarWidth: secondarySideBarWidth.value,
      menuBarVisible: menuBarVisible.value,
      statusBarVisible: statusBarVisible.value,
    }
  }

  function pushSnapshot() {
    if (historyIndex.value < historyStack.value.length - 1) {
      historyStack.value = historyStack.value.slice(0, historyIndex.value + 1)
    }
    historyStack.value.push(captureSnapshot())
    if (historyStack.value.length > MAX_HISTORY) {
      historyStack.value.shift()
    } else {
      historyIndex.value = historyStack.value.length - 1
    }
  }

  function applySnapshot(snapshot: LayoutSnapshot) {
    leftEdgeGroupCollapsed.value = snapshot.leftEdgeGroupCollapsed
    rightEdgeGroupCollapsed.value = snapshot.rightEdgeGroupCollapsed
    primarySideBarWidth.value = snapshot.primarySideBarWidth
    secondarySideBarWidth.value = snapshot.secondarySideBarWidth
    menuBarVisible.value = snapshot.menuBarVisible
    statusBarVisible.value = snapshot.statusBarVisible
    const api = dockviewApi.value
    if (api) {
      const leftGroup = api.getEdgeGroup?.('left')
      const rightGroup = api.getEdgeGroup?.('right')
      if (snapshot.leftEdgeGroupCollapsed) {
        leftGroup?.collapse()
      } else {
        leftGroup?.expand()
      }
      if (snapshot.rightEdgeGroupCollapsed) {
        rightGroup?.collapse()
      } else {
        rightGroup?.expand()
      }
      leftGroup?.setSize({ width: snapshot.primarySideBarWidth })
      rightGroup?.setSize({ width: snapshot.secondarySideBarWidth })
    }
  }

  function undo() {
    if (!canUndo.value) return
    historyIndex.value--
    applySnapshot(historyStack.value[historyIndex.value])
  }

  function redo() {
    if (!canRedo.value) return
    historyIndex.value++
    applySnapshot(historyStack.value[historyIndex.value])
  }

  // ============================================
  // 用户自定义布局模板
  // ============================================
  const customTemplates = ref<LayoutTemplate[]>([])

  function loadCustomTemplates() {
    try {
      const stored = localStorage.getItem(TEMPLATES_STORAGE_KEY)
      if (stored) {
        customTemplates.value = JSON.parse(stored)
      }
    } catch {
      customTemplates.value = []
    }
  }

  function saveCustomTemplate(name: string) {
    const snapshot = captureSnapshot()
    const template: LayoutTemplate = { name, snapshot, createdAt: Date.now() }
    const existing = customTemplates.value.findIndex(t => t.name === name)
    if (existing >= 0) {
      customTemplates.value[existing] = template
    } else {
      customTemplates.value.push(template)
    }
    localStorage.setItem(TEMPLATES_STORAGE_KEY, JSON.stringify(customTemplates.value))
  }

  function deleteCustomTemplate(name: string) {
    customTemplates.value = customTemplates.value.filter(t => t.name !== name)
    localStorage.setItem(TEMPLATES_STORAGE_KEY, JSON.stringify(customTemplates.value))
  }

  function applyTemplate(template: LayoutTemplate) {
    applySnapshot(template.snapshot)
    pushSnapshot()
  }

  // ============================================
  // 面板位置配置
  // ============================================
  const panelConfigs = ref<Map<string, PanelConfig>>(new Map())

  interface FloatingPanelRef {
    id: string
    api: { close(): void }
  }

  // ============================================
  // 浮动窗口列表
  // ============================================
  // IDockviewPanel from dockview-core has type incompatibility with dockview-vue runtime objects;
  // panel.group has additional private members (_model, _explicitConstraints, etc.)
  const floatingPanels = ref<FloatingPanelRef[]>([])

  // ============================================
  // 钉住的面板 ID 集合
  // ============================================
  const pinnedPanelIds = ref<Set<string>>(new Set())

  // ============================================
  // Dockview 布局数据（完全托管）
  // ============================================
  const layoutData = ref<unknown | null>(null)

  // ============================================
  // 底部 Panel 模式: 'editor' (仅B区下方) | 'full' (横跨全宽)
  // ============================================
  const bottomPanelMode = ref<'editor' | 'full'>('editor')
  const openPanelIds = ref<string[]>([])

  // ============================================
  // 面板布局模式: 'tabs' | 'vertical-split'
  // ============================================
  const leftPanelLayoutMode = ref<'tabs' | 'vertical-split'>('tabs')
  const rightPanelLayoutMode = ref<'tabs' | 'vertical-split'>('tabs')

  // ============================================
  // 计算属性
  // ============================================
  const leftSidebarVisible = computed(
    () => leftActivityBarVisible.value || primarySideBarVisible.value
  )
  const rightSidebarVisible = computed(
    () => rightActivityBarVisible.value || secondarySideBarVisible.value
  )

  const leftContentVisible = computed(
    () => primarySideBarVisible.value && primarySideBarExpanded.value
  )
  const rightContentVisible = computed(
    () => secondarySideBarVisible.value && secondarySideBarExpanded.value
  )

  const currentLeftComponentId = computed(() => selectedLeftItem.value || null)
  const currentRightComponentId = computed(() => selectedRightItem.value || null)

  // ============================================
  // 方法 - Dockview API
  // ============================================
  function setDockviewApi(api: DockviewApi) {
    dockviewApi.value = api
    // eslint-disable-next-line no-console
    console.debug('[LayoutStore] Dockview API registered')
  }

  function collapseLeftEdgeGroup() {
    pushSnapshot()
    leftEdgeGroupCollapsed.value = true
    dockviewApi.value?.getEdgeGroup?.('left')?.collapse()
  }

  function expandLeftEdgeGroup() {
    pushSnapshot()
    leftEdgeGroupCollapsed.value = false
    dockviewApi.value?.getEdgeGroup?.('left')?.expand()
  }

  function collapseRightEdgeGroup() {
    pushSnapshot()
    rightEdgeGroupCollapsed.value = true
    dockviewApi.value?.getEdgeGroup?.('right')?.collapse()
  }

  function expandRightEdgeGroup() {
    pushSnapshot()
    rightEdgeGroupCollapsed.value = false
    dockviewApi.value?.getEdgeGroup?.('right')?.expand()
  }

  /**
   * 切换左侧 Edge Group 折叠状态
   */
  function toggleLeftEdgeGroup() {
    if (leftEdgeGroupCollapsed.value) {
      expandLeftEdgeGroup()
    } else {
      collapseLeftEdgeGroup()
    }
  }

  /**
   * 打开设置页面
   */
  function openCustomizeLayoutDialog() {
    showCustomizeLayoutDialog.value = true
  }

  /**
   * 关闭设置页面
   */
  function closeCustomizeLayoutDialog() {
    showCustomizeLayoutDialog.value = false
  }

  /**
   * 设置布局数据（由 dockview 完全托管）
   */
  function setLayoutData(data: SerializedDockviewLayout | null) {
    layoutData.value = data
    saveLayoutConfig()

    const appStore = useAppStore()
    if (appStore.projectOpen && data) {
      appStore.saveDockviewLayout(data as SerializedDockviewLayout).catch(e => {
        console.warn('[LayoutStore] Failed to save dockview layout:', e)
      })
    }
  }

  // ============================================
  // 方法 - 面板管理
  // ============================================

  /**
   * 获取面板位置配置
   */
  function getPanelConfig(panelId: string): PanelConfig | undefined {
    return panelConfigs.value.get(panelId)
  }

  /**
   * 更新面板位置配置
   */
  function updatePanelConfig(panelId: string, config: Partial<PanelConfig>) {
    const current = panelConfigs.value.get(panelId) || {
      location: 'center' as PanelLocation,
      isVisible: true,
      order: 0,
    }
    panelConfigs.value.set(panelId, { ...current, ...config })
  }

  /**
   * 激活面板
   */
  function activatePanel(panelId: string) {
    if (!dockviewApi.value) {
      console.warn('[LayoutStore] Dockview API not available')
      return
    }

    const panel = dockviewApi.value.getPanel(panelId)
    if (panel) {
      try {
        panel.api.setActive()
        // eslint-disable-next-line no-console
        console.debug('[LayoutStore] Activated panel:', panelId)
      } catch (e) {
        console.warn('[LayoutStore] Failed to activate panel:', e)
      }
    } else {
      console.warn('[LayoutStore] Panel not found:', panelId)
    }
  }

  /**
   * 移动面板到指定位置
   */
  function movePanelToLocation(panelId: string, location: PanelLocation) {
    if (!dockviewApi.value) {
      console.warn('[LayoutStore] Dockview API not available')
      return
    }

    const panel = dockviewApi.value.getPanel(panelId)
    if (!panel) {
      console.warn('[LayoutStore] Panel not found:', panelId)
      return
    }

    const currentConfig = panelConfigs.value.get(panelId)
    if (currentConfig?.location === location) {
      // eslint-disable-next-line no-console
      console.debug('[LayoutStore] Panel already at location:', location)
      return
    }

    // ============================================
    // dockview 6.0: 通过 panel.api.moveTo() 将面板移动到目标位置的 group
    // ============================================

    const api = dockviewApi.value
    const targetGroup = api.groups.find(g => {
      const groupPanelIds = g.panels.map(p => p.id)
      return groupPanelIds.some(id => {
        const cfg = panelConfigs.value.get(id)
        return cfg?.location === location
      })
    })

    if (targetGroup) {
      panel.api.moveTo({ group: targetGroup })
      panelConfigs.value.set(panelId, {
        ...(currentConfig || {
          id: panelId,
          title: panel.id,
          icon: null,
          isVisible: true,
          order: 0,
        }),
        location,
      })
      // eslint-disable-next-line no-console
      console.debug('[LayoutStore] Panel moved to location:', location)
    } else {
      console.warn('[LayoutStore] No target group found for location:', location)
    }
  }

  /**
   * 获取第一个中心面板 ID
   */
  function _getFirstCenterPanelId(): string | undefined {
    const centerPanels = dockviewApi.value?.panels?.filter(
      p => panelConfigs.value.get(p.id)?.location === 'center'
    )
    return centerPanels?.[0]?.id
  }

  /**
   * 获取所有面板
   */
  function getAllPanels(): IDockviewPanel[] {
    return dockviewApi.value?.panels || []
  }

  /**
   * 获取指定位置的所有面板
   */
  function getPanelsByLocation(location: PanelLocation): IDockviewPanel[] {
    return getAllPanels().filter(p => panelConfigs.value.get(p.id)?.location === location)
  }

  /**
   * 创建浮动面板
   */
  function createFloatingPanel(
    panelId: string,
    coordinates?: { x?: number; y?: number; width?: number; height?: number }
  ) {
    if (!dockviewApi.value) {
      console.warn('[LayoutStore] Dockview API not available')
      return
    }

    const existingPanel = dockviewApi.value.getPanel(panelId)
    if (!existingPanel) {
      console.warn('[LayoutStore] Panel not found:', panelId)
      return
    }

    // ============================================
    // dockview 6.0: 通过 DockviewApi.addFloatingGroup() 创建浮动面板
    // ============================================

    dockviewApi.value.addFloatingGroup(existingPanel, {
      x: coordinates?.x,
      y: coordinates?.y,
      height: coordinates?.height,
      width: coordinates?.width,
    })

    floatingPanels.value.push(existingPanel)
    // eslint-disable-next-line no-console
    console.debug('[LayoutStore] Panel floated:', panelId)
  }

  /**
   * 关闭浮动面板
   */
  function closeFloatingPanel(panelId: string) {
    if (!dockviewApi.value) {
      console.warn('[LayoutStore] Dockview API not available')
      return
    }

    const panel = floatingPanels.value.find(p => p.id === panelId)
    if (panel) {
      panel.api.close()
      floatingPanels.value = floatingPanels.value.filter(p => p.id !== panelId)
      panelConfigs.value.delete(panelId)
      // eslint-disable-next-line no-console
      console.debug('[LayoutStore] Closed floating panel:', panelId)
    }
  }

  /**
   * 获取浮动面板列表
   */
  function getFloatingPanels(): IDockviewPanel[] {
    return floatingPanels.value as IDockviewPanel[]
  }

  /**
   * 检查面板是否被钉住
   */
  function isPanelPinned(panelId: string): boolean {
    return pinnedPanelIds.value.has(panelId)
  }

  /**
   * 切换面板钉住状态
   */
  function togglePanelPinned(panelId: string): boolean {
    if (pinnedPanelIds.value.has(panelId)) {
      pinnedPanelIds.value.delete(panelId)
      return false
    } else {
      pinnedPanelIds.value.add(panelId)
      return true
    }
  }

  /**
   * 获取所有钉住的面板 ID
   */
  function getPinnedPanelIds(): Set<string> {
    return pinnedPanelIds.value
  }

  // ============================================
  // 方法 - 可见性切换
  // ============================================
  function toggleMenuBar() {
    pushSnapshot()
    menuBarVisible.value = !menuBarVisible.value
  }

  function toggleLeftActivityBar() {
    leftActivityBarVisible.value = !leftActivityBarVisible.value
  }

  function toggleRightActivityBar() {
    rightActivityBarVisible.value = !rightActivityBarVisible.value
  }

  function togglePrimarySideBar() {
    primarySideBarVisible.value = !primarySideBarVisible.value

    // 切换左侧面板组的可见性
    // 注意：DockviewApi 没有直接的 setVisible 方法，这里通过关闭/重新添加面板来实现
    // 或者使用 CSS 控制侧边栏容器的显示/隐藏
    // eslint-disable-next-line no-console
    console.debug('[LayoutStore] Primary sidebar visibility toggled:', primarySideBarVisible.value)
  }

  function toggleSecondarySideBar() {
    secondarySideBarVisible.value = !secondarySideBarVisible.value

    // 切换右侧面板组的可见性
    // eslint-disable-next-line no-console
    console.debug(
      '[LayoutStore] Secondary sidebar visibility toggled:',
      secondarySideBarVisible.value
    )
  }

  function togglePanel() {
    panelVisible.value = !panelVisible.value
  }

  function toggleStatusBar() {
    pushSnapshot()
    statusBarVisible.value = !statusBarVisible.value
  }

  function setBottomPanelMode(mode: 'editor' | 'full') {
    bottomPanelMode.value = mode
  }

  function setBottomInsightTab(tab: 'multi' | 'history' | 'schema' | 'table') {
    bottomInsightTab.value = tab
  }

  function setOpenPanelIds(ids: string[]) {
    openPanelIds.value = ids
  }

  function toggleBottomPanelMode() {
    bottomPanelMode.value = bottomPanelMode.value === 'editor' ? 'full' : 'editor'
    saveLayoutConfig()
  }

  // ============================================
  // 方法 - 选择活动项
  // ============================================
  function selectLeftItem(id: string) {
    if (selectedLeftItem.value === id) {
      primarySideBarExpanded.value = !primarySideBarExpanded.value
    } else {
      selectedLeftItem.value = id
      primarySideBarExpanded.value = true
    }

    // 激活对应的 dockview 面板
    const panelId = ACTIVEBAR_TO_PANEL_ID[id]
    if (panelId) {
      activatePanelByRegistryId(panelId, 'left')
    }
  }

  function selectRightItem(id: string) {
    if (selectedRightItem.value === id) {
      secondarySideBarExpanded.value = !secondarySideBarExpanded.value
    } else {
      selectedRightItem.value = id
      secondarySideBarExpanded.value = true
    }

    // 激活对应的 dockview 面板
    const panelId = ACTIVEBAR_TO_PANEL_ID[id]
    if (panelId) {
      activatePanelByRegistryId(panelId, 'right')
    }
  }

  /**
   * 通过面板注册表 ID 激活面板
   */
  function activatePanelByRegistryId(registryId: string, _location: PanelLocation) {
    if (!dockviewApi.value) {
      console.warn('[LayoutStore] Dockview API not available')
      return
    }

    const panelId = `panel_${registryId}`
    const panel = dockviewApi.value.getPanel(panelId)

    if (panel) {
      // 面板已存在，激活它
      try {
        panel.api.setActive()
        // eslint-disable-next-line no-console
        console.debug('[LayoutStore] Activated panel:', panelId)
      } catch (e) {
        console.warn('[LayoutStore] Failed to activate panel:', e)
      }
    } else {
      console.warn('[LayoutStore] Panel not found, cannot activate:', panelId)
    }
  }

  // ============================================
  // 方法 - 尺寸设置
  // ============================================
  function setPrimarySideBarWidth(width: number) {
    primarySideBarWidth.value = Math.max(MIN_SIDEBAR_WIDTH, Math.min(width, MAX_SIDEBAR_WIDTH))
  }

  function setSecondarySideBarWidth(width: number) {
    secondarySideBarWidth.value = Math.max(MIN_SIDEBAR_WIDTH, Math.min(width, MAX_SIDEBAR_WIDTH))
  }

  function setPanelHeight(height: number) {
    panelHeight.value = Math.max(100, Math.min(height, 600))
  }

  // ============================================
  // 方法 - 重置布局
  // ============================================
  function resetLayout() {
    menuBarVisible.value = true
    leftActivityBarVisible.value = true
    rightActivityBarVisible.value = true
    primarySideBarVisible.value = true
    secondarySideBarVisible.value = true
    panelVisible.value = true
    statusBarVisible.value = true
    primarySideBarExpanded.value = true
    secondarySideBarExpanded.value = true
    selectedLeftItem.value = 'database'
    selectedRightItem.value = 'column-insights'
    primarySideBarWidth.value = DEFAULT_PRIMARY_SIDEBAR_WIDTH
    secondarySideBarWidth.value = DEFAULT_SECONDARY_SIDEBAR_WIDTH
    panelHeight.value = DEFAULT_PANEL_HEIGHT
    panelConfigs.value.clear()
    floatingPanels.value = []
  }

  // ============================================
  // 方法 - 完整的 Customize Layout
  // ============================================
  function setLayoutVisibility(visibility: Partial<LayoutVisibility>) {
    if (visibility.menuBar !== undefined) menuBarVisible.value = visibility.menuBar
    if (visibility.activityBar !== undefined) {
      leftActivityBarVisible.value = visibility.activityBar
      rightActivityBarVisible.value = visibility.activityBar
    }
    if (visibility.primarySideBar !== undefined)
      primarySideBarVisible.value = visibility.primarySideBar
    if (visibility.secondarySideBar !== undefined)
      secondarySideBarVisible.value = visibility.secondarySideBar
    if (visibility.panel !== undefined) panelVisible.value = visibility.panel
    if (visibility.statusBar !== undefined) statusBarVisible.value = visibility.statusBar
  }

  // ============================================
  // 方法 - 保存/加载布局配置
  // ============================================
  function saveLayoutConfig() {
    try {
      const config = {
        visibility: {
          menuBarVisible: menuBarVisible.value,
          leftActivityBarVisible: leftActivityBarVisible.value,
          rightActivityBarVisible: rightActivityBarVisible.value,
          primarySideBarVisible: primarySideBarVisible.value,
          secondarySideBarVisible: secondarySideBarVisible.value,
          panelVisible: panelVisible.value,
          statusBarVisible: statusBarVisible.value,
          primarySideBarExpanded: primarySideBarExpanded.value,
          secondarySideBarExpanded: secondarySideBarExpanded.value,
        },
        selection: {
          selectedLeftItem: selectedLeftItem.value,
          selectedRightItem: selectedRightItem.value,
        },
        sizes: {
          primarySideBarWidth: primarySideBarWidth.value,
          secondarySideBarWidth: secondarySideBarWidth.value,
          panelHeight: panelHeight.value,
        },
        panelConfigs: Object.fromEntries(panelConfigs.value),
        layoutData: layoutData.value,
        bottomInsightTab: bottomInsightTab.value,
        timestamp: Date.now(),
      }
      localStorage.setItem('rdata_station_layout_config', JSON.stringify(config))

      const appStore = useAppStore()
      if (appStore.projectOpen) {
        const sidebarState: SerializedSidebarState = {
          leftActivityBarVisible: leftActivityBarVisible.value,
          rightActivityBarVisible: rightActivityBarVisible.value,
          primarySideBarVisible: primarySideBarVisible.value,
          secondarySideBarVisible: secondarySideBarVisible.value,
          panelVisible: panelVisible.value,
          statusBarVisible: statusBarVisible.value,
          primarySideBarExpanded: primarySideBarExpanded.value,
          secondarySideBarExpanded: secondarySideBarExpanded.value,
          selectedLeftItem: selectedLeftItem.value,
          selectedRightItem: selectedRightItem.value,
          primarySideBarWidth: primarySideBarWidth.value,
          secondarySideBarWidth: secondarySideBarWidth.value,
          panelHeight: panelHeight.value,
          bottomPanelMode: bottomPanelMode.value,
          openPanelIds: openPanelIds.value,
          bottomInsightTab: bottomInsightTab.value,
        }
        appStore.saveSidebarState(sidebarState).catch(e => {
          console.warn('[LayoutStore] Failed to save sidebar state:', e)
        })
      }
    } catch (error) {
      console.error('[LayoutStore] Failed to save layout config:', error)
    }
  }

  function loadLayoutConfig() {
    const appStore = useAppStore()

    if (appStore.projectOpen && appStore.effectiveSidebarState) {
      const state = appStore.effectiveSidebarState
      leftActivityBarVisible.value = state.leftActivityBarVisible ?? true
      rightActivityBarVisible.value = state.rightActivityBarVisible ?? true
      primarySideBarVisible.value = state.primarySideBarVisible ?? true
      secondarySideBarVisible.value = state.secondarySideBarVisible ?? true
      panelVisible.value = state.panelVisible ?? true
      statusBarVisible.value = state.statusBarVisible ?? true
      primarySideBarExpanded.value = state.primarySideBarExpanded ?? true
      secondarySideBarExpanded.value = state.secondarySideBarExpanded ?? true
      selectedLeftItem.value = state.selectedLeftItem ?? 'database'
      selectedRightItem.value = state.selectedRightItem ?? 'column-insights'
      primarySideBarWidth.value = state.primarySideBarWidth ?? DEFAULT_PRIMARY_SIDEBAR_WIDTH
      secondarySideBarWidth.value = state.secondarySideBarWidth ?? DEFAULT_SECONDARY_SIDEBAR_WIDTH
      panelHeight.value = state.panelHeight ?? DEFAULT_PANEL_HEIGHT
      bottomPanelMode.value = state.bottomPanelMode ?? 'editor'
      openPanelIds.value = state.openPanelIds ?? []
      bottomInsightTab.value = state.bottomInsightTab ?? 'multi'
      return
    }

    try {
      const stored = localStorage.getItem('rdata_station_layout_config')
      if (stored) {
        const config = JSON.parse(stored)
        const age = Date.now() - config.timestamp
        const maxAge = 30 * 24 * 60 * 60 * 1000

        if (age < maxAge) {
          if (config.visibility) {
            menuBarVisible.value = config.visibility.menuBarVisible ?? true
            leftActivityBarVisible.value = config.visibility.leftActivityBarVisible ?? true
            rightActivityBarVisible.value = config.visibility.rightActivityBarVisible ?? true
            primarySideBarVisible.value = config.visibility.primarySideBarVisible ?? true
            secondarySideBarVisible.value = config.visibility.secondarySideBarVisible ?? true
            panelVisible.value = config.visibility.panelVisible ?? true
            statusBarVisible.value = config.visibility.statusBarVisible ?? true
            primarySideBarExpanded.value = config.visibility.primarySideBarExpanded ?? true
            secondarySideBarExpanded.value = config.visibility.secondarySideBarExpanded ?? true
          }
          if (config.selection) {
            selectedLeftItem.value = config.selection.selectedLeftItem ?? 'database'
            selectedRightItem.value = config.selection.selectedRightItem ?? 'column-insights'
          }
          if (config.sizes) {
            primarySideBarWidth.value =
              config.sizes.primarySideBarWidth ?? DEFAULT_PRIMARY_SIDEBAR_WIDTH
            secondarySideBarWidth.value =
              config.sizes.secondarySideBarWidth ?? DEFAULT_SECONDARY_SIDEBAR_WIDTH
            panelHeight.value = config.sizes.panelHeight ?? DEFAULT_PANEL_HEIGHT
          }
          if (config.bottomInsightTab) {
            bottomInsightTab.value = config.bottomInsightTab
          }
        }
      }
    } catch (error) {
      console.error('[LayoutStore] Failed to load layout config:', error)
    }
  }

  function setLeftPanelLayoutMode(mode: 'tabs' | 'vertical-split') {
    leftPanelLayoutMode.value = mode
  }

  function setRightPanelLayoutMode(mode: 'tabs' | 'vertical-split') {
    rightPanelLayoutMode.value = mode
  }

  // ============================================
  // 返回
  // ============================================
  return {
    // 可见性状态
    menuBarVisible,
    leftActivityBarVisible,
    rightActivityBarVisible,
    primarySideBarVisible,
    secondarySideBarVisible,
    panelVisible,
    statusBarVisible,

    // 侧边栏锁定状态
    primarySideBarLocked,
    secondarySideBarLocked,

    // 选中状态
    selectedLeftItem,
    selectedRightItem,

    // 展开状态
    primarySideBarExpanded,
    secondarySideBarExpanded,

    // 尺寸状态
    primarySideBarWidth,
    secondarySideBarWidth,
    panelHeight,

    // 面板布局模式
    leftPanelLayoutMode,
    rightPanelLayoutMode,

    // 计算属性
    leftSidebarVisible,
    rightSidebarVisible,
    leftContentVisible,
    rightContentVisible,
    currentLeftComponentId,
    currentRightComponentId,

    // 活动栏项目
    leftActivityItems,
    rightActivityItems,

    // Dockview API
    dockviewApi,

    // 面板配置
    panelConfigs,
    floatingPanels,
    pinnedPanelIds,

    // Dockview 布局数据
    layoutData,

    // Edge Group 折叠状态
    leftEdgeGroupCollapsed,
    rightEdgeGroupCollapsed,

    // 设置页面
    showCustomizeLayoutDialog,

    // 方法 - API
    setDockviewApi,
    setLayoutData,
    collapseLeftEdgeGroup,
    expandLeftEdgeGroup,
    collapseRightEdgeGroup,
    expandRightEdgeGroup,
    toggleLeftEdgeGroup,
    openCustomizeLayoutDialog,
    closeCustomizeLayoutDialog,

    // 方法 - 面板管理
    getPanelConfig,
    updatePanelConfig,
    activatePanel,
    movePanelToLocation,
    getAllPanels,
    getPanelsByLocation,
    createFloatingPanel,
    closeFloatingPanel,
    getFloatingPanels,
    isPanelPinned,
    togglePanelPinned,
    getPinnedPanelIds,

    // 方法 - 可见性
    toggleMenuBar,
    toggleLeftActivityBar,
    toggleRightActivityBar,
    togglePrimarySideBar,
    toggleSecondarySideBar,
    togglePanel,
    toggleStatusBar,

    // 底部 Panel 模式
    bottomPanelMode,
    setBottomPanelMode,
    toggleBottomPanelMode,
    setBottomInsightTab,
    bottomInsightTab,

    // 面板 ID 追踪
    openPanelIds,
    setOpenPanelIds,

    // 方法 - 选择
    selectLeftItem,
    selectRightItem,

    // 方法 - 尺寸
    setPrimarySideBarWidth,
    setSecondarySideBarWidth,
    setPanelHeight,

    // 方法 - 布局操作
    resetLayout,
    setLayoutVisibility,
    saveLayoutConfig,
    loadLayoutConfig,
    setLeftPanelLayoutMode,
    setRightPanelLayoutMode,

    // Undo / Redo
    canUndo,
    canRedo,
    undo,
    redo,
    pushSnapshot,
    captureSnapshot,

    // 自定义布局模板
    customTemplates,
    loadCustomTemplates,
    saveCustomTemplate,
    deleteCustomTemplate,
    applyTemplate,
  }
})
