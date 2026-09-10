/**
 * 数据库元数据类型定义
 */

import type { DatabaseType } from './sql'

// ============================================================================
// Schema 对象树模型（与 Rust driver::traits::SchemaObject 对齐）
// ============================================================================

/**
 * Schema 对象类别
 *
 * 对应 Rust `SchemaObjectKind` 枚举。
 */
export type SchemaObjectKind =
  | 'Catalog'
  | 'Schema'
  | 'Table'
  | 'View'
  | 'Column'
  | 'Index'
  | 'PrimaryKey'
  | 'ForeignKey'
  | 'Procedure'
  | 'Function'

/**
 * Schema 对象（对象树模型）
 *
 * 前端友好的统一结构，支持懒加载：
 * - `children` 为 `undefined` 表示**未加载**（不要误判为空）
 * - `children` 为 `[]` 表示已加载但无子节点
 *
 * 对应 Rust `driver::traits::SchemaObject`。
 */
export interface SchemaObject {
  name: string
  kind: SchemaObjectKind
  children?: SchemaObject[]
  comment?: string | null
}

// ============================================================================
// 数据库元数据配置
// ============================================================================

export interface DatabaseMetaConfig {
  // 数据库类型
  dbType: DatabaseType

  // 显示名称
  displayName?: string

  // 名称
  name?: string

  // 版本
  version?: string

  // 默认端口
  defaultPort?: number

  // URL 模板
  urlTemplate?: string

  // 支持的节点类型
  supportedNodeTypes: string[]

  // 节点类型定义
  nodeTypes?: NodeTypeConfig[]

  // 属性配置
  propertiesConfig: PropertiesConfig

  // 标签页配置
  tabsConfig: TabConfig[]

  // 标签分组配置
  tabGroups?: TabGroupConfig[]

  // 层级结构
  hierarchy?: Array<{ level: number; type: string }>

  // ID (用于标识)
  id?: string

  // 特性支持
  features?: Record<string, boolean>

  // 默认查询
  defaultQueries?: Record<string, string>

  // 图标配置
  icons?: Record<string, string>
}

export interface NodeTypeConfig {
  id: string
  label: string
  icon: string
  isContainer?: boolean
  children?: string[]
  showCount?: boolean
  parentTypes?: string[]
  actions?: NodeAction[] | string[]
  query?: string
  metadata?: string[]
}

export interface NodeAction {
  id: string
  label: string
  icon: string
  command?: string
  type?: string
}

export interface PropertiesConfig {
  generalFields?: PropertyField[]
  table?: NodePropertiesConfig
  view?: NodePropertiesConfig
  column?: NodePropertiesConfig
  index?: NodePropertiesConfig
  [key: string]: NodePropertiesConfig | PropertyField[] | undefined
}

export interface NodePropertiesConfig {
  tabs: TabConfig[]
  generalFields: PropertyField[]
}

export interface PropertyField {
  key: string
  label: string
  type: 'text' | 'number' | 'boolean' | 'datetime' | 'size'
  format?: string
  editable?: boolean
}

export interface TabConfig {
  id: string
  label: string
  icon?: string
  default?: boolean
  filter?: (nodeType: string) => boolean
}

export interface TabGroupConfig {
  id: string
  label: string
  icon: string
  filter: (nodeType: string) => boolean
  order: number
  expanded: boolean
}

// ============================================================================
// 导航器节点类型
// ============================================================================

export interface NavigatorNode {
  id: string
  name: string
  type:
    | 'connection'
    | 'catalog'
    | 'schema'
    | 'table'
    | 'view'
    | 'column'
    | 'index'
    | 'folder'
    | string
  connectionId?: string
  database?: string
  schema?: string
  parentId?: string | null
  children?: NavigatorNode[]
  /** 节点加载状态，优先使用 `state` 字段 */
  isLoading?: boolean
  /** @deprecated 使用 `isLoading` 替代 */
  loading?: boolean
  /** 节点展开状态 */
  isExpanded?: boolean
  /** @deprecated 使用 `isExpanded` 替代 */
  expanded?: boolean
  /** 节点生命周期状态 */
  state?: 'idle' | 'loading' | 'error' | 'loaded' | 'empty'
  /** @deprecated 使用 `metadata` 替代 */
  meta?: Record<string, unknown>
  /** 节点附加元数据 */
  metadata?: Record<string, unknown>
  properties?: NodeProperties
}

export interface NodeProperties {
  schema?: string
  catalog?: string
  type?: string
  nullable?: boolean
  dataType?: string
  columnSize?: number
  decimalDigits?: number
  ordinalPosition?: number
  isPrimaryKey?: boolean
  isForeignKey?: boolean
  isUnique?: boolean
  isIndexed?: boolean
  defaultValue?: string
  autoIncrement?: boolean
  charset?: string
  collation?: string
  comment?: string
  engine?: string
  rowCount?: number
  dataSize?: number
  indexSize?: number
  createdAt?: string
  updatedAt?: string
  general?: Record<string, unknown>
  columns?: Record<string, unknown>[]
  indexes?: Record<string, unknown>[]
  ddl?: string
}

// ============================================================================
// 查询上下文
// ============================================================================

export interface QueryContext {
  connectionId: string
  database?: string
  schema?: string
  table?: string
}

// ============================================================================
// 数据库适配器类型
// ============================================================================

export interface DatabaseMetaAdapter {
  readonly driverId: string
  readonly driverName: string
  readonly features: DatabaseFeature[]

  // 元数据查询方法
  getDatabases(connectionId: string): Promise<DatabaseInfo[]>
  getSchemas(connectionId: string, database?: string): Promise<SchemaInfo[]>
  getTables(connectionId: string, database?: string, schema?: string): Promise<TableInfo[]>
  getViews(connectionId: string, database?: string, schema?: string): Promise<ViewInfo[]>
  getColumns(
    connectionId: string,
    database: string,
    schema: string,
    table: string
  ): Promise<ColumnInfo[]>
  getIndexes(
    connectionId: string,
    database: string,
    schema: string,
    table: string
  ): Promise<IndexInfo[]>
  getForeignKeys(
    connectionId: string,
    database: string,
    schema: string,
    table: string
  ): Promise<ForeignKeyInfo[]>

  // 构建导航器节点
  buildConnectionNode(connectionId: string, name: string): NavigatorNode
  buildDatabaseNode(connectionId: string, database: DatabaseInfo): NavigatorNode
  buildSchemaNode(connectionId: string, database: string, schema: SchemaInfo): NavigatorNode
  buildTableNode(
    connectionId: string,
    database: string,
    schema: string,
    table: TableInfo
  ): NavigatorNode
  buildViewNode(
    connectionId: string,
    database: string,
    schema: string,
    view: ViewInfo
  ): NavigatorNode
  buildColumnNode(
    connectionId: string,
    database: string,
    schema: string,
    table: string,
    column: ColumnInfo
  ): NavigatorNode

  // 导航器加载方法
  getChildrenQuery(nodeType: string, context: QueryContext): string | null
  parseChildrenResult(nodeType: string, rows: unknown[], context: QueryContext): NavigatorNode[]

  // 获取配置
  getConfig(): DatabaseMetaConfig
}

export type DatabaseFeature =
  | 'schemas'
  | 'tables'
  | 'views'
  | 'procedures'
  | 'functions'
  | 'triggers'
  | 'indexes'
  | 'foreignKeys'
  | 'ssl'
  | 'sshTunnel'
  | 'httpProxy'

// ============================================================================
// 数据库对象信息
// ============================================================================

export interface DatabaseInfo {
  name: string
  charset?: string
  collation?: string
}

export interface SchemaInfo {
  name: string
  catalog?: string
  owner?: string
}

export interface TableInfo {
  name: string
  schema?: string
  catalog?: string
  type: 'table' | 'system_table' | 'temporary'
  engine?: string
  charset?: string
  collation?: string
  rowCount?: number
  dataSize?: number
  indexSize?: number
  comment?: string
  createdAt?: string
  updatedAt?: string
}

export interface ViewInfo {
  name: string
  schema?: string
  catalog?: string
  definition?: string
  isUpdatable?: boolean
  comment?: string
}

/**
 * 列元数据（共享规范 — 前后端均可使用的标准列信息模型）
 *
 * @deprecated 此接口已废弃，请使用 `navigator.ts::ColumnInfo` 作为 IPC 传输的权威类型定义。
 *             本接口保留用于内部存储 / 缓存 / Mock 生成等非 IPC 场景。
 *
 * @remarks
 * 命名约定：
 * - `dataType` / `isNullable` — 统一驼峰 + is 前缀，匹配后端 ColumnDetail 序列化
 * - `columnSize` / `decimalDigits` — 仅 JDBC 驱动可用，原生驱动可能为空
 */
export interface ColumnInfo {
  name: string
  dataType: string
  isNullable: boolean
  defaultValue?: string
  isPrimaryKey?: boolean
  isForeignKey?: boolean
  isAutoIncrement?: boolean
  isUnique?: boolean
  charset?: string
  collation?: string
  comment?: string
  ordinalPosition?: number
  columnSize?: number
  decimalDigits?: number
}

/** 索引信息（内部详细模型，含列排序信息） */
export interface IndexInfo {
  name: string
  tableName: string
  schema?: string
  isUnique: boolean
  isPrimary: boolean
  /** 索引类型（btree/hash/gist 等） */
  type?: string
  /** 索引注释 */
  comment?: string
  /** 索引列详细信息（含排序方向）— 内部使用；IPC 版本见 navigator.ts::IndexInfo.columnNames */
  columns?: IndexColumnInfo[]
}

export interface IndexColumnInfo {
  name: string
  order: 'asc' | 'desc'
  ordinalPosition: number
}

/** 外键信息（与后端 ConstraintMeta JSON 输出对齐） */
export interface ForeignKeyInfo {
  name: string
  tableName: string
  schema?: string
  /** 外键列名列表（支持多列复合外键） */
  columnNames: string[]
  referencedTableName: string
  referencedSchema?: string
  /** 被引用列名列表（支持多列复合外键） */
  referencedColumnNames: string[]
  /** 外键更新规则（CASCADE/SET NULL/RESTRICT/NO ACTION） */
  updateRule?: string
  /** 外键删除规则（CASCADE/SET NULL/RESTRICT/NO ACTION） */
  deleteRule?: string
}

// ============================================================================
// 连接配置
// ============================================================================

export interface ConnectionConfig {
  id?: string
  name: string
  driver: DatabaseType
  host: string
  port?: number
  database?: string
  username?: string
  password?: string
  /** 多主机配置（故障转移），存在时优先于 host/port */
  hosts?: Array<{
    host: string
    port?: number
    priority?: number
    role?: 'primary' | 'replica'
  }>
  properties?: Record<string, unknown>
}

// ============================================================================
// 驱动配置
// ============================================================================

export interface DriverConfig {
  id: string
  name: string
  icon: string
  features: DatabaseFeature[]
  defaultPort?: number
  fields: DriverField[]
}

export interface DriverField {
  name: string
  key?: string
  label: string
  type: 'text' | 'number' | 'password' | 'file' | 'select' | 'checkbox'
  field_type?: 'text' | 'number' | 'password' | 'file' | 'select' | 'checkbox'
  required?: boolean
  default?: unknown
  placeholder?: string
  options?: { label: string; value: unknown }[]
  option_type?: string
  optionType?: string
  description?: string
}
