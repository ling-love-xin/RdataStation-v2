/**
 * useSidebarConnection — 侧边栏连接操作 Composable
 *
 * 从 DataSourceSidebar.vue 提取，处理：
 * - 打开已保存连接 → 建立运行时连接 + 切换活动连接 + 打开查询编辑器
 * - 测试已保存连接 → 调用 test_connection 并更新状态
 */
import { ref } from 'vue'

import {
  WorkbenchEvent,
  dispatchWorkbenchEvent,
} from '@/extensions/builtin/workbench/ui/constants/workbench-events'

import type { ProjectConnection } from '../../types/connection'

export interface SidebarConnectionDeps {
  getConnectionUrl: (conn: ProjectConnection) => string
  updateConnectionStatus: (id: string, status: string, errorMsg?: string) => Promise<void>
  loadConnections: () => Promise<void>
  currentProjectId: () => string | null
}

export function useSidebarConnection(deps: SidebarConnectionDeps) {
  const testingId = ref<string | null>(null)

  /** 从侧边栏打开已有连接 → 切换活动连接并打开查询编辑器 */
  async function openSavedConnection(conn: ProjectConnection) {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      const url = deps.getConnectionUrl(conn)
      const driverName = conn.driver || 'mysql'

      const input: Record<string, unknown> = {
        db_type: driverName,
        url,
        name: conn.name,
        connection_type: conn.connection_type || 'project',
        project_id: deps.currentProjectId() ?? null,
        driver_id: conn.driver_id ?? null,
        auth_config_id: conn.auth_config_id ?? null,
        auth_method: conn.auth_method ?? null,
        network_config_id: conn.network_config_id ?? null,
        driver_properties: conn.driver_properties ?? null,
        advanced_options: conn.advanced_options ?? null,
        description: conn.description ?? null,
        environment_id: conn.environment_id ?? null,
        options: conn.options ?? null,
        tags: conn.tags ?? null,
        metadata_path: conn.metadata_path ?? null,
        schema_name: conn.schema_name ?? null,
        use_duckdb_fed: conn.use_duckdb_fed ?? false,
      }

      // 1. 建立（或复用）运行时连接
      const r = await invoke<{ conn_id: string; name: string; db_type: string; url: string }>(
        'connect_database',
        { input }
      )

      // 2. 切换为活动连接
      await invoke('switch_connection', { connId: r.conn_id })

      // 3. 派发事件打开查询编辑器
      dispatchWorkbenchEvent(WorkbenchEvent.NewQuery, {
        connectionId: r.conn_id,
        databaseName: conn.database || '',
        sql: '',
      })

      // 4. 刷新侧边栏连接列表（更新状态）
      await deps.loadConnections()
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      // eslint-disable-next-line no-console
      console.error(`[sidebar:open] ${conn.name}:`, msg)
    }
  }

  /** 从侧边栏测试已保存连接 */
  async function testSavedConnection(conn: ProjectConnection) {
    testingId.value = conn.id
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      const url = deps.getConnectionUrl(conn)
      const driverName = conn.driver || 'mysql'
      const r = await invoke<{ success: boolean; message?: string; response_time_ms?: number }>(
        'test_connection',
        { dbType: driverName, url }
      )
      const msg = r.success
        ? `✓ 连接成功 (${r.response_time_ms ?? '?'}ms)`
        : `✗ ${r.message || '连接失败'}`
      // 更新本地连接状态
      await deps.updateConnectionStatus(
        conn.id,
        r.success ? 'connected' : 'error',
        r.success ? undefined : r.message || '连接失败'
      )
      // eslint-disable-next-line no-console
      console.warn(`[sidebar:test] ${conn.name}: ${msg}`)
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      // eslint-disable-next-line no-console
      console.error(`[sidebar:test] ${conn.name} 失败:`, msg)
      await deps.updateConnectionStatus(conn.id, 'error', msg)
    } finally {
      testingId.value = null
    }
  }

  return { testingId, openSavedConnection, testSavedConnection }
}
