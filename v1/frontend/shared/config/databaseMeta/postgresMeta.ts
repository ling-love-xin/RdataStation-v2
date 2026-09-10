/**
 * PostgreSQL 元数据配置
 * 定义 PostgreSQL 数据库的导航结构和节点类型
 */

import type { DatabaseMetaConfig, NodeTypeConfig } from '@/shared/types/databaseMeta'

// PostgreSQL 支持的节点类型
const postgresNodeTypes: NodeTypeConfig[] = [
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
    query: `
      SELECT datname as name 
      FROM pg_database 
      WHERE datistemplate = false 
      ORDER BY datname
    `,
  },
  {
    id: 'database',
    label: '数据库',
    icon: 'Database',
    isContainer: true,
    children: ['schema-folder'],
    actions: ['query', 'backup', 'restore'],
    metadata: ['encoding', 'collate', 'ctype', 'size'],
  },
  {
    id: 'schema-folder',
    label: 'Schema',
    icon: 'Folder',
    isContainer: true,
    children: ['schema'],
    showCount: true,
    query: `
      SELECT schema_name as name 
      FROM information_schema.schemata 
      WHERE schema_name NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
      ORDER BY schema_name
    `,
  },
  {
    id: 'schema',
    label: 'Schema',
    icon: 'FolderOpen',
    isContainer: true,
    children: [
      'table-folder',
      'view-folder',
      'function-folder',
      'procedure-folder',
      'sequence-folder',
      'type-folder',
    ],
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
    children: ['column-folder', 'index-folder', 'constraint-folder', 'trigger-folder'],
    actions: ['query', 'design', 'truncate', 'drop'],
    metadata: ['rowCount', 'size', 'owner', 'created'],
  },
  {
    id: 'column-folder',
    label: '列',
    icon: 'Columns',
    isContainer: true,
    children: ['column'],
    showCount: true,
    query: `
      SELECT column_name as name, data_type, is_nullable, column_default
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
    metadata: ['dataType', 'nullable', 'default', 'isPrimaryKey', 'isForeignKey'],
  },
  {
    id: 'index-folder',
    label: '索引',
    icon: 'List',
    isContainer: true,
    children: ['index'],
    showCount: true,
    query: `
      SELECT indexname as name, indexdef as definition
      FROM pg_indexes
      WHERE schemaname = $1 AND tablename = $2
      ORDER BY indexname
    `,
  },
  {
    id: 'index',
    label: '索引',
    icon: 'List',
    isContainer: false,
    actions: ['drop'],
    metadata: ['isUnique', 'isPrimary', 'columns'],
  },
  {
    id: 'constraint-folder',
    label: '约束',
    icon: 'Key',
    isContainer: true,
    children: ['constraint'],
    showCount: true,
    query: `
      SELECT constraint_name as name, constraint_type as type
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
    metadata: ['definition', 'owner'],
  },
  // 函数
  {
    id: 'function-folder',
    label: '函数',
    icon: 'FunctionSquare',
    isContainer: true,
    children: ['function'],
    showCount: true,
    query: `
      SELECT routine_name as name, routine_type as type
      FROM information_schema.routines
      WHERE routine_schema = $1 AND routine_type = 'FUNCTION'
      ORDER BY routine_name
    `,
  },
  {
    id: 'function',
    label: '函数',
    icon: 'FunctionSquare',
    isContainer: false,
    actions: ['execute', 'edit', 'drop'],
    metadata: ['returnType', 'arguments', 'language'],
  },
  // 存储过程
  {
    id: 'procedure-folder',
    label: '存储过程',
    icon: 'Workflow',
    isContainer: true,
    children: ['procedure'],
    showCount: true,
    query: `
      SELECT routine_name as name
      FROM information_schema.routines
      WHERE routine_schema = $1 AND routine_type = 'PROCEDURE'
      ORDER BY routine_name
    `,
  },
  {
    id: 'procedure',
    label: '存储过程',
    icon: 'Workflow',
    isContainer: false,
    actions: ['execute', 'edit', 'drop'],
    metadata: ['arguments', 'language'],
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
  // 自定义类型
  {
    id: 'type-folder',
    label: '类型',
    icon: 'Type',
    isContainer: true,
    children: ['type'],
    showCount: true,
    query: `
      SELECT typname as name
      FROM pg_type t
      JOIN pg_namespace n ON t.typnamespace = n.oid
      WHERE n.nspname = $1 AND t.typtype = 'c'
      ORDER BY typname
    `,
  },
  {
    id: 'type',
    label: '类型',
    icon: 'Type',
    isContainer: false,
    actions: ['properties', 'drop'],
    metadata: ['type', 'attributes'],
  },
  // 触发器
  {
    id: 'trigger-folder',
    label: '触发器',
    icon: 'Zap',
    isContainer: true,
    children: ['trigger'],
    showCount: true,
    query: `
      SELECT trigger_name as name, event_manipulation as event
      FROM information_schema.triggers
      WHERE event_object_schema = $1 AND event_object_table = $2
      ORDER BY trigger_name
    `,
  },
  {
    id: 'trigger',
    label: '触发器',
    icon: 'Zap',
    isContainer: false,
    actions: ['enable', 'disable', 'drop'],
    metadata: ['event', 'timing', 'action'],
  },
]

// PostgreSQL 元数据配置
export const PostgresMetaConfig: DatabaseMetaConfig = {
  dbType: 'postgres',
  id: 'postgres',
  name: 'PostgreSQL',
  version: '14.0+',

  // 支持的节点类型
  supportedNodeTypes: [
    'connection',
    'database',
    'schema',
    'table',
    'view',
    'function',
    'procedure',
    'sequence',
    'type',
    'column',
    'index',
    'constraint',
    'trigger',
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
  nodeTypes: postgresNodeTypes,

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
    supportsStoredProcedure: true,
    supportsFunction: true,
    supportsTrigger: true,
    supportsView: true,
    supportsIndex: true,
    supportsConstraint: true,
    supportsSequence: true,
    supportsCustomType: true,
    supportsPartition: true,
    supportsInheritance: true,
  },

  // 默认查询
  defaultQueries: {
    tableList: `
      SELECT 
        schemaname as schema,
        tablename as name,
        pg_size_pretty(pg_total_relation_size(schemaname||'.'||tablename)) as size
      FROM pg_tables
      WHERE schemaname = $1
      ORDER BY tablename
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
    indexList: `
      SELECT 
        indexname as name,
        indexdef as definition
      FROM pg_indexes
      WHERE schemaname = $1 AND tablename = $2
    `,
    tableInfo: `
      SELECT 
        relname as name,
        n_live_tup as row_count,
        pg_size_pretty(pg_total_relation_size(relid)) as size
      FROM pg_stat_user_tables
      WHERE schemaname = $1 AND relname = $2
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
    function: 'FunctionSquare',
    procedure: 'Workflow',
    sequence: 'ArrowRightLeft',
    type: 'Type',
    column: 'Hash',
    index: 'List',
    constraint: 'Key',
    trigger: 'Zap',
  },
}
