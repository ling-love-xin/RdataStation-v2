import { describe, it, expect } from 'vitest'

import { mutateTreeNode, getTreeNode, mutateCatalogNode } from '../tree-mutation'

import type { CatalogNode, SchemaNode, TableNode } from '../../types/nav-types'

function makeCatalog(name: string): CatalogNode {
  return { name, schemas: [] }
}

function makeSchema(name: string): SchemaNode {
  return { name, tables: [], views: [] }
}

function makeTable(name: string): TableNode {
  return { name, type: 'TABLE', columns: [] }
}

describe('tree-mutation', () => {
  describe('mutateTreeNode', () => {
    it('should mutate a schema node at valid path', () => {
      const schema = makeSchema('public')
      const catalog = makeCatalog('mydb')
      catalog.schemas.push(schema)
      const catalogs = new Map<string, CatalogNode[]>([['conn1', [catalog]]])

      const result = mutateTreeNode(
        catalogs,
        'conn1',
        { catalogName: 'mydb', schemaName: 'public' },
        node => {
          ;(node as SchemaNode).totalTables = 42
        }
      )

      expect(result).toBe(true)
      expect(schema.totalTables).toBe(42)
    })

    it('should return false for non-existent catalog', () => {
      const catalog = makeCatalog('mydb')
      const catalogs = new Map<string, CatalogNode[]>([['conn1', [catalog]]])

      const result = mutateTreeNode(
        catalogs,
        'conn1',
        { catalogName: 'nonexistent', schemaName: 'public' },
        () => {}
      )

      expect(result).toBe(false)
    })

    it('should return false for non-existent schema', () => {
      const catalog = makeCatalog('mydb')
      catalog.schemas.push(makeSchema('public'))
      const catalogs = new Map<string, CatalogNode[]>([['conn1', [catalog]]])

      const result = mutateTreeNode(
        catalogs,
        'conn1',
        { catalogName: 'mydb', schemaName: 'nonexistent' },
        () => {}
      )

      expect(result).toBe(false)
    })

    it('should mutate a table node at valid path', () => {
      const table = makeTable('users')
      const schema = makeSchema('public')
      schema.tables.push(table)
      const catalog = makeCatalog('mydb')
      catalog.schemas.push(schema)
      const catalogs = new Map<string, CatalogNode[]>([['conn1', [catalog]]])

      const result = mutateTreeNode(
        catalogs,
        'conn1',
        { catalogName: 'mydb', schemaName: 'public', tableName: 'users' },
        node => {
          ;(node as TableNode).rowCount = 100
        }
      )

      expect(result).toBe(true)
      expect(table.rowCount).toBe(100)
    })

    it('should not mutate catalog node (only schemas/tables)', () => {
      const catalog = makeCatalog('mydb')
      const catalogs = new Map<string, CatalogNode[]>([['conn1', [catalog]]])

      const result = mutateTreeNode(catalogs, 'conn1', { catalogName: 'mydb' }, () => {})

      expect(result).toBe(false)
    })
  })

  describe('getTreeNode', () => {
    it('should return schema node at valid path', () => {
      const schema = makeSchema('public')
      const catalog = makeCatalog('mydb')
      catalog.schemas.push(schema)
      const catalogs = new Map<string, CatalogNode[]>([['conn1', [catalog]]])

      const result = getTreeNode(catalogs, 'conn1', { catalogName: 'mydb', schemaName: 'public' })

      expect(result).toBe(schema)
    })

    it('should return table node at valid path', () => {
      const table = makeTable('users')
      const schema = makeSchema('public')
      schema.tables.push(table)
      const catalog = makeCatalog('mydb')
      catalog.schemas.push(schema)
      const catalogs = new Map<string, CatalogNode[]>([['conn1', [catalog]]])

      const result = getTreeNode(catalogs, 'conn1', {
        catalogName: 'mydb',
        schemaName: 'public',
        tableName: 'users',
      })

      expect(result).toBe(table)
    })

    it('should return undefined for invalid path', () => {
      const catalog = makeCatalog('mydb')
      const catalogs = new Map<string, CatalogNode[]>([['conn1', [catalog]]])

      const result = getTreeNode(catalogs, 'conn1', {
        catalogName: 'nonexistent',
        schemaName: 'public',
      })

      expect(result).toBeUndefined()
    })
  })

  describe('mutateCatalogNode', () => {
    it('should mutate a catalog node', () => {
      const catalog = makeCatalog('mydb')
      const catalogs = new Map<string, CatalogNode[]>([['conn1', [catalog]]])

      const result = mutateCatalogNode(catalogs, 'conn1', 'mydb', cat => {
        cat.schemas.push(makeSchema('new_schema'))
      })

      expect(result).toBe(true)
      expect(catalog.schemas).toHaveLength(1)
      expect(catalog.schemas[0].name).toBe('new_schema')
    })

    it('should return false for non-existent catalog', () => {
      const catalogs = new Map<string, CatalogNode[]>([['conn1', []]])

      const result = mutateCatalogNode(catalogs, 'conn1', 'nonexistent', () => {})

      expect(result).toBe(false)
    })
  })
})
