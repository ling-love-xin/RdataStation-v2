/**
 * Workbench Extension
 *
 * 提供工作台布局管理能力
 */

import DynamicObjectPropertiesPanel from './ui/components/panels/DynamicObjectPropertiesPanel.vue'
import EmptyWorkbenchPanel from './ui/components/panels/EmptyWorkbenchPanel.vue'
import MockPanel from './ui/components/panels/MockPanel.vue'
import OutputPanel from './ui/components/panels/OutputPanel.vue'
import PluginsPanel from './ui/components/panels/PluginsPanel.vue'
import SqlHistoryPanel from './ui/components/panels/SqlHistoryPanel.vue'

import type { ExtensionContext, ExtensionAPI, ExtensionModule, Disposable } from '../../core/types'

// Workbench 扩展特定的 API 接口
interface WorkbenchExtensionAPI extends ExtensionAPI {
  workbench: {
    openPanel(panelId: string, options?: { title?: string; component?: unknown }): void
    closePanel(panelId: string): void
    focusPanel(panelId: string): void
  }
}

interface PanelState {
  id: string
  title: string
  component?: unknown
  isActive: boolean
}

/**
 * 扩展激活函数
 */
const activate = (context: ExtensionContext): WorkbenchExtensionAPI => {
  // eslint-disable-next-line no-console
  console.debug('[Workbench] Activating for project:', context.project.name)

  // 面板状态管理
  const panels = new Map<string, PanelState>()

  const openPanel = (panelId: string, options?: { title?: string; component?: unknown }): void => {
    panels.set(panelId, {
      id: panelId,
      title: options?.title || panelId,
      component: options?.component,
      isActive: true,
    })
    // eslint-disable-next-line no-console
    console.debug(`[Workbench] Opened panel: ${panelId}`)
  }

  const closePanel = (panelId: string): void => {
    panels.delete(panelId)
    // eslint-disable-next-line no-console
    console.debug(`[Workbench] Closed panel: ${panelId}`)
  }

  const focusPanel = (panelId: string): void => {
    const panel = panels.get(panelId)
    if (panel) {
      panel.isActive = true
      // eslint-disable-next-line no-console
      console.debug(`[Workbench] Focused panel: ${panelId}`)
    }
  }

  // 注册空工作台面板（作为欢迎页，不注册到具体位置，由 WorkbenchView 动态控制）
  const emptyPanelDisposable = context.window.registerViewProvider('emptyWorkbench', {
    component: EmptyWorkbenchPanel,
    title: '欢迎',
    location: 'center',
    icon: 'Home',
    order: 0,
  })

  // 注册SQL历史面板（右侧）
  const sqlHistoryDisposable = context.window.registerViewProvider('sqlHistory', {
    component: SqlHistoryPanel,
    title: 'SQL历史',
    location: 'right',
    icon: 'Clock',
    order: 2,
  })

  // 注册输出面板（底部）
  const outputDisposable = context.window.registerViewProvider('outputPanel', {
    component: OutputPanel,
    title: '输出',
    location: 'bottom',
    icon: 'Terminal',
    order: 1,
  })

  // 注册插件面板（左侧）
  const pluginsDisposable = context.window.registerViewProvider('plugins', {
    component: PluginsPanel,
    title: '插件',
    location: 'left',
    icon: 'Puzzle',
    order: 3,
  })

  // 注册Mock数据生成面板（右侧）
  const mockPanelDisposable = context.window.registerViewProvider('mockPanel', {
    component: MockPanel,
    title: 'Mock 数据',
    location: 'right',
    icon: 'Database',
    order: 1,
  })

  // 注册动态对象属性面板（中心区域，动态创建）
  const objectPropertiesDisposable = context.window.registerViewProvider(
    'dynamicObjectProperties',
    {
      component: DynamicObjectPropertiesPanel,
      title: '对象属性',
      location: 'center',
      icon: 'Info',
      order: 10,
    }
  )

  const disposables: Disposable[] = [
    emptyPanelDisposable,
    sqlHistoryDisposable,
    outputDisposable,
    pluginsDisposable,
    mockPanelDisposable,
    objectPropertiesDisposable,
    context.commands.registerCommand('workbench.openPanel', (...args: unknown[]) =>
      openPanel(args[0] as string, args[1] as { title?: string; component?: unknown })
    ),
    context.commands.registerCommand('workbench.closePanel', (...args: unknown[]) =>
      closePanel(args[0] as string)
    ),
    context.commands.registerCommand('workbench.focusPanel', (...args: unknown[]) =>
      focusPanel(args[0] as string)
    ),
  ]

  return {
    version: '1.0.0',
    project: context.project,
    commands: context.commands,
    window: context.window,
    workspace: context.workspace,
    database: context.database,
    sqlEditor: context.sqlEditor,
    events: context.events,
    configuration: context.configuration,
    utils: context.utils,

    workbench: {
      openPanel,
      closePanel,
      focusPanel,
    },

    dispose: () => {
      disposables.forEach(d => d.dispose())
      panels.clear()
    },
  }
}

const deactivate = (): void => {
  // eslint-disable-next-line no-console
  console.debug('[Workbench] Deactivated')
}

const extension: ExtensionModule = {
  activate: activate as (context: ExtensionContext) => ExtensionAPI,
  deactivate,
}

export default extension
