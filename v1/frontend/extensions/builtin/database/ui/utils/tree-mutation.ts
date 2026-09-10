/**
 * 树状态变更工具函数
 *
 * 统一封装 connectionCatalogs → catalog → schema → table 的遍历和修改模式，
 * 消除 Store 中 8 处重复的 `catalogs.find → catalog.schemas.find → mutate` 代码。
 */

import type { CatalogNode, SchemaNode, TableNode } from '../types/nav-types'

// ========== 类型 ==========

export interface TreeMutationPath {
  /** 必填：Catalog 名称 */
  catalogName: string
  /** 可选：Schema 名称（无 Schema 的数据库如 MySQL 不传） */
  schemaName?: string
  /** 可选：Table 名称（操作表级别字段时传入） */
  tableName?: string
}

/** mutateTreeNode 的更新函数接收的类型 */
export type TreeNodeMutable = SchemaNode | TableNode

// ========== 内部查找辅助函数 ==========

function _findCatalogNode(catalogs: CatalogNode[], name: string): CatalogNode | undefined {
  return catalogs.find(c => c.name === name)
}

function _findSchemaNode(catalog: CatalogNode, name: string): SchemaNode | undefined {
  return catalog.schemas.find(s => s.name === name)
}

function _findTableNode(container: SchemaNode | CatalogNode, name: string): TableNode | undefined {
  if ('tables' in container && Array.isArray(container.tables)) {
    return container.tables.find(t => t.name === name)
  }
  return undefined
}

// ========== 公开 API ==========

/**
 * 按路径获取树节点（只读）
 *
 * @example
 * ```ts
 * const schema = getTreeNode(catalogs, connectionId, { catalogName: 'mydb', schemaName: 'public' })
 * ```
 */
export function getTreeNode(
  catalogs: Map<string, CatalogNode[]>,
  connectionId: string,
  path: TreeMutationPath
): CatalogNode | SchemaNode | TableNode | undefined {
  const nodes = catalogs.get(connectionId)
  if (!nodes) return undefined

  const catalog = _findCatalogNode(nodes, path.catalogName)
  if (!catalog) return undefined

  if (path.tableName && path.schemaName) {
    const schema = _findSchemaNode(catalog, path.schemaName)
    if (!schema) return undefined
    return _findTableNode(schema, path.tableName)
  }

  if (path.schemaName) {
    return _findSchemaNode(catalog, path.schemaName)
  }

  if (path.tableName) {
    return _findTableNode(catalog, path.tableName)
  }

  return catalog
}

/**
 * 按路径变更树节点（在节点上直接修改）
 *
 * @returns true 表示成功找到并变更了节点，false 表示路径不存在
 *
 * @example
 * ```ts
 * mutateTreeNode(catalogs, connectionId, { catalogName: 'mydb', schemaName: 'public' }, (schema) => {
 *   schema.procedures = [{ name: 'sp_foo' }]
 * })
 * ```
 */
export function mutateTreeNode(
  catalogs: Map<string, CatalogNode[]>,
  connectionId: string,
  path: TreeMutationPath,
  updater: (node: SchemaNode | TableNode) => void
): boolean {
  const node = getTreeNode(catalogs, connectionId, path)
  if (!node) return false

  // 确保是 SchemaNode 或 TableNode（不是 CatalogNode）
  if (!('tables' in node) && !('columns' in node)) return false

  updater(node as SchemaNode | TableNode)
  return true
}

/**
 * 按路径更新 catalog 节点本身
 *
 * @example
 * ```ts
 * mutateCatalogNode(catalogs, connectionId, 'mydb', (cat) => {
 *   cat.schemas = [...]
 * })
 * ```
 */
export function mutateCatalogNode(
  catalogs: Map<string, CatalogNode[]>,
  connectionId: string,
  catalogName: string,
  updater: (catalog: CatalogNode) => void
): boolean {
  const nodes = catalogs.get(connectionId)
  if (!nodes) return false
  const catalog = _findCatalogNode(nodes, catalogName)
  if (!catalog) return false
  updater(catalog)
  return true
}
