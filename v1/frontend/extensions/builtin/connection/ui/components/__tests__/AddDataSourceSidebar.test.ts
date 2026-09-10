/**
 * @vitest-environment jsdom
 */
import { describe, expect, it } from 'vitest'

// ===== Pure logic extracted from AddDataSourceSidebar =====

const typeColors: Record<string, string> = {
  mysql: '#00758f',
  postgresql: '#336791',
  sqlite: '#003b57',
  duckdb: '#f9a825',
  mariadb: '#c49a6c',
  mongodb: '#47a248',
  redis: '#dc382d',
  clickhouse: '#faff00',
  sqlserver: '#cc2927',
  oracle: '#f80000',
  couchbase: '#ea2328',
  cockroachdb: '#6933ff',
  snowflake: '#29b5e8',
  bigquery: '#4285f4',
  redshift: '#205b97',
  h2: '#fc7303',
  trino: '#dd00a1',
  presto: '#5890ff',
  elasticsearch: '#fed10a',
}

function typeColor(n?: string): string {
  return n ? (typeColors[n.toLowerCase()] ?? '#555') : '#555'
}

const catLabels: Record<string, string> = {
  relational: 'relational',
  'file-based': 'file-based',
  nosql: 'nosql',
  analytics: 'analytics',
  cloud: 'Cloud',
  mq: 'MQ',
  http: 'HTTP API',
}

function catLabel(c: string) {
  return catLabels[c] ?? c
}

function catIcon(c: string): string {
  return (
    {
      relational: '\u{1F5C4}',
      'file-based': '\u{1F4C1}',
      nosql: '\u{1F4E1}',
      analytics: '\u{1F4CA}',
      cloud: '\u2601',
      mq: '\u{1F4E8}',
      http: '\u{1F310}',
    }[c] ?? '\u{1F4E6}'
  )
}

interface DataSourceType {
  id: string
  name: string
  enabled: boolean
}
interface Driver {
  id: string
  type_id: string
  enabled: boolean
}

interface GroupedType {
  category: string
  types: (DataSourceType & { driverCount: number })[]
  totalDrivers: number
}

function getGroupedTypes(types: DataSourceType[], drivers: Driver[]): GroupedType[] {
  const groups: Record<string, DataSourceType[]> = {}
  for (const t of types) {
    if (!groups[t.id]) groups[t.id] = []
    groups[t.id].push(t)
  }
  return Object.entries(groups).map(([category, types]) => {
    const enriched = types.map(t => ({
      ...t,
      driverCount: drivers.filter(d => d.type_id === t.id && d.enabled).length,
    }))
    return {
      category,
      types: enriched,
      totalDrivers: enriched.reduce((s, t) => s + t.driverCount, 0),
    }
  })
}

function toggleCat(expanded: Set<string>, cat: string): Set<string> {
  const s = new Set(expanded)
  if (s.has(cat)) s.delete(cat)
  else s.add(cat)
  return s
}

// ===== Tests =====

describe('AddDataSourceSidebar — typeColor', () => {
  it('returns known color for mysql', () => expect(typeColor('mysql')).toBe('#00758f'))
  it('returns default for undefined', () => expect(typeColor()).toBe('#555'))
  it('returns default for unknown', () => expect(typeColor('unknown')).toBe('#555'))
  it('case insensitive', () => expect(typeColor('MySQL')).toBe('#00758f'))
})

describe('AddDataSourceSidebar — catLabel', () => {
  it('returns label for relational', () => expect(catLabel('relational')).toBe('relational'))
  it('returns raw for unknown', () => expect(catLabel('unknown-cat')).toBe('unknown-cat'))
})

describe('AddDataSourceSidebar — catIcon', () => {
  it('returns icon for relational', () => expect(catIcon('relational')).toBe('\u{1F5C4}'))
  it('returns icon for file-based', () => expect(catIcon('file-based')).toBe('\u{1F4C1}'))
  it('returns default for unknown', () => expect(catIcon('unknown')).toBe('\u{1F4E6}'))
})

describe('AddDataSourceSidebar — toggleCat', () => {
  it('adds category to set', () => {
    const result = toggleCat(new Set(), 'relational')
    expect(result.has('relational')).toBe(true)
  })

  it('removes category from set', () => {
    const result = toggleCat(new Set(['relational']), 'relational')
    expect(result.has('relational')).toBe(false)
  })

  it('does not mutate original', () => {
    const original = new Set(['relational'])
    const result = toggleCat(original, 'nosql')
    expect(original.has('nosql')).toBe(false)
    expect(result.has('nosql')).toBe(true)
  })
})
