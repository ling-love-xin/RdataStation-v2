/**
 * 数据库导航树右键菜单操作
 *
 * 为不同节点类型提供完整的上下文菜单操作
 * 支持创建、删除、刷新、复制等操作
 */

import { useConnectionStore } from '@/extensions/builtin/connection/ui/stores/connection-store'
import { useProjectConnectionStore } from '@/extensions/builtin/connection/ui/stores/project-connection-store'
import { useRuntimeConnectionStore } from '@/extensions/builtin/connection/ui/stores/runtime-connection-store'
import { useInsightStore } from '@/extensions/builtin/workbench/ui/stores/insight-store'

import { useDatabaseNavigatorStore } from '../stores/database-navigator-store'
import { NodeKeyEncoder } from '../types/virtual-tree'

import type { VirtualTreeNode } from '../types/virtual-tree'

export type IContextMenuItem =
  | {
      /** 分隔线 */
      separator: true
    }
  | {
      /** 菜单项 ID */
      id: string
      /** 显示文本 */
      label: string
      /** 图标 */
      icon?: string
      /** 快捷键 */
      shortcut?: string
      /** 是否禁用 */
      disabled?: boolean
      /** 是否隐藏 */
      hidden?: boolean
      /** 子菜单 */
      children?: IContextMenuItem[]
      /** 分隔线 */
      separator?: false
      /** 点击回调 */
      action?: () => Promise<void> | void
    }

export interface IContextMenuConfig {
  /** 菜单项列表 */
  items: IContextMenuItem[]
  /** 节点信息 */
  node: VirtualTreeNode
}

export function useContextMenuActions() {
  const navigatorStore = useDatabaseNavigatorStore()
  const runtimeConnectionStore = useRuntimeConnectionStore()
  const projectConnectionStore = useProjectConnectionStore()
  const connectionStore = useConnectionStore()

  /**
   * 获取连接节点菜单
   */
  function getConnectionMenu(node: VirtualTreeNode): IContextMenuItem[] {
    const connectionId = node.data?.connectionId as string
    if (!connectionId) return []
    const isConnected = runtimeConnectionStore.runtimeConnectionIds.has(connectionId)
    const duckDbOn = runtimeConnectionStore.isDuckDbEnabled(connectionId)

    return [
      {
        id: 'edit-connection',
        label: '编辑连接',
        icon: 'Settings',
        shortcut: 'F4',
        action: () => editConnection(connectionId),
      },
      {
        id: 'test-connection',
        label: '测试连接',
        icon: 'Zap',
        action: () => testConnection(connectionId),
      },
      {
        id: isConnected ? 'disconnect' : 'connect',
        label: isConnected ? '断开连接' : '连接',
        icon: isConnected ? 'LogOut' : 'LogIn',
        shortcut: isConnected ? 'Ctrl+D' : 'Ctrl+E',
        action: () => toggleConnection(connectionId, isConnected),
      },
      { separator: true },
      {
        id: 'duckdb-accel',
        label: duckDbOn ? '✓ DuckDB 本地加速' : 'DuckDB 本地加速',
        icon: 'Zap',
        action: () => {
          runtimeConnectionStore.toggleDuckDbEnabled(connectionId)
        },
      },
      { separator: true },
      {
        id: 'sql-editor',
        label: '打开 SQL 编辑器',
        icon: 'Code',
        shortcut: 'Ctrl+Shift+E',
        action: () => openSqlEditor(connectionId),
      },
      { separator: true },
      {
        id: 'refresh',
        label: '刷新',
        icon: 'RefreshCw',
        shortcut: 'F5',
        action: () => refreshConnection(connectionId),
      },
      {
        id: 'refresh-all',
        label: '刷新所有',
        icon: 'RefreshCw',
        shortcut: 'Ctrl+F5',
        action: () => refreshAllConnections(),
      },
      { separator: true },
      {
        id: 'copy-name',
        label: '复制名称',
        icon: 'Copy',
        shortcut: 'Ctrl+C',
        action: () => copyToClipboard(node.label),
      },
      { separator: true },
      {
        id: 'delete-connection',
        label: '删除连接',
        icon: 'Trash2',
        action: () => deleteConnection(connectionId),
      },
    ]
  }

  /**
   * 获取数据库节点菜单
   */
  function getDatabaseMenu(node: VirtualTreeNode): IContextMenuItem[] {
    const connectionId = node.data?.connectionId as string
    const dbName = node.data?.dbName as string
    if (!connectionId || !dbName) return []

    return [
      {
        id: 'new-table',
        label: '新建表',
        icon: 'Plus',
        shortcut: 'Ctrl+T',
        action: () => createTable(connectionId as string, dbName as string),
      },
      {
        id: 'new-view',
        label: '新建视图',
        icon: 'Plus',
        action: () => createView(connectionId as string, dbName as string),
      },
      { separator: true },
      {
        id: 'refresh',
        label: '刷新',
        icon: 'RefreshCw',
        shortcut: 'F5',
        action: () => refreshDatabase(connectionId as string, dbName as string),
      },
      { separator: true },
      {
        id: 'sql-editor',
        label: '打开 SQL 编辑器',
        icon: 'Code',
        shortcut: 'Ctrl+Shift+E',
        action: () => openSqlEditor(connectionId as string, dbName as string),
      },
      { separator: true },
      {
        id: 'copy-name',
        label: '复制名称',
        icon: 'Copy',
        shortcut: 'Ctrl+C',
        action: () => copyToClipboard(dbName as string),
      },
      {
        id: 'copy-qualified-name',
        label: '复制限定名称',
        icon: 'Copy',
        action: () => copyToClipboard(dbName as string),
      },
    ]
  }

  /**
   * 获取 Schema 节点菜单
   */
  function getSchemaMenu(node: VirtualTreeNode): IContextMenuItem[] {
    const connectionId = node.data?.connectionId as string
    const dbName = node.data?.dbName as string
    const schemaName = node.data?.schemaName as string
    if (!connectionId || !dbName || !schemaName) return []

    return [
      {
        id: 'new-table',
        label: '新建表',
        icon: 'Plus',
        shortcut: 'Ctrl+T',
        action: () => createTable(connectionId as string, dbName as string, schemaName as string),
      },
      {
        id: 'new-view',
        label: '新建视图',
        icon: 'Plus',
        action: () => createView(connectionId as string, dbName as string, schemaName as string),
      },
      {
        id: 'new-function',
        label: '新建函数',
        icon: 'Plus',
        action: () =>
          createFunction(connectionId as string, dbName as string, schemaName as string),
      },
      {
        id: 'new-procedure',
        label: '新建存储过程',
        icon: 'Plus',
        action: () =>
          createProcedure(connectionId as string, dbName as string, schemaName as string),
      },
      { separator: true },
      {
        id: 'refresh',
        label: '刷新',
        icon: 'RefreshCw',
        shortcut: 'F5',
        action: () => refreshSchema(connectionId as string, dbName as string, schemaName as string),
      },
      { separator: true },
      {
        id: 'sql-editor',
        label: '打开 SQL 编辑器',
        icon: 'Code',
        shortcut: 'Ctrl+Shift+E',
        action: () => openSqlEditor(connectionId as string, dbName as string, schemaName as string),
      },
      { separator: true },
      {
        id: 'schema-insight',
        label: 'Schema 洞察',
        icon: 'Search',
        action: () =>
          quickSchemaInsight(connectionId as string, dbName as string, schemaName as string),
      },
      { separator: true },
      {
        id: 'copy-name',
        label: '复制名称',
        icon: 'Copy',
        shortcut: 'Ctrl+C',
        action: () => copyToClipboard(schemaName as string),
      },
      {
        id: 'copy-qualified-name',
        label: '复制限定名称',
        icon: 'Copy',
        action: () => copyToClipboard(`${dbName}.${schemaName}`),
      },
    ]
  }

  /**
   * 获取表节点菜单
   */
  function getTableMenu(node: VirtualTreeNode): IContextMenuItem[] {
    const connectionId = node.data?.connectionId as string
    const dbName = node.data?.dbName as string
    const schemaName = node.data?.schemaName as string
    const tableName = node.data?.tableName as string
    if (!connectionId || !dbName || !schemaName || !tableName) return []

    return [
      {
        id: 'view-data',
        label: '查看数据',
        icon: 'Table',
        shortcut: 'F4',
        action: () =>
          viewTableData(
            connectionId as string,
            dbName as string,
            schemaName as string,
            tableName as string
          ),
      },
      {
        id: 'view-ddl',
        label: '查看 DDL',
        icon: 'Code',
        shortcut: 'Ctrl+D',
        action: () =>
          viewTableDDL(
            connectionId as string,
            dbName as string,
            schemaName as string,
            tableName as string
          ),
      },
      { separator: true },
      {
        id: 'truncate-table',
        label: '清空表',
        icon: 'Trash2',
        action: () =>
          truncateTable(
            connectionId as string,
            dbName as string,
            schemaName as string,
            tableName as string
          ),
      },
      {
        id: 'drop-table',
        label: '删除表',
        icon: 'Trash2',
        action: () =>
          dropTable(
            connectionId as string,
            dbName as string,
            schemaName as string,
            tableName as string
          ),
      },
      { separator: true },
      {
        id: 'analyze-table',
        label: '分析表',
        icon: 'BarChart3',
        action: () =>
          analyzeTable(
            connectionId as string,
            dbName as string,
            schemaName as string,
            tableName as string
          ),
      },
      {
        id: 'quick-profile',
        label: '快速探查',
        icon: 'Eye',
        shortcut: 'Ctrl+Shift+P',
        action: () =>
          quickProfile(
            connectionId as string,
            dbName as string,
            schemaName as string,
            tableName as string
          ),
      },
      {
        id: 'eval-quality',
        label: '评估表质量',
        icon: 'Search',
        action: () =>
          evaluateTableQuality(
            connectionId as string,
            dbName as string,
            schemaName as string,
            tableName as string
          ),
      },
      { separator: true },
      {
        id: 'generate-select',
        label: '生成 SELECT 语句',
        icon: 'FileText',
        action: () =>
          generateSelect(
            connectionId as string,
            dbName as string,
            schemaName as string,
            tableName as string
          ),
      },
      {
        id: 'generate-insert',
        label: '生成 INSERT 语句',
        icon: 'FileText',
        action: () =>
          generateInsert(
            connectionId as string,
            dbName as string,
            schemaName as string,
            tableName as string
          ),
      },
      {
        id: 'generate-update',
        label: '生成 UPDATE 语句',
        icon: 'FileEdit',
        action: () =>
          generateUpdate(
            connectionId as string,
            dbName as string,
            schemaName as string,
            tableName as string
          ),
      },
      {
        id: 'generate-delete',
        label: '生成 DELETE 语句',
        icon: 'Trash2',
        action: () =>
          generateDelete(
            connectionId as string,
            dbName as string,
            schemaName as string,
            tableName as string
          ),
      },
      { separator: true },
      {
        id: 'sql-editor',
        label: '打开 SQL 编辑器',
        icon: 'Code',
        shortcut: 'Ctrl+Shift+E',
        action: () =>
          openSqlEditor(
            connectionId as string,
            dbName as string,
            schemaName as string,
            tableName as string
          ),
      },
      { separator: true },
      {
        id: 'copy-name',
        label: '复制名称',
        icon: 'Copy',
        shortcut: 'Ctrl+C',
        action: () => copyToClipboard(tableName as string),
      },
      {
        id: 'copy-qualified-name',
        label: '复制限定名称',
        icon: 'Copy',
        action: () => copyToClipboard(`${dbName}.${schemaName}.${tableName}`),
      },
    ]
  }

  /**
   * 获取视图节点菜单
   */
  function getViewMenu(node: VirtualTreeNode): IContextMenuItem[] {
    const connectionId = node.data?.connectionId as string
    const dbName = node.data?.dbName as string
    const schemaName = node.data?.schemaName as string
    const viewName = node.data?.viewName as string
    if (!connectionId || !dbName || !schemaName || !viewName) return []

    return [
      {
        id: 'view-data',
        label: '查看数据',
        icon: 'Table',
        shortcut: 'F4',
        action: () =>
          viewTableData(
            connectionId as string,
            dbName as string,
            schemaName as string,
            viewName as string
          ),
      },
      {
        id: 'view-ddl',
        label: '查看 DDL',
        icon: 'Code',
        shortcut: 'Ctrl+D',
        action: () =>
          viewTableDDL(
            connectionId as string,
            dbName as string,
            schemaName as string,
            viewName as string
          ),
      },
      { separator: true },
      {
        id: 'generate-select-view',
        label: '生成 SELECT 语句',
        icon: 'FileText',
        action: () =>
          generateSelect(
            connectionId as string,
            dbName as string,
            schemaName as string,
            viewName as string
          ),
      },
      { separator: true },
      {
        id: 'drop-view',
        label: '删除视图',
        icon: 'Trash2',
        action: () =>
          dropTable(
            connectionId as string,
            dbName as string,
            schemaName as string,
            viewName as string
          ),
      },
      { separator: true },
      {
        id: 'sql-editor',
        label: '打开 SQL 编辑器',
        icon: 'Code',
        shortcut: 'Ctrl+Shift+E',
        action: () =>
          openSqlEditor(
            connectionId as string,
            dbName as string,
            schemaName as string,
            viewName as string
          ),
      },
      { separator: true },
      {
        id: 'copy-name',
        label: '复制名称',
        icon: 'Copy',
        shortcut: 'Ctrl+C',
        action: () => copyToClipboard(viewName as string),
      },
      {
        id: 'copy-qualified-name',
        label: '复制限定名称',
        icon: 'Copy',
        action: () => copyToClipboard(`${dbName}.${schemaName}.${viewName}`),
      },
    ]
  }

  /**
   * 获取列节点菜单
   */
  function getColumnMenu(node: VirtualTreeNode): IContextMenuItem[] {
    const connectionId = node.data?.connectionId as string
    const dbName = node.data?.dbName as string
    const schemaName = node.data?.schemaName as string
    const tableName = node.data?.tableName as string
    const columnName = node.data?.columnName as string
    if (!connectionId || !dbName || !schemaName || !tableName || !columnName) return []

    return [
      {
        id: 'copy-name',
        label: '复制名称',
        icon: 'Copy',
        shortcut: 'Ctrl+C',
        action: () => copyToClipboard(columnName as string),
      },
      {
        id: 'copy-qualified-name',
        label: '复制限定名称',
        icon: 'Copy',
        action: () => copyToClipboard(`${tableName}.${columnName}`),
      },
      { separator: true },
      {
        id: 'sql-editor',
        label: '打开 SQL 编辑器',
        icon: 'Code',
        shortcut: 'Ctrl+Shift+E',
        action: () =>
          openSqlEditor(
            connectionId as string,
            dbName as string,
            schemaName as string,
            tableName as string
          ),
      },
    ]
  }

  /**
   * 获取索引节点菜单
   */
  function getIndexMenu(node: VirtualTreeNode): IContextMenuItem[] {
    const indexName = node.data?.indexName as string
    if (!indexName) return []

    return [
      {
        id: 'properties',
        label: '属性',
        icon: 'Info',
        action: () => showIndexProperties(node),
      },
      { separator: true },
      {
        id: 'copy-name',
        label: '复制名称',
        icon: 'Copy',
        shortcut: 'Ctrl+C',
        action: () => copyToClipboard(indexName),
      },
    ]
  }

  /**
   * 获取约束节点菜单
   */
  function getConstraintMenu(node: VirtualTreeNode): IContextMenuItem[] {
    const constraintName = node.data?.constraintName as string
    if (!constraintName) return []

    return [
      {
        id: 'properties',
        label: '属性',
        icon: 'Info',
        action: () => showConstraintProperties(node),
      },
      { separator: true },
      {
        id: 'copy-name',
        label: '复制名称',
        icon: 'Copy',
        shortcut: 'Ctrl+C',
        action: () => copyToClipboard(constraintName),
      },
    ]
  }

  /**
   * 获取文件夹节点菜单
   */
  function getFolderMenu(node: VirtualTreeNode): IContextMenuItem[] {
    const connectionId = node.data?.connectionId as string
    const dbName = node.data?.dbName as string
    const schemaName = node.data?.schemaName as string | undefined
    const folderType = node.type
    if (!connectionId || !dbName) return []

    const items: IContextMenuItem[] = []

    if (folderType === 'tables-folder') {
      items.push({
        id: 'new-table',
        label: '新建表',
        icon: 'Plus',
        shortcut: 'Ctrl+T',
        action: () => createTable(connectionId as string, dbName as string, schemaName),
      })
    } else if (folderType === 'views-folder') {
      items.push({
        id: 'new-view',
        label: '新建视图',
        icon: 'Plus',
        action: () => createView(connectionId as string, dbName as string, schemaName),
      })
    }

    if (items.length > 0) {
      items.push({ separator: true })
    }

    items.push({
      id: 'refresh',
      label: '刷新',
      icon: 'RefreshCw',
      shortcut: 'F5',
      action: () => refreshFolder(node),
    })

    items.push({ separator: true })

    items.push({
      id: 'sql-editor',
      label: '打开 SQL 编辑器',
      icon: 'Code',
      shortcut: 'Ctrl+Shift+E',
      action: () => openSqlEditor(connectionId as string, dbName as string, schemaName),
    })

    return items
  }

  /**
   * 获取节点菜单配置
   */
  function getNodeMenu(node: VirtualTreeNode): IContextMenuItem[] {
    switch (node.type) {
      case 'connection':
        return getConnectionMenu(node)
      case 'catalog':
        return getDatabaseMenu(node)
      case 'schema':
        return getSchemaMenu(node)
      case 'table':
        return getTableMenu(node)
      case 'view':
        return getViewMenu(node)
      case 'column':
        return getColumnMenu(node)
      case 'index':
        return getIndexMenu(node)
      case 'constraint':
        return getConstraintMenu(node)
      case 'tables-folder':
      case 'views-folder':
      case 'functions-folder':
      case 'procedures-folder':
      case 'sequences-folder':
      case 'triggers-folder':
      case 'columns-folder':
      case 'indexes-folder':
      case 'constraints-folder':
        return getFolderMenu(node)
      default:
        return []
    }
  }

  // ==================== 操作实现 ====================

  async function editConnection(connectionId: string): Promise<void> {
    window.dispatchEvent(new CustomEvent('open-connection-editor', { detail: { connectionId } }))
  }

  async function testConnection(connectionId: string): Promise<void> {
    try {
      const startTime = Date.now()
      await navigatorStore.loadCatalogs(connectionId)
      const latency = Date.now() - startTime
      // eslint-disable-next-line no-console
      console.debug(`连接测试成功，延迟: ${latency}ms`)
    } catch (error) {
      console.error('连接测试失败:', error)
    }
  }

  async function toggleConnection(connectionId: string, isConnected: boolean): Promise<void> {
    if (isConnected) {
      await runtimeConnectionStore.closeRuntimeConnection(connectionId)
      navigatorStore.disconnectConnection(connectionId)
    } else {
      // 优先查项目连接
      const projectConn = projectConnectionStore.connections.find(c => c.id === connectionId)
      if (projectConn) {
        await runtimeConnectionStore.establishRuntimeConnection(projectConn)
        return
      }
      // 查 connectionStore（包含全局连接）
      const conn = connectionStore.connections.find(c => c.connId === connectionId)
      if (conn) {
        await runtimeConnectionStore.establishFromConnection(conn)
      }
    }
  }

  async function refreshConnection(connectionId: string): Promise<void> {
    await navigatorStore.refreshMetadata(connectionId)
  }

  async function refreshAllConnections(): Promise<void> {
    const allConnections = [...projectConnectionStore.connections]
    for (const conn of allConnections) {
      await navigatorStore.refreshMetadata(conn.id)
    }
  }

  async function deleteConnection(connectionId: string): Promise<void> {
    if (confirm('确定要删除此连接吗？')) {
      await projectConnectionStore.deleteConnection(connectionId)
      navigatorStore.disconnectConnection(connectionId)
    }
  }

  async function createTable(
    connectionId: string,
    dbName: string,
    schemaName?: string
  ): Promise<void> {
    window.dispatchEvent(
      new CustomEvent('open-create-table', {
        detail: { connectionId, dbName, schemaName },
      })
    )
  }

  async function createView(
    connectionId: string,
    dbName: string,
    schemaName?: string
  ): Promise<void> {
    window.dispatchEvent(
      new CustomEvent('open-create-view', {
        detail: { connectionId, dbName, schemaName },
      })
    )
  }

  async function createFunction(
    connectionId: string,
    dbName: string,
    schemaName?: string
  ): Promise<void> {
    window.dispatchEvent(
      new CustomEvent('open-create-function', {
        detail: { connectionId, dbName, schemaName },
      })
    )
  }

  async function createProcedure(
    connectionId: string,
    dbName: string,
    schemaName?: string
  ): Promise<void> {
    window.dispatchEvent(
      new CustomEvent('open-create-procedure', {
        detail: { connectionId, dbName, schemaName },
      })
    )
  }

  async function refreshDatabase(connectionId: string, _dbName: string): Promise<void> {
    await navigatorStore.loadCatalogs(connectionId)
  }

  async function refreshSchema(
    connectionId: string,
    dbName: string,
    schemaName: string
  ): Promise<void> {
    await Promise.all([
      navigatorStore.loadTables(connectionId, dbName, schemaName),
      navigatorStore.loadTables(connectionId, dbName, schemaName),
    ])
  }

  async function openSqlEditor(
    connectionId: string,
    dbName?: string,
    schemaName?: string,
    objectName?: string
  ): Promise<void> {
    // 确保 connectionStore 中有此连接的状态
    const hasRuntime = runtimeConnectionStore.runtimeConnectionIds.has(connectionId)
    if (hasRuntime) {
      connectionStore.syncConnectionStatus(connectionId, true)
    }

    let sql = ''

    if (dbName && schemaName && objectName) {
      sql = `SELECT * FROM ${dbName}.${schemaName}.${objectName} LIMIT 100;`
    }

    window.dispatchEvent(
      new CustomEvent('open-sql-editor', {
        detail: { connectionId, databaseName: dbName, sql },
      })
    )
  }

  async function viewTableData(
    connectionId: string,
    dbName: string,
    schemaName: string,
    tableName: string
  ): Promise<void> {
    window.dispatchEvent(
      new CustomEvent('open-table-data', {
        detail: { connectionId, dbName, schemaName, tableName },
      })
    )
  }

  async function viewTableDDL(
    connectionId: string,
    dbName: string,
    schemaName: string,
    tableName: string
  ): Promise<void> {
    window.dispatchEvent(
      new CustomEvent('open-table-ddl', {
        detail: { connectionId, dbName, schemaName, tableName },
      })
    )
  }

  async function truncateTable(
    connectionId: string,
    dbName: string,
    schemaName: string,
    tableName: string
  ): Promise<void> {
    if (confirm(`确定要清空表 ${tableName} 吗？此操作不可恢复。`)) {
      try {
        await navigatorStore.executeSql(
          connectionId,
          dbName,
          `TRUNCATE TABLE ${schemaName}.${tableName}`
        )
      } catch (error) {
        console.error('清空表失败:', error)
      }
    }
  }

  async function dropTable(
    connectionId: string,
    dbName: string,
    schemaName: string,
    tableName: string
  ): Promise<void> {
    if (confirm(`确定要删除表 ${tableName} 吗？此操作不可恢复。`)) {
      try {
        await navigatorStore.executeSql(
          connectionId,
          dbName,
          `DROP TABLE ${schemaName}.${tableName}`
        )
      } catch (error) {
        console.error('删除表失败:', error)
      }
    }
  }

  async function analyzeTable(
    connectionId: string,
    dbName: string,
    schemaName: string,
    tableName: string
  ): Promise<void> {
    try {
      await navigatorStore.executeSql(
        connectionId,
        dbName,
        `ANALYZE TABLE ${schemaName}.${tableName}`
      )
    } catch (error) {
      console.error('分析表失败:', error)
    }
  }

  async function quickProfile(
    connectionId: string,
    dbName: string,
    schemaName: string,
    tableName: string
  ): Promise<void> {
    const dbType = getDbTypeForConnection(connectionId)
    const insightStore = useInsightStore()
    insightStore.requestTableProfile({
      connId: connectionId,
      dbType,
      database: dbName,
      schema: schemaName,
      table: tableName,
    })
  }

  function evaluateTableQuality(
    connectionId: string,
    dbName: string,
    schemaName: string,
    tableName: string
  ): void {
    const dbType = getDbTypeForConnection(connectionId)
    const insightStore = useInsightStore()
    insightStore.requestTableProfile({
      connId: connectionId,
      dbType,
      database: dbName,
      schema: schemaName,
      table: tableName,
      autoEvaluate: true,
    })
  }

  function quickSchemaInsight(connectionId: string, dbName: string, schemaName: string): void {
    const insightStore = useInsightStore()
    const dbType = getDbTypeForConnection(connectionId)
    insightStore.requestSchemaInsight({
      connId: connectionId,
      dbType,
      database: dbName,
      schema: schemaName,
    })
  }

  function getDbTypeForConnection(connectionId: string): string {
    const conn = connectionStore.connections.find(c => c.connId === connectionId)
    if (conn) return conn.dbType
    const runtimeConnId = runtimeConnectionStore.runtimeConnectionIds.get(connectionId)
    if (runtimeConnId) {
      const runtimeConn = connectionStore.connections.find(c => c.connId === runtimeConnId)
      if (runtimeConn) return runtimeConn.dbType
    }
    const projectConn = projectConnectionStore.connections.find(c => c.id === connectionId)
    if (projectConn) return projectConn.driver
    return 'unknown'
  }

  async function generateSelect(
    connectionId: string,
    dbName: string,
    schemaName: string,
    tableName: string
  ): Promise<void> {
    const sql = `SELECT * FROM ${dbName}.${schemaName}.${tableName} LIMIT 100;`
    await copyToClipboard(sql)
    window.dispatchEvent(
      new CustomEvent('open-sql-editor', {
        detail: { connectionId, databaseName: dbName, sql },
      })
    )
  }

  async function generateInsert(
    connectionId: string,
    dbName: string,
    schemaName: string,
    tableName: string
  ): Promise<void> {
    const sql = `INSERT INTO ${dbName}.${schemaName}.${tableName} (\n  -- column_list\n) VALUES (\n  -- value_list\n);`
    await copyToClipboard(sql)
    window.dispatchEvent(
      new CustomEvent('open-sql-editor', {
        detail: { connectionId, databaseName: dbName, sql },
      })
    )
  }

  async function generateUpdate(
    connectionId: string,
    dbName: string,
    schemaName: string,
    tableName: string
  ): Promise<void> {
    const sql = `UPDATE ${dbName}.${schemaName}.${tableName}\nSET\n  -- column = value\nWHERE\n  -- condition;`
    await copyToClipboard(sql)
    window.dispatchEvent(
      new CustomEvent('open-sql-editor', {
        detail: { connectionId, databaseName: dbName, sql },
      })
    )
  }

  async function generateDelete(
    connectionId: string,
    dbName: string,
    schemaName: string,
    tableName: string
  ): Promise<void> {
    const sql = `DELETE FROM ${dbName}.${schemaName}.${tableName}\nWHERE\n  -- condition;`
    await copyToClipboard(sql)
    window.dispatchEvent(
      new CustomEvent('open-sql-editor', {
        detail: { connectionId, databaseName: dbName, sql },
      })
    )
  }

  async function refreshFolder(node: VirtualTreeNode): Promise<void> {
    const keyParts = NodeKeyEncoder.decode(node.key)
    const nodeType = keyParts[0]
    const connectionId = keyParts[1]
    const dbName = keyParts[2]
    const schemaName = keyParts[3]

    if (nodeType === 'tables-folder' && schemaName) {
      await navigatorStore.loadTables(connectionId, dbName, schemaName)
    } else if (nodeType === 'views-folder' && schemaName) {
      await navigatorStore.loadTables(connectionId, dbName, schemaName)
    }
  }

  function showIndexProperties(node: VirtualTreeNode): void {
    window.dispatchEvent(new CustomEvent('show-index-properties', { detail: { node } }))
  }

  function showConstraintProperties(node: VirtualTreeNode): void {
    window.dispatchEvent(new CustomEvent('show-constraint-properties', { detail: { node } }))
  }

  async function copyToClipboard(text: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(text)
    } catch {
      const textarea = document.createElement('textarea')
      textarea.value = text
      document.body.appendChild(textarea)
      textarea.select()
      document.execCommand('copy')
      document.body.removeChild(textarea)
    }
  }

  return {
    getNodeMenu,
  }
}
