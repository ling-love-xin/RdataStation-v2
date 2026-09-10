/**
 * MySQL 数据库元数据配置
 *
 * MySQL 结构：Connection → Database → [Tables/Views/Procedures/Functions/Triggers/Events] → Object
 */

import type { DatabaseMetaConfig, NavigatorNode } from '@/shared/types/databaseMeta'

export const MySQLMetaConfig: DatabaseMetaConfig = {
  dbType: 'mysql',
  displayName: 'MySQL',
  defaultPort: 3306,
  urlTemplate: 'mysql://{host}:{port}/{database}',

  // 支持的节点类型
  supportedNodeTypes: [
    'connection',
    'database',
    'table',
    'view',
    'procedure',
    'function',
    'trigger',
    'event',
    'column',
    'index',
  ],

  // 标签页配置
  tabsConfig: [
    { id: 'general', label: '通用', icon: 'Info', default: true },
    { id: 'columns', label: '列', icon: 'Columns' },
    { id: 'indexes', label: '索引', icon: 'List' },
    { id: 'ddl', label: 'DDL', icon: 'Code' },
    { id: 'data', label: '数据', icon: 'Table2' },
  ],

  // 节点类型定义
  nodeTypes: [
    {
      id: 'connection',
      label: '连接',
      icon: 'Database',
      isContainer: true,
      children: ['database'],
    },
    {
      id: 'database',
      label: '数据库',
      icon: 'FolderOpen',
      isContainer: true,
      showCount: false,
      children: [
        'table-folder',
        'view-folder',
        'procedure-folder',
        'function-folder',
        'trigger-folder',
        'event-folder',
      ],
    },
    // 分类文件夹
    {
      id: 'table-folder',
      label: '表',
      icon: 'Folder',
      isContainer: true,
      showCount: true,
      children: ['table'],
      parentTypes: ['database'],
    },
    {
      id: 'view-folder',
      label: '视图',
      icon: 'Folder',
      isContainer: true,
      showCount: true,
      children: ['view'],
      parentTypes: ['database'],
    },
    {
      id: 'procedure-folder',
      label: '存储过程',
      icon: 'Folder',
      isContainer: true,
      showCount: true,
      children: ['procedure'],
      parentTypes: ['database'],
    },
    {
      id: 'function-folder',
      label: '函数',
      icon: 'Folder',
      isContainer: true,
      showCount: true,
      children: ['function'],
      parentTypes: ['database'],
    },
    {
      id: 'trigger-folder',
      label: '触发器',
      icon: 'Folder',
      isContainer: true,
      showCount: true,
      children: ['trigger'],
      parentTypes: ['database', 'table'],
    },
    {
      id: 'event-folder',
      label: '事件',
      icon: 'Folder',
      isContainer: true,
      showCount: true,
      children: ['event'],
      parentTypes: ['database'],
    },
    // 具体对象
    {
      id: 'table',
      label: '表',
      icon: 'Table2',
      isContainer: true,
      children: ['column-folder', 'index-folder', 'trigger-folder'],
      parentTypes: ['table-folder'],
      actions: [
        { id: 'open', label: '查看数据', icon: 'Table2', type: 'open' },
        { id: 'edit', label: '编辑表', icon: 'Edit', type: 'edit' },
        { id: 'ddl', label: '查看DDL', icon: 'Code', type: 'ddl' },
        { id: 'refresh', label: '刷新', icon: 'RefreshCw', type: 'refresh' },
      ],
    },
    {
      id: 'view',
      label: '视图',
      icon: 'Eye',
      isContainer: true,
      children: ['column-folder'],
      parentTypes: ['view-folder'],
      actions: [
        { id: 'open', label: '查看数据', icon: 'Table2', type: 'open' },
        { id: 'ddl', label: '查看DDL', icon: 'Code', type: 'ddl' },
      ],
    },
    {
      id: 'procedure',
      label: '存储过程',
      icon: 'Cog',
      isContainer: false,
      parentTypes: ['procedure-folder'],
      actions: [
        { id: 'ddl', label: '查看DDL', icon: 'Code', type: 'ddl' },
        { id: 'execute', label: '执行', icon: 'Play', type: 'execute' },
      ],
    },
    {
      id: 'function',
      label: '函数',
      icon: 'FunctionSquare',
      isContainer: false,
      parentTypes: ['function-folder'],
      actions: [
        { id: 'ddl', label: '查看DDL', icon: 'Code', type: 'ddl' },
        { id: 'execute', label: '执行', icon: 'Play', type: 'execute' },
      ],
    },
    {
      id: 'trigger',
      label: '触发器',
      icon: 'Zap',
      isContainer: false,
      parentTypes: ['trigger-folder'],
      actions: [{ id: 'ddl', label: '查看DDL', icon: 'Code', type: 'ddl' }],
    },
    {
      id: 'event',
      label: '事件',
      icon: 'Clock',
      isContainer: false,
      parentTypes: ['event-folder'],
      actions: [{ id: 'ddl', label: '查看DDL', icon: 'Code', type: 'ddl' }],
    },
    // 子对象文件夹
    {
      id: 'column-folder',
      label: '列',
      icon: 'Folder',
      isContainer: true,
      showCount: true,
      children: ['column'],
      parentTypes: ['table', 'view'],
    },
    {
      id: 'index-folder',
      label: '索引',
      icon: 'Folder',
      isContainer: true,
      showCount: true,
      children: ['index'],
      parentTypes: ['table'],
    },
    // 子对象
    {
      id: 'column',
      label: '列',
      icon: 'Columns',
      isContainer: false,
      parentTypes: ['column-folder'],
    },
    {
      id: 'index',
      label: '索引',
      icon: 'List',
      isContainer: false,
      parentTypes: ['index-folder'],
    },
  ],

  // 层级结构
  hierarchy: [
    { level: 0, type: 'connection' },
    { level: 1, type: 'database' },
    { level: 2, type: 'object-type-folder' },
    { level: 3, type: 'object' },
    { level: 4, type: 'sub-object-folder' },
    { level: 5, type: 'sub-object' },
  ],

  // 属性面板配置
  propertiesConfig: {
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
        { key: 'schema', label: '数据库', type: 'text' },
        { key: 'engine', label: '引擎', type: 'text' },
        { key: 'rowCount', label: '行数', type: 'number' },
        { key: 'dataSize', label: '数据大小', type: 'size' },
        { key: 'indexSize', label: '索引大小', type: 'size' },
        { key: 'totalSize', label: '总大小', type: 'size' },
        { key: 'charset', label: '字符集', type: 'text' },
        { key: 'collation', label: '排序规则', type: 'text' },
        { key: 'createdAt', label: '创建时间', type: 'datetime' },
        { key: 'updatedAt', label: '更新时间', type: 'datetime' },
        { key: 'comment', label: '注释', type: 'text', editable: true },
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
        { key: 'schema', label: '数据库', type: 'text' },
        { key: 'definer', label: '定义者', type: 'text' },
        { key: 'isUpdatable', label: '可更新', type: 'boolean' },
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
        { key: 'isAutoIncrement', label: '自增', type: 'boolean' },
        { key: 'comment', label: '注释', type: 'text', editable: true },
      ],
    },
  },

  // 标签分组配置
  tabGroups: [
    {
      id: 'tables',
      label: '表',
      icon: 'Table2',
      filter: nodeType => nodeType === 'table',
      order: 1,
      expanded: true,
    },
    {
      id: 'views',
      label: '视图',
      icon: 'Eye',
      filter: nodeType => nodeType === 'view',
      order: 2,
      expanded: false,
    },
    {
      id: 'procedures',
      label: '存储过程',
      icon: 'Cog',
      filter: nodeType => nodeType === 'procedure',
      order: 3,
      expanded: false,
    },
    {
      id: 'functions',
      label: '函数',
      icon: 'FunctionSquare',
      filter: nodeType => nodeType === 'function',
      order: 4,
      expanded: false,
    },
    {
      id: 'triggers',
      label: '触发器',
      icon: 'Zap',
      filter: nodeType => nodeType === 'trigger',
      order: 5,
      expanded: false,
    },
    {
      id: 'events',
      label: '事件',
      icon: 'Clock',
      filter: nodeType => nodeType === 'event',
      order: 6,
      expanded: false,
    },
  ],
}

// MySQL 查询模板
export const MySQLQueries = {
  // 获取数据库列表
  getDatabases: () => `
    SELECT 
      SCHEMA_NAME as name,
      DEFAULT_CHARACTER_SET_NAME as charset,
      DEFAULT_COLLATION_NAME as collation
    FROM information_schema.SCHEMATA
    ORDER BY SCHEMA_NAME
  `,

  // 获取表列表
  getTables: (database: string) => `
    SELECT 
      TABLE_NAME as name,
      ENGINE as engine,
      TABLE_ROWS as rowCount,
      DATA_LENGTH as dataLength,
      INDEX_LENGTH as indexLength,
      TABLE_COMMENT as comment,
      CREATE_TIME as createdAt,
      UPDATE_TIME as updatedAt
    FROM information_schema.TABLES
    WHERE TABLE_SCHEMA = '${database}'
      AND TABLE_TYPE = 'BASE TABLE'
    ORDER BY TABLE_NAME
  `,

  // 获取视图列表
  getViews: (database: string) => `
    SELECT 
      TABLE_NAME as name,
      VIEW_DEFINITION as definition,
      DEFINER as definer,
      IS_UPDATABLE as isUpdatable
    FROM information_schema.VIEWS
    WHERE TABLE_SCHEMA = '${database}'
    ORDER BY TABLE_NAME
  `,

  // 获取列列表
  getColumns: (database: string, table: string) => `
    SELECT 
      COLUMN_NAME as name,
      DATA_TYPE as dataType,
      COLUMN_TYPE as fullDataType,
      IS_NULLABLE as nullable,
      COLUMN_DEFAULT as defaultValue,
      COLUMN_COMMENT as comment,
      ORDINAL_POSITION as ordinalPosition,
      COLUMN_KEY = 'PRI' as isPrimaryKey,
      EXTRA = 'auto_increment' as isAutoIncrement
    FROM information_schema.COLUMNS
    WHERE TABLE_SCHEMA = '${database}'
      AND TABLE_NAME = '${table}'
    ORDER BY ORDINAL_POSITION
  `,

  // 获取索引列表
  getIndexes: (database: string, table: string) => `
    SELECT 
      INDEX_NAME as name,
      NON_UNIQUE = 0 as isUnique,
      INDEX_TYPE as type,
      GROUP_CONCAT(COLUMN_NAME ORDER BY SEQ_IN_INDEX) as columns
    FROM information_schema.STATISTICS
    WHERE TABLE_SCHEMA = '${database}'
      AND TABLE_NAME = '${table}'
    GROUP BY INDEX_NAME, NON_UNIQUE, INDEX_TYPE
    ORDER BY INDEX_NAME
  `,

  // 获取表DDL
  getTableDDL: (database: string, table: string) => `
    SHOW CREATE TABLE \`${database}\`.\`${table}\`
  `,
}

// 辅助函数：格式化大小
export function formatSize(bytes: number): string {
  if (bytes === 0) return '0 B'
  const k = 1024
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB']
  const i = Math.floor(Math.log(bytes) / Math.log(k))
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i]
}

// 辅助函数：解析表状态结果
export function parseTableStatus(row: Record<string, unknown>): Partial<NavigatorNode['metadata']> {
  return {
    engine: row.engine,
    rowCount: row.rowCount,
    size: formatSize((row.dataLength || 0) + (row.indexLength || 0)),
    dataSize: formatSize(row.dataLength || 0),
    indexSize: formatSize(row.indexLength || 0),
    comment: row.comment,
    createdAt: row.createdAt,
    updatedAt: row.updatedAt,
  }
}
