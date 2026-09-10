/**
 * 属性面板注册表（Dynamic Registry）
 *
 * 对标 nav-router.ts 的 Record<string, NodeHandler> 模式。
 * 每种节点类型对应一个 PropertiesExtractor，返回 SubEntity[]。
 * PropertiesEditor.vue 只遍历 SubEntity[] 渲染，不关心节点类型。
 *
 * 新增数据库类型只需添加新的 extractor 条目，零前端改动。
 */

import { useDatabaseNavigatorStore } from '../stores/database-navigator-store'

import type {
  CatalogNode,
  SchemaNode,
  TableNode,
  ViewNode,
  ColumnNode,
  IndexNode,
  ConstraintNode,
} from '../types/nav-types'

// ========== SubEntity 数据模型 ==========

export interface ColumnDef {
  key: string
  label: string
  width?: string
}

export interface SubEntity {
  id: string
  label: string
  icon: string
  count: number
  kind: 'table' | 'code' | 'empty'
  table?: {
    columns: ColumnDef[]
    rows: string[][]
  }
  code?: string
  emptyMessage?: string
}

export interface PropertyRow {
  label: string
  value: string
  label2?: string
  value2?: string
}

export interface ExtractorContext {
  connectionId: string
  scope: 'global' | 'project'
  dbType: string
  catalogName: string
  schemaName: string
  objectName: string
  /** 列/索引/约束所属的表名 */
  tableName?: string
  /** 列名 */
  columnName?: string
  /** 索引名 */
  indexName?: string
  /** 约束名 */
  constraintName?: string
  navigatorStore: ReturnType<typeof useDatabaseNavigatorStore>
  t: (key: string) => string
}

// ========== 提取器类型 ==========

export interface PropertiesResult {
  title: string
  objectType: string
  scope: 'global' | 'project'
  properties: PropertyRow[]
  subEntities: SubEntity[]
}

type PropertiesExtractor = (ctx: ExtractorContext) => PropertiesResult

// ========== 工具函数 ==========

function findTable(ctx: ExtractorContext): TableNode | null {
  const catalogs = ctx.navigatorStore.connectionCatalogs.get(ctx.connectionId)
  if (!catalogs) return null
  for (const cat of catalogs) {
    if (cat.name !== ctx.catalogName) continue
    for (const schema of cat.schemas) {
      if (schema.name !== ctx.schemaName) continue
      return schema.tables.find(t => t.name === ctx.objectName) ?? null
    }
  }
  return null
}

function findView(ctx: ExtractorContext): ViewNode | null {
  const catalogs = ctx.navigatorStore.connectionCatalogs.get(ctx.connectionId)
  if (!catalogs) return null
  for (const cat of catalogs) {
    if (cat.name !== ctx.catalogName) continue
    for (const schema of cat.schemas) {
      if (schema.name !== ctx.schemaName) continue
      return schema.views.find(v => v.name === ctx.objectName) ?? null
    }
  }
  return null
}

function findSchema(ctx: ExtractorContext): SchemaNode | null {
  const catalogs = ctx.navigatorStore.connectionCatalogs.get(ctx.connectionId)
  if (!catalogs) return null
  for (const cat of catalogs) {
    if (cat.name !== ctx.catalogName) continue
    return cat.schemas.find(s => s.name === ctx.schemaName) ?? null
  }
  return null
}

function findCatalog(ctx: ExtractorContext): CatalogNode | null {
  const catalogs = ctx.navigatorStore.connectionCatalogs.get(ctx.connectionId)
  if (!catalogs) return null
  return catalogs.find(c => c.name === ctx.catalogName) ?? null
}

// ========== 列格式化 ==========

function formatColumns(columns: ColumnNode[]): string[][] {
  return columns.map((col, i) => [
    col.isPrimaryKey ? 'PK' : '',
    col.name,
    col.dataType,
    col.nullable === false ? 'NOT NULL' : '',
    col.defaultValue ?? '—',
  ])
}

const COLUMN_DEF: ColumnDef[] = [
  { key: 'pk', label: '#', width: '40px' },
  { key: 'name', label: 'Name', width: '180px' },
  { key: 'type', label: 'Data Type', width: '140px' },
  { key: 'nullable', label: 'Not Null', width: '80px' },
  { key: 'default', label: 'Default', width: '120px' },
]

function formatIndexes(indexes: IndexNode[]): string[][] {
  return indexes.map((idx, i) => [
    idx.isPrimary ? 'PK' : '',
    idx.name,
    idx.isUnique ? 'UNIQUE' : 'BTREE',
    idx.isUnique ? '✓' : '',
    idx.columns.join(', '),
  ])
}

const INDEX_DEF: ColumnDef[] = [
  { key: 'pk', label: '#', width: '40px' },
  { key: 'name', label: 'Name', width: '200px' },
  { key: 'type', label: 'Type', width: '100px' },
  { key: 'unique', label: 'Unique', width: '60px' },
  { key: 'columns', label: 'Columns', width: '200px' },
]

function formatConstraints(constraints: ConstraintNode[]): string[][] {
  return constraints.map((c, i) => [
    c.type === 'FOREIGN KEY' ? 'FK' : c.type === 'PRIMARY KEY' ? 'PK' : '',
    c.name,
    c.type,
    c.columns.join(', '),
  ])
}

const CONSTRAINT_DEF: ColumnDef[] = [
  { key: 'pk', label: '#', width: '40px' },
  { key: 'name', label: 'Name', width: '200px' },
  { key: 'type', label: 'Type', width: '120px' },
  { key: 'columns', label: 'Columns', width: '200px' },
]

function formatBytes(bytes: number | null | undefined): string {
  if (bytes == null) return '—'
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`
}

function formatNumber(n: number | null | undefined): string {
  if (n == null) return '—'
  return n.toLocaleString()
}

// ========== 提取器注册 ==========

export const propertiesRegistry: Record<string, PropertiesExtractor> = {
  /**
   * 表属性
   */
  table: (ctx: ExtractorContext): PropertiesResult => {
    const table = findTable(ctx)
    if (!table) {
      return {
        title: `${ctx.objectName} (TABLE)`,
        objectType: 'TABLE',
        scope: ctx.scope,
        properties: [{ label: 'Error', value: 'Table not found' }],
        subEntities: [],
      }
    }

    const props: PropertyRow[] = [
      { label: 'Name', value: table.name, label2: 'Type', value2: table.type },
      { label: 'Schema', value: ctx.schemaName, label2: 'Catalog', value2: ctx.catalogName },
      {
        label: 'Row Count',
        value: formatNumber(table.rowCount),
        label2: 'Data Size',
        value2: formatBytes(table.dataLength),
      },
      {
        label: 'Index Size',
        value: formatBytes(table.indexLength),
        label2: 'Total Size',
        value2: formatBytes((table.dataLength ?? 0) + (table.indexLength ?? 0)),
      },
      {
        label: 'Columns',
        value: String(table.columns.length),
        label2: 'Indexes',
        value2: String(table.indexes?.length ?? 0),
      },
      {
        label: 'Scope',
        value:
          ctx.scope === 'global' ? ctx.t('workbench.scopeGlobal') : ctx.t('workbench.scopeProject'),
        label2: '',
        value2: '',
      },
    ]

    const subEntities: SubEntity[] = []

    // Columns
    if (table.columns.length > 0) {
      subEntities.push({
        id: 'columns',
        label: 'Columns',
        icon: 'columns',
        count: table.columns.length,
        kind: 'table',
        table: { columns: COLUMN_DEF, rows: formatColumns(table.columns) },
      })
    }

    // Indexes
    if (table.indexes && table.indexes.length > 0) {
      subEntities.push({
        id: 'indexes',
        label: 'Indexes',
        icon: 'key',
        count: table.indexes.length,
        kind: 'table',
        table: { columns: INDEX_DEF, rows: formatIndexes(table.indexes) },
      })
    }

    // Constraints
    if (table.constraints && table.constraints.length > 0) {
      subEntities.push({
        id: 'constraints',
        label: 'Constraints',
        icon: 'link',
        count: table.constraints.length,
        kind: 'table',
        table: { columns: CONSTRAINT_DEF, rows: formatConstraints(table.constraints) },
      })
    }

    // DDL (placeholder — will be populated by M3 backend)
    subEntities.push({
      id: 'ddl',
      label: 'DDL',
      icon: 'code',
      count: 0,
      kind: 'code',
      code: `-- DDL for ${ctx.schemaName}.${ctx.objectName}\n-- ${ctx.t('workbench.ddlNotAvailable')}`,
    })

    return {
      title: `${ctx.objectName} (TABLE)`,
      objectType: 'TABLE',
      scope: ctx.scope,
      properties: props,
      subEntities,
    }
  },

  /**
   * 视图属性
   */
  view: (ctx: ExtractorContext): PropertiesResult => {
    const view = findView(ctx)
    if (!view) {
      return {
        title: `${ctx.objectName} (VIEW)`,
        objectType: 'VIEW',
        scope: ctx.scope,
        properties: [{ label: 'Error', value: 'View not found' }],
        subEntities: [],
      }
    }

    const props: PropertyRow[] = [
      { label: 'Name', value: view.name, label2: 'Type', value2: view.type },
      { label: 'Schema', value: ctx.schemaName, label2: 'Catalog', value2: ctx.catalogName },
      { label: 'Columns', value: String(view.columns.length), label2: '', value2: '' },
      {
        label: 'Scope',
        value:
          ctx.scope === 'global' ? ctx.t('workbench.scopeGlobal') : ctx.t('workbench.scopeProject'),
        label2: '',
        value2: '',
      },
    ]

    const subEntities: SubEntity[] = []

    if (view.columns.length > 0) {
      subEntities.push({
        id: 'columns',
        label: 'Columns',
        icon: 'columns',
        count: view.columns.length,
        kind: 'table',
        table: { columns: COLUMN_DEF, rows: formatColumns(view.columns) },
      })
    }

    subEntities.push({
      id: 'ddl',
      label: 'DDL',
      icon: 'code',
      count: 0,
      kind: 'code',
      code: `-- DDL for ${ctx.schemaName}.${ctx.objectName}\n-- ${ctx.t('workbench.ddlNotAvailable')}`,
    })

    return {
      title: `${ctx.objectName} (VIEW)`,
      objectType: 'VIEW',
      scope: ctx.scope,
      properties: props,
      subEntities,
    }
  },

  /**
   * Schema 属性
   */
  schema: (ctx: ExtractorContext): PropertiesResult => {
    const schema = findSchema(ctx)
    if (!schema) {
      return {
        title: `${ctx.schemaName} (SCHEMA)`,
        objectType: 'SCHEMA',
        scope: ctx.scope,
        properties: [{ label: 'Error', value: 'Schema not found' }],
        subEntities: [],
      }
    }

    const props: PropertyRow[] = [
      { label: 'Name', value: schema.name, label2: 'Catalog', value2: ctx.catalogName },
      {
        label: 'Tables',
        value: String(schema.tables.length),
        label2: 'Views',
        value2: String(schema.views.length),
      },
      {
        label: 'Functions',
        value: String(schema.functions?.length ?? 0),
        label2: 'Procedures',
        value2: String(schema.procedures?.length ?? 0),
      },
      {
        label: 'Sequences',
        value: String(schema.sequences?.length ?? 0),
        label2: 'Total Size',
        value2: formatBytes(schema.totalSizeBytes),
      },
      {
        label: 'Scope',
        value:
          ctx.scope === 'global' ? ctx.t('workbench.scopeGlobal') : ctx.t('workbench.scopeProject'),
        label2: '',
        value2: '',
      },
    ]

    const subEntities: SubEntity[] = []

    if (schema.tables.length > 0) {
      subEntities.push({
        id: 'tables',
        label: 'Tables',
        icon: 'table',
        count: schema.tables.length,
        kind: 'table',
        table: {
          columns: [
            { key: 'name', label: 'Name', width: '200px' },
            { key: 'type', label: 'Type', width: '100px' },
            { key: 'rows', label: 'Rows', width: '100px' },
            { key: 'size', label: 'Size', width: '100px' },
          ],
          rows: schema.tables.map(t => [
            t.name,
            t.type,
            formatNumber(t.rowCount),
            formatBytes(t.dataLength),
          ]),
        },
      })
    }

    if (schema.views.length > 0) {
      subEntities.push({
        id: 'views',
        label: 'Views',
        icon: 'eye',
        count: schema.views.length,
        kind: 'table',
        table: {
          columns: [
            { key: 'name', label: 'Name', width: '200px' },
            { key: 'type', label: 'Type', width: '100px' },
            { key: 'columns', label: 'Columns', width: '100px' },
          ],
          rows: schema.views.map(v => [v.name, v.type, String(v.columns.length)]),
        },
      })
    }

    return {
      title: `${ctx.schemaName} (SCHEMA)`,
      objectType: 'SCHEMA',
      scope: ctx.scope,
      properties: props,
      subEntities,
    }
  },

  /**
   * Catalog 属性
   */
  catalog: (ctx: ExtractorContext): PropertiesResult => {
    const catalog = findCatalog(ctx)
    if (!catalog) {
      return {
        title: `${ctx.catalogName} (CATALOG)`,
        objectType: 'CATALOG',
        scope: ctx.scope,
        properties: [{ label: 'Error', value: 'Catalog not found' }],
        subEntities: [],
      }
    }

    const totalTables = catalog.schemas.reduce((sum, s) => sum + s.tables.length, 0)
    const totalViews = catalog.schemas.reduce((sum, s) => sum + s.views.length, 0)

    const props: PropertyRow[] = [
      {
        label: 'Name',
        value: catalog.name,
        label2: 'Schemas',
        value2: String(catalog.schemas.length),
      },
      { label: 'Tables', value: String(totalTables), label2: 'Views', value2: String(totalViews) },
      {
        label: 'Scope',
        value:
          ctx.scope === 'global' ? ctx.t('workbench.scopeGlobal') : ctx.t('workbench.scopeProject'),
        label2: '',
        value2: '',
      },
    ]

    const subEntities: SubEntity[] = []

    if (catalog.schemas.length > 0) {
      subEntities.push({
        id: 'schemas',
        label: 'Schemas',
        icon: 'folder',
        count: catalog.schemas.length,
        kind: 'table',
        table: {
          columns: [
            { key: 'name', label: 'Name', width: '200px' },
            { key: 'tables', label: 'Tables', width: '80px' },
            { key: 'views', label: 'Views', width: '80px' },
          ],
          rows: catalog.schemas.map(s => [s.name, String(s.tables.length), String(s.views.length)]),
        },
      })
    }

    return {
      title: `${ctx.catalogName} (CATALOG)`,
      objectType: 'CATALOG',
      scope: ctx.scope,
      properties: props,
      subEntities,
    }
  },

  /**
   * 连接属性
   */
  connection: (ctx: ExtractorContext): PropertiesResult => {
    const catalogs = ctx.navigatorStore.connectionCatalogs.get(ctx.connectionId)
    const catalogCount = catalogs?.length ?? 0

    // 聚合统计
    let totalSchemas = 0
    let totalTables = 0
    let totalViews = 0
    if (catalogs) {
      for (const cat of catalogs) {
        totalSchemas += cat.schemas.length
        for (const schema of cat.schemas) {
          totalTables += schema.tables.length
          totalViews += schema.views.length
        }
      }
    }

    const props: PropertyRow[] = [
      { label: 'Name', value: ctx.objectName, label2: 'Type', value2: ctx.dbType || '—' },
      {
        label: 'Catalogs',
        value: String(catalogCount),
        label2: 'Schemas',
        value2: String(totalSchemas),
      },
      { label: 'Tables', value: String(totalTables), label2: 'Views', value2: String(totalViews) },
      {
        label: 'Scope',
        value:
          ctx.scope === 'global' ? ctx.t('workbench.scopeGlobal') : ctx.t('workbench.scopeProject'),
        label2: '',
        value2: '',
      },
    ]

    const subEntities: SubEntity[] = []

    if (catalogs && catalogs.length > 0) {
      subEntities.push({
        id: 'catalogs',
        label: 'Catalogs',
        icon: 'database',
        count: catalogs.length,
        kind: 'table',
        table: {
          columns: [
            { key: 'name', label: 'Name', width: '200px' },
            { key: 'schemas', label: 'Schemas', width: '100px' },
          ],
          rows: catalogs.map(c => [c.name, String(c.schemas.length)]),
        },
      })
    }

    return {
      title: `${ctx.objectName} (CONNECTION)`,
      objectType: 'CONNECTION',
      scope: ctx.scope,
      properties: props,
      subEntities,
    }
  },

  /**
   * 列属性
   */
  column: (ctx: ExtractorContext): PropertiesResult => {
    const table = findTable(ctx)
    const column = table?.columns.find(c => c.name === (ctx.columnName || ctx.objectName))

    if (!column) {
      return {
        title: `${ctx.objectName} (COLUMN)`,
        objectType: 'COLUMN',
        scope: ctx.scope,
        properties: [{ label: 'Error', value: 'Column not found' }],
        subEntities: [],
      }
    }

    const props: PropertyRow[] = [
      {
        label: 'Name',
        value: column.name,
        label2: 'Table',
        value2: ctx.tableName || ctx.objectName,
      },
      {
        label: 'Data Type',
        value: column.dataType,
        label2: 'Nullable',
        value2: column.nullable === false ? 'NOT NULL' : 'NULL',
      },
      {
        label: 'Default',
        value: column.defaultValue ?? '—',
        label2: 'Primary Key',
        value2: column.isPrimaryKey ? 'YES' : 'NO',
      },
      {
        label: 'Max Length',
        value: column.charMaxLength ? String(column.charMaxLength) : '—',
        label2: 'Numeric Prec',
        value2: column.numericPrecision ? String(column.numericPrecision) : '—',
      },
      { label: 'Schema', value: ctx.schemaName, label2: 'Catalog', value2: ctx.catalogName },
      {
        label: 'Scope',
        value:
          ctx.scope === 'global' ? ctx.t('workbench.scopeGlobal') : ctx.t('workbench.scopeProject'),
        label2: '',
        value2: '',
      },
    ]

    return {
      title: `${ctx.objectName} (COLUMN)`,
      objectType: 'COLUMN',
      scope: ctx.scope,
      properties: props,
      subEntities: [],
    }
  },

  /**
   * 索引属性
   */
  index: (ctx: ExtractorContext): PropertiesResult => {
    const table = findTable(ctx)
    const index = table?.indexes?.find(i => i.name === (ctx.indexName || ctx.objectName))

    if (!index) {
      return {
        title: `${ctx.objectName} (INDEX)`,
        objectType: 'INDEX',
        scope: ctx.scope,
        properties: [{ label: 'Error', value: 'Index not found' }],
        subEntities: [],
      }
    }

    const props: PropertyRow[] = [
      { label: 'Name', value: index.name, label2: 'Table', value2: ctx.tableName || '' },
      {
        label: 'Type',
        value: (index as { type?: string }).type || 'BTREE',
        label2: 'Unique',
        value2: index.isUnique ? 'YES' : 'NO',
      },
      {
        label: 'Primary',
        value: index.isPrimary ? 'YES' : 'NO',
        label2: 'Columns',
        value2: index.columns.join(', '),
      },
      { label: 'Schema', value: ctx.schemaName, label2: 'Catalog', value2: ctx.catalogName },
      {
        label: 'Scope',
        value:
          ctx.scope === 'global' ? ctx.t('workbench.scopeGlobal') : ctx.t('workbench.scopeProject'),
        label2: '',
        value2: '',
      },
    ]

    return {
      title: `${ctx.objectName} (INDEX)`,
      objectType: 'INDEX',
      scope: ctx.scope,
      properties: props,
      subEntities: [],
    }
  },

  /**
   * 约束属性
   */
  constraint: (ctx: ExtractorContext): PropertiesResult => {
    const table = findTable(ctx)
    const constraint = table?.constraints?.find(
      c => c.name === (ctx.constraintName || ctx.objectName)
    )

    if (!constraint) {
      return {
        title: `${ctx.objectName} (CONSTRAINT)`,
        objectType: 'CONSTRAINT',
        scope: ctx.scope,
        properties: [{ label: 'Error', value: 'Constraint not found' }],
        subEntities: [],
      }
    }

    const props: PropertyRow[] = [
      { label: 'Name', value: constraint.name, label2: 'Table', value2: ctx.tableName || '' },
      {
        label: 'Type',
        value: constraint.type,
        label2: 'Columns',
        value2: constraint.columns.join(', '),
      },
      { label: 'Schema', value: ctx.schemaName, label2: 'Catalog', value2: ctx.catalogName },
      {
        label: 'Scope',
        value:
          ctx.scope === 'global' ? ctx.t('workbench.scopeGlobal') : ctx.t('workbench.scopeProject'),
        label2: '',
        value2: '',
      },
    ]

    // 外键额外信息
    const refTable = (constraint as { referencedTable?: string }).referencedTable
    const refCols = (constraint as { referencedColumns?: string[] }).referencedColumns
    if (refTable) {
      props.push(
        {
          label: 'Ref Table',
          value: refTable,
          label2: 'Ref Columns',
          value2: refCols?.join(', ') || '—',
        },
        {
          label: 'Update Rule',
          value: (constraint as { updateRule?: string }).updateRule || '—',
          label2: 'Delete Rule',
          value2: (constraint as { deleteRule?: string }).deleteRule || '—',
        }
      )
    }

    return {
      title: `${ctx.objectName} (CONSTRAINT)`,
      objectType: 'CONSTRAINT',
      scope: ctx.scope,
      properties: props,
      subEntities: [],
    }
  },

  /**
   * 存储过程属性
   */
  procedure: (ctx: ExtractorContext): PropertiesResult => {
    const props: PropertyRow[] = [
      { label: 'Name', value: ctx.objectName, label2: 'Type', value2: 'PROCEDURE' },
      { label: 'Schema', value: ctx.schemaName, label2: 'Catalog', value2: ctx.catalogName },
      {
        label: 'Scope',
        value:
          ctx.scope === 'global' ? ctx.t('workbench.scopeGlobal') : ctx.t('workbench.scopeProject'),
        label2: '',
        value2: '',
      },
    ]
    return {
      title: `${ctx.objectName} (PROCEDURE)`,
      objectType: 'PROCEDURE',
      scope: ctx.scope,
      properties: props,
      subEntities: [],
    }
  },

  /**
   * 函数属性
   */
  function: (ctx: ExtractorContext): PropertiesResult => {
    const props: PropertyRow[] = [
      { label: 'Name', value: ctx.objectName, label2: 'Type', value2: 'FUNCTION' },
      { label: 'Schema', value: ctx.schemaName, label2: 'Catalog', value2: ctx.catalogName },
      {
        label: 'Scope',
        value:
          ctx.scope === 'global' ? ctx.t('workbench.scopeGlobal') : ctx.t('workbench.scopeProject'),
        label2: '',
        value2: '',
      },
    ]
    return {
      title: `${ctx.objectName} (FUNCTION)`,
      objectType: 'FUNCTION',
      scope: ctx.scope,
      properties: props,
      subEntities: [],
    }
  },

  /**
   * 序列属性
   */
  sequence: (ctx: ExtractorContext): PropertiesResult => {
    const props: PropertyRow[] = [
      { label: 'Name', value: ctx.objectName, label2: 'Type', value2: 'SEQUENCE' },
      { label: 'Schema', value: ctx.schemaName, label2: 'Catalog', value2: ctx.catalogName },
      {
        label: 'Scope',
        value:
          ctx.scope === 'global' ? ctx.t('workbench.scopeGlobal') : ctx.t('workbench.scopeProject'),
        label2: '',
        value2: '',
      },
    ]
    return {
      title: `${ctx.objectName} (SEQUENCE)`,
      objectType: 'SEQUENCE',
      scope: ctx.scope,
      properties: props,
      subEntities: [],
    }
  },

  /**
   * 触发器属性
   */
  trigger: (ctx: ExtractorContext): PropertiesResult => {
    const props: PropertyRow[] = [
      { label: 'Name', value: ctx.objectName, label2: 'Type', value2: 'TRIGGER' },
      { label: 'Schema', value: ctx.schemaName, label2: 'Catalog', value2: ctx.catalogName },
      {
        label: 'Scope',
        value:
          ctx.scope === 'global' ? ctx.t('workbench.scopeGlobal') : ctx.t('workbench.scopeProject'),
        label2: '',
        value2: '',
      },
    ]
    return {
      title: `${ctx.objectName} (TRIGGER)`,
      objectType: 'TRIGGER',
      scope: ctx.scope,
      properties: props,
      subEntities: [],
    }
  },
}
