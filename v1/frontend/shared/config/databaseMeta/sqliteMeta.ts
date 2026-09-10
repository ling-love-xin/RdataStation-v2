/**
 * SQLite 数据库元数据配置
 *
 * SQLite 结构：Connection → [Tables/Views/Indexes/Triggers] → Object
 * 注意：SQLite 是文件型数据库，没有 Database 层级
 */

import type { DatabaseMetaConfig, NavigatorNode } from '@/shared/types/databaseMeta'

export const SQLiteMetaConfig: DatabaseMetaConfig = {
  dbType: 'sqlite',
  displayName: 'SQLite',
  defaultPort: 0, // SQLite 不需要端口
  urlTemplate: 'sqlite:///{filePath}',

  // 支持的节点类型
  supportedNodeTypes: ['connection', 'table', 'view', 'index', 'trigger', 'column'],

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
      children: ['table-folder', 'view-folder', 'index-folder', 'trigger-folder'],
    },
    // 分类文件夹 - SQLite 直接挂在连接下，没有 Database 层级
    {
      id: 'table-folder',
      label: '表',
      icon: 'Folder',
      isContainer: true,
      showCount: true,
      children: ['table'],
      parentTypes: ['connection'],
    },
    {
      id: 'view-folder',
      label: '视图',
      icon: 'Folder',
      isContainer: true,
      showCount: true,
      children: ['view'],
      parentTypes: ['connection'],
    },
    {
      id: 'index-folder',
      label: '索引',
      icon: 'Folder',
      isContainer: true,
      showCount: true,
      children: ['index'],
      parentTypes: ['connection'],
    },
    {
      id: 'trigger-folder',
      label: '触发器',
      icon: 'Folder',
      isContainer: true,
      showCount: true,
      children: ['trigger'],
      parentTypes: ['connection'],
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
      id: 'index',
      label: '索引',
      icon: 'List',
      isContainer: false,
      parentTypes: ['index-folder', 'table'],
      actions: [{ id: 'ddl', label: '查看DDL', icon: 'Code', type: 'ddl' }],
    },
    {
      id: 'trigger',
      label: '触发器',
      icon: 'Zap',
      isContainer: false,
      parentTypes: ['trigger-folder', 'table'],
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
    // 子对象
    {
      id: 'column',
      label: '列',
      icon: 'Columns',
      isContainer: false,
      parentTypes: ['column-folder'],
    },
  ],

  // 层级结构 - SQLite 比 MySQL 少一层 Database
  hierarchy: [
    { level: 0, type: 'connection' },
    { level: 1, type: 'object-type-folder' },
    { level: 2, type: 'object' },
    { level: 3, type: 'sub-object-folder' },
    { level: 4, type: 'sub-object' },
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
        { key: 'rowCount', label: '行数', type: 'number' },
        { key: 'size', label: '大小', type: 'size' },
        { key: 'createdAt', label: '创建时间', type: 'datetime' },
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
        { key: 'definer', label: '定义者', type: 'text' },
        { key: 'createdAt', label: '创建时间', type: 'datetime' },
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
        { key: 'comment', label: '注释', type: 'text', editable: true },
      ],
    },
    index: {
      tabs: [
        { id: 'general', label: '通用', icon: 'Info', default: true },
        { id: 'ddl', label: 'DDL', icon: 'Code' },
      ],
      generalFields: [
        { key: 'name', label: '索引名', type: 'text' },
        { key: 'table', label: '所属表', type: 'text' },
        { key: 'unique', label: '唯一', type: 'boolean' },
        { key: 'columns', label: '列', type: 'text' },
      ],
    },
    trigger: {
      tabs: [
        { id: 'general', label: '通用', icon: 'Info', default: true },
        { id: 'ddl', label: 'DDL', icon: 'Code' },
      ],
      generalFields: [
        { key: 'name', label: '触发器名', type: 'text' },
        { key: 'table', label: '所属表', type: 'text' },
        { key: 'event', label: '事件', type: 'text' },
        { key: 'timing', label: '时机', type: 'text' },
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
      id: 'indexes',
      label: '索引',
      icon: 'List',
      filter: nodeType => nodeType === 'index',
      order: 3,
      expanded: false,
    },
    {
      id: 'triggers',
      label: '触发器',
      icon: 'Zap',
      filter: nodeType => nodeType === 'trigger',
      order: 4,
      expanded: false,
    },
  ],
}

// SQLite 查询模板
export const SQLiteQueries = {
  // 获取表列表
  getTables: () => `
    SELECT 
      name,
      sql as ddl,
      (SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = t.name) as rowCount
    FROM sqlite_master t
    WHERE type = 'table'
      AND name NOT LIKE 'sqlite_%'
    ORDER BY name
  `,

  // 获取视图列表
  getViews: () => `
    SELECT 
      name,
      sql as ddl
    FROM sqlite_master
    WHERE type = 'view'
      AND name NOT LIKE 'sqlite_%'
    ORDER BY name
  `,

  // 获取索引列表
  getIndexes: () => `
    SELECT 
      name,
      tbl_name as tableName,
      sql as ddl,
      CASE WHEN sql LIKE '%UNIQUE%' THEN 1 ELSE 0 END as isUnique
    FROM sqlite_master
    WHERE type = 'index'
      AND name NOT LIKE 'sqlite_%'
    ORDER BY name
  `,

  // 获取触发器列表
  getTriggers: () => `
    SELECT 
      name,
      tbl_name as tableName,
      sql as ddl
    FROM sqlite_master
    WHERE type = 'trigger'
    ORDER BY name
  `,

  // 获取表的索引列表
  getTableIndexes: (table: string) => `
    SELECT 
      name,
      sql as ddl,
      CASE WHEN sql LIKE '%UNIQUE%' THEN 1 ELSE 0 END as isUnique
    FROM sqlite_master
    WHERE type = 'index'
      AND tbl_name = '${table}'
      AND name NOT LIKE 'sqlite_%'
    ORDER BY name
  `,

  // 获取表的触发器列表
  getTableTriggers: (table: string) => `
    SELECT 
      name,
      sql as ddl
    FROM sqlite_master
    WHERE type = 'trigger'
      AND tbl_name = '${table}'
    ORDER BY name
  `,

  // 获取列列表
  getColumns: (table: string) => `
    PRAGMA table_info('${table}')
  `,

  // 获取索引的列
  getIndexColumns: (index: string) => `
    PRAGMA index_info('${index}')
  `,

  // 获取表DDL
  getTableDDL: (table: string) => `
    SELECT sql as ddl
    FROM sqlite_master
    WHERE type = 'table'
      AND name = '${table}'
  `,

  // 获取视图DDL
  getViewDDL: (view: string) => `
    SELECT sql as ddl
    FROM sqlite_master
    WHERE type = 'view'
      AND name = '${view}'
  `,

  // 获取索引DDL
  getIndexDDL: (index: string) => `
    SELECT sql as ddl
    FROM sqlite_master
    WHERE type = 'index'
      AND name = '${index}'
  `,

  // 获取触发器DDL
  getTriggerDDL: (trigger: string) => `
    SELECT sql as ddl
    FROM sqlite_master
    WHERE type = 'trigger'
      AND name = '${trigger}'
  `,

  // 获取表行数
  getTableRowCount: (table: string) => `
    SELECT COUNT(*) as count FROM '${table}'
  `,

  // 获取数据库文件大小
  getDatabaseSize: () => `
    SELECT page_count * page_size as size
    FROM pragma_page_count(), pragma_page_size()
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

// 辅助函数：解析 SQLite 数据类型
export function parseSQLiteDataType(type: string): {
  type: string
  nullable: boolean
  defaultValue?: string
} {
  if (!type) return { type: 'TEXT', nullable: true }

  const upperType = type.toUpperCase()
  const nullable = !upperType.includes('NOT NULL')

  // 提取默认值
  let defaultValue: string | undefined
  const defaultMatch = type.match(/DEFAULT\s+(.+?)(?:\s|$)/i)
  if (defaultMatch) {
    defaultValue = defaultMatch[1].trim()
  }

  // 提取基本类型
  const baseType = upperType
    .replace(/NOT\s+NULL/i, '')
    .replace(/DEFAULT\s+.+?(?:\s|$)/i, '')
    .replace(/PRIMARY\s+KEY/i, '')
    .replace(/AUTOINCREMENT/i, '')
    .replace(/UNIQUE/i, '')
    .trim()

  return { type: baseType, nullable, defaultValue }
}

// 辅助函数：解析表信息
export function parseTableInfo(row: Record<string, unknown>): Partial<NavigatorNode['metadata']> {
  return {
    ddl: row.ddl,
    rowCount: row.rowCount || 0,
  }
}

// 辅助函数：解析列信息
export function parseColumnInfo(row: Record<string, unknown>): Partial<NavigatorNode['metadata']> {
  return {
    dataType: row.type,
    nullable: row.notnull === 0,
    defaultValue: row.dflt_value,
    isPrimaryKey: row.pk === 1,
    ordinalPosition: row.cid,
  }
}
