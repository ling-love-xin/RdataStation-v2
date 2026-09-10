/**
 * DuckDB 元数据配置
 * 定义 DuckDB 数据库的导航结构和节点类型
 */

import type { DatabaseMetaConfig, NodeTypeConfig } from '@/shared/types/databaseMeta'

// DuckDB 支持的节点类型
const duckdbNodeTypes: NodeTypeConfig[] = [
  {
    id: 'connection',
    label: '连接',
    icon: 'Database',
    isContainer: true,
    children: ['database-folder'],
    actions: ['refresh', 'disconnect', 'properties'],
  },
  {
    id: 'database-folder',
    label: '数据库',
    icon: 'Folder',
    isContainer: true,
    children: ['database'],
    showCount: true,
    query: `SELECT database_name as name FROM information_schema.schemata GROUP BY database_name`,
  },
  {
    id: 'database',
    label: '数据库',
    icon: 'Database',
    isContainer: true,
    children: ['schema-folder'],
    actions: ['query', 'attach', 'detach'],
    metadata: ['path', 'size'],
  },
  {
    id: 'schema-folder',
    label: 'Schema',
    icon: 'Folder',
    isContainer: true,
    children: ['schema'],
    showCount: true,
    query: `SELECT schema_name as name FROM information_schema.schemata ORDER BY schema_name`,
  },
  {
    id: 'schema',
    label: 'Schema',
    icon: 'FolderOpen',
    isContainer: true,
    children: ['table-folder', 'view-folder', 'macro-folder', 'sequence-folder'],
    actions: ['query', 'properties'],
  },
  // 表
  {
    id: 'table-folder',
    label: '表',
    icon: 'Table2',
    isContainer: true,
    children: ['table'],
    showCount: true,
    query: `
      SELECT table_name as name 
      FROM information_schema.tables 
      WHERE table_schema = $1 AND table_type = 'BASE TABLE'
      ORDER BY table_name
    `,
  },
  {
    id: 'table',
    label: '表',
    icon: 'Table2',
    isContainer: true,
    children: ['column-folder', 'constraint-folder'],
    actions: ['query', 'design', 'export', 'drop'],
    metadata: ['rowCount', 'size', 'estimatedSize'],
  },
  {
    id: 'column-folder',
    label: '列',
    icon: 'Columns',
    isContainer: true,
    children: ['column'],
    showCount: true,
    query: `
      SELECT 
        column_name as name, 
        data_type,
        is_nullable,
        column_default
      FROM information_schema.columns
      WHERE table_schema = $1 AND table_name = $2
      ORDER BY ordinal_position
    `,
  },
  {
    id: 'column',
    label: '列',
    icon: 'Hash',
    isContainer: false,
    actions: ['properties'],
    metadata: ['dataType', 'nullable', 'default'],
  },
  {
    id: 'constraint-folder',
    label: '约束',
    icon: 'Key',
    isContainer: true,
    children: ['constraint'],
    showCount: true,
    query: `
      SELECT 
        constraint_name as name, 
        constraint_type as type
      FROM information_schema.table_constraints
      WHERE table_schema = $1 AND table_name = $2
      ORDER BY constraint_name
    `,
  },
  {
    id: 'constraint',
    label: '约束',
    icon: 'Key',
    isContainer: false,
    actions: ['drop'],
    metadata: ['constraintType', 'columns'],
  },
  // 视图
  {
    id: 'view-folder',
    label: '视图',
    icon: 'Eye',
    isContainer: true,
    children: ['view'],
    showCount: true,
    query: `
      SELECT table_name as name 
      FROM information_schema.views 
      WHERE table_schema = $1
      ORDER BY table_name
    `,
  },
  {
    id: 'view',
    label: '视图',
    icon: 'Eye',
    isContainer: false,
    children: ['column-folder'],
    actions: ['query', 'design', 'drop'],
    metadata: ['definition'],
  },
  // 宏 (DuckDB特有)
  {
    id: 'macro-folder',
    label: '宏',
    icon: 'FunctionSquare',
    isContainer: true,
    children: ['macro'],
    showCount: true,
    query: `
      SELECT macro_name as name, macro_type as type
      FROM duckdb_macros()
      WHERE schema_name = $1
      ORDER BY macro_name
    `,
  },
  {
    id: 'macro',
    label: '宏',
    icon: 'FunctionSquare',
    isContainer: false,
    actions: ['execute', 'edit', 'drop'],
    metadata: ['parameters', 'returnType', 'definition'],
  },
  // 序列
  {
    id: 'sequence-folder',
    label: '序列',
    icon: 'ArrowRightLeft',
    isContainer: true,
    children: ['sequence'],
    showCount: true,
    query: `
      SELECT sequence_name as name
      FROM information_schema.sequences
      WHERE sequence_schema = $1
      ORDER BY sequence_name
    `,
  },
  {
    id: 'sequence',
    label: '序列',
    icon: 'ArrowRightLeft',
    isContainer: false,
    actions: ['properties', 'drop'],
    metadata: ['startValue', 'increment', 'minValue', 'maxValue', 'currentValue'],
  },
]

// DuckDB 元数据配置
export const DuckDBMetaConfig: DatabaseMetaConfig = {
  dbType: 'duckdb',
  id: 'duckdb',
  name: 'DuckDB',
  version: '0.10+',

  // 支持的节点类型
  supportedNodeTypes: [
    'connection',
    'database',
    'schema',
    'table',
    'view',
    'macro',
    'sequence',
    'column',
    'constraint',
  ],

  // 标签页配置
  tabsConfig: [
    { id: 'general', label: '通用', icon: 'Info', default: true },
    { id: 'columns', label: '列', icon: 'Columns' },
    { id: 'indexes', label: '索引', icon: 'List' },
    { id: 'ddl', label: 'DDL', icon: 'Code' },
    { id: 'data', label: '数据', icon: 'Table2' },
  ],

  // 节点类型配置
  nodeTypes: duckdbNodeTypes,

  // 层级结构定义
  hierarchy: [
    { level: 0, type: 'connection' },
    { level: 1, type: 'database-folder' },
    { level: 2, type: 'database' },
    { level: 3, type: 'schema-folder' },
    { level: 4, type: 'schema' },
    { level: 5, type: 'table-folder' },
    { level: 6, type: 'table' },
    { level: 7, type: 'column-folder' },
    { level: 8, type: 'column' },
  ],

  // 功能特性
  features: {
    supportsSchema: true,
    supportsMultipleDatabases: true,
    supportsStoredProcedure: false, // DuckDB不支持存储过程
    supportsFunction: false, // 使用宏替代
    supportsTrigger: false, // DuckDB不支持触发器
    supportsView: true,
    supportsIndex: false, // DuckDB自动管理索引
    supportsConstraint: true,
    supportsSequence: true,
    supportsCustomType: false,
    supportsPartition: true,
    supportsInheritance: false,
  },

  // 默认查询
  defaultQueries: {
    tableList: `
      SELECT 
        table_schema as schema,
        table_name as name,
        estimated_size as size
      FROM information_schema.tables
      WHERE table_schema = $1 AND table_type = 'BASE TABLE'
      ORDER BY table_name
    `,
    columnList: `
      SELECT 
        column_name as name,
        data_type as type,
        is_nullable as nullable,
        column_default as default_value
      FROM information_schema.columns
      WHERE table_schema = $1 AND table_name = $2
      ORDER BY ordinal_position
    `,
    tableInfo: `
      SELECT 
        table_name as name,
        estimated_size as size
      FROM information_schema.tables
      WHERE table_schema = $1 AND table_name = $2
    `,
    macroList: `
      SELECT 
        macro_name as name,
        macro_type as type,
        parameters
      FROM duckdb_macros()
      WHERE schema_name = $1
      ORDER BY macro_name
    `,
  },

  // 属性面板配置
  propertiesConfig: {
    generalFields: [],
    table: {
      tabs: [
        { id: 'general', label: '通用', icon: 'Info', default: true },
        { id: 'columns', label: '列', icon: 'Columns' },
        { id: 'indexes', label: '索引', icon: 'List' },
        { id: 'ddl', label: 'DDL', icon: 'Code' },
        { id: 'data', label: '数据', icon: 'Table2' },
      ],
      generalFields: [
        { key: 'name', label: '表名', type: 'text' },
        { key: 'schema', label: '模式', type: 'text' },
        { key: 'rowCount', label: '行数', type: 'number' },
        { key: 'dataSize', label: '数据大小', type: 'size' },
        { key: 'createdAt', label: '创建时间', type: 'datetime' },
        { key: 'comment', label: '注释', type: 'text' },
      ],
    },
    view: {
      tabs: [
        { id: 'general', label: '通用', icon: 'Info', default: true },
        { id: 'columns', label: '列', icon: 'Columns' },
        { id: 'ddl', label: 'DDL', icon: 'Code' },
        { id: 'data', label: '数据', icon: 'Table2' },
      ],
      generalFields: [
        { key: 'name', label: '视图名', type: 'text' },
        { key: 'schema', label: '模式', type: 'text' },
        { key: 'createdAt', label: '创建时间', type: 'datetime' },
        { key: 'comment', label: '注释', type: 'text' },
      ],
    },
    column: {
      tabs: [{ id: 'general', label: '通用', icon: 'Info', default: true }],
      generalFields: [
        { key: 'name', label: '列名', type: 'text' },
        { key: 'dataType', label: '数据类型', type: 'text' },
        { key: 'nullable', label: '可空', type: 'boolean' },
        { key: 'defaultValue', label: '默认值', type: 'text' },
        { key: 'isPrimaryKey', label: '主键', type: 'boolean' },
        { key: 'comment', label: '注释', type: 'text' },
      ],
    },
  },

  // 图标映射
  icons: {
    connection: 'Database',
    database: 'Database',
    schema: 'FolderOpen',
    table: 'Table2',
    view: 'Eye',
    macro: 'FunctionSquare',
    sequence: 'ArrowRightLeft',
    column: 'Hash',
    constraint: 'Key',
  },
}
