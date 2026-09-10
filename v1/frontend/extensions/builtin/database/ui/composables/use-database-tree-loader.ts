/**
 * 数据库树节点加载器
 *
 * 实现 DBeaver 级别的目录树结构
 * 支持动态渲染，根据数据库类型自适应结构
 */

import { useRuntimeConnectionStore } from '@/extensions/builtin/connection/ui/stores/runtime-connection-store'
import type { ProjectConnection } from '@/extensions/builtin/connection/ui/types/connection'
import type {
  NavigationConfig,
  NavigationFolderConfig,
} from '@/extensions/builtin/connection/ui/types/form-schema'
import {
  loadNavigationConfig,
  getDefaultNavigationConfig,
} from '@/extensions/builtin/connection/ui/utils/schema-loader'

import { nodeHandlers } from './nav-router'
import { useDatabaseNavigatorStore } from '../stores/database-navigator-store'
import { NodeKeyEncoder } from '../types/virtual-tree'

import type { VirtualTreeNode } from '../types/virtual-tree'

interface GlobalConnection {
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

const navConfigCache = new Map<string, NavigationConfig>()

async function getNavConfig(dbType: string): Promise<NavigationConfig> {
  const key = dbType.toLowerCase()
  if (navConfigCache.has(key)) {
    return navConfigCache.get(key) ?? getDefaultNavigationConfig()
  }
  const config = await loadNavigationConfig(key)
  const resolved = config || getDefaultNavigationConfig()
  navConfigCache.set(key, resolved)
  return resolved
}

function isFolderEnabled(config: NavigationConfig, folderKey: string): boolean {
  const folders = config.folders as Record<string, NavigationFolderConfig | undefined>
  return folders[folderKey]?.enabled ?? false
}

/** 计算 NavigationConfig 中启用的文件夹数量 */
function countEnabledFolders(config: NavigationConfig): number {
  let count = 0
  const folderKeys: (keyof typeof config.folders)[] = [
    'tables',
    'views',
    'functions',
    'procedures',
    'sequences',
    'triggers',
  ]
  for (const key of folderKeys) {
    if (isFolderEnabled(config, key)) count++
  }
  return count
}

/** 计算表节点的子节点数量（Columns + 启用的 Indexes/Constraints 等） */
function countTableChildren(config: NavigationConfig, _columnCount: number): number {
  let count = 0
  if (config.tableChildren.columns) count++ // columns-folder
  if (config.tableChildren.indexes) count++
  if (config.tableChildren.constraints) count++
  if (config.tableChildren.triggers) count++
  if (config.tableChildren.foreignKeys) count++
  if (config.tableChildren.references) count++
  return count
}

export function useDatabaseTreeLoader() {
  const navigatorStore = useDatabaseNavigatorStore()
  const runtimeConnectionStore = useRuntimeConnectionStore()

  // AbortController 用于取消进行中的加载请求（组件卸载时）
  let currentAbortController: AbortController | null = null

  /**
   * 创建连接节点
   */
  function createConnectionNode(
    conn: ProjectConnection | GlobalConnection,
    scope: 'global' | 'project'
  ): VirtualTreeNode {
    const catalogs = navigatorStore.getCatalogs(conn.id)
    const hasRuntimeConn = runtimeConnectionStore.runtimeConnectionIds.has(conn.id)
    const key = NodeKeyEncoder.encode(['connection', scope, conn.id])

    return {
      key,
      level: 0,
      isExpanded: false,
      isLeaf: false,
      label: conn.name,
      type: 'connection',
      data: { connectionId: conn.id, driver: conn.driver, scope },
      parentId: null,
      childCount: catalogs.length,
      connectionTags: [scope === 'global' ? 'Global' : 'Project'],
      connectionStatus: hasRuntimeConn ? 'connected' : 'disconnected',
    }
  }

  /**
   * 创建错误占位节点（用户可见可重试）
   */
  function createErrorPlaceholderNode(parentKey: string, errorMessage: string): VirtualTreeNode {
    return {
      key: `${parentKey}::error`,
      level: 0, // 由调用方覆盖
      isExpanded: false,
      isLeaf: true,
      label: `⚠ 加载失败：${errorMessage}`,
      type: 'placeholder',
      data: { errorMessage, isError: true },
      parentId: parentKey,
      childCount: 0,
    }
  }

  /**
   * 创建 Catalog 节点（ANSI SQL 标准：Catalog → Schema → Table）
   */
  function createCatalogNodes(
    connectionId: string,
    scope: 'global' | 'project'
  ): VirtualTreeNode[] {
    const catalogs = navigatorStore.getCatalogs(connectionId)
    const parentKey = NodeKeyEncoder.encode(['connection', scope, connectionId])

    return catalogs.map(cat => ({
      key: NodeKeyEncoder.encode(['catalog', connectionId, cat.name]),
      level: 1,
      isExpanded: false,
      isLeaf: false,
      label: cat.name,
      type: 'catalog',
      data: { connectionId, catalogName: cat.name },
      parentId: parentKey,
      childCount: cat.schemas?.length || 0,
    }))
  }

  /**
   * 创建 Schema 节点（PostgreSQL/DuckDB）
   */
  function createSchemaNodes(
    connectionId: string,
    dbName: string,
    config: NavigationConfig
  ): VirtualTreeNode[] {
    const schemas = navigatorStore.getCatalogSchemas(connectionId, dbName)
    const parentKey = NodeKeyEncoder.encode(['catalog', connectionId, dbName])
    const folderCount = countEnabledFolders(config)

    return schemas
      .filter(schema => !config.systemSchemas.includes(schema.name))
      .map(schema => ({
        key: NodeKeyEncoder.encode(['schema', connectionId, dbName, schema.name]),
        level: 2,
        isExpanded: false,
        isLeaf: false,
        label: schema.name,
        type: 'schema',
        data: {
          connectionId,
          dbName,
          schemaName: schema.name,
          tableCount: schema.totalTables,
          viewCount: schema.totalViews,
          totalSizeBytes: schema.totalSizeBytes,
          rowCountTotal: schema.rowCountTotal,
        },
        parentId: parentKey,
        childCount: folderCount,
      }))
  }

  /**
   * 创建对象文件夹（Catalog/Schema 下通用，也支持直接挂 Connection 下）
   */
  function createCatalogObjectNodes(
    connectionId: string,
    dbName: string,
    config: NavigationConfig,
    parentKey?: string,
    baseLevel?: number
  ): VirtualTreeNode[] {
    const key = parentKey || NodeKeyEncoder.encode(['catalog', connectionId, dbName])
    const level = baseLevel ?? 2
    const nodes: VirtualTreeNode[] = []

    if (isFolderEnabled(config, 'tables')) {
      nodes.push({
        key: NodeKeyEncoder.encode(['tables-folder', connectionId, dbName]),
        level,
        isExpanded: false,
        isLeaf: false,
        label: config.folders.tables.label,
        type: 'tables-folder',
        data: { connectionId, dbName },
        parentId: key,
        childCount: 0,
      })
    }

    if (isFolderEnabled(config, 'views')) {
      nodes.push({
        key: NodeKeyEncoder.encode(['views-folder', connectionId, dbName]),
        level,
        isExpanded: false,
        isLeaf: false,
        label: config.folders.views.label,
        type: 'views-folder',
        data: { connectionId, dbName },
        parentId: key,
        childCount: 0,
      })
    }

    if (isFolderEnabled(config, 'functions')) {
      nodes.push({
        key: NodeKeyEncoder.encode(['functions-folder', connectionId, dbName]),
        level,
        isExpanded: false,
        isLeaf: false,
        label: config.folders.functions.label,
        type: 'functions-folder',
        data: { connectionId, dbName },
        parentId: key,
        childCount: 0,
      })
    }

    if (isFolderEnabled(config, 'procedures')) {
      nodes.push({
        key: NodeKeyEncoder.encode(['procedures-folder', connectionId, dbName]),
        level,
        isExpanded: false,
        isLeaf: false,
        label: config.folders.procedures.label,
        type: 'procedures-folder',
        data: { connectionId, dbName },
        parentId: key,
        childCount: 0,
      })
    }

    if (isFolderEnabled(config, 'triggers')) {
      nodes.push({
        key: NodeKeyEncoder.encode(['triggers-folder', connectionId, dbName]),
        level,
        isExpanded: false,
        isLeaf: false,
        label: config.folders.triggers.label,
        type: 'triggers-folder',
        data: { connectionId, dbName },
        parentId: key,
        childCount: 0,
      })
    }

    return nodes
  }

  /**
   * 创建 Schema 下的对象文件夹（DBeaver 风格）
   */
  function createSchemaObjectNodes(
    connectionId: string,
    dbName: string,
    schemaName: string,
    config: NavigationConfig
  ): VirtualTreeNode[] {
    const parentKey = NodeKeyEncoder.encode(['schema', connectionId, dbName, schemaName])
    const nodes: VirtualTreeNode[] = []

    // 获取该 schema 下实际的表和视图数量
    const tables = navigatorStore.getSchemaTables(connectionId, dbName, schemaName)
    const views = navigatorStore.getSchemaViews(connectionId, dbName, schemaName)
    // 获取 schema 对象以读取 functions/procedures/sequences/triggers 计数
    const schema = navigatorStore
      .getCatalogSchemas(connectionId, dbName)
      .find(s => s.name === schemaName)

    if (isFolderEnabled(config, 'tables')) {
      nodes.push({
        key: NodeKeyEncoder.encode(['tables-folder', connectionId, dbName, schemaName]),
        level: 3,
        isExpanded: false,
        isLeaf: false,
        label: config.folders.tables.label,
        type: 'tables-folder',
        data: { connectionId, dbName, schemaName },
        parentId: parentKey,
        childCount: tables.length, // 已加载的数据中的真实表数量
      })
    }

    if (isFolderEnabled(config, 'views')) {
      nodes.push({
        key: NodeKeyEncoder.encode(['views-folder', connectionId, dbName, schemaName]),
        level: 3,
        isExpanded: false,
        isLeaf: false,
        label: config.folders.views.label,
        type: 'views-folder',
        data: { connectionId, dbName, schemaName },
        parentId: parentKey,
        childCount: views.length,
      })
    }

    if (isFolderEnabled(config, 'functions')) {
      nodes.push({
        key: NodeKeyEncoder.encode(['functions-folder', connectionId, dbName, schemaName]),
        level: 3,
        isExpanded: false,
        isLeaf: false,
        label: config.folders.functions.label,
        type: 'functions-folder',
        data: { connectionId, dbName, schemaName },
        parentId: parentKey,
        childCount: schema?.functions?.length ?? 0,
      })
    }

    if (isFolderEnabled(config, 'procedures')) {
      nodes.push({
        key: NodeKeyEncoder.encode(['procedures-folder', connectionId, dbName, schemaName]),
        level: 3,
        isExpanded: false,
        isLeaf: false,
        label: config.folders.procedures.label,
        type: 'procedures-folder',
        data: { connectionId, dbName, schemaName },
        parentId: parentKey,
        childCount: schema?.procedures?.length ?? 0,
      })
    }

    if (isFolderEnabled(config, 'sequences')) {
      nodes.push({
        key: NodeKeyEncoder.encode(['sequences-folder', connectionId, dbName, schemaName]),
        level: 3,
        isExpanded: false,
        isLeaf: false,
        label: config.folders.sequences.label,
        type: 'sequences-folder',
        data: { connectionId, dbName, schemaName },
        parentId: parentKey,
        childCount: schema?.sequences?.length ?? 0,
      })
    }

    if (isFolderEnabled(config, 'triggers')) {
      nodes.push({
        key: NodeKeyEncoder.encode(['triggers-folder', connectionId, dbName, schemaName]),
        level: 3,
        isExpanded: false,
        isLeaf: false,
        label: config.folders.triggers.label,
        type: 'triggers-folder',
        data: { connectionId, dbName, schemaName },
        parentId: parentKey,
        childCount: schema?.triggers?.length ?? 0,
      })
    }

    return nodes
  }

  /**
   * 创建表节点
   */
  function createTableNodes(
    connectionId: string,
    dbName: string,
    schemaName: string | undefined,
    config: NavigationConfig,
    parentLevel?: number
  ): VirtualTreeNode[] {
    const tables = navigatorStore.getSchemaTables(connectionId, dbName, schemaName || '')
    const parentKey = schemaName
      ? NodeKeyEncoder.encode(['tables-folder', connectionId, dbName, schemaName])
      : NodeKeyEncoder.encode(['tables-folder', connectionId, dbName])
    const level = parentLevel !== undefined ? parentLevel + 1 : schemaName ? 4 : 3

    return tables.map(table => {
      const colCount = table.columns?.length || 0
      const configChildCount = countTableChildren(config, colCount)
      return {
        key: NodeKeyEncoder.encode(['table', connectionId, dbName, schemaName || '', table.name]),
        level,
        isExpanded: false,
        isLeaf: false,
        label: table.name,
        type: 'table',
        data: {
          connectionId,
          dbName,
          schemaName,
          tableName: table.name,
          rowCount: table.rowCount ?? undefined,
          dataLength: table.dataLength ?? undefined,
          indexLength: table.indexLength ?? undefined,
        },
        parentId: parentKey,
        childCount: configChildCount, // 按配置动态计算子文件夹数
      }
    })
  }

  /**
   * 创建视图节点
   */
  function createViewNodes(
    connectionId: string,
    dbName: string,
    schemaName: string | undefined,
    parentLevel?: number
  ): VirtualTreeNode[] {
    const views = navigatorStore.getSchemaViews(connectionId, dbName, schemaName || '')
    const parentKey = schemaName
      ? NodeKeyEncoder.encode(['views-folder', connectionId, dbName, schemaName])
      : NodeKeyEncoder.encode(['views-folder', connectionId, dbName])
    const level = parentLevel !== undefined ? parentLevel + 1 : schemaName ? 4 : 3

    return views.map(view => ({
      key: NodeKeyEncoder.encode(['view', connectionId, dbName, schemaName || '', view.name]),
      level,
      isExpanded: false,
      isLeaf: false,
      label: view.name,
      type: 'view',
      data: { connectionId, dbName, schemaName, viewName: view.name },
      parentId: parentKey,
      childCount: view.columns?.length || 0,
    }))
  }

  /**
   * 创建存储过程节点
   */
  function createProcedureNodes(
    connectionId: string,
    dbName: string,
    schemaName: string | undefined
  ): VirtualTreeNode[] {
    const schema = navigatorStore
      .getCatalogSchemas(connectionId, dbName)
      .find(s => s.name === (schemaName || ''))

    if (!schema || !schema.procedures) return []

    const parentKey = schemaName
      ? NodeKeyEncoder.encode(['procedures-folder', connectionId, dbName, schemaName])
      : NodeKeyEncoder.encode(['procedures-folder', connectionId, dbName])

    return schema.procedures.map(proc => ({
      key: NodeKeyEncoder.encode(['procedure', connectionId, dbName, schemaName || '', proc.name]),
      level: schemaName ? 4 : 3,
      isExpanded: false,
      isLeaf: true,
      label: proc.name,
      type: 'procedure',
      data: { connectionId, dbName, schemaName, procedureName: proc.name },
      parentId: parentKey,
      childCount: 0,
    }))
  }

  /**
   * 创建函数节点
   */
  function createFunctionNodes(
    connectionId: string,
    dbName: string,
    schemaName: string | undefined
  ): VirtualTreeNode[] {
    const schema = navigatorStore
      .getCatalogSchemas(connectionId, dbName)
      .find(s => s.name === (schemaName || ''))

    if (!schema || !schema.functions) return []

    const parentKey = schemaName
      ? NodeKeyEncoder.encode(['functions-folder', connectionId, dbName, schemaName])
      : NodeKeyEncoder.encode(['functions-folder', connectionId, dbName])

    return schema.functions.map(func => ({
      key: NodeKeyEncoder.encode(['function', connectionId, dbName, schemaName || '', func.name]),
      level: schemaName ? 4 : 3,
      isExpanded: false,
      isLeaf: true,
      label: func.name,
      type: 'function',
      data: { connectionId, dbName, schemaName, functionName: func.name },
      parentId: parentKey,
      childCount: 0,
    }))
  }

  /**
   * 创建序列节点
   */
  function createSequenceNodes(
    connectionId: string,
    dbName: string,
    schemaName: string | undefined
  ): VirtualTreeNode[] {
    const schema = navigatorStore
      .getCatalogSchemas(connectionId, dbName)
      .find(s => s.name === (schemaName || ''))

    if (!schema || !schema.sequences) return []

    const parentKey = NodeKeyEncoder.encode([
      'sequences-folder',
      connectionId,
      dbName,
      schemaName || '',
    ])

    return schema.sequences.map(seq => ({
      key: NodeKeyEncoder.encode(['sequence', connectionId, dbName, schemaName || '', seq.name]),
      level: schemaName ? 4 : 3,
      isExpanded: false,
      isLeaf: true,
      label: seq.name,
      type: 'sequence',
      data: { connectionId, dbName, schemaName, sequenceName: seq.name },
      parentId: parentKey,
      childCount: 0,
    }))
  }

  /**
   * 创建触发器节点
   */
  function createTriggerNodes(
    connectionId: string,
    dbName: string,
    schemaName: string | undefined
  ): VirtualTreeNode[] {
    const schema = navigatorStore
      .getCatalogSchemas(connectionId, dbName)
      .find(s => s.name === (schemaName || ''))

    if (!schema || !schema.triggers) return []

    const parentKey = NodeKeyEncoder.encode([
      'triggers-folder',
      connectionId,
      dbName,
      schemaName || '',
    ])

    return schema.triggers.map(trg => ({
      key: NodeKeyEncoder.encode(['trigger', connectionId, dbName, schemaName || '', trg.name]),
      level: schemaName ? 4 : 3,
      isExpanded: false,
      isLeaf: true,
      label: trg.name,
      type: 'trigger',
      data: { connectionId, dbName, schemaName, triggerName: trg.name },
      parentId: parentKey,
      childCount: 0,
    }))
  }

  /**
   * 创建表的子文件夹节点（DBeaver 风格）
   */
  function createTableSubFolderNodes(
    connectionId: string,
    dbName: string,
    schemaName: string | undefined,
    tableName: string,
    config: NavigationConfig
  ): VirtualTreeNode[] {
    const table = navigatorStore
      .getSchemaTables(connectionId, dbName, schemaName || '')
      .find(t => t.name === tableName)

    if (!table) return []

    const parentKey = NodeKeyEncoder.encode([
      'table',
      connectionId,
      dbName,
      schemaName || '',
      tableName,
    ])
    const level = schemaName ? 5 : 4
    const nodes: VirtualTreeNode[] = []

    if (config.tableChildren.columns && table.columns) {
      nodes.push({
        key: NodeKeyEncoder.encode([
          'columns-folder',
          connectionId,
          dbName,
          schemaName || '',
          tableName,
        ]),
        level,
        isExpanded: false,
        isLeaf: false,
        label: 'Columns',
        type: 'columns-folder',
        data: { connectionId, dbName, schemaName, tableName },
        parentId: parentKey,
        childCount: table.columns.length,
      })
    }

    if (config.tableChildren.indexes && table.indexes && table.indexes.length > 0) {
      nodes.push({
        key: NodeKeyEncoder.encode([
          'indexes-folder',
          connectionId,
          dbName,
          schemaName || '',
          tableName,
        ]),
        level,
        isExpanded: false,
        isLeaf: false,
        label: 'Indexes',
        type: 'indexes-folder',
        data: { connectionId, dbName, schemaName, tableName },
        parentId: parentKey,
        childCount: table.indexes.length,
      })
    }

    if (config.tableChildren.constraints && table.constraints && table.constraints.length > 0) {
      nodes.push({
        key: NodeKeyEncoder.encode([
          'constraints-folder',
          connectionId,
          dbName,
          schemaName || '',
          tableName,
        ]),
        level,
        isExpanded: false,
        isLeaf: false,
        label: 'Constraints',
        type: 'constraints-folder',
        data: { connectionId, dbName, schemaName, tableName },
        parentId: parentKey,
        childCount: table.constraints.length,
      })
    }

    return nodes
  }

  /**
   * 创建列节点
   */
  function createColumnNodes(
    connectionId: string,
    dbName: string,
    schemaName: string | undefined,
    tableName: string
  ): VirtualTreeNode[] {
    const table = navigatorStore
      .getSchemaTables(connectionId, dbName, schemaName || '')
      .find(t => t.name === tableName)

    const views = navigatorStore.getSchemaViews(connectionId, dbName, schemaName || '')
    const view = views.find(v => v.name === tableName)

    const target = table || view

    if (!target || !target.columns) return []

    const parentKey = NodeKeyEncoder.encode([
      'columns-folder',
      connectionId,
      dbName,
      schemaName || '',
      tableName,
    ])

    return target.columns.map(col => ({
      key: NodeKeyEncoder.encode([
        'column',
        connectionId,
        dbName,
        schemaName || '',
        tableName,
        col.name,
      ]),
      level: schemaName ? 6 : 5,
      isExpanded: false,
      isLeaf: true,
      label: col.name,
      type: 'column',
      data: {
        connectionId,
        dbName,
        schemaName,
        tableName,
        columnName: col.name,
        dataType: col.dataType,
      },
      parentId: parentKey,
      childCount: 0,
    }))
  }

  /**
   * 创建索引节点
   */
  function createIndexNodes(
    connectionId: string,
    dbName: string,
    schemaName: string | undefined,
    tableName: string
  ): VirtualTreeNode[] {
    const table = navigatorStore
      .getSchemaTables(connectionId, dbName, schemaName || '')
      .find(t => t.name === tableName)

    if (!table || !table.indexes) return []

    const parentKey = NodeKeyEncoder.encode([
      'indexes-folder',
      connectionId,
      dbName,
      schemaName || '',
      tableName,
    ])

    return table.indexes.map(idx => ({
      key: NodeKeyEncoder.encode([
        'index',
        connectionId,
        dbName,
        schemaName || '',
        tableName,
        idx.name,
      ]),
      level: schemaName ? 6 : 5,
      isExpanded: false,
      isLeaf: true,
      label: idx.name,
      type: 'index',
      data: {
        connectionId,
        dbName,
        schemaName,
        tableName,
        indexName: idx.name,
        isUnique: idx.isUnique,
        isPrimary: idx.isPrimary,
        indexType: (idx as { type?: string }).type || 'BTREE',
        indexComment: (idx as { comment?: string | null }).comment || null,
        indexColumnNames: idx.columns || [],
      },
      parentId: parentKey,
      childCount: 0,
    }))
  }

  /**
   * 创建约束节点
   */
  function createConstraintNodes(
    connectionId: string,
    dbName: string,
    schemaName: string | undefined,
    tableName: string
  ): VirtualTreeNode[] {
    const table = navigatorStore
      .getSchemaTables(connectionId, dbName, schemaName || '')
      .find(t => t.name === tableName)

    if (!table || !table.constraints) return []

    const parentKey = NodeKeyEncoder.encode([
      'constraints-folder',
      connectionId,
      dbName,
      schemaName || '',
      tableName,
    ])

    return table.constraints.map(con => ({
      key: NodeKeyEncoder.encode([
        'constraint',
        connectionId,
        dbName,
        schemaName || '',
        tableName,
        con.name,
      ]),
      level: schemaName ? 6 : 5,
      isExpanded: false,
      isLeaf: true,
      label: con.name,
      type: 'constraint',
      data: {
        connectionId,
        dbName,
        schemaName,
        tableName,
        constraintName: con.name,
        constraintType: con.type,
        constraintColumnNames: con.columns || [],
        referencedTable: (con as { referencedTable?: string }).referencedTable,
        referencedColumns: (con as { referencedColumns?: string[] }).referencedColumns,
        updateRule: (con as { updateRule?: string }).updateRule,
        deleteRule: (con as { deleteRule?: string }).deleteRule,
      },
      parentId: parentKey,
      childCount: 0,
    }))
  }

  /**
   * 加载子节点 - 核心加载逻辑（路由分发模式）
   */
  async function loadChildren(node: VirtualTreeNode): Promise<VirtualTreeNode[]> {
    // 取消前一次加载（避免快速展开/折叠导致状态竞争）
    if (currentAbortController) {
      currentAbortController.abort()
    }
    currentAbortController = new AbortController()

    const keyParts = NodeKeyEncoder.decode(node.key)
    if (keyParts.length === 0) return []

    const nodeType = keyParts[0]
    const handler = nodeHandlers[nodeType]
    if (!handler) return []

    // connection 节点的 keyParts 结构不同：[type, scope, connId]
    // 其他节点：[type, connId, dbName, ...]
    const connectionId = nodeType === 'connection' ? keyParts[2] : keyParts[1]
    const dbType = node.data.driver || navigatorStore.getDbType(connectionId) || ''
    const config = await getNavConfig(dbType)

    try {
      return await handler({
        node,
        keyParts,
        connectionId,
        dbType,
        config,
        navigatorStore,
        runtimeConnectionStore,
        createCatalogNodes,
        createSchemaNodes,
        createCatalogObjectNodes,
        createSchemaObjectNodes,
        createTableNodes,
        createViewNodes,
        createProcedureNodes,
        createFunctionNodes,
        createSequenceNodes,
        createTriggerNodes,
        createTableSubFolderNodes,
        createColumnNodes,
        createIndexNodes,
        createConstraintNodes,
      })
    } catch (error) {
      const msg = error instanceof Error ? error.message : '加载失败'
      console.error('加载树节点失败:', node.key, error)
      navigatorStore.setNodeError(node.key, msg)
      return [createErrorPlaceholderNode(node.key, msg)]
    }
  }

  /**
   * 创建根节点列表
   */
  function createRootNodes(
    globalConnections: GlobalConnection[],
    projectConnections: ProjectConnection[]
  ): VirtualTreeNode[] {
    const rootNodes: VirtualTreeNode[] = []

    globalConnections.forEach(conn => {
      rootNodes.push(createConnectionNode(conn, 'global'))
    })

    projectConnections.forEach(conn => {
      rootNodes.push(createConnectionNode(conn, 'project'))
    })

    return rootNodes
  }

  return {
    createConnectionNode,
    createCatalogNodes,
    createSchemaNodes,
    createCatalogObjectNodes,
    createSchemaObjectNodes,
    createTableNodes,
    createViewNodes,
    createTableSubFolderNodes,
    createColumnNodes,
    createIndexNodes,
    createConstraintNodes,
    loadChildren,
    createRootNodes,
    /** 中止所有进行中的加载请求（组件卸载时调用） */
    abortPendingLoads: () => {
      if (currentAbortController) {
        currentAbortController.abort()
        currentAbortController = null
      }
    },
  }
}
