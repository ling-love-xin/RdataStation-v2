/**
 * @deprecated 此模块为遗留代码，已被 database-navigator 的主加载流程替代。
 *
 * 旧版适配器模式（DatabaseMetaAdapter + execute_sql）不再使用。
 * 当前主流程：use-database-tree-loader.ts → database-navigator-store.ts → database-api.ts (tauri.invoke)
 *
 * @see ../../ui/composables/use-database-tree-loader.ts  — 当前树加载器
 * @see ../../ui/api/database-api.ts              — 当前 IPC API 层
 * @see ../../ui/stores/database-navigator-store.ts — 当前状态管理
 *
 * 计划移除版本：v0.6.0
 */

import { invoke } from '@tauri-apps/api/core'

import { getAdapter } from '@/adapters'
import type { DatabaseMetaAdapter } from '@/adapters'
import type { NavigatorNode, QueryContext } from '@/shared/types/databaseMeta'

/**
 * 加载节点子节点
 * 根据节点类型和数据库类型选择正确的加载方式
 *
 * @deprecated 使用 use-database-tree-loader.ts::loadChildren() 替代
 */
export async function loadNodeChildren(
  node: NavigatorNode,
  dbType: string = 'mysql'
): Promise<NavigatorNode[]> {
  console.warn(
    '[NavigatorLoader][DEPRECATED] loadNodeChildren 已弃用，' + '请使用 use-database-tree-loader.ts'
  )

  const adapter = getAdapter(dbType)
  if (!adapter) {
    console.warn(`[NavigatorLoader] 未找到适配器: ${dbType}`)
    return []
  }

  if (node.type === 'connection') {
    return loadConnectionChildren(node, dbType, adapter)
  }

  const context: QueryContext = {
    connectionId: node.connectionId || '',
    database: node.database,
    schema: node.schema,
    table: (node.metadata?.tableName as string) || node.name,
  }

  const query = adapter.getChildrenQuery(node.type, context)

  if (!query) {
    return adapter.parseChildrenResult(node.type, [], context)
  }

  try {
    const result = await executeQuery(query, context)
    return adapter.parseChildrenResult(node.type, result, context)
  } catch (e) {
    console.error(`[NavigatorLoader] 查询失败:`, e)
    return []
  }
}

function loadConnectionChildren(
  node: NavigatorNode,
  dbType: string,
  adapter: DatabaseMetaAdapter
): NavigatorNode[] {
  const context: QueryContext = {
    connectionId: node.connectionId || '',
  }

  return adapter.parseChildrenResult('connection', [], context)
}

async function executeQuery(query: string, context: QueryContext): Promise<unknown[]> {
  const response = await invoke<{ result: unknown[] }>('execute_sql', {
    input: {
      conn_id: context.connectionId,
      sql: query,
      timeout_ms: null,
    },
  })
  return response.result ?? []
}
