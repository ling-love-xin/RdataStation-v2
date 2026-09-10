/**
 * 数据库导航树节点图标配置
 *
 * 使用 lucide-vue-next 图标库
 * 为不同节点类型提供专业图标
 */

import {
  Database,
  Server,
  Table,
  FileText,
  Columns,
  Key,
  Lock,
  FunctionSquare,
  GitBranch,
  FolderOpen,
  Folder,
  Globe,
  HardDrive,
  Layers,
  List,
  Settings,
  Shield,
  Zap,
  Clock,
} from 'lucide-vue-next'

import type { Component } from 'vue'

/**
 * 节点图标配置
 */
export interface INodeIconConfig {
  /** 图标组件 */
  icon: Component
  /** 图标颜色 */
  color?: string
  /** 图标大小 */
  size?: number
}

/**
 * 节点类型图标映射
 */
export const NODE_TYPE_ICONS: Record<string, INodeIconConfig> = {
  // 连接层
  connection: {
    icon: Server,
    color: '#4f46e5',
  },

  // 数据库层
  database: {
    icon: Database,
    color: '#3b82f6',
  },
  schema: {
    icon: Layers,
    color: '#10b981',
  },

  // 文件夹层
  'tables-folder': {
    icon: FolderOpen,
    color: '#f59e0b',
  },
  'views-folder': {
    icon: FolderOpen,
    color: '#8b5cf6',
  },
  'functions-folder': {
    icon: Folder,
    color: '#06b6d4',
  },
  'procedures-folder': {
    icon: Folder,
    color: '#ec4899',
  },
  'sequences-folder': {
    icon: Folder,
    color: '#84cc16',
  },
  'triggers-folder': {
    icon: Folder,
    color: '#f97316',
  },
  'columns-folder': {
    icon: Folder,
    color: '#6b7280',
  },
  'indexes-folder': {
    icon: Folder,
    color: '#6b7280',
  },
  'constraints-folder': {
    icon: Folder,
    color: '#6b7280',
  },

  // 对象层
  table: {
    icon: Table,
    color: '#3b82f6',
  },
  view: {
    icon: FileText,
    color: '#8b5cf6',
  },
  column: {
    icon: Columns,
    color: '#6b7280',
  },
  index: {
    icon: Key,
    color: '#f59e0b',
  },
  constraint: {
    icon: Lock,
    color: '#ef4444',
  },
  function: {
    icon: FunctionSquare,
    color: '#06b6d4',
  },
  procedure: {
    icon: GitBranch,
    color: '#ec4899',
  },
  sequence: {
    icon: List,
    color: '#84cc16',
  },
  trigger: {
    icon: Zap,
    color: '#f97316',
  },
}

/**
 * 获取节点图标配置
 */
export function getNodeIcon(nodeType: string): INodeIconConfig {
  return (
    NODE_TYPE_ICONS[nodeType] || {
      icon: Globe,
      color: '#6b7280',
    }
  )
}

/**
 * 连接状态图标
 */
export const CONNECTION_STATUS_ICONS = {
  connected: {
    icon: Zap,
    color: '#00B42A',
  },
  disconnected: {
    icon: Clock,
    color: '#999999',
  },
  connecting: {
    icon: Settings,
    color: '#165DFF',
  },
  error: {
    icon: Shield,
    color: '#F53F3F',
  },
}

/**
 * 数据库类型图标
 */
export const DB_TYPE_ICONS: Record<string, INodeIconConfig> = {
  mysql: {
    icon: HardDrive,
    color: '#00758f',
  },
  postgres: {
    icon: HardDrive,
    color: '#336791',
  },
  sqlite: {
    icon: HardDrive,
    color: '#003b57',
  },
  duckdb: {
    icon: HardDrive,
    color: '#fff000',
  },
}
