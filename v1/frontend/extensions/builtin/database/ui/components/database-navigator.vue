<template>
  <div class="database-navigator dbeaver-style" :class="{ dark: uiStore.isDark }">
    <NavigatorToolbar
      :has-connection="!!currentConnection"
      :is-refreshing="isRefreshing"
      :show-filter="showFilter"
      :is-in-transaction="isInTransaction"
      @new-connection="handleNewConnection"
      @new-group="openCreateGroupDialog"
      @disconnect="handleDisconnect"
      @begin-transaction="handleBeginTransaction"
      @commit-transaction="handleCommitTransaction"
      @rollback-transaction="handleRollbackTransaction"
      @refresh="handleRefresh"
      @focus-search="focusSearch"
      @toggle-filter="toggleFilter"
      @toggle-view="toggleView"
    />

    <NavigatorSearch
      ref="searchRef"
      :show="showSearch"
      :query="searchQuery"
      @update:query="onSearchQueryChange"
      @clear="clearSearch"
      @select="handleSearchSelect"
    />

    <NavigatorFilter
      :show="showFilter"
      :config="filterConfig"
      @close="showFilter = false"
      @update:show-tables="filterConfig.showTables = $event"
      @update:show-views="filterConfig.showViews = $event"
      @update:show-columns="filterConfig.showColumns = $event"
      @update:show-system-schemas="filterConfig.showSystemSchemas = $event"
    />

    <div class="navigator-tree">
      <VirtualTree
        ref="virtualTreeRef"
        :nodes="virtualTreeNodes"
        :selected-key="selectedKey"
        :item-height="28"
        @select="handleVirtualTreeSelect"
        @toggle="handleVirtualTreeToggle"
        @context-menu="handleVirtualTreeContextMenu"
        @dblclick="handleVirtualTreeDblClick"
        @dragstart="handleNodeDragStart"
        @dragend="handleNodeDragEnd"
      />
    </div>

    <NavigatorContextMenuV2 ref="contextMenuRef" :items="contextMenuItems" />

    <NavigatorError
      :visible="showError"
      :error="currentError"
      @retry="handleErrorRetry"
      @close="handleErrorClose"
    />

    <NavigatorGroupDialog
      :visible="showGroupDialog"
      :is-edit="isEditGroup"
      :initial-data="getEditingGroupData()"
      @close="closeGroupDialog"
      @submit="handleGroupSubmit"
    />

    <NavigatorStatus
      :text="statusText"
      :is-in-transaction="isInTransaction"
      :transaction-duration="transactionDuration"
    />

    <NModal
      v-model:show="showPropertiesDialog"
      preset="card"
      :title="propertiesDialogTitle"
      :style="{ width: '480px' }"
      :mask-closable="true"
    >
      <div class="properties-content">
        <NDescriptions label-placement="left" bordered :column="1" size="small">
          <NDescriptionsItem v-for="item in propertiesItems" :key="item.label" :label="item.label">
            {{ item.value }}
          </NDescriptionsItem>
        </NDescriptions>
      </div>
    </NModal>
  </div>
</template>

<script setup lang="ts">
import { NModal, NDescriptions, NDescriptionsItem } from 'naive-ui'
import { ref, computed, watch, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'

import { getGlobalConnections } from '@/extensions/builtin/connection/ui/services/connection'
import type { GlobalConnectionInfo } from '@/extensions/builtin/connection/ui/services/connection'
import { useConnectionStore } from '@/extensions/builtin/connection/ui/stores/connection-store'
import { useProjectConnectionStore } from '@/extensions/builtin/connection/ui/stores/project-connection-store'
import { useRuntimeConnectionStore } from '@/extensions/builtin/connection/ui/stores/runtime-connection-store'
import type { ProjectConnection } from '@/extensions/builtin/connection/ui/types/connection'
import {
  WorkbenchEvent,
  dispatchWorkbenchEvent,
} from '@/extensions/builtin/workbench/ui/constants/workbench-events'
import { useWorkbenchStore } from '@/extensions/builtin/workbench/ui/stores/workbench-store'
import { useUiStore } from '@/shared/stores/ui'

import NavigatorGroupDialog from './group-dialog.vue'
import NavigatorContextMenuV2 from './navigator-context-menu-v2.vue'
import NavigatorError from './navigator-error.vue'
import NavigatorFilter from './navigator-filter.vue'
import NavigatorSearch from './navigator-search.vue'
import NavigatorStatus from './navigator-status.vue'
import NavigatorToolbar from './navigator-toolbar.vue'
import VirtualTree from './virtual-tree.vue'
import { useAdjacentPreload } from '../composables/use-adjacent-preload'
import { useCacheWarming } from '../composables/use-cache-warming'
import { useConnectionHandler } from '../composables/use-connection-handler'
import { useConnectionStatusSync } from '../composables/use-connection-status-sync'
import { useContextMenuActions } from '../composables/use-context-menu-actions'
import { useDatabaseTreeLoader } from '../composables/use-database-tree-loader'
import { useDatabaseTreeSearch } from '../composables/use-database-tree-search'
import { useDragDrop } from '../composables/use-drag-drop'
import { useGroupManager } from '../composables/use-group-manager'
import { useKeyboardShortcuts } from '../composables/use-keyboard-shortcuts'
import { useVirtualTree } from '../composables/use-virtual-tree'
import { useDatabaseNavigatorStore } from '../stores/database-navigator-store'
import { NodeKeyEncoder } from '../types/virtual-tree'
import { debounceAsync } from '../utils/debounce'
import {
  clearConnectionNavigatorState,
  getConnectionNavigatorState,
  saveConnectionNavigatorState,
  saveLastActiveConnection,
  getLastActiveConnection,
} from '../utils/navigator-persistence'

import type { NavigatorError as NavigatorErrorType } from './navigator-error.vue'
import type { IContextMenuItem } from '../composables/use-context-menu-actions'
import type { VirtualTreeNode } from '../types/virtual-tree'

interface FilterConfig {
  showTables: boolean
  showViews: boolean
  showSystemSchemas: boolean
  showColumns: boolean
}

const { t } = useI18n()
const uiStore = useUiStore()
const projectConnectionStore = useProjectConnectionStore()
const runtimeConnectionStore = useRuntimeConnectionStore()
const connectionStore = useConnectionStore()
const navigatorStore = useDatabaseNavigatorStore()
const workbenchStore = useWorkbenchStore()

const searchQuery = ref('')
const selectedKey = ref<string | null>(null)
const currentConnection = ref<ProjectConnection | null>(null)
const isRefreshing = ref(false)
const showSearch = ref(false)
const showFilter = ref(false)
const isInTransaction = ref(false)
const transactionDuration = ref(0)
let transactionTimer: ReturnType<typeof setInterval> | null = null
const showError = ref(false)
const currentError = ref<NavigatorErrorType>()

// 分组相关状态
const showGroupDialog = ref(false)
const isEditGroup = ref(false)
const editingGroupId = ref<string | null>(null)
const searchRef = ref<InstanceType<typeof NavigatorSearch> | null>(null)
const contextMenuRef = ref<InstanceType<typeof NavigatorContextMenuV2> | null>(null)
const virtualTreeRef = ref<InstanceType<typeof VirtualTree> | null>(null)
const globalConnections = ref<GlobalConnectionInfo[]>([])
const contextMenuItems = ref<IContextMenuItem[]>([])
const contextMenuCurrentNode = ref<VirtualTreeNode | null>(null)

const showPropertiesDialog = ref(false)
const propertiesDialogTitle = ref('')
const propertiesItems = ref<Array<{ label: string; value: string }>>([])
const selectedNodeForStatus = ref<VirtualTreeNode | null>(null)

// 属性面板防抖：防止快速双击同一节点创建多个面板
const lastPropsPanelNode = ref<string>('')
const lastPropsPanelTime = ref<number>(0)

const filterConfig = ref<FilterConfig>({
  showTables: true,
  showViews: true,
  showSystemSchemas: false,
  showColumns: true,
})

// 业务逻辑 composables
const treeLoader = useDatabaseTreeLoader()
const treeSearch = useDatabaseTreeSearch()
const cacheWarming = useCacheWarming()
const groupManager = useGroupManager()
const adjacentPreload = useAdjacentPreload()
const connectionHandler = useConnectionHandler()
const contextMenuActions = useContextMenuActions()
const connectionStatusSync = useConnectionStatusSync()
const dragDrop = useDragDrop()

// 虚拟树控制器 - 先声明回调函数的引用
const handleVirtualTreeLoadChildrenRef: {
  value: (node: VirtualTreeNode) => Promise<VirtualTreeNode[]>
} = { value: async () => [] }
const handleVirtualTreeSelectRef: { value: (node: VirtualTreeNode) => void } = { value: () => {} }

// 模板中使用的事件处理器
function handleVirtualTreeSelect(node: VirtualTreeNode) {
  handleVirtualTreeSelectRef.value(node)
}

function handleVirtualTreeToggle(node: VirtualTreeNode) {
  toggleNode(node)

  if (node.isExpanded && node.data) {
    const { connectionId, dbName, schemaName, tableName } = node.data
    if (!connectionId || !dbName) return
    const connectionType = navigatorStore.getConnectionType(connectionId) || 'global'
    const projectPath = navigatorStore.getProjectPath(connectionId)

    if (node.type === 'table' && tableName) {
      adjacentPreload
        .preloadAdjacentNodes(
          connectionId,
          connectionType,
          dbName,
          schemaName,
          'table',
          tableName,
          projectPath
        )
        .catch(err => console.error('相邻节点预加载失败:', err))
    } else if (node.type === 'columns-folder' && tableName) {
      adjacentPreload
        .preloadAdjacentNodes(
          connectionId,
          connectionType,
          dbName,
          schemaName,
          'columns-folder',
          tableName,
          projectPath
        )
        .catch(err => console.error('相邻节点预加载失败:', err))
    }
  }
}

const {
  flatNodes: virtualTreeNodes,
  selectedKey: _virtualSelectedKey,
  setRootNodes,
  toggleNode,
  selectNode,
  clearConnection,
} = useVirtualTree({
  onLoadChildren: async (node: VirtualTreeNode) => handleVirtualTreeLoadChildrenRef.value(node),
  onSelect: (node: VirtualTreeNode) => handleVirtualTreeSelectRef.value(node),
})

const statusText = computed(() => {
  const node = selectedNodeForStatus.value
  if (node?.type === 'index') {
    const d = node.data
    const parts: string[] = []
    parts.push(`索引: ${d.indexName || node.label}`)
    if (d.indexType) parts.push(d.indexType)
    if (d.isUnique) parts.push('唯一')
    if (d.isPrimary) parts.push('主键')
    if (d.indexColumnNames?.length) parts.push(`[${d.indexColumnNames.join(', ')}]`)
    return parts.join(' | ')
  }
  if (node?.type === 'constraint') {
    const d = node.data
    const parts: string[] = []
    parts.push(`约束: ${d.constraintName || node.label} (${d.constraintType})`)
    if (d.constraintColumnNames?.length) parts.push(`[${d.constraintColumnNames.join(', ')}]`)
    if (d.referencedTable) parts.push(`→ ${d.referencedTable}`)
    return parts.join(' | ')
  }

  const allConnections = [...globalConnections.value, ...projectConnectionStore.connections]
  const totalConnections = allConnections.length
  let totalCatalogs = 0
  let totalTables = 0
  let totalViews = 0

  allConnections.forEach(conn => {
    const catalogs = navigatorStore.getCatalogs(conn.id)
    totalCatalogs += catalogs.length

    catalogs.forEach(cat => {
      if (!cat.schemas) return
      cat.schemas.forEach(schema => {
        totalTables += schema.tables?.length || 0
        totalViews += schema.views?.length || 0
      })
    })
  })

  return t('navigator.statusSummary', {
    connections: totalConnections,
    catalogs: totalCatalogs,
    tables: totalTables,
    views: totalViews,
  })
})

/**
 * 加载子节点 - 委托给 treeLoader composable
 */
handleVirtualTreeLoadChildrenRef.value = async function (
  node: VirtualTreeNode
): Promise<VirtualTreeNode[]> {
  return treeLoader.loadChildren(node)
}

function openIndexProperties(node: VirtualTreeNode) {
  const d = node.data
  propertiesDialogTitle.value = `索引属性 - ${d.indexName || node.label}`
  propertiesItems.value = [
    { label: '索引名称', value: d.indexName || node.label },
    { label: '所属表', value: `${d.dbName}.${d.schemaName || ''}.${d.tableName}` },
    { label: '索引类型', value: d.indexType || '-' },
    { label: '是否唯一', value: d.isUnique ? '是' : '否' },
    { label: '是否主键', value: d.isPrimary ? '是' : '否' },
    { label: '包含列', value: d.indexColumnNames?.join(', ') || '-' },
    { label: '注释', value: d.indexComment || '-' },
  ]
  showPropertiesDialog.value = true
}

function openConstraintProperties(node: VirtualTreeNode) {
  const d = node.data
  const items: Array<{ label: string; value: string }> = [
    { label: '约束名称', value: d.constraintName || node.label },
    { label: '所属表', value: `${d.dbName}.${d.schemaName || ''}.${d.tableName}` },
    { label: '约束类型', value: d.constraintType || '-' },
    { label: '包含列', value: d.constraintColumnNames?.join(', ') || '-' },
  ]
  if (d.referencedTable) {
    items.push({ label: '引用表', value: d.referencedTable })
    items.push({ label: '引用列', value: d.referencedColumns?.join(', ') || '-' })
    items.push({ label: '更新规则', value: d.updateRule || '-' })
    items.push({ label: '删除规则', value: d.deleteRule || '-' })
  }
  propertiesDialogTitle.value = `约束属性 - ${d.constraintName || node.label}`
  propertiesItems.value = items
  showPropertiesDialog.value = true
}

/**
 * 初始化根节点
 */
function initializeRootNodes() {
  const globalConns = globalConnections.value.map(conn => ({
    ...conn,
    db_type: conn.driver,
  }))
  const projectConns = projectConnectionStore.connections

  const rootNodes = treeLoader.createRootNodes(globalConns, projectConns)
  setRootNodes(rootNodes)

  for (const conn of globalConns) {
    const catalogs = navigatorStore.getCatalogs(conn.id)
    if (catalogs.length > 0) {
      const catalogNames = catalogs.map(cat => cat.name)
      cacheWarming.warmConnection(conn.id, 'global', catalogNames).catch(() => {})
    }
  }
  for (const conn of projectConns) {
    const catalogs = navigatorStore.getCatalogs(conn.id)
    if (catalogs.length > 0) {
      const catalogNames = catalogs.map(cat => cat.name)
      const projectPath = navigatorStore.getProjectPath(conn.id)
      cacheWarming.warmConnection(conn.id, 'project', catalogNames, projectPath).catch(() => {})
    }
  }
}

const onSearchQueryChange = debounceAsync(async (query: string) => {
  searchQuery.value = query

  const results = treeSearch.searchTables(
    query,
    filterConfig.value,
    globalConnections.value,
    projectConnectionStore.connections
  )

  if (searchRef.value) {
    searchRef.value.setSearchResults(results)
  }
}, 300)

const handleSearchSelect = async (result: {
  nodeKey: string
  tableName: string
  path: string
  connectionId: string
  dbName: string
  schemaName: string
}) => {
  const { nodeKey, connectionId, dbName, schemaName } = result

  const pathNodes = treeSearch.findNodePath(
    connectionId,
    dbName,
    schemaName,
    virtualTreeNodes.value
  )

  for (const pathNode of pathNodes) {
    if (!pathNode.isExpanded) {
      await toggleNode(pathNode)
    }
  }

  const targetNode = virtualTreeNodes.value.find(n => n.key === nodeKey)
  if (targetNode) {
    selectNode(targetNode)

    if (virtualTreeRef.value) {
      virtualTreeRef.value.scrollToNode(nodeKey)
    }
  }
}

const focusSearch = () => {
  showSearch.value = true
  setTimeout(() => {
    searchRef.value?.focus()
  }, 100)
}

const clearSearch = () => {
  searchQuery.value = ''
  showSearch.value = false
}

const toggleFilter = () => {
  showFilter.value = !showFilter.value
}

const toggleView = () => {
  uiStore.toggleNavigatorViewMode()
}

const handleNewConnection = () => {
  dispatchWorkbenchEvent(WorkbenchEvent.NewConnection)
}

const handleDisconnect = async () => {
  if (currentConnection.value) {
    await navigatorStore.disconnectConnection(currentConnection.value.id)
    currentConnection.value = null
    stopTransactionTimer()
  }
}

const handleRefresh = async () => {
  isRefreshing.value = true

  try {
    await loadGlobalConnections()

    const allConnections = [...globalConnections.value, ...projectConnectionStore.connections]

    for (const conn of allConnections) {
      navigatorStore.clearCache(conn.id)
      await navigatorStore.loadCatalogs(conn.id)
    }

    initializeRootNodes()
    handleErrorClose()
  } catch (error) {
    console.error('刷新连接失败:', error)
    showErrorMessage(
      t('navigator.refreshFailed'),
      error instanceof Error ? error.message : t('navigator.refreshError')
    )
  } finally {
    isRefreshing.value = false
  }
}

const handleBeginTransaction = async () => {
  if (!currentConnection.value) return

  try {
    await connectionStore.beginTransaction(currentConnection.value.id)
    isInTransaction.value = true
    transactionDuration.value = 0

    if (transactionTimer) {
      clearInterval(transactionTimer)
    }

    transactionTimer = setInterval(() => {
      transactionDuration.value += 1000
    }, 1000)
  } catch (error) {
    console.error('开始事务失败:', error)
    showErrorMessage(
      t('navigator.transactionFailed'),
      error instanceof Error ? error.message : t('navigator.beginTransactionError')
    )
  }
}

const handleCommitTransaction = async () => {
  if (!currentConnection.value) return

  try {
    await connectionStore.commitTransaction(currentConnection.value.id)
    stopTransactionTimer()
  } catch (error) {
    console.error('提交事务失败:', error)
    showErrorMessage(
      t('navigator.transactionFailed'),
      error instanceof Error ? error.message : t('navigator.commitTransactionError')
    )
  }
}

const handleRollbackTransaction = async () => {
  if (!currentConnection.value) return

  try {
    await connectionStore.rollbackTransaction(currentConnection.value.id)
    stopTransactionTimer()
  } catch (error) {
    console.error('回滚事务失败:', error)
    showErrorMessage(
      t('navigator.transactionFailed'),
      error instanceof Error ? error.message : t('navigator.rollbackTransactionError')
    )
  }
}

function stopTransactionTimer() {
  if (transactionTimer) {
    clearInterval(transactionTimer)
    transactionTimer = null
  }
  isInTransaction.value = false
  transactionDuration.value = 0
}

function openCreateGroupDialog() {
  isEditGroup.value = false
  editingGroupId.value = null
  showGroupDialog.value = true
}

function _openEditGroupDialog(groupId: string) {
  isEditGroup.value = true
  editingGroupId.value = groupId
  showGroupDialog.value = true
}

function closeGroupDialog() {
  showGroupDialog.value = false
  editingGroupId.value = null
}

function getEditingGroupData() {
  if (!editingGroupId.value) return undefined
  const group = groupManager.getGroupById(editingGroupId.value)
  if (group) {
    return {
      name: group.name,
      description: group.description,
      color: group.color,
    }
  }
  return undefined
}

function handleGroupSubmit(data: { name: string; description?: string; color?: string }) {
  if (isEditGroup.value && editingGroupId.value) {
    groupManager.updateGroup(editingGroupId.value, {
      name: data.name,
      description: data.description,
      color: data.color,
    })
  } else {
    groupManager.createGroup(data.name, data.description)
  }
  closeGroupDialog()
}

function _handleDeleteGroup(groupId: string) {
  groupManager.deleteGroup(groupId)
}

function showErrorMessage(title: string, message: string) {
  currentError.value = { title, message }
  showError.value = true
}

function handleErrorClose() {
  showError.value = false
  currentError.value = undefined
}

async function handleErrorRetry() {
  showError.value = false
  currentError.value = undefined
  await handleRefresh()
}

handleVirtualTreeSelectRef.value = async (node: VirtualTreeNode) => {
  selectedNodeForStatus.value = node

  if (node.type === 'connection') {
    // 保存 key，因为 handleConnectionClick 内部会调用 initializeRootNodes
    // 重建整个树，导致旧的 node 引用失效
    const connKey = node.key

    const currentConn = await connectionHandler.handleConnectionClick(
      node,
      globalConnections.value,
      projectConnectionStore.connections,
      clearConnection,
      initializeRootNodes
    )
    if (currentConn) {
      currentConnection.value = currentConn
      // 保存最后活跃连接，支持跨会话恢复
      const scopeRaw = node.data.scope
      const scope: 'global' | 'project' =
        scopeRaw === 'global' || scopeRaw === 'project' ? scopeRaw : 'global'
      saveLastActiveConnection(currentConn.id, scope, navigatorStore.getProjectPath(currentConn.id))
      // 同步到全局 connectionStore，打通 SQL 编辑器状态栏
      connectionStore.syncConnectionStatus(
        currentConn.id,
        runtimeConnectionStore.runtimeConnectionIds.has(currentConn.id)
      )

      // 连接建立后自动展开节点，加载 Catalog 列表
      // 从重建后的树中查找新节点（旧引用已失效）
      const newNode = virtualTreeNodes.value.find(n => n.key === connKey)
      if (newNode && !newNode.isExpanded && !newNode.isLeaf) {
        await toggleNode(newNode)
      }
    }
  }

  if (node.type === 'table' || node.type === 'view') {
    const result = connectionHandler.handleOpenTableOrView(node, projectConnectionStore.connections)
    if (result?.connection) {
      currentConnection.value = result.connection
      // 同步到全局 connectionStore，打通 SQL 编辑器状态栏
      connectionStore.syncConnectionStatus(
        result.connection.id,
        runtimeConnectionStore.runtimeConnectionIds.has(result.connection.id)
      )
    }
    if (result?.connectionId && result?.dbName && result?.schemaName && result?.tableName) {
      workbenchStore.openTableData(
        result.connectionId,
        result.dbName,
        result.schemaName,
        result.tableName
      )
    }
  }
}

const handleVirtualTreeContextMenu = (node: VirtualTreeNode, event: MouseEvent) => {
  contextMenuCurrentNode.value = node
  contextMenuItems.value = contextMenuActions.getNodeMenu(node)

  if (contextMenuRef.value) {
    contextMenuRef.value.show(event)
  }
}

const handleVirtualTreeDblClick = (node: VirtualTreeNode) => {
  if (!node.isLeaf) {
    toggleNode(node)
  } else {
    handleNodeDblClick(node)
  }
}

function handleNodeDblClick(node: VirtualTreeNode) {
  // 防抖：同一节点 300ms 内不重复打开属性面板
  const now = Date.now()
  if (lastPropsPanelNode.value === node.key && now - lastPropsPanelTime.value < 300) {
    return
  }
  lastPropsPanelNode.value = node.key
  lastPropsPanelTime.value = now

  const keyParts = NodeKeyEncoder.decode(node.key)
  if (keyParts.length < 2) return

  const nodeType = keyParts[0]

  // connection 节点 key 结构：[type, scope, connId]
  // 其他节点 key 结构：[type, connId, dbName, ...]
  const connectionId = nodeType === 'connection' ? keyParts[2] : keyParts[1]
  const scope =
    nodeType === 'connection'
      ? (keyParts[1] as 'global' | 'project')
      : (node.data?.scope as 'global' | 'project') || 'global'

  const dbType = (node.data?.driver as string) || ''
  const dbName = nodeType === 'connection' ? (node.data?.name as string) || '' : keyParts[2]
  const schemaName = keyParts.length > 3 ? keyParts[3] : ''

  // 双击时打开属性面板（DBeaver 对标行为）
  if (
    [
      'table',
      'view',
      'schema',
      'catalog',
      'connection',
      'column',
      'index',
      'constraint',
      'function',
      'procedure',
      'sequence',
      'trigger',
    ].includes(nodeType)
  ) {
    const objectName =
      nodeType === 'connection'
        ? (node.data?.name as string) || ''
        : nodeType === 'catalog'
          ? keyParts[2]
          : nodeType === 'schema'
            ? keyParts[3]
            : nodeType === 'column'
              ? keyParts.length > 5
                ? keyParts[5]
                : keyParts[keyParts.length - 1]
              : nodeType === 'index' || nodeType === 'constraint'
                ? keyParts.length > 5
                  ? keyParts[5]
                  : keyParts[keyParts.length - 1]
                : keyParts.length > 4
                  ? keyParts[4]
                  : keyParts[keyParts.length - 1]

    // 对于 column/index/constraint，需要传递额外的上下文信息
    const extra: Record<string, unknown> = {}
    if (nodeType === 'column') {
      extra.tableName = keyParts.length > 4 ? keyParts[4] : ''
      extra.columnName = objectName
    } else if (nodeType === 'index') {
      extra.tableName = keyParts.length > 4 ? keyParts[4] : ''
      extra.indexName = objectName
    } else if (nodeType === 'constraint') {
      extra.tableName = keyParts.length > 4 ? keyParts[4] : ''
      extra.constraintName = objectName
    }

    window.dispatchEvent(
      new CustomEvent('open-object-properties', {
        detail: {
          connectionId,
          scope,
          dbType,
          dbName,
          schemaName,
          objectType: nodeType,
          objectName,
          ...extra,
        },
      })
    )
  }
}

function handleNodeDragStart(node: VirtualTreeNode, event: DragEvent) {
  if (!dragDrop.isDraggable(node)) {
    event.preventDefault()
    return
  }

  dragDrop.handleDragStart(node, event)
  if (event.dataTransfer) {
    event.dataTransfer.effectAllowed = 'copy'
  }
}

function handleNodeDragEnd() {
  dragDrop.handleDragEnd()
}

function setupWindowEventListeners() {
  window.addEventListener('open-create-table', handleOpenCreateTable)
  window.addEventListener('open-create-view', handleOpenCreateView)
  window.addEventListener('open-create-function', handleOpenCreateFunction)
  window.addEventListener('open-create-procedure', handleOpenCreateProcedure)
  window.addEventListener('open-sql-editor', handleOpenSqlEditor)
  window.addEventListener('open-table-data', handleOpenTableData)
  window.addEventListener('open-table-ddl', handleOpenTableDdl)
  window.addEventListener('open-connection-editor', handleOpenConnectionEditor)
  window.addEventListener('show-index-properties', handleShowIndexProperties)
  window.addEventListener('show-constraint-properties', handleShowConstraintProperties)
}

function cleanupWindowEventListeners() {
  window.removeEventListener('open-create-table', handleOpenCreateTable)
  window.removeEventListener('open-create-view', handleOpenCreateView)
  window.removeEventListener('open-create-function', handleOpenCreateFunction)
  window.removeEventListener('open-create-procedure', handleOpenCreateProcedure)
  window.removeEventListener('open-sql-editor', handleOpenSqlEditor)
  window.removeEventListener('open-table-data', handleOpenTableData)
  window.removeEventListener('open-table-ddl', handleOpenTableDdl)
  window.removeEventListener('open-connection-editor', handleOpenConnectionEditor)
  window.removeEventListener('show-index-properties', handleShowIndexProperties)
  window.removeEventListener('show-constraint-properties', handleShowConstraintProperties)
}

function handleOpenCreateTable(event: Event) {
  const detail = (event as CustomEvent).detail
  const { connectionId, dbName, schemaName: _schemaName } = detail
  if (!connectionId || !dbName) return

  dispatchWorkbenchEvent(WorkbenchEvent.NewConnection)
}

function handleOpenCreateView(event: Event) {
  const detail = (event as CustomEvent).detail
  const { connectionId, dbName, schemaName } = detail
  if (!connectionId || !dbName) return

  window.dispatchEvent(
    new CustomEvent('open-sql-editor', {
      detail: {
        connectionId,
        databaseName: dbName,
        schemaName: schemaName || dbName,
        sql: `CREATE VIEW new_view AS SELECT 1;`,
      },
    })
  )
}

function handleOpenCreateFunction(event: Event) {
  const detail = (event as CustomEvent).detail
  const { connectionId, dbName, schemaName } = detail
  if (!connectionId || !dbName) return

  window.dispatchEvent(
    new CustomEvent('open-sql-editor', {
      detail: {
        connectionId,
        databaseName: dbName,
        schemaName: schemaName || dbName,
        sql: `CREATE FUNCTION new_function() RETURNS INT AS $$ BEGIN RETURN 1; END; $$ LANGUAGE plpgsql;`,
      },
    })
  )
}

function handleOpenCreateProcedure(event: Event) {
  const detail = (event as CustomEvent).detail
  const { connectionId, dbName, schemaName } = detail
  if (!connectionId || !dbName) return

  window.dispatchEvent(
    new CustomEvent('open-sql-editor', {
      detail: {
        connectionId,
        databaseName: dbName,
        schemaName: schemaName || dbName,
        sql: `CREATE PROCEDURE new_procedure() BEGIN END;`,
      },
    })
  )
}

function handleOpenSqlEditor(event: Event) {
  const detail = (event as CustomEvent).detail
  const { connectionId, databaseName, schemaName, sql } = detail

  workbenchStore.addEditorTab(connectionId, sql || '', `${databaseName}.${schemaName || ''}`)
}

function handleOpenTableData(event: Event) {
  const detail = (event as CustomEvent).detail
  const { connectionId, dbName, schemaName, tableName } = detail

  workbenchStore.openTableData(connectionId, dbName, schemaName, tableName)
}

function handleOpenTableDdl(event: Event) {
  const detail = (event as CustomEvent).detail
  const { connectionId, dbName, schemaName, tableName } = detail

  workbenchStore.openTableData(connectionId, dbName, schemaName, tableName)
}

function handleOpenConnectionEditor(event: Event) {
  const _detail = (event as CustomEvent).detail
  dispatchWorkbenchEvent(WorkbenchEvent.NewConnection)
}

function handleShowIndexProperties(event: Event) {
  const detail = (event as CustomEvent).detail
  openIndexProperties(detail.node)
}

function handleShowConstraintProperties(event: Event) {
  const detail = (event as CustomEvent).detail
  openConstraintProperties(detail.node)
}

// 键盘快捷键 - 必须在所有函数定义之后初始化
const _keyboardShortcuts = useKeyboardShortcuts({
  onNewConnection: handleNewConnection,
  onDisconnect: handleDisconnect,
  onRefresh: handleRefresh,
  onSearch: focusSearch,
  onBeginTransaction: handleBeginTransaction,
  onCommitTransaction: handleCommitTransaction,
  onRollbackTransaction: handleRollbackTransaction,
})

const _handleContextMenuRefresh = async () => {
  if (contextMenuCurrentNode.value?.data?.connectionId) {
    const connId = contextMenuCurrentNode.value?.data.connectionId
    clearConnection(connId)
    await navigatorStore.loadCatalogs(connId)
    initializeRootNodes()
  }
}

const _handleContextMenuCopyName = () => {
  const node = contextMenuCurrentNode.value
  if (!node) return

  const name = node.label || node.key.split('_').pop() || ''
  navigator.clipboard.writeText(name).catch(() => {
    console.warn('[navigator] 复制名称失败')
  })
}

const _handleContextMenuOpenTable = () => {
  const node = contextMenuCurrentNode.value
  if (!node || node.type !== 'table') return

  const { connectionId, dbName, schemaName, tableName } = node.data
  if (connectionId && dbName && tableName) {
    workbenchStore.openTableData(connectionId, dbName, schemaName || '', tableName)
  }
}

const _handleContextMenuOpenView = () => {
  const node = contextMenuCurrentNode.value
  if (!node || node.type !== 'view') return

  const { connectionId, dbName, schemaName, viewName } = node.data
  if (connectionId && dbName && viewName) {
    workbenchStore.openTableData(connectionId, dbName, schemaName || '', viewName)
  }
}

const _handleExpandAll = async () => {
  if (!virtualTreeRef.value) return

  const allNodes = virtualTreeNodes.value
  for (const node of allNodes) {
    if (!node.isExpanded && !node.isLeaf) {
      await toggleNode(node)
    }
  }
}

const _handleCollapseAll = async () => {
  if (!virtualTreeRef.value) return

  const allNodes = [...virtualTreeNodes.value]
  for (const node of allNodes.reverse()) {
    if (node.isExpanded && !node.isLeaf) {
      await toggleNode(node)
    }
  }
}

const _handleContextMenuRefreshSchema = async () => {
  const node = contextMenuCurrentNode.value
  if (!node?.data) return

  const { connectionId, dbName, schemaName } = node.data
  if (!connectionId) return

  if (dbName && schemaName) {
    await navigatorStore.refreshMetadata(connectionId, dbName, schemaName)
  } else if (dbName) {
    await navigatorStore.refreshMetadata(connectionId, dbName)
  }

  initializeRootNodes()
}

const _handleContextMenuRefreshDatabase = async () => {
  const node = contextMenuCurrentNode.value
  if (!node?.data?.connectionId) return

  const connId = node.data.connectionId
  if (!connId) return
  clearConnectionNavigatorState(connId)
  clearConnection(connId)
  await navigatorStore.refreshMetadata(connId)
  initializeRootNodes()
}

async function loadGlobalConnections() {
  try {
    const connections = await getGlobalConnections()
    globalConnections.value = connections

    for (const conn of connections) {
      navigatorStore.setConnectionInfo(conn.id, 'global', undefined, conn.driver)
    }

    // 同步到 connectionStore
    for (const conn of connections) {
      connectionStore.syncConnectionStatus(
        conn.id,
        runtimeConnectionStore.runtimeConnectionIds.has(conn.id)
      )
    }
  } catch (error) {
    console.error('加载全局连接失败:', error)
    globalConnections.value = []
  }
}

watch(
  () => projectConnectionStore.connections,
  () => {
    initializeRootNodes()
    // 同步项目连接到 connectionStore
    for (const conn of projectConnectionStore.connections) {
      connectionStore.syncConnectionStatus(
        conn.id,
        runtimeConnectionStore.runtimeConnectionIds.has(conn.id)
      )
    }
  },
  { deep: true }
)

watch(
  () => runtimeConnectionStore.runtimeConnectionIds,
  () => {
    initializeRootNodes()
    // 同步运行时连接状态到 connectionStore
    for (const conn of globalConnections.value) {
      connectionStore.syncConnectionStatus(
        conn.id,
        runtimeConnectionStore.runtimeConnectionIds.has(conn.id)
      )
    }
    for (const conn of projectConnectionStore.connections) {
      connectionStore.syncConnectionStatus(
        conn.id,
        runtimeConnectionStore.runtimeConnectionIds.has(conn.id)
      )
    }
  },
  { deep: true }
)

watch(
  () => virtualTreeNodes.value.map(n => ({ key: n.key, isExpanded: n.isExpanded })),
  () => {
    const connId = currentConnection.value?.id
    if (connId) {
      debouncedPersistSave(connId)
    }
  },
  { deep: true }
)

onMounted(async () => {
  await loadGlobalConnections()
  await projectConnectionStore.loadConnections()

  for (const conn of projectConnectionStore.connections) {
    navigatorStore.setConnectionInfo(conn.id, 'project', undefined, conn.driver)
  }

  initializeRootNodes()

  setupWindowEventListeners()

  // 恢复上次活跃连接（跨会话持久化）
  try {
    const lastActive = getLastActiveConnection()
    if (lastActive) {
      const scopeNode = virtualTreeNodes.value.find(
        n =>
          n.type === 'connection' &&
          n.data.connectionId === lastActive.connId &&
          n.data.scope === lastActive.scope
      )
      if (scopeNode && !scopeNode.isExpanded) {
        await toggleNode(scopeNode)
        await handleVirtualTreeSelect(scopeNode)
      }
    }
  } catch (e) {
    console.warn('[navigator] 恢复上次活跃连接失败', e)
  }

  const allConnections = [...globalConnections.value, ...projectConnectionStore.connections]
  for (const conn of allConnections) {
    connectionStatusSync.startHealthCheck(conn.id)
    restoreNavigatorState(conn.id)
  }
})

onUnmounted(() => {
  treeLoader.abortPendingLoads()
  if (persistTimer) clearTimeout(persistTimer)
  connectionStatusSync.cleanup()
  cleanupWindowEventListeners()
  saveAllNavigatorStates()
})

function getExpandedKeys(): string[] {
  return virtualTreeNodes.value.filter(n => n.isExpanded).map(n => n.key)
}

function restoreNavigatorState(connId: string): void {
  try {
    const entry = getConnectionNavigatorState(connId)
    if (!entry?.expandedKeys.length) return

    for (const node of virtualTreeNodes.value) {
      if (entry.expandedKeys.includes(node.key) && !node.isExpanded && !node.isLeaf) {
        node.isExpanded = true
      }
    }
  } catch (e) {
    console.warn(`[navigator] 恢复持久化状态失败 (${connId})`, e)
  }
}

let persistTimer: ReturnType<typeof setTimeout> | null = null

function debouncedPersistSave(connId: string): void {
  if (persistTimer) clearTimeout(persistTimer)
  persistTimer = setTimeout(() => {
    saveConnectionNavigatorState(connId, { expandedKeys: getExpandedKeys() })
  }, 800)
}

function saveAllNavigatorStates(): void {
  try {
    const connIds = new Set<string>()
    for (const node of virtualTreeNodes.value) {
      const connId = node.data?.connectionId
      if (connId) {
        connIds.add(connId)
      }
    }
    for (const connId of connIds) {
      saveConnectionNavigatorState(connId, { expandedKeys: getExpandedKeys() })
    }
  } catch (e) {
    console.warn('[navigator] 保存持久化状态失败', e)
  }
}
</script>

<style scoped>
.database-navigator {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg-primary);
}

.navigator-tree {
  flex: 1;
  overflow: hidden;
  min-height: 0;
}

.dbeaver-style.dark {
  --bg-primary: #1e1e1e;
  --bg-secondary: #252526;
  --bg-tertiary: #2d2d30;
  --text-primary: #cccccc;
  --text-secondary: #858585;
  --text-tertiary: #666666;
  --border-color: #3e3e42;
}

:root:not(.dbeaver-style.dark) {
  --bg-primary: #ffffff;
  --bg-secondary: #f5f5f5;
  --bg-tertiary: #e8e8e8;
  --text-primary: #333333;
  --text-secondary: #666666;
  --text-tertiary: #999999;
  --border-color: #d9d9d9;
}

.properties-content {
  padding: 4px 0;
}
</style>
