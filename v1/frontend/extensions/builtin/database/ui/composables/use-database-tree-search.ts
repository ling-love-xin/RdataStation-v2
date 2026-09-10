/**
 * 数据库树搜索功能
 *
 * 提供搜索表/视图并定位到树节点的功能
 * 从 DatabaseNavigator.vue 中提取，实现业务逻辑与 UI 分离
 *
 * v2: 预建搜索索引（Map memorization），避免每次搜索 O(N) 遍历全部 catalogs/schemas/tables
 */

import type { ProjectConnection } from '@/extensions/builtin/connection/ui/types/connection'

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

export interface SearchResult {
  nodeKey: string
  tableName: string
  path: string
  connectionId: string
  dbName: string
  schemaName: string
}

interface FilterConfig {
  showTables: boolean
  showViews: boolean
  showSystemSchemas: boolean
  showColumns: boolean
}

/**
 * 构建搜索索引：将所有表/视图展平为 Map<lowerName, SearchResult[]>
 *
 * 索引在每次 catalogs 更新时重建（惰性求值，O(E) 一次），
 * 后续搜索变为 O(1) 查找 + O(M) 过滤（M = 匹配条目数）。
 */
function buildSearchIndex(
  navigatorStore: ReturnType<typeof useDatabaseNavigatorStore>,
  globalConnections: GlobalConnection[],
  projectConnections: ProjectConnection[],
  filterConfig: FilterConfig,
  connectionId?: string
): Map<string, SearchResult[]> {
  const index = new Map<string, SearchResult[]>()
  const allConnections = [...globalConnections, ...projectConnections]

  for (const conn of allConnections) {
    // 限定连接范围
    if (connectionId && conn.id !== connectionId) continue
    const catalogs = navigatorStore.getCatalogs(conn.id)

    for (const cat of catalogs) {
      if (!cat.schemas) continue

      for (const schema of cat.schemas) {
        if (
          !filterConfig.showSystemSchemas &&
          (schema.name === 'information_schema' || schema.name === 'pg_catalog')
        ) {
          continue
        }

        if (filterConfig.showTables && schema.tables) {
          for (const table of schema.tables) {
            const key = table.name.toLowerCase()
            const tableKey = NodeKeyEncoder.encode([
              'table',
              conn.id,
              cat.name,
              schema.name,
              table.name,
            ])
            const entry: SearchResult = {
              nodeKey: tableKey,
              tableName: table.name,
              path: `${conn.name} / ${cat.name} / ${schema.name} / ${table.name}`,
              connectionId: conn.id,
              dbName: cat.name,
              schemaName: schema.name,
            }
            const existing = index.get(key)
            if (existing) {
              existing.push(entry)
            } else {
              index.set(key, [entry])
            }
          }
        }

        if (filterConfig.showViews && schema.views) {
          for (const view of schema.views) {
            const key = view.name.toLowerCase()
            const viewKey = NodeKeyEncoder.encode([
              'view',
              conn.id,
              cat.name,
              schema.name,
              view.name,
            ])
            const entry: SearchResult = {
              nodeKey: viewKey,
              tableName: view.name,
              path: `${conn.name} / ${cat.name} / ${schema.name} / ${view.name} (视图)`,
              connectionId: conn.id,
              dbName: cat.name,
              schemaName: schema.name,
            }
            const existing = index.get(key)
            if (existing) {
              existing.push(entry)
            } else {
              index.set(key, [entry])
            }
          }
        }
      }
    }
  }

  return index
}

export function useDatabaseTreeSearch() {
  const navigatorStore = useDatabaseNavigatorStore()

  /**
   * 搜索表/视图 — 使用预建索引 O(1) 查找
   *
   * @param query 搜索关键词
   * @param filterConfig 过滤配置
   * @param globalConnections 全局连接列表
   * @param projectConnections 项目连接列表
   * @param prebuiltIndex 可选的预建索引（由外部提供，避免重复构建）
   */
  function searchTables(
    query: string,
    filterConfig: FilterConfig,
    globalConnections: GlobalConnection[],
    projectConnections: ProjectConnection[],
    prebuiltIndex?: Map<string, SearchResult[]>,
    connectionId?: string
  ): SearchResult[] {
    if (!query || query.trim().length === 0) return []

    const lowerQuery = query.toLowerCase()

    // 使用预建索引或临时构建（限定连接范围）
    const index =
      prebuiltIndex ??
      buildSearchIndex(
        navigatorStore,
        globalConnections,
        projectConnections,
        filterConfig,
        connectionId
      )

    // O(1) 精确匹配
    const exactMatch = index.get(lowerQuery)
    if (exactMatch && exactMatch.length > 0) {
      return exactMatch.slice(0, 50)
    }

    // 回退：模糊匹配（前缀/包含）
    const results: SearchResult[] = []
    for (const [name, entries] of index) {
      if (name.includes(lowerQuery)) {
        for (const entry of entries) {
          results.push(entry)
          if (results.length >= 50) return results
        }
      }
    }

    return results
  }

  /**
   * 构建搜索索引（供外部缓存并复用）
   */
  function createSearchIndex(
    globalConnections: GlobalConnection[],
    projectConnections: ProjectConnection[],
    filterConfig: FilterConfig,
    connectionId?: string
  ): Map<string, SearchResult[]> {
    return buildSearchIndex(
      navigatorStore,
      globalConnections,
      projectConnections,
      filterConfig,
      connectionId
    )
  }

  /**
   * 查找需要展开的节点路径
   */
  function findNodePath(
    connectionId: string,
    dbName: string,
    schemaName: string,
    nodes: VirtualTreeNode[]
  ): VirtualTreeNode[] {
    const path: VirtualTreeNode[] = []

    const connNode = nodes.find(n => {
      const parts = NodeKeyEncoder.decode(n.key)
      return parts[0] === 'connection' && parts[2] === connectionId
    })
    if (connNode) path.push(connNode)

    const dbNode = nodes.find(n => {
      const parts = NodeKeyEncoder.decode(n.key)
      return parts[0] === 'catalog' && parts[1] === connectionId && parts[2] === dbName
    })
    if (dbNode) path.push(dbNode)

    const schemaNode = nodes.find(n => {
      const parts = NodeKeyEncoder.decode(n.key)
      return (
        parts[0] === 'schema' &&
        parts[1] === connectionId &&
        parts[2] === dbName &&
        parts[3] === schemaName
      )
    })
    if (schemaNode) path.push(schemaNode)

    return path
  }

  return {
    searchTables,
    createSearchIndex,
    findNodePath,
  }
}
