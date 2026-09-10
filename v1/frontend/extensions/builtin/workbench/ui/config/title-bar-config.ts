import { BookOpen, History, Keyboard, Settings, Terminal, Zap } from 'lucide-vue-next'
import { markRaw } from 'vue'

import { WorkbenchEvent, dispatchWorkbenchEvent } from '../constants/workbench-events'

import type { MenuConfig, ToolbarTool } from '../components/title-bar/title-bar-types'

/**
 * 创建菜单配置
 * @param t - i18n 翻译函数
 * @param handleOpenProject - 打开项目处理函数
 * @param handleOpenCommandPalette - 打开命令面板处理函数
 */
export function createMenuConfig(
  t: (key: string) => string,
  _handleOpenProject: () => void,
  _handleOpenCommandPalette: () => void
): MenuConfig[] {
  return [
    {
      id: 'file',
      label: t('workbench.fileMenu'),
      items: [
        { id: 'newQuery', label: t('menu.newQuery'), shortcut: 'Ctrl+N' },
        { id: 'newConnection', label: t('menu.newConnection'), shortcut: 'Ctrl+Shift+N' },
        { separator: true, id: 'sep1' },
        { id: 'openProject', label: t('menu.openProject'), shortcut: 'Ctrl+O' },
        { id: 'save', label: t('menu.save'), shortcut: 'Ctrl+S' },
      ],
    },
    {
      id: 'edit',
      label: t('workbench.editMenu'),
      items: [
        { id: 'undo', label: t('menu.undo'), shortcut: 'Ctrl+Z' },
        { id: 'redo', label: t('menu.redo'), shortcut: 'Ctrl+Y' },
        { separator: true, id: 'sep2' },
        { id: 'cut', label: t('menu.cut'), shortcut: 'Ctrl+X' },
        { id: 'copy', label: t('menu.copy'), shortcut: 'Ctrl+C' },
        { id: 'paste', label: t('menu.paste'), shortcut: 'Ctrl+V' },
        { separator: true, id: 'sep3' },
        { id: 'find', label: t('menu.find'), shortcut: 'Ctrl+F' },
        { id: 'replace', label: t('menu.replace'), shortcut: 'Ctrl+H' },
      ],
    },
    {
      id: 'view',
      label: t('workbench.viewMenu'),
      items: [
        { id: 'commandPalette', label: t('menu.commandPalette'), shortcut: 'Ctrl+Shift+P' },
        { separator: true, id: 'sep4' },
        { id: 'toggleSidebar', label: t('menu.toggleSidebar'), shortcut: 'Ctrl+B' },
        { id: 'togglePanel', label: t('menu.togglePanel'), shortcut: 'Ctrl+J' },
      ],
    },
    {
      id: 'connection',
      label: t('workbench.connectionMenu'),
      items: [
        { id: 'newConnection', label: t('menu.newConnection') },
        { id: 'manageConnections', label: t('workbench.manageConnections') },
        { separator: true, id: 'sep5' },
        { id: 'disconnect', label: t('workbench.disconnect') },
      ],
    },
    {
      id: 'run',
      label: t('workbench.runMenu'),
      items: [
        { id: 'executeSql', label: t('menu.executeSql'), shortcut: 'Ctrl+Enter' },
        { id: 'executeScript', label: t('workbench.executeScript'), shortcut: 'Ctrl+Shift+Enter' },
        { separator: true, id: 'sep6' },
        { id: 'stopExecution', label: t('menu.stopExecution') },
      ],
    },
    {
      id: 'tools',
      label: t('workbench.toolsMenu'),
      items: [
        { id: 'pluginManagement', label: t('menu.pluginManagement') },
        { separator: true, id: 'sep7' },
        { id: 'settings', label: t('menu.settings'), shortcut: 'Ctrl+,' },
        { id: 'keyboardShortcuts', label: t('menu.keyboardShortcuts'), shortcut: 'Ctrl+K Ctrl+S' },
      ],
    },
    {
      id: 'help',
      label: t('workbench.helpMenu'),
      items: [
        { id: 'documentation', label: t('menu.documentation') },
        { id: 'shortcuts', label: t('menu.keyboardShortcuts') },
        { separator: true, id: 'sep8' },
        { id: 'checkUpdates', label: t('menu.checkUpdates') },
        { id: 'about', label: t('menu.about') },
      ],
    },
  ]
}

/**
 * 创建工具栏配置
 * @param t - i18n 翻译函数
 * @param handleOpenCommandPalette - 打开命令面板处理函数
 */
export function createToolbarConfig(
  t: (key: string) => string,
  handleOpenCommandPalette: () => void
): ToolbarTool[] {
  return [
    {
      id: 'settings',
      name: t('workbench.settings'),
      icon: markRaw(Settings),
      enabled: false,
      action: () => dispatchWorkbenchEvent(WorkbenchEvent.OpenSettings),
    },
    {
      id: 'history',
      name: t('workbench.history'),
      icon: markRaw(History),
      enabled: false,
      action: () => dispatchWorkbenchEvent(WorkbenchEvent.OpenHistory),
    },
    {
      id: 'docs',
      name: t('workbench.docs'),
      icon: markRaw(BookOpen),
      enabled: false,
      action: () => dispatchWorkbenchEvent(WorkbenchEvent.OpenDocs),
    },
    {
      id: 'shortcuts',
      name: t('workbench.shortcuts'),
      icon: markRaw(Keyboard),
      enabled: false,
      action: () => dispatchWorkbenchEvent(WorkbenchEvent.KeyboardShortcuts),
    },
    {
      id: 'terminal',
      name: t('workbench.terminal'),
      icon: markRaw(Terminal),
      enabled: false,
      action: () => dispatchWorkbenchEvent(WorkbenchEvent.OpenTerminal),
    },
    {
      id: 'quick',
      name: t('workbench.quickActions'),
      icon: markRaw(Zap),
      enabled: false,
      action: handleOpenCommandPalette,
    },
  ]
}

/**
 * 创建菜单动作映射表
 */
export function createMenuActionMap(
  handleOpenProject: () => void,
  handleOpenCommandPalette: () => void
): Record<string, () => void> {
  return {
    newQuery: () => dispatchWorkbenchEvent(WorkbenchEvent.NewQuery),
    newConnection: () => dispatchWorkbenchEvent(WorkbenchEvent.NewConnection),
    openProject: handleOpenProject,
    save: () => dispatchWorkbenchEvent(WorkbenchEvent.Save),
    undo: () => dispatchWorkbenchEvent(WorkbenchEvent.Undo),
    redo: () => dispatchWorkbenchEvent(WorkbenchEvent.Redo),
    cut: () => dispatchWorkbenchEvent(WorkbenchEvent.Cut),
    copy: () => dispatchWorkbenchEvent(WorkbenchEvent.Copy),
    paste: () => dispatchWorkbenchEvent(WorkbenchEvent.Paste),
    find: () => dispatchWorkbenchEvent(WorkbenchEvent.Find),
    replace: () => dispatchWorkbenchEvent(WorkbenchEvent.Replace),
    commandPalette: handleOpenCommandPalette,
    toggleSidebar: () => dispatchWorkbenchEvent(WorkbenchEvent.ToggleSidebar),
    togglePanel: () => dispatchWorkbenchEvent(WorkbenchEvent.TogglePanel),
    manageConnections: () => dispatchWorkbenchEvent(WorkbenchEvent.ManageConnections),
    disconnect: () => dispatchWorkbenchEvent(WorkbenchEvent.Disconnect),
    executeSql: () => dispatchWorkbenchEvent(WorkbenchEvent.ExecuteSql),
    executeScript: () => dispatchWorkbenchEvent(WorkbenchEvent.ExecuteScript),
    stopExecution: () => dispatchWorkbenchEvent(WorkbenchEvent.StopExecution),
    pluginManagement: () => dispatchWorkbenchEvent(WorkbenchEvent.PluginManagement),
    settings: () => dispatchWorkbenchEvent(WorkbenchEvent.OpenSettings),
    keyboardShortcuts: () => dispatchWorkbenchEvent(WorkbenchEvent.KeyboardShortcuts),
    documentation: () => dispatchWorkbenchEvent(WorkbenchEvent.OpenDocs),
    checkUpdates: () => dispatchWorkbenchEvent(WorkbenchEvent.CheckUpdates),
    about: () => dispatchWorkbenchEvent(WorkbenchEvent.About),
  }
}
