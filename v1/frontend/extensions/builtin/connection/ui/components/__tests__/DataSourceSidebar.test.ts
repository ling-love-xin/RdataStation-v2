/**
 * @vitest-environment jsdom
 */
import { describe, expect, it } from 'vitest'

// ===== Pure logic extracted from DataSourceSidebar =====

const driverColors: Record<string, string> = {
  mysql: '#00758f',
  postgresql: '#336791',
  sqlite: '#003b57',
  duckdb: '#f9a825',
  mariadb: '#c49a6c',
  oracle: '#f80000',
  mssql: '#0089b6',
  clickhouse: '#faff00',
  mongodb: '#47a248',
  redis: '#dc382d',
  cassandra: '#1287b1',
  cockroachdb: '#6933ff',
  snowflake: '#29bfff',
  bigquery: '#4285f4',
  redshift: '#8c4fff',
  elasticsearch: '#00bfb3',
  neo4j: '#018bff',
  couchbase: '#ea2328',
  influxdb: '#22adf6',
  timescaledb: '#fec514',
}

function driverColor(name: string): string {
  return driverColors[name.toLowerCase()] || '#555'
}

function driverInitials(name: string): string {
  return name.slice(0, 2).toUpperCase()
}

type ConnectionStatus = 'connected' | 'disconnected' | 'connecting' | 'error'

interface ProjectConnection {
  id: string
  name: string
  driver: string
  status: ConnectionStatus
  connection_type: 'global' | 'project'
}

function filterConnections(
  connections: ProjectConnection[],
  searchQuery: string
): ProjectConnection[] {
  const q = searchQuery.toLowerCase().trim()
  if (!q) return connections
  return connections.filter(
    c => c.name.toLowerCase().includes(q) || c.driver?.toLowerCase().includes(q)
  )
}

// ===== Tests =====

describe('DataSourceSidebar — driverColor', () => {
  it('returns known color for mysql', () => expect(driverColor('mysql')).toBe('#00758f'))
  it('returns known color for postgresql', () => expect(driverColor('postgresql')).toBe('#336791'))
  it('returns known color for sqlite', () => expect(driverColor('sqlite')).toBe('#003b57'))
  it('returns known color for duckdb', () => expect(driverColor('duckdb')).toBe('#f9a825'))
  it('returns default for unknown', () => expect(driverColor('unknown_db')).toBe('#555'))
  it('case insensitive', () => expect(driverColor('MySQL')).toBe('#00758f'))
  it('returns default for empty', () => expect(driverColor('')).toBe('#555'))
})

describe('DataSourceSidebar — driverInitials', () => {
  it('returns first 2 chars uppercase', () => expect(driverInitials('mysql')).toBe('MY'))
  it('returns uppercase', () => expect(driverInitials('postgresql')).toBe('PO'))
  it('returns short string', () => expect(driverInitials('H2')).toBe('H2'))
})

describe('DataSourceSidebar — filterConnections', () => {
  const conns: ProjectConnection[] = [
    {
      id: '1',
      name: 'Production MySQL',
      driver: 'MySQL',
      status: 'connected',
      connection_type: 'global',
    },
    {
      id: '2',
      name: 'Staging PG',
      driver: 'PostgreSQL',
      status: 'disconnected',
      connection_type: 'project',
    },
    {
      id: '3',
      name: 'Test SQLite',
      driver: 'SQLite',
      status: 'connected',
      connection_type: 'global',
    },
  ]

  it('returns all when no query', () => {
    expect(filterConnections(conns, '').length).toBe(3)
  })

  it('filters by name', () => {
    expect(filterConnections(conns, 'mysql').length).toBe(1)
    expect(filterConnections(conns, 'mysql')[0].id).toBe('1')
  })

  it('filters by driver', () => {
    expect(filterConnections(conns, 'postgresql').length).toBe(1)
    expect(filterConnections(conns, 'postgresql')[0].id).toBe('2')
  })

  it('case insensitive', () => {
    expect(filterConnections(conns, 'MYSQL').length).toBe(1)
  })

  it('returns empty for no match', () => {
    expect(filterConnections(conns, 'mongo').length).toBe(0)
  })

  it('handles empty connections array', () => {
    expect(filterConnections([], 'test').length).toBe(0)
  })
})
