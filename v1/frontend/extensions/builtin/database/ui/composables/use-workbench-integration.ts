import { ref } from 'vue'

import { useWorkbenchStore } from '../../../workbench/ui/stores/workbench-store'
import { useDatabaseNavigatorStore } from '../stores/database-navigator-store'

export interface WorkbenchIntegrationState {
  activeEditorConnectionId: string | null
  activeEditorTable: string | null
  isSyncingNavigation: boolean
}

export function useWorkbenchIntegration() {
  const workbenchStore = useWorkbenchStore()
  const navigatorStore = useDatabaseNavigatorStore()

  const state = ref<WorkbenchIntegrationState>({
    activeEditorConnectionId: null,
    activeEditorTable: null,
    isSyncingNavigation: false,
  })

  function openTableInWorkbench(
    connectionId: string,
    schemaName: string,
    tableName: string,
    _objectType: 'table' | 'view' = 'table'
  ) {
    const sql = `SELECT * FROM ${schemaName}.${tableName} LIMIT 100;`
    const tabTitle = `${schemaName}.${tableName}`

    workbenchStore.addEditorTab(connectionId, sql, tabTitle)

    state.value.activeEditorConnectionId = connectionId
    state.value.activeEditorTable = `${schemaName}.${tableName}`
  }

  function openViewInWorkbench(connectionId: string, schemaName: string, viewName: string) {
    openTableInWorkbench(connectionId, schemaName, viewName, 'view')
  }

  function generateInsertSQL(
    connectionId: string,
    schemaName: string,
    tableName: string,
    columns: Array<{ name: string; dataType: string }>
  ) {
    const columnNames = columns.map(c => c.name).join(', ')
    const placeholders = columns.map(() => '?').join(', ')
    const sql = `INSERT INTO ${schemaName}.${tableName} (${columnNames}) VALUES (${placeholders});`

    workbenchStore.addEditorTab(connectionId, sql, `INSERT: ${schemaName}.${tableName}`)
  }

  function generateCreateTableSQL(connectionId: string, schemaName: string, tableName: string) {
    const sql = `CREATE TABLE ${schemaName}.${tableName} (
  id SERIAL PRIMARY KEY,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);`

    workbenchStore.addEditorTab(connectionId, sql, `CREATE: ${schemaName}.${tableName}`)
  }

  function syncNavigationToTree(connectionId: string, tableName: string) {
    state.value.isSyncingNavigation = true

    const tableKey = `table_${connectionId}_${tableName}`
    const connKey = `conn_${connectionId}`

    navigatorStore.expandToNode(connKey)
    navigatorStore.expandToNode(tableKey)
    navigatorStore.selectNode(tableKey)

    state.value.isSyncingNavigation = false
  }

  function onEditorTabChanged(connectionId: string, tableName: string) {
    state.value.activeEditorConnectionId = connectionId
    state.value.activeEditorTable = tableName

    syncNavigationToTree(connectionId, tableName)
  }

  function getActiveEditorContext() {
    return {
      connectionId: state.value.activeEditorConnectionId,
      table: state.value.activeEditorTable,
    }
  }

  return {
    state,
    openTableInWorkbench,
    openViewInWorkbench,
    generateInsertSQL,
    generateCreateTableSQL,
    syncNavigationToTree,
    onEditorTabChanged,
    getActiveEditorContext,
  }
}
