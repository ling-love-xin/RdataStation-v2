/**
 * 导航树节点加载路由器
 *
 * 将 loadChildren 中的 if/else 链拆分为独立的 handler 函数，
 * 通过 nodeType → handler 映射实现路由分发。
 */

import type { NavigationConfig } from '@/extensions/builtin/connection/ui/types/form-schema'

import { NodeKeyEncoder } from '../types/virtual-tree'

import type { useDatabaseNavigatorStore } from '../stores/database-navigator-store'
import type { VirtualTreeNode } from '../types/virtual-tree'

// ============================================================================
// 类型定义
// ============================================================================

export interface NodeHandlerContext {
  node: VirtualTreeNode
  keyParts: string[]
  connectionId: string
  dbType: string
  config: NavigationConfig
  navigatorStore: ReturnType<typeof useDatabaseNavigatorStore>
  runtimeConnectionStore: { runtimeConnectionIds: Map<string, string> }

  // 工厂函数（由 use-database-tree-loader 注入，避免循环依赖）
  createCatalogNodes: (connectionId: string, scope: 'global' | 'project') => VirtualTreeNode[]
  createSchemaNodes: (
    connectionId: string,
    dbName: string,
    config: NavigationConfig
  ) => VirtualTreeNode[]
  createCatalogObjectNodes: (
    connectionId: string,
    dbName: string,
    config: NavigationConfig,
    parentKey?: string,
    baseLevel?: number
  ) => VirtualTreeNode[]
  createSchemaObjectNodes: (
    connectionId: string,
    dbName: string,
    schemaName: string,
    config: NavigationConfig
  ) => VirtualTreeNode[]
  createTableNodes: (
    connectionId: string,
    dbName: string,
    schemaName: string | undefined,
    config: NavigationConfig,
    parentLevel?: number
  ) => VirtualTreeNode[]
  createViewNodes: (
    connectionId: string,
    dbName: string,
    schemaName: string | undefined,
    parentLevel?: number
  ) => VirtualTreeNode[]
  createProcedureNodes: (
    connectionId: string,
    dbName: string,
    schemaName: string | undefined
  ) => VirtualTreeNode[]
  createFunctionNodes: (
    connectionId: string,
    dbName: string,
    schemaName: string | undefined
  ) => VirtualTreeNode[]
  createSequenceNodes: (
    connectionId: string,
    dbName: string,
    schemaName: string | undefined
  ) => VirtualTreeNode[]
  createTriggerNodes: (
    connectionId: string,
    dbName: string,
    schemaName: string | undefined
  ) => VirtualTreeNode[]
  createTableSubFolderNodes: (
    connectionId: string,
    dbName: string,
    schemaName: string | undefined,
    tableName: string,
    config: NavigationConfig
  ) => VirtualTreeNode[]
  createColumnNodes: (
    connectionId: string,
    dbName: string,
    schemaName: string | undefined,
    tableName: string
  ) => VirtualTreeNode[]
  createIndexNodes: (
    connectionId: string,
    dbName: string,
    schemaName: string | undefined,
    tableName: string
  ) => VirtualTreeNode[]
  createConstraintNodes: (
    connectionId: string,
    dbName: string,
    schemaName: string | undefined,
    tableName: string
  ) => VirtualTreeNode[]
}

type NodeHandler = (ctx: NodeHandlerContext) => Promise<VirtualTreeNode[]>

// ============================================================================
// Handler 实现（从 loadChildren 原有逻辑逐块提取）
// ============================================================================

const handleConnection: NodeHandler = async ctx => {
  const {
    keyParts,
    config,
    navigatorStore,
    runtimeConnectionStore,
    createCatalogNodes,
    createCatalogObjectNodes,
  } = ctx
  const scopeRaw = keyParts[1]
  const scope: 'global' | 'project' =
    scopeRaw === 'global' || scopeRaw === 'project' ? scopeRaw : 'global'
  const connId = keyParts[2]

  const hasRuntimeConn = runtimeConnectionStore.runtimeConnectionIds.has(connId)

  try {
    // 在线：从数据库加载 catalogs
    if (hasRuntimeConn) {
      await navigatorStore.loadCatalogs(connId)
    } else {
      // 离线：尝试从 L2 缓存加载 catalogs
      await navigatorStore.loadCatalogsFromCacheSilent(connId)
    }
  } catch (err) {
    console.error(`[nav-router] 加载 catalogs 失败 (${connId}):`, err)
    // 返回空列表，让用户看到"连接失败"的状态
    return []
  }

  if (config.hasCatalogs) {
    return createCatalogNodes(connId, scope)
  }

  const catalogs = navigatorStore.getCatalogs(connId)
  if (catalogs.length === 0) {
    // 无 catalogs 数据，返回空列表
    return []
  }
  const defaultCatalogName = catalogs[0]?.name || 'main'
  const connKey = NodeKeyEncoder.encode(['connection', scope, connId])
  return createCatalogObjectNodes(connId, defaultCatalogName, config, connKey, 1)
}

const handleCatalog: NodeHandler = async ctx => {
  const {
    keyParts,
    connectionId,
    config,
    navigatorStore,
    createSchemaNodes,
    createCatalogObjectNodes,
  } = ctx
  const dbName = keyParts[2]

  if (config.hasSchemas) {
    // 缓存守卫：如果 catalog 已有 schema 数据，跳过重复加载
    const catalogs = navigatorStore.getCatalogs(connectionId)
    const catalog = catalogs.find(c => c.name === dbName)
    const hasSchemaData = !!(catalog && catalog.schemas && catalog.schemas.length > 0)
    if (!hasSchemaData) {
      await navigatorStore.loadSchemas(connectionId, dbName)
    }
    return createSchemaNodes(connectionId, dbName, config)
  }

  return createCatalogObjectNodes(connectionId, dbName, config)
}

const handleSchema: NodeHandler = async ctx => {
  const { keyParts, connectionId, config, navigatorStore, createSchemaObjectNodes } = ctx
  const dbName = keyParts[2]
  const schemaName = keyParts[3]

  // 缓存守卫：如果 schema 已有表/视图数据，跳过重复加载
  const catalogs = navigatorStore.getCatalogs(connectionId)
  const catalog = catalogs.find(c => c.name === dbName)
  let hasTableData = false
  if (catalog) {
    const schema = catalog.schemas?.find(s => s.name === schemaName)
    hasTableData = !!(schema && (schema.tables.length > 0 || schema.views.length > 0))
  }
  if (!hasTableData) {
    await navigatorStore.loadTables(connectionId, dbName, schemaName)
  }

  return createSchemaObjectNodes(connectionId, dbName, schemaName, config)
}

const handleTablesFolder: NodeHandler = async ctx => {
  const { node, keyParts, connectionId, config, navigatorStore, createTableNodes } = ctx
  const dbName = keyParts[2]
  const schemaName = keyParts[3] || undefined

  // 优化：如果 Schema 展开时已加载过表数据，跳过重复请求
  const catalogs = navigatorStore.getCatalogs(connectionId)
  const catalog = catalogs.find(c => c.name === dbName)
  let hasTableData = false
  if (catalog) {
    if (schemaName) {
      const schema = catalog.schemas?.find(s => s.name === schemaName)
      hasTableData = !!(schema && (schema.tables.length > 0 || schema.views.length > 0))
    } else {
      hasTableData = !!(catalog.tables && catalog.tables.length > 0)
    }
  }

  if (!hasTableData) {
    if (schemaName) {
      await navigatorStore.loadTables(connectionId, dbName, schemaName)
    } else {
      // MySQL 等无 Schema 的数据库：用 dbName 代替 schemaName
      await navigatorStore.loadTables(connectionId, dbName, dbName)
    }
  }

  return createTableNodes(connectionId, dbName, schemaName, config, node.level)
}

const handleViewsFolder: NodeHandler = async ctx => {
  const { node, keyParts, connectionId, navigatorStore, createViewNodes } = ctx
  const dbName = keyParts[2]
  const schemaName = keyParts[3] || undefined

  // 同上：如果表数据已加载，跳过重复请求
  const catalogs = navigatorStore.getCatalogs(connectionId)
  const catalog = catalogs.find(c => c.name === dbName)
  let hasTableData = false
  if (catalog && schemaName) {
    const schema = catalog.schemas?.find(s => s.name === schemaName)
    hasTableData = !!(schema && (schema.tables.length > 0 || schema.views.length > 0))
  }

  if (!hasTableData && schemaName) {
    await navigatorStore.loadTables(connectionId, dbName, schemaName)
  }

  return createViewNodes(connectionId, dbName, schemaName, node.level)
}

const handleProceduresFolder: NodeHandler = async ctx => {
  const { keyParts, connectionId, navigatorStore, createProcedureNodes } = ctx
  const dbName = keyParts[2]
  const schemaName = keyParts[3] || undefined

  if (schemaName) {
    await navigatorStore.loadProcedures(connectionId, dbName, schemaName)
  }

  return createProcedureNodes(connectionId, dbName, schemaName)
}

const handleFunctionsFolder: NodeHandler = async ctx => {
  const { keyParts, connectionId, navigatorStore, createFunctionNodes } = ctx
  const dbName = keyParts[2]
  const schemaName = keyParts[3] || undefined

  if (schemaName) {
    await navigatorStore.loadFunctions(connectionId, dbName, schemaName)
  }

  return createFunctionNodes(connectionId, dbName, schemaName)
}

const handleSequencesFolder: NodeHandler = async ctx => {
  const { keyParts, connectionId, navigatorStore, createSequenceNodes } = ctx
  const dbName = keyParts[2]
  const schemaName = keyParts[3] || undefined

  if (schemaName) {
    await navigatorStore.loadSequences(connectionId, dbName, schemaName)
  }

  return createSequenceNodes(connectionId, dbName, schemaName)
}

const handleTriggersFolder: NodeHandler = async ctx => {
  const { keyParts, connectionId, navigatorStore, createTriggerNodes } = ctx
  const dbName = keyParts[2]
  const schemaName = keyParts[3] || undefined

  if (schemaName) {
    await navigatorStore.loadTriggers(connectionId, dbName, schemaName)
  }

  return createTriggerNodes(connectionId, dbName, schemaName)
}

const handleTable: NodeHandler = async ctx => {
  const { keyParts, connectionId, config, navigatorStore, createTableSubFolderNodes } = ctx
  const dbName = keyParts[2]
  const schemaName = keyParts[3] || undefined
  const tableName = keyParts[4]

  await navigatorStore.loadColumns(connectionId, dbName, schemaName || '', tableName)

  return createTableSubFolderNodes(connectionId, dbName, schemaName, tableName, config)
}

const handleView: NodeHandler = async ctx => {
  const { keyParts, connectionId, navigatorStore, createColumnNodes } = ctx
  const dbName = keyParts[2]
  const schemaName = keyParts[3] || undefined
  const viewName = keyParts[4]

  await navigatorStore.loadColumns(connectionId, dbName, schemaName || '', viewName)

  return createColumnNodes(connectionId, dbName, schemaName, viewName)
}

const handleColumnsFolder: NodeHandler = async ctx => {
  const { keyParts, connectionId, createColumnNodes } = ctx
  const dbName = keyParts[2]
  const schemaName = keyParts[3] || undefined
  const tableName = keyParts[4]
  return createColumnNodes(connectionId, dbName, schemaName, tableName)
}

const handleIndexesFolder: NodeHandler = async ctx => {
  const { keyParts, connectionId, navigatorStore, createIndexNodes } = ctx
  const dbName = keyParts[2]
  const schemaName = keyParts[3] || undefined
  const tableName = keyParts[4]
  if (schemaName) {
    await navigatorStore.loadIndexes(connectionId, dbName, schemaName, tableName)
  }
  return createIndexNodes(connectionId, dbName, schemaName, tableName)
}

const handleConstraintsFolder: NodeHandler = async ctx => {
  const { keyParts, connectionId, navigatorStore, createConstraintNodes } = ctx
  const dbName = keyParts[2]
  const schemaName = keyParts[3] || undefined
  const tableName = keyParts[4]
  if (schemaName) {
    await navigatorStore.loadConstraints(connectionId, dbName, schemaName, tableName)
  }
  return createConstraintNodes(connectionId, dbName, schemaName, tableName)
}

// ============================================================================
// 路由表
// ============================================================================

export const nodeHandlers: Record<string, NodeHandler> = {
  connection: handleConnection,
  catalog: handleCatalog,
  schema: handleSchema,
  'tables-folder': handleTablesFolder,
  'views-folder': handleViewsFolder,
  'procedures-folder': handleProceduresFolder,
  'functions-folder': handleFunctionsFolder,
  'sequences-folder': handleSequencesFolder,
  'triggers-folder': handleTriggersFolder,
  table: handleTable,
  view: handleView,
  'columns-folder': handleColumnsFolder,
  'indexes-folder': handleIndexesFolder,
  'constraints-folder': handleConstraintsFolder,
}
