/**
 * 虚拟树节点类型定义
 */

export type VirtualTreeNodeType =
  | 'connection'
  | 'catalog'
  | 'schema'
  | 'tables-folder'
  | 'views-folder'
  | 'materialized-views-folder'
  | 'functions-folder'
  | 'procedures-folder'
  | 'sequences-folder'
  | 'triggers-folder'
  | 'columns-folder'
  | 'indexes-folder'
  | 'constraints-folder'
  | 'table'
  | 'view'
  | 'materialized-view'
  | 'function'
  | 'procedure'
  | 'sequence'
  | 'column'
  | 'index'
  | 'constraint'
  | 'trigger'
  | 'placeholder'

export interface ITreeNodeData {
  connectionId?: string
  driver?: string
  scope?: 'global' | 'project'
  dbName?: string
  schemaName?: string
  tableName?: string
  viewName?: string
  columnName?: string
  dataType?: string
  indexName?: string
  isUnique?: boolean
  isPrimary?: boolean
  constraintName?: string
  constraintType?: 'PRIMARY KEY' | 'FOREIGN KEY' | 'UNIQUE' | 'CHECK'
  indexType?: string
  indexComment?: string | null
  indexColumnNames?: string[]
  constraintColumnNames?: string[]
  referencedTable?: string
  referencedColumns?: string[]
  updateRule?: string
  deleteRule?: string
  rowCount?: number
  dataLength?: number
  indexLength?: number
  tableCount?: number
  viewCount?: number
  totalSizeBytes?: number
  rowCountTotal?: number
  [key: string]: unknown
}

export interface IVirtualTreeNode {
  /** 节点唯一标识 (使用 base64 编码避免特殊字符问题) */
  key: string
  /** 节点层级（用于缩进） */
  level: number
  /** 是否展开 */
  isExpanded: boolean
  /** 是否叶子节点 */
  isLeaf: boolean
  /** 显示文本 */
  label: string
  /** 节点类型 */
  type: VirtualTreeNodeType
  /** 原始数据 */
  data: ITreeNodeData
  /** 父节点 ID */
  parentId: string | null
  /** 子节点数量 */
  childCount: number
  /** 是否加载中 */
  isLoading?: boolean
  /** 连接标签（仅 connection 类型） */
  connectionTags?: string[]
  /** 连接状态（仅 connection 类型） */
  connectionStatus?: 'connected' | 'connecting' | 'disconnected'
  /** 节点是否已加载过子节点（用于缓存判断） */
  isLoaded?: boolean
}

export interface IVirtualTreeContext {
  /** 连接 ID */
  connectionId?: string
  /** 数据库名 */
  dbName?: string
  /** Schema 名 */
  schemaName?: string
  /** 表名 */
  tableName?: string
}

/**
 * 节点 key 编码/解码工具
 * 使用 JSON + base64 编码，避免特殊字符问题
 */
export const NodeKeyEncoder = {
  encode(parts: string[]): string {
    return btoa(JSON.stringify(parts))
  },

  decode(key: string): string[] {
    try {
      return JSON.parse(atob(key))
    } catch {
      return []
    }
  },
}

// 保持向后兼容的别名
export type TreeNodeData = ITreeNodeData
export type VirtualTreeNode = IVirtualTreeNode
export type VirtualTreeContext = IVirtualTreeContext
