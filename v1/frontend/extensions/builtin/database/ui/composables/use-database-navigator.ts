import { ref, computed } from 'vue'

import {
  getGlobalConnections,
  saveNavigatorState,
  loadNavigatorState as fetchNavigatorState,
} from '../../../connection/ui/services/connection'
import { useProjectConnectionStore } from '../../../connection/ui/stores/project-connection-store'
import { useWorkbenchStore } from '../../../workbench/ui/stores/workbench-store'
import { useDatabaseNavigatorStore } from '../stores/database-navigator-store'

import type { ProjectConnection } from '../../../connection/types/connection'
import type { GlobalConnection, FilterConfig } from '../types/navigator'

export function useDatabaseNavigator() {
  const navigatorStore = useDatabaseNavigatorStore()
  const projectConnectionStore = useProjectConnectionStore()
  const workbenchStore = useWorkbenchStore()

  const searchQuery = ref('')
  const expandedKeys = ref<string[]>([])
  const selectedKeys = ref<string[]>([])
  const currentConnection = ref<GlobalConnection | ProjectConnection | null>(null)
  const isRefreshing = ref(false)
  const showSearch = ref(false)
  const showFilter = ref(false)

  const filterConfig = ref<FilterConfig>({
    showTables: true,
    showViews: true,
    showSystemSchemas: false,
    showColumns: true,
  })

  const globalConnections = ref<GlobalConnection[]>([])

  const allConnections = computed(() => [
    ...globalConnections.value,
    ...projectConnectionStore.connections,
  ])

  const statusText = computed(() => {
    const totalConnections = allConnections.value.length
    let totalCatalogs = 0
    let totalTables = 0
    let totalViews = 0

    allConnections.value.forEach(conn => {
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

    return `连接数: ${totalConnections} | Catalog: ${totalCatalogs} | 表: ${totalTables} | 视图: ${totalViews}`
  })

  async function loadGlobalConnections() {
    try {
      const connections = await getGlobalConnections()
      globalConnections.value = connections

      for (const conn of connections) {
        navigatorStore.setConnectionInfo(conn.id, 'global', undefined, conn.driver)
        await navigatorStore.loadCatalogs(conn.id)
        navigatorStore.startCacheWarming(conn.id)
      }
    } catch (error) {
      console.error('加载全局连接失败:', error)
    }
  }

  async function loadProjectConnections() {
    if (projectConnectionStore.connections.length === 0) {
      await projectConnectionStore.loadConnections()
    }

    const projectStore = await import('@/core/project/stores/project').then(m =>
      m.useProjectStore()
    )
    const projectPath = projectStore.currentProject?.path

    for (const conn of projectConnectionStore.connections) {
      navigatorStore.setConnectionInfo(conn.id, 'project', projectPath, conn.driver)
      await navigatorStore.loadCatalogs(conn.id)
      navigatorStore.startCacheWarming(conn.id)
    }
  }

  async function refreshAllMetadata() {
    isRefreshing.value = true

    try {
      await Promise.all([
        ...projectConnectionStore.connections.map(conn => navigatorStore.refreshMetadata(conn.id)),
        ...globalConnections.value.map(conn => navigatorStore.refreshMetadata(conn.id)),
      ])

      expandedKeys.value = []
      selectedKeys.value = []
    } catch (error) {
      console.error('刷新失败:', error)
    } finally {
      isRefreshing.value = false
    }
  }

  async function disconnectConnection(connectionId: string) {
    await navigatorStore.disconnectConnection(connectionId)

    if (currentConnection.value?.id === connectionId) {
      currentConnection.value = null
    }
  }

  function handleNodeSelect(nodeKey: string) {
    selectedKeys.value = [nodeKey]
    saveNavigatorStateDebounced()

    const keyParts = nodeKey.split('_')

    if (keyParts[0] === 'table' || keyParts[0] === 'view') {
      const connectionId = keyParts[1]
      const schemaName = keyParts[3]
      const tableName = keyParts.slice(4).join('_')

      const sql = `SELECT * FROM ${schemaName}.${tableName} LIMIT 100;`
      workbenchStore.addEditorTab(connectionId, sql)
    } else if (keyParts[0] === 'conn') {
      const connectionId = keyParts[1]
      const conn = allConnections.value.find(c => c.id === connectionId)
      if (conn) {
        currentConnection.value = conn
      }
    }
  }

  function handleNodeExpand(nodeKey: string) {
    if (!expandedKeys.value.includes(nodeKey)) {
      expandedKeys.value.push(nodeKey)
    }
    saveNavigatorStateDebounced()
  }

  function handleNodeCollapse(nodeKey: string) {
    expandedKeys.value = expandedKeys.value.filter(k => k !== nodeKey)
    saveNavigatorStateDebounced()
  }

  function handleSearch(query: string) {
    searchQuery.value = query

    if (query && query.trim().length > 0) {
      const results = navigatorStore.searchObjects(query)
      if (results.length > 0) {
        const connectionIds = [...new Set(results.map(r => r.connectionId))]
        connectionIds.forEach(connId => {
          const connKey = `conn_${connId}`
          if (!expandedKeys.value.includes(connKey)) {
            expandedKeys.value.push(connKey)
          }
        })
      }
    } else {
      clearSearch()
    }
  }

  function clearSearch() {
    searchQuery.value = ''
    showSearch.value = false
  }

  function focusSearch() {
    showSearch.value = true
  }

  function toggleFilter() {
    showFilter.value = !showFilter.value
  }

  let saveTimeout: ReturnType<typeof setTimeout> | null = null

  function saveNavigatorStateDebounced() {
    if (saveTimeout) {
      clearTimeout(saveTimeout)
    }

    saveTimeout = setTimeout(async () => {
      try {
        for (const conn of allConnections.value) {
          await saveNavigatorState(
            conn.id,
            expandedKeys.value.filter(k => k.startsWith(`conn_${conn.id}`)),
            selectedKeys.value.filter(k => k.startsWith(`conn_${conn.id}`)),
            {
              showTables: filterConfig.value.showTables,
              showViews: filterConfig.value.showViews,
              showColumns: filterConfig.value.showColumns,
              showSystemSchemas: filterConfig.value.showSystemSchemas,
            }
          )
        }
      } catch (error) {
        console.error('保存导航器状态失败:', error)
      }
    }, 1000)
  }

  async function loadNavigatorState() {
    try {
      const firstConn = allConnections.value[0]
      if (!firstConn) return

      const state = await fetchNavigatorState(firstConn.id)

      if (state.expanded_keys.length > 0) {
        expandedKeys.value = state.expanded_keys
      }

      if (state.selected_keys.length > 0) {
        selectedKeys.value = state.selected_keys
      }

      if (state.filter_config) {
        const fc = state.filter_config as Record<string, boolean>
        filterConfig.value = {
          showTables: fc.showTables ?? true,
          showViews: fc.showViews ?? true,
          showColumns: fc.showColumns ?? true,
          showSystemSchemas: fc.showSystemSchemas ?? false,
        }
      }
    } catch (error) {
      console.error('加载导航器状态失败:', error)
    }
  }

  return {
    searchQuery,
    expandedKeys,
    selectedKeys,
    currentConnection,
    isRefreshing,
    showSearch,
    showFilter,
    filterConfig,
    globalConnections,
    allConnections,
    statusText,
    loadGlobalConnections,
    loadProjectConnections,
    refreshAllMetadata,
    disconnectConnection,
    handleNodeSelect,
    handleNodeExpand,
    handleNodeCollapse,
    handleSearch,
    clearSearch,
    focusSearch,
    toggleFilter,
    loadNavigatorState,
    saveNavigatorStateDebounced,
  }
}
