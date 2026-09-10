import type { SearchObjectResult, DatabaseInfo } from '../types/navigator'

export function searchDatabaseObjects(
  connections: Array<{ id: string; databases: DatabaseInfo[] }>,
  query: string
): SearchObjectResult[] {
  if (!query || query.trim().length === 0) {
    return []
  }

  const normalizedQuery = query.toLowerCase().trim()
  const results: SearchObjectResult[] = []

  for (const conn of connections) {
    for (const db of conn.databases) {
      for (const schema of db.schemas) {
        if (schema.name.toLowerCase().includes(normalizedQuery)) {
          results.push({
            connectionId: conn.id,
            databaseName: db.name,
            schemaName: schema.name,
            objectName: schema.name,
            objectType: 'table',
          })
        }

        for (const table of schema.tables) {
          if (table.name.toLowerCase().includes(normalizedQuery)) {
            results.push({
              connectionId: conn.id,
              databaseName: db.name,
              schemaName: schema.name,
              objectName: table.name,
              objectType: 'table',
            })
          }

          for (const col of table.columns) {
            if (col.name.toLowerCase().includes(normalizedQuery)) {
              results.push({
                connectionId: conn.id,
                databaseName: db.name,
                schemaName: schema.name,
                objectName: `${table.name}.${col.name}`,
                objectType: 'column',
              })
            }
          }
        }

        for (const view of schema.views) {
          if (view.name.toLowerCase().includes(normalizedQuery)) {
            results.push({
              connectionId: conn.id,
              databaseName: db.name,
              schemaName: schema.name,
              objectName: view.name,
              objectType: 'view',
            })
          }

          for (const col of view.columns) {
            if (col.name.toLowerCase().includes(normalizedQuery)) {
              results.push({
                connectionId: conn.id,
                databaseName: db.name,
                schemaName: schema.name,
                objectName: `${view.name}.${col.name}`,
                objectType: 'column',
              })
            }
          }
        }
      }
    }
  }

  return results.slice(0, 100)
}

export function highlightText(text: string, query: string): string {
  if (!query || query.trim().length === 0) {
    return text
  }

  const normalizedQuery = query.toLowerCase()
  const normalizedText = text.toLowerCase()
  const index = normalizedText.indexOf(normalizedQuery)

  if (index === -1) {
    return text
  }

  const before = text.slice(0, index)
  const match = text.slice(index, index + query.length)
  const after = text.slice(index + query.length)

  return `${before}<mark class="search-highlight">${match}</mark>${after}`
}
