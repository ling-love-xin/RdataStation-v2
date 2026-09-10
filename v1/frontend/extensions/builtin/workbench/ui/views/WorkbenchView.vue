<template>
  <div :class="['workbench-view', uiStore.isDark ? 'dockview-theme-dark' : 'dockview-theme-light']">
    <!-- 崩溃恢复横幅 -->
    <div v-if="showRecoveryBanner" class="recovery-banner">
      <span class="recovery-icon">⚠</span>
      <span class="recovery-text">
        检测到上次未正常关闭，有 {{ recoverySnapshots.length }} 个文件可以恢复
      </span>
      <button class="recovery-btn recovery-btn-primary" @click="handleRestoreAllRecovery">
        恢复全部
      </button>
      <button class="recovery-btn recovery-btn-dismiss" @click="handleDismissRecovery">
        忽略
      </button>
    </div>

    <!-- Dockview 布局 -->
    <!-- 面板组件通过 app.component() 全局注册，由 dockview-vue 的 findComponent 自动解析 -->
    <DockviewVue
      ref="dockviewRef"
      class="dockview"
      :popout-url="'/popout.html'"
      :floating-group-bounds="'boundedWithinViewport'"
      :right-header-actions-component="'PanelHeaderActions'"
      :default-tab-component="'IconTab'"
      :get-tab-context-menu-items="getTabContextMenuItems"
      @ready="onReady"
    />

    <!-- 自定义布局对话框 -->
    <CustomizeLayoutDialog />

    <!-- 新增数据源对话框 -->
    <AddDataSourceDialog
      v-model="showAddDataSourceDialog"
      :initial-driver="dialogInitialDriver"
      :initial-connection="dialogInitialConnection"
      @save="handleDataSourceSaved"
    />
  </div>
</template>

<script setup lang="ts">
import {
  DockviewVue,
  type DockviewReadyEvent,
  type DockviewApi as DockviewVueApi,
  type GetTabContextMenuItemsParams,
  type ContextMenuItem,
  type DockviewGroupPanel,
  type IDockviewPanel,
} from 'dockview-vue'
import { useMessage } from 'naive-ui'
import { ref, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'

import { panelRegistry } from '@/core/panel-registry'
import { useProjectStore } from '@/core/project/stores/project'
import AddDataSourceDialog from '@/extensions/builtin/connection/ui/components/AddDataSourceDialog.vue'
import { useConnectionStore } from '@/extensions/builtin/connection/ui/stores/connection-store'
import { EditorManager } from '@/extensions/builtin/workbench/manager/EditorManager'
import { ShortcutManager } from '@/extensions/builtin/workbench/manager/ShortcutManager'
import {
  PANEL_ID_EMPTY_WORKBENCH,
  isEditorPanel,
  isResultPanel,
} from '@/extensions/builtin/workbench/types/editor-types'
import CustomizeLayoutDialog from '@/extensions/builtin/workbench/ui/components/CustomizeLayoutDialog.vue'
import IconTabComponent from '@/extensions/builtin/workbench/ui/components/IconTab.vue'
import {
  WorkbenchEvent,
  listenWorkbenchEvent,
} from '@/extensions/builtin/workbench/ui/constants/workbench-events'
import { useLayoutStore } from '@/extensions/builtin/workbench/ui/stores/layout-store'
import { useUiStore } from '@/shared/stores/ui'
import type { SerializedDockviewLayout } from '@/stores/config'
import { useAppStore } from '@/stores/useAppStore'
import { useScratchpadEditorStore } from '@/stores/useScratchpadEditorStore'

interface DockviewGroupPanelAPI {
  setSize?(args: { width: number }): void
  updateConstraints?(constraints: Record<string, number>): void
}

type DockviewLayoutJSON = Record<string, unknown>

defineOptions({
  components: {
    IconTab: IconTabComponent,
  },
})

const { t } = useI18n()
const uiStore = useUiStore()
const layoutStore = useLayoutStore()
const appStore = useAppStore()
const _projectStore = useProjectStore()
const connectionStore = useConnectionStore()
const editorStore = useScratchpadEditorStore()
const message = useMessage()

const dockviewRef = ref<InstanceType<typeof DockviewVue> | null>(null)
const recoverySnapshots = ref<
  { filePath: string; fileName: string; language: string; isDirty: boolean }[]
>([])
const showRecoveryBanner = ref(false)
const showAddDataSourceDialog = ref(false)
const dialogInitialDriver = ref<
  import('@/extensions/builtin/connection/domain/types').Driver | null
>(null)
const dialogInitialConnection = ref<
  import('@/extensions/builtin/connection/types/connection').ProjectConnection | null
>(null)

let dockviewApi: DockviewVueApi | null = null

function checkRecoveryState(): void {
  if (!EditorManager.hasRecoveryData()) return

  try {
    const snapshots = EditorManager.loadRecoverySnapshots()
    if (snapshots.length > 0) {
      recoverySnapshots.value = snapshots
      showRecoveryBanner.value = true
    }
  } catch {
    console.warn('[WorkbenchView] recovery check failed')
  }
}

function handleRestoreAllRecovery(): void {
  const count = recoverySnapshots.value.length
  for (const snap of recoverySnapshots.value) {
    EditorManager.openFile({
      filePath: snap.filePath,
      fileName: snap.fileName,
      language: snap.language,
      sql: '',
      type: 'file',
    })
  }
  EditorManager.clearRecovery()
  showRecoveryBanner.value = false
  recoverySnapshots.value = []
  message.success(`已恢复 ${count} 个文件`)
}

function handleDismissRecovery(): void {
  EditorManager.clearRecovery()
  showRecoveryBanner.value = false
  recoverySnapshots.value = []
}

const getTabContextMenuItems = (params: GetTabContextMenuItemsParams): ContextMenuItem[] => {
  const panelId = params.panel.id
  const maximized = params.group.api.isMaximized()
  const isPinned = layoutStore.isPanelPinned(panelId)

  const menuItems: ContextMenuItem[] = [
    {
      label: isPinned ? t('workbench.unpin') : t('workbench.pin'),
      action: () => {
        layoutStore.togglePanelPinned(panelId)
      },
    },
    'separator',
    {
      label: 'Float Tab',
      action: () => {
        params.api.addFloatingGroup(params.panel)
      },
    },
    {
      label: 'Popout Tab',
      action: () => {
        params.api.addPopoutGroup(params.panel)
      },
    },
    'separator',
    {
      label: 'Add to new group',
      action: () => {
        const tabGroup = params.api.createTabGroup({
          groupId: params.group.id,
          label: 'New Group',
          color: 'blue',
        })
        params.api.addPanelToTabGroup({
          groupId: params.group.id,
          tabGroupId: tabGroup.id,
          panelId: params.panel.id,
        })
      },
    },
    {
      label: 'Move to next group',
      action: () => {
        const groups = params.api.getTabGroups({ groupId: params.group.id })
        const currentGroup = params.api.getTabGroupForPanel({
          groupId: params.group.id,
          panelId: params.panel.id,
        })
        if (currentGroup && groups.length > 1) {
          const currentIndex = groups.indexOf(currentGroup)
          const nextIndex = (currentIndex + 1) % groups.length
          params.api.addPanelToTabGroup({
            groupId: params.group.id,
            tabGroupId: groups[nextIndex].id,
            panelId: params.panel.id,
          })
        }
      },
    },
    'separator',
    {
      label: maximized ? t('workbench.restoreMaximize') : t('workbench.maximizeGroup'),
      action: () => {
        if (maximized) params.group.api.exitMaximized()
        else params.group.api.maximize()
      },
    },
    {
      label: t('workbench.floatGroup'),
      action: () => {
        params.api.addFloatingGroup(params.group)
      },
    },
    {
      label: t('workbench.popoutGroup'),
      action: () => {
        params.api.addPopoutGroup(params.group)
      },
    },
    'separator',
  ]

  if (!isPinned) {
    menuItems.push('close', 'closeOthers', 'closeAll')
  }

  return menuItems
}

const onReady = (event: DockviewReadyEvent) => {
  const api = event.api
  dockviewApi = api
  layoutStore.setDockviewApi(api)

  EditorManager.init(api)

  // 跨窗口通信监听（popout 合并回主窗口）
  EditorManager.setupCrossWindowListeners()

  // 崩溃恢复检测
  checkRecoveryState()

  ShortcutManager.register(
    'Ctrl+B',
    'global',
    () => {
      const leftGroup = api.getEdgeGroup('left')
      if (leftGroup?.isCollapsed()) {
        layoutStore.expandLeftEdgeGroup()
      } else {
        layoutStore.collapseLeftEdgeGroup()
      }
    },
    '切换左侧边栏'
  )

  ShortcutManager.register(
    'Ctrl+Shift+B',
    'global',
    () => {
      const rightGroup = api.getEdgeGroup('right')
      if (rightGroup?.isCollapsed()) {
        layoutStore.expandRightEdgeGroup()
      } else {
        layoutStore.collapseRightEdgeGroup()
      }
    },
    '切换右侧边栏'
  )

  ShortcutManager.register(
    'Escape',
    'global',
    () => {
      const activeEl = document.activeElement
      if (activeEl && (activeEl.tagName === 'INPUT' || activeEl.tagName === 'TEXTAREA')) return
      const leftGroup = api.getEdgeGroup('left')
      if (leftGroup?.isMaximized()) {
        leftGroup.exitMaximized()
        return
      }
      const rightGroup = api.getEdgeGroup('right')
      if (rightGroup?.isMaximized()) {
        rightGroup.exitMaximized()
      }
    },
    '退出最大化面板'
  )

  const panels = panelRegistry.getAll()
  // eslint-disable-next-line no-console
  console.debug(`[Workbench] Creating ${panels.length} panels from registry`)

  const leftPanelsAll = panels
    .filter(p => p.location === 'left')
    .sort((a, b) => (a.order || 0) - (b.order || 0))
  const centerPanels = panels
    .filter(p => p.location === 'center')
    .sort((a, b) => (a.order || 0) - (b.order || 0))
  const rightPanels = panels
    .filter(p => p.location === 'right')
    .sort((a, b) => (a.order || 0) - (b.order || 0))

  const containerEl = dockviewRef.value?.$el as HTMLElement | undefined
  const totalWidth = containerEl?.clientWidth || 1200
  const oneQuarter = Math.round(totalWidth * 0.25)

  // ============================================
  // 第 1 步：A 栏 — 左侧 Edge Group（面板直接作为 tab 列在标题区）
  //   [草稿箱, 数据库导航, 分析资源, 插件]
  //   初始展开，A:B:C = 1:2:1
  // ============================================
  api.addEdgeGroup('left', {
    id: 'left-edge',
    initialSize: oneQuarter,
    minimumSize: 48,
    maximumSize: Math.round(totalWidth * 0.4),
  })

  if (leftPanelsAll.length > 0) {
    const firstLeftPanel = leftPanelsAll[0]
    const firstLeftPanelId = `panel_${firstLeftPanel.id}`
    api.addPanel({
      id: firstLeftPanelId,
      component: firstLeftPanel.id,
      title: firstLeftPanel.name,
      position: { referenceGroup: 'left-edge' },
      renderer: 'onlyWhenVisible',
    })
    layoutStore.updatePanelConfig(firstLeftPanelId, { location: 'left', isVisible: true, order: 0 })

    for (let i = 1; i < leftPanelsAll.length; i++) {
      const panel = leftPanelsAll[i]
      const panelId = `panel_${panel.id}`
      api.addPanel({
        id: panelId,
        component: panel.id,
        title: panel.name,
        position: { referencePanel: firstLeftPanelId, direction: 'within' },
        renderer: 'onlyWhenVisible',
      })
      layoutStore.updatePanelConfig(panelId, { location: 'left', isVisible: true, order: i })
    }
  }
  // eslint-disable-next-line no-console
  console.debug(`[Workbench] Created left edge group with ${leftPanelsAll.length} panels`)

  // ============================================
  // 第 2 步：B 栏 — 中心 Normal Group（主工作区）
  //   初始: Welcome Page，动态: SQL Editor / Query Result
  // ============================================
  const welcomePanel = centerPanels.find(p => p.id === 'emptyWorkbench')
  if (welcomePanel) {
    api.addPanel({
      id: `panel_${welcomePanel.id}`,
      component: welcomePanel.id,
      title: welcomePanel.name,
      position: { direction: 'right' },
    })
    // eslint-disable-next-line no-console
    console.debug(`[Workbench] Created welcome panel: panel_${welcomePanel.id}`)
    layoutStore.updatePanelConfig(`panel_${welcomePanel.id}`, {
      location: 'center',
      isVisible: true,
      order: 0,
    })
  }

  // ============================================
  // 第 3 步：C 栏 — 右侧 Edge Group（面板直接作为 tab 列在标题区）
  //   [列洞察, Mock数据, SQL历史]
  //   初始展开，A:B:C = 1:2:1
  // ============================================
  if (rightPanels.length > 0) {
    api.addEdgeGroup('right', {
      id: 'right-edge',
      initialSize: oneQuarter,
      minimumSize: 48,
      maximumSize: Math.round(totalWidth * 0.4),
    })

    const firstRightPanel = rightPanels[0]
    const firstRightPanelId = `panel_${firstRightPanel.id}`
    api.addPanel({
      id: firstRightPanelId,
      component: firstRightPanel.id,
      title: firstRightPanel.name,
      position: { referenceGroup: 'right-edge' },
      renderer: 'onlyWhenVisible',
    })
    layoutStore.updatePanelConfig(firstRightPanelId, {
      location: 'right',
      isVisible: true,
      order: 0,
    })

    for (let i = 1; i < rightPanels.length; i++) {
      const panel = rightPanels[i]
      const panelId = `panel_${panel.id}`
      api.addPanel({
        id: panelId,
        component: panel.id,
        title: panel.name,
        position: { referencePanel: firstRightPanelId, direction: 'within' },
        renderer: 'onlyWhenVisible',
      })
      layoutStore.updatePanelConfig(panelId, { location: 'right', isVisible: true, order: i })
    }
  }
  // eslint-disable-next-line no-console
  console.debug(`[Workbench] Created right edge group with ${rightPanels.length} panels`)

  // eslint-disable-next-line no-console
  console.debug(
    `[Workbench] Final layout - groups: ${api.groups.length}, panels: ${api.panels.length}`
  )
  // eslint-disable-next-line no-console
  console.debug(
    '[Workbench] All groups:',
    api.groups.map(g => ({
      id: g.id,
      panels: g.panels.map(p => p.id),
    }))
  )

  layoutStore.setBottomPanelMode('editor')

  // A 和 C 初始均展开，不调用 collapse

  // ============================================
  // 事件监听
  // ============================================
  api.onDidActivePanelChange?.(panel => {
    if (!panel) {
      ShortcutManager.setActiveScope('none')
      return
    }
    if (isEditorPanel(panel.id)) {
      const filePath = EditorManager.panelIdToFilePath(panel.id)
      if (filePath) {
        EditorManager.switchToFile(filePath)
      }
    } else if (isResultPanel(panel.id)) {
      ShortcutManager.setActiveScope('result')
    } else if (panel.id === 'scratchpad' || panel.id.startsWith('panel_scratchpad')) {
      ShortcutManager.setActiveScope('scratchpad')
    } else {
      ShortcutManager.setActiveScope('none')
    }
  })

  api.onDidRemovePanel?.(panel => {
    const panelId = panel.id
    if (layoutStore.isPanelPinned(panelId)) {
      // eslint-disable-next-line no-console
      console.debug(`[Workbench] Attempted to close pinned panel: ${panelId}, restoring...`)
      setTimeout(() => {
        restorePinnedPanel(panelId)
      }, 100)
    }
    if (!panelId.startsWith('panel_editor_')) {
      editorStore.removeByPanelId(panelId)
    }

    if (panelId.startsWith('panel_editor_')) {
      const filePath = EditorManager.panelIdToFilePath(panelId)
      if (filePath) {
        EditorManager.closeFile(filePath)
        // eslint-disable-next-line no-console
        console.debug(`[Workbench] 编辑器面板关闭，清理 Model: ${filePath}`)
      }
    }
  })

  api.onDidLayoutChange?.(() => {
    try {
      const serialized = api.toJSON() as unknown as DockviewLayoutJSON
      layoutStore.setLayoutData(serialized as unknown as SerializedDockviewLayout)

      const ids = api.panels.map((p: { id: string }) => p.id)
      layoutStore.setOpenPanelIds(ids)
    } catch {
      console.warn('[WorkbenchView] dockview serialization failed')
    }
  })

  api.onDidMovePanel?.(e => {
    const movedPanelId = e.panel.id
    if (movedPanelId.startsWith('panel_editor_')) {
      const targetGroupId = e.panel.group?.id
      if (targetGroupId) {
        EditorManager.updatePanelGroup(movedPanelId, targetGroupId)
        // eslint-disable-next-line no-console
        console.debug(`[Workbench] Editor panel ${movedPanelId} moved to group ${targetGroupId}`)
      }
    }
  })

  api.onDidAddGroup?.((group: DockviewGroupPanel) => {
    for (const panel of group.panels) {
      const pid = (panel as IDockviewPanel).id
      if (pid.startsWith('panel_editor_')) {
        EditorManager.updatePanelGroup(pid, group.id)
      }
    }
  })

  api.onDidRemoveGroup?.((group: DockviewGroupPanel) => {
    const editorPanels = group.panels.filter((p: IDockviewPanel) =>
      p.id.startsWith('panel_editor_')
    )
    if (editorPanels.length > 0) {
      for (const panel of editorPanels) {
        EditorManager.onPanelUndocked(panel.id)
      }
    }
  })

  restoreSavedPanels()
}

function restoreSavedPanels() {
  if (!dockviewApi) return

  const savedIds = layoutStore.openPanelIds
  if (!savedIds || savedIds.length === 0) return

  const currentIds = new Set(dockviewApi.panels.map((p: { id: string }) => p.id))

  for (const panelId of savedIds) {
    if (currentIds.has(panelId)) continue

    const componentKey = panelId.replace(/^panel_/, '').replace(/_\d+$/, '')
    const panelInfo = panelRegistry.get(componentKey)
    if (!panelInfo) continue

    try {
      dockviewApi.addPanel({
        id: panelId,
        component: panelInfo.id,
        title: panelInfo.name,
      })
    } catch {
      console.warn('[WorkbenchView] panel restore failed')
    }
  }
}

function restorePinnedPanel(panelId: string) {
  if (!dockviewApi) return

  const panelConfig = layoutStore.getPanelConfig(panelId)
  if (!panelConfig) return

  const panelInfo = panelRegistry.get(panelId.replace('panel_', ''))
  if (!panelInfo) return

  dockviewApi.addPanel({
    id: panelId,
    component: panelInfo.id,
    title: panelInfo.name,
    position: {
      direction:
        panelConfig.location === 'left'
          ? 'left'
          : panelConfig.location === 'right'
            ? 'right'
            : 'center',
    },
  })
  // eslint-disable-next-line no-console
  console.debug(`[Workbench] Restored pinned panel: ${panelId}`)
}

const handleLayoutSettingsUpdate = (_event: CustomEvent) => {
  const { leftWidth, rightWidth, minimumWidth, maximumWidth } = _event.detail || {}
  if (!dockviewApi) return

  try {
    const groups = dockviewApi.groups || []

    if (leftWidth !== undefined) {
      const leftGroup = groups.find((g: { model?: { panels?: { id: string }[] } }) =>
        g.model?.panels?.some((p: { id: string }) => {
          const panelId = p.id
          return (
            panelId === 'panel_databaseNavigator' ||
            panelId === 'panel_analytics-resource-manager' ||
            panelId === 'panel_plugins'
          )
        })
      )
      if (leftGroup) {
        const g = leftGroup as unknown as DockviewGroupPanelAPI
        g.setSize?.({ width: leftWidth })
      }
    }

    if (rightWidth !== undefined) {
      const rightGroup = groups.find((g: { model?: { panels?: { id: string }[] } }) =>
        g.model?.panels?.some((p: { id: string }) => {
          const panelId = p.id
          return panelId === 'panel_sqlHistory' || panelId === 'panel_columnInsights'
        })
      )
      if (rightGroup) {
        const g = rightGroup as unknown as DockviewGroupPanelAPI
        g.setSize?.({ width: rightWidth })
      }
    }

    if (minimumWidth !== undefined || maximumWidth !== undefined) {
      for (const group of groups) {
        const constraints: Record<string, number> = {}
        if (minimumWidth !== undefined) constraints.minimumWidth = minimumWidth
        if (maximumWidth !== undefined) constraints.maximumWidth = maximumWidth
        const g = group as unknown as DockviewGroupPanelAPI
        g.updateConstraints?.(constraints)
      }
    }
  } catch (error) {
    console.error('[Workbench] Failed to apply layout settings:', error)
  }
}

const handleResetLayout = () => {
  if (!dockviewApi) return
  try {
    const allGroups = dockviewApi.groups || []
    for (const group of allGroups) {
      const g = group as { model?: { panels?: { id: string }[] } }
      const panels = g.model?.panels || []
      for (const panelInfo of panels) {
        const panel = dockviewApi.getPanel?.(panelInfo.id)
        panel?.api?.close?.()
      }
    }
    setTimeout(() => {
      onReady({ api: dockviewApi } as unknown as DockviewReadyEvent)
    }, 100)
  } catch (error) {
    console.error('[Workbench] Failed to reset layout:', error)
  }
}

function findCenterGroupReference():
  | { direction: 'right' }
  | { referencePanel: string; direction: 'within' } {
  if (!dockviewApi) return { direction: 'right' }

  for (const group of dockviewApi.groups || []) {
    for (const panel of group.model?.panels || []) {
      if (panel.id?.startsWith('panel_editor_')) {
        return { referencePanel: panel.id, direction: 'within' }
      }
    }
  }

  const welcomePanel = dockviewApi.getPanel(PANEL_ID_EMPTY_WORKBENCH)
  if (welcomePanel) {
    return { referencePanel: PANEL_ID_EMPTY_WORKBENCH, direction: 'within' }
  }

  for (const group of dockviewApi.groups || []) {
    if (group.id !== 'left-edge' && group.id !== 'right-edge') {
      const firstPanel = group.model?.panels?.[0]
      if (firstPanel) {
        return { referencePanel: firstPanel.id, direction: 'within' }
      }
    }
  }

  return { direction: 'right' }
}

/**
 * 打开 SQL 编辑器。
 * - 有 scratchpadRelativePath → 委托 EditorManager.openFile（新架构）
 * - 无 scratchpadRelativePath  → 委托 EditorManager.openNewQuery（自动创建草稿文件）
 */
const handleOpenSqlEditor = (event: CustomEvent) => {
  const { connectionId, databaseName, sql, scratchpadRelativePath, scratchpadFileName, language } =
    event.detail || {}

  if (scratchpadRelativePath) {
    const resolvedLang = language || (scratchpadFileName?.endsWith('.sql') ? 'sql' : 'plaintext')
    const fileName = scratchpadFileName || scratchpadRelativePath.split('/').pop() || 'Untitled'

    EditorManager.openFile({
      filePath: scratchpadRelativePath,
      fileName,
      language: resolvedLang,
      sql: sql || '',
      type: 'file',
      connectionId: connectionId || '',
      databaseName: databaseName || '',
    })

    const welcomePanel = dockviewApi?.getPanel(PANEL_ID_EMPTY_WORKBENCH)
    if (welcomePanel) welcomePanel.api.close()
    // eslint-disable-next-line no-console
    console.debug(`[Workbench] 打开草稿箱文件: ${scratchpadRelativePath}`)
    return
  }

  EditorManager.openNewQuery(connectionId || '', databaseName || '')
    .then(() => {
      const welcomePanel = dockviewApi?.getPanel(PANEL_ID_EMPTY_WORKBENCH)
      if (welcomePanel) welcomePanel.api.close()
    })
    .catch(error => {
      console.error('[Workbench] 新建查询失败:', error)
    })
}

const handleOpenObjectProperties = (event: CustomEvent) => {
  try {
    if (!dockviewApi) return
    const {
      connectionId,
      scope,
      dbType,
      dbName,
      schemaName,
      objectType,
      objectName,
      tableName,
      columnName,
      indexName,
      constraintName,
    } = event.detail || {}

    if (!connectionId || !objectType) {
      console.warn('[Workbench] open-object-properties 缺少必要参数', { connectionId, objectType })
      return
    }

    const title = [objectType, objectName].filter(Boolean).join(': ') || 'Properties'
    const panelId = `panel_dynamicObjectProperties_${Date.now()}`
    dockviewApi.addPanel({
      id: panelId,
      component: 'dynamicObjectProperties',
      title,
      position: findCenterGroupReference(),
      params: {
        connectionId,
        scope: scope || 'global',
        dbType: dbType || '',
        dbName: dbName || '',
        schemaName: schemaName || '',
        objectType: objectType || '',
        objectName: objectName || '',
        tableName: tableName || '',
        columnName: columnName || '',
        indexName: indexName || '',
        constraintName: constraintName || '',
      },
    })
    // eslint-disable-next-line no-console
    console.debug(`[Workbench] 创建对象属性面板: ${panelId} (${objectType}: ${objectName})`)
  } catch (err) {
    console.error('[Workbench] 创建属性面板失败:', err)
  }
}

const handleKeydown = (e: KeyboardEvent) => {
  if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.code === 'KeyE') {
    e.preventDefault()
    const conns = connectionStore.connections
    const firstConn = conns.length > 0 ? conns[0] : null
    EditorManager.openNewQuery(firstConn?.name || '', firstConn?.url || '')
  }
}

// 标题栏菜单/工具栏/命令面板事件处理
const handleWorkbenchNewQuery = async (e?: CustomEvent) => {
  const detail = e?.detail as
    | { connectionId?: string; databaseName?: string; sql?: string }
    | undefined

  // 优先使用事件载荷（来自侧边栏等的指定连接），否则 fallback 到首个活动连接
  if (detail?.connectionId) {
    // eslint-disable-next-line no-console
    console.debug('[Workbench] handleWorkbenchNewQuery 由侧边栏触发:', detail.connectionId)
    await EditorManager.openNewQuery(detail.connectionId, detail.databaseName || '')
    return
  }

  // fallback: Ctrl+N 无连接上下文时，使用首个活动连接
  const conns = connectionStore.connections
  const firstConn = conns.length > 0 ? conns[0] : null
  await EditorManager.openNewQuery(
    firstConn?.connId || '',
    ((firstConn as Record<string, unknown>)?.database as string) || ''
  )
}

const handleWorkbenchNewConnection = (e?: CustomEvent) => {
  const connection = e?.detail?.connection as
    | import('@/extensions/builtin/connection/types/connection').ProjectConnection
    | undefined
  if (connection) {
    dialogInitialConnection.value = connection
    dialogInitialDriver.value = null
  } else {
    dialogInitialDriver.value =
      (e?.detail?.driver as import('@/extensions/builtin/connection/domain/types').Driver) || null
    dialogInitialConnection.value = null
  }
  showAddDataSourceDialog.value = true
}

const handleWorkbenchManageConnections = () => {
  const leftGroup = dockviewApi?.getEdgeGroup('left')
  if (leftGroup?.isCollapsed()) {
    layoutStore.expandLeftEdgeGroup()
  }
  const dsPanel = dockviewApi?.getPanel('panel_databaseNavigator')
  if (dsPanel) {
    dsPanel.focus()
  }
}

const handleDataSourceSaved = () => {
  dialogInitialDriver.value = null
  dialogInitialConnection.value = null
  window.dispatchEvent(new CustomEvent('navigator-refresh'))
}

const handleWorkbenchSave = () => {
  EditorManager.saveCurrentFile()
}

const handleWorkbenchExecuteSql = () => {
  EditorManager.executeCurrentSQL()
}

const handleWorkbenchOpenDocs = () => {
  // 打开文档链接
  window.open('https://docs.rdatastation.dev', '_blank')
}

const handleWorkbenchKeyboardShortcuts = () => {
  message.info(t('workbench.keyboardShortcuts') + ' - ' + t('workbench.comingSoon'))
}

const handleWorkbenchOpenTerminal = () => {
  message.info(t('workbench.terminal') + ' - ' + t('workbench.comingSoon'))
}

const handleWorkbenchOpenHistory = () => {
  if (!dockviewApi) return
  const panelId = 'panel_sqlHistory'
  const existing = dockviewApi.getPanel(panelId)
  if (existing) {
    existing.focus()
    return
  }
  dockviewApi.addPanel({
    id: panelId,
    component: 'sqlHistory',
    title: t('workbench.sqlHistory'),
    position: { direction: 'right' },
  })
}

const handleWorkbenchToggleSidebar = () => {
  layoutStore.toggleLeftEdgeGroup()
}

const handleWorkbenchOpenCustomizeLayout = () => {
  layoutStore.openCustomizeLayoutDialog()
}

const handleWorkbenchTogglePanel = () => {
  layoutStore.toggleBottomPanelMode()
}

// 标题栏事件监听器清理函数数组
const cleanupListeners: (() => void)[] = []

onMounted(() => {
  layoutStore.loadLayoutConfig()

  window.addEventListener(
    'layout-settings-update',
    handleLayoutSettingsUpdate as (e: Event) => void
  )
  window.addEventListener('reset-layout', handleResetLayout as (e: Event) => void)
  window.addEventListener(
    'open-object-properties',
    handleOpenObjectProperties as (e: Event) => void
  )
  window.addEventListener('open-sql-editor', handleOpenSqlEditor as (e: Event) => void)
  window.addEventListener('keydown', handleKeydown)

  // 标题栏事件监听 - 使用常量枚举 + listenWorkbenchEvent
  cleanupListeners.push(listenWorkbenchEvent(WorkbenchEvent.NewQuery, handleWorkbenchNewQuery))
  cleanupListeners.push(
    listenWorkbenchEvent(WorkbenchEvent.NewConnection, handleWorkbenchNewConnection)
  )
  cleanupListeners.push(
    listenWorkbenchEvent(WorkbenchEvent.ManageConnections, handleWorkbenchManageConnections)
  )
  cleanupListeners.push(listenWorkbenchEvent(WorkbenchEvent.Save, handleWorkbenchSave))
  cleanupListeners.push(listenWorkbenchEvent(WorkbenchEvent.ExecuteSql, handleWorkbenchExecuteSql))
  cleanupListeners.push(listenWorkbenchEvent(WorkbenchEvent.OpenDocs, handleWorkbenchOpenDocs))
  cleanupListeners.push(
    listenWorkbenchEvent(WorkbenchEvent.KeyboardShortcuts, handleWorkbenchKeyboardShortcuts)
  )
  cleanupListeners.push(
    listenWorkbenchEvent(WorkbenchEvent.OpenTerminal, handleWorkbenchOpenTerminal)
  )
  cleanupListeners.push(
    listenWorkbenchEvent(WorkbenchEvent.OpenHistory, handleWorkbenchOpenHistory)
  )
  cleanupListeners.push(
    listenWorkbenchEvent(WorkbenchEvent.ToggleSidebar, handleWorkbenchToggleSidebar)
  )
  cleanupListeners.push(
    listenWorkbenchEvent(WorkbenchEvent.TogglePanel, handleWorkbenchTogglePanel)
  )
  cleanupListeners.push(
    listenWorkbenchEvent(WorkbenchEvent.OpenCustomizeLayout, handleWorkbenchOpenCustomizeLayout)
  )
})

onUnmounted(() => {
  appStore.closeProject().catch(e => {
    console.warn('[Workbench] closeProject failed:', e)
  })

  window.removeEventListener(
    'layout-settings-update',
    handleLayoutSettingsUpdate as (e: Event) => void
  )
  window.removeEventListener('reset-layout', handleResetLayout as (e: Event) => void)
  window.removeEventListener(
    'open-object-properties',
    handleOpenObjectProperties as (e: Event) => void
  )
  window.removeEventListener('open-sql-editor', handleOpenSqlEditor as (e: Event) => void)
  window.removeEventListener('keydown', handleKeydown)

  // 清理标题栏事件监听
  cleanupListeners.forEach(cleanup => cleanup())
  cleanupListeners.length = 0
})
</script>

<style scoped>
.workbench-view {
  width: 100%;
  height: 100%;
  background: var(--bg-primary);
  color: var(--text-primary);
  font-family: var(--font-sans);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.dockview {
  flex: 1 1 0%;
  overflow: hidden;
  min-width: 0;
  min-height: 0;
}

.recovery-banner {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 16px;
  background: rgba(250, 173, 20, 0.1);
  border-bottom: 1px solid rgba(250, 173, 20, 0.25);
  font-size: 13px;
}

.recovery-icon {
  font-size: 16px;
}

.recovery-text {
  flex: 1;
  color: #d4a017;
}

.recovery-btn {
  padding: 4px 12px;
  border-radius: 4px;
  border: none;
  font-size: 12px;
  cursor: pointer;
  transition: background 0.15s;
}

.recovery-btn-primary {
  background: #faad14;
  color: #fff;
}

.recovery-btn-primary:hover {
  background: #d89614;
}

.recovery-btn-dismiss {
  background: transparent;
  color: #999;
  border: 1px solid #444;
}

.recovery-btn-dismiss:hover {
  color: #ccc;
  border-color: #666;
}
</style>
