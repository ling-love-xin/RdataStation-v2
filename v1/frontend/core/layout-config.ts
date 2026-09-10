/**
 * 布局配置管理
 *
 * 将面板布局配置外部化，便于维护和动态调整
 */

import type { Component } from 'vue'

/**
 * 面板位置类型
 */
export type PanelLocation = 'left' | 'right' | 'center' | 'bottom'

/**
 * 面板配置项
 */
export interface PanelConfig {
  /**
   * 面板唯一标识
   */
  id: string

  /**
   * 面板显示名称
   */
  name: string

  /**
   * Vue 组件（支持懒加载）
   */
  component: Component | (() => Promise<{ default: Component }>)

  /**
   * 面板位置
   */
  location: PanelLocation

  /**
   * 面板图标（可选）
   */
  icon?: Component

  /**
   * 排序顺序（数字越小越靠前）
   */
  order?: number

  /**
   * 是否默认显示
   */
  defaultVisible?: boolean

  /**
   * 是否可关闭
   */
  closable?: boolean
}

/**
 * 布局配置接口
 */
export interface LayoutConfig {
  /**
   * 默认布局名称
   */
  defaultLayout: string

  /**
   * 布局列表
   */
  layouts: Record<string, LayoutDefinition>
}

/**
 * 布局定义
 */
export interface LayoutDefinition {
  /**
   * 布局名称
   */
  name: string

  /**
   * 布局描述
   */
  description: string

  /**
   * 面板配置列表
   */
  panels: PanelConfig[]
}

/**
 * 默认布局配置
 */
export const defaultLayoutConfig: LayoutConfig = {
  defaultLayout: 'default',
  layouts: {
    default: {
      name: '默认布局',
      description: '标准三栏布局，左侧导航、中央编辑区、右侧辅助面板',
      panels: [
        // 左侧面板
        {
          id: 'database-nav',
          name: '数据库',
          component: () =>
            import('@/extensions/builtin/database/ui/components/database-navigator.vue'),
          location: 'left',
          order: 1,
          defaultVisible: true,
          closable: false,
        },
        {
          id: 'analytics',
          name: '分析资源',
          component: () =>
            import('@/extensions/builtin/analytics-resource/ui/components/AnalyticsResourceManager.vue'),
          location: 'left',
          order: 2,
          defaultVisible: false,
        },
        {
          id: 'plugins',
          name: '插件',
          component: () =>
            import('@/extensions/builtin/workbench/ui/components/panels/PluginsPanel.vue'),
          location: 'left',
          order: 3,
          defaultVisible: false,
        },

        // 中央面板
        {
          id: 'empty-workbench',
          name: '欢迎',
          component: () =>
            import('@/extensions/builtin/workbench/ui/components/panels/EmptyWorkbenchPanel.vue'),
          location: 'center',
          order: 1,
          defaultVisible: true,
          closable: false,
        },

        // 右侧面板
        {
          id: 'sql-history',
          name: 'SQL历史',
          component: () =>
            import('@/extensions/builtin/workbench/ui/components/panels/SqlHistoryPanel.vue'),
          location: 'right',
          order: 1,
          defaultVisible: false,
        },
        {
          id: 'output',
          name: '输出',
          component: () =>
            import('@/extensions/builtin/workbench/ui/components/panels/OutputPanel.vue'),
          location: 'right',
          order: 2,
          defaultVisible: false,
        },
        {
          id: 'column-insights',
          name: '列洞察',
          component: () =>
            import('@/extensions/builtin/workbench/ui/components/panels/ColumnInsightsPanel.vue'),
          location: 'right',
          order: 3,
          defaultVisible: false,
        },
        {
          id: 'log-viewer',
          name: '日志',
          component: () =>
            import('@/extensions/builtin/workbench/ui/components/panels/LogViewerPanel.vue'),
          location: 'bottom',
          order: 1,
          defaultVisible: false,
        },
      ],
    },
    compact: {
      name: '紧凑布局',
      description: '简化布局，仅显示核心面板',
      panels: [
        {
          id: 'database-nav',
          name: '数据库',
          component: () =>
            import('@/extensions/builtin/database/ui/components/database-navigator.vue'),
          location: 'left',
          order: 1,
          defaultVisible: true,
          closable: false,
        },
        {
          id: 'empty-workbench',
          name: '欢迎',
          component: () =>
            import('@/extensions/builtin/workbench/ui/components/panels/EmptyWorkbenchPanel.vue'),
          location: 'center',
          order: 1,
          defaultVisible: true,
          closable: false,
        },
        {
          id: 'log-viewer',
          name: '日志',
          component: () =>
            import('@/extensions/builtin/workbench/ui/components/panels/LogViewerPanel.vue'),
          location: 'bottom',
          order: 1,
          defaultVisible: false,
        },
      ],
    },
    analysis: {
      name: '分析布局',
      description: '优化的数据分析布局，右侧显示洞察面板',
      panels: [
        {
          id: 'database-nav',
          name: '数据库',
          component: () =>
            import('@/extensions/builtin/database/ui/components/database-navigator.vue'),
          location: 'left',
          order: 1,
          defaultVisible: true,
          closable: false,
        },
        {
          id: 'analytics',
          name: '分析资源',
          component: () =>
            import('@/extensions/builtin/analytics-resource/ui/components/AnalyticsResourceManager.vue'),
          location: 'left',
          order: 2,
          defaultVisible: true,
        },
        {
          id: 'empty-workbench',
          name: '欢迎',
          component: () =>
            import('@/extensions/builtin/workbench/ui/components/panels/EmptyWorkbenchPanel.vue'),
          location: 'center',
          order: 1,
          defaultVisible: true,
          closable: false,
        },
        {
          id: 'column-insights',
          name: '列洞察',
          component: () =>
            import('@/extensions/builtin/workbench/ui/components/panels/ColumnInsightsPanel.vue'),
          location: 'right',
          order: 1,
          defaultVisible: true,
        },
        {
          id: 'output',
          name: '输出',
          component: () =>
            import('@/extensions/builtin/workbench/ui/components/panels/OutputPanel.vue'),
          location: 'right',
          order: 2,
          defaultVisible: false,
        },
        {
          id: 'log-viewer',
          name: '日志',
          component: () =>
            import('@/extensions/builtin/workbench/ui/components/panels/LogViewerPanel.vue'),
          location: 'bottom',
          order: 1,
          defaultVisible: false,
        },
      ],
    },
  },
}

/**
 * 获取指定布局配置
 * @param layoutName 布局名称
 * @returns 布局定义
 */
export function getLayoutConfig(layoutName?: string): LayoutDefinition {
  const name = layoutName || defaultLayoutConfig.defaultLayout
  return defaultLayoutConfig.layouts[name] || defaultLayoutConfig.layouts.default
}

/**
 * 获取所有布局名称列表
 * @returns 布局名称数组
 */
export function getLayoutNames(): string[] {
  return Object.keys(defaultLayoutConfig.layouts)
}
