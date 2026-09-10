/**
 * 导航栏共享类型定义
 *
 * 供 Store、tree-mutation、子 loader、tree loader 统一引用，
 * 消除各模块独立定义同名类型导致的结构冲突。
 */

// ========== 树节点 ==========

export interface CatalogNode {
  name: string
  schemas: SchemaNode[]
  /** 无 Schema 的数据库（如 MySQL）直接在此存储表 */
  tables?: TableNode[]
}

export interface SchemaNode {
  name: string
  tables: TableNode[]
  views: ViewNode[]
  procedures?: ProcedureNode[]
  functions?: FunctionNode[]
  sequences?: SequenceNode[]
  triggers?: TriggerNode[]
  totalTables?: number
  totalViews?: number
  totalSizeBytes?: number
  rowCountTotal?: number
}

export interface TableNode {
  name: string
  type: string
  columns: ColumnNode[]
  indexes?: IndexNode[]
  constraints?: ConstraintNode[]
  rowCount?: number | null
  dataLength?: number | null
  indexLength?: number | null
}

export interface ViewNode {
  name: string
  type: string
  columns: ColumnNode[]
}

export interface ProcedureNode {
  name: string
}

export interface FunctionNode {
  name: string
}

export interface SequenceNode {
  name: string
}

export interface TriggerNode {
  name: string
  tableName?: string
  event?: string
}

export interface ColumnNode {
  name: string
  dataType: string
  nullable?: boolean
  defaultValue?: string
  isPrimaryKey?: boolean
  charMaxLength?: number
  numericPrecision?: number
  numericScale?: number
}

export interface IndexNode {
  name: string
  columns: string[]
  isUnique: boolean
  isPrimary: boolean
}

export interface ConstraintNode {
  name: string
  type: 'PRIMARY KEY' | 'FOREIGN KEY' | 'UNIQUE' | 'CHECK'
  columns: string[]
}

// ========== 其他 ==========

export interface SelectedObject {
  name: string
  kind: 'catalog' | 'schema' | 'table' | 'view' | 'column'
  catalog?: string
  schema?: string
  table?: string
  connectionId: string
  [key: string]: unknown
}

export interface SearchResult {
  connectionId: string
  type: 'catalog' | 'schema' | 'table' | 'view' | 'column'
  name: string
  path: string
  matchType: 'name' | 'type'
  parentTable?: string
}
