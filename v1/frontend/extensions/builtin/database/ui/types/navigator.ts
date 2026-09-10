export interface GlobalConnection {
  id: string
  name: string
  driver: string
  host: string | null
  port: number | null
  database: string | null
  tags: string[]
  is_active: boolean
  created_at: string
  updated_at: string
}

export interface FilterConfig {
  showTables: boolean
  showViews: boolean
  showSystemSchemas: boolean
  showColumns: boolean
}

export interface DatabaseInfo {
  name: string
  schemas: SchemaInfo[]
}

/** 索引信息（与后端 IndexMeta JSON 输出对齐） */
export interface IndexInfo {
  name: string
  /** 所属表名 */
  tableName: string
  /** 索引列名列表 */
  columnNames: string[]
  /** 是否唯一索引 */
  isUnique: boolean
  /** 是否主键索引 */
  isPrimary: boolean
  /** 索引类型（btree/hash/gist 等） */
  type?: string
  /** 索引注释 */
  comment?: string
}

export interface TriggerInfo {
  name: string
  event: 'INSERT' | 'UPDATE' | 'DELETE' | 'TRUNCATE'
  timing: 'BEFORE' | 'AFTER' | 'INSTEAD OF'
  function: string
  enabled: boolean
}

/** 约束信息（与后端 ConstraintMeta JSON 输出对齐） */
export interface ConstraintInfo {
  name: string
  /** 所属表名 */
  tableName: string
  /** 约束类型（PRIMARY KEY/FOREIGN KEY/UNIQUE/CHECK/NOT NULL） */
  constraintType: string
  /** 约束列名列表 */
  columnNames: string[]
  /** 外键引用的表名 */
  referencedTable?: string
  /** 外键引用的列名列表 */
  referencedColumns?: string[]
  /** 外键更新规则（CASCADE/SET NULL/RESTRICT/NO ACTION） */
  updateRule?: string
  /** 外键删除规则（CASCADE/SET NULL/RESTRICT/NO ACTION） */
  deleteRule?: string
}

export interface ProcedureInfo {
  name: string
  language: string
  returnType: string
  parameters: Array<{ name: string; type: string; mode: 'IN' | 'OUT' | 'INOUT' }>
  definition: string
}

export interface FunctionInfo {
  name: string
  language: string
  returnType: string
  parameters: Array<{ name: string; type: string }>
  isAggregate: boolean
  definition: string
}

export interface SequenceInfo {
  name: string
  currentValue: number
  minValue: number
  maxValue: number
  increment: number
  cacheSize: number
  isCycled: boolean
}

export interface SchemaInfo {
  name: string
  tables: TableInfo[]
  views: ViewInfo[]
  indexes: IndexInfo[]
  triggers: TriggerInfo[]
  procedures: ProcedureInfo[]
  functions: FunctionInfo[]
  sequences: SequenceInfo[]
}

export interface TableInfo {
  name: string
  type: string
  columns: ColumnInfo[]
  description?: string
}

export interface ViewInfo {
  name: string
  type: string
  columns: ColumnInfo[]
  description?: string
  definition?: string
}

/** 列信息（与后端 ColumnMeta JSON 输出对齐） */
export interface ColumnInfo {
  name: string
  /** 数据类型 */
  dataType: string
  /** 是否可空 */
  isNullable: boolean
  /** 默认值（后端始终返回，可能为 null） */
  defaultValue: string | null
  /** 是否主键列（后端始终返回） */
  isPrimaryKey: boolean
  /** 是否外键列（后端始终返回） */
  isForeignKey: boolean
  /** 列注释 */
  comment?: string | null
}

export interface ConnectionInfo {
  id: string
  name: string
  driver: string
  tags?: string[]
}

export interface SearchObjectResult {
  connectionId: string
  databaseName: string
  schemaName: string
  objectName: string
  objectType: 'table' | 'view' | 'column'
}

export interface NavigatorState {
  expanded_keys: string[]
  selected_keys: string[]
  filter_config: FilterConfig | null
}

export interface ContextMenuNodeData {
  nodeKey: string
  keyParts: string[]
  connectionId: string
  catalogName: string
  schemaName: string
  tableName: string
}

export const NODE_TYPE_MAP: Record<string, string> = {
  conn: 'connection',
  db: 'catalog',
  schema: 'schema',
  table: 'table',
  view: 'view',
  col: 'column',
  tables: 'folder',
  views: 'folder',
  procedure: 'procedure',
  function: 'function',
  procedures: 'folder',
  functions: 'folder',
}

export const NODE_TYPE_ICONS: Record<string, string> = {
  connection: 'Database',
  catalog: 'Database',
  schema: 'Folder',
  table: 'Table',
  view: 'FileText',
  column: 'Columns',
  folder: 'FolderOpen',
  procedure: 'Code',
  function: 'FunctionSquare',
}

export const NODE_TYPE_COLORS: Record<string, string> = {
  connection: '#4f46e5',
  database: '#3b82f6',
  schema: '#10b981',
  table: '#3b82f6',
  view: '#8b5cf6',
  column: '#6b7280',
  folder: '#f59e0b',
  procedure: '#ef4444',
  function: '#14b8a6',
}
