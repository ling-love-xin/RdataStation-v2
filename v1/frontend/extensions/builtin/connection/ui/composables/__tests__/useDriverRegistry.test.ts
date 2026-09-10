/**
 * useDriverRegistry 驱动注册表单元测试
 *
 * 测试 getDriversByType、getTypesByCategory、getGroupedTypes 等纯过滤函数。
 */
import { describe, expect, it } from 'vitest'

import type { DataSourceType, Driver } from '../../../domain/types'

// ==================== 纯函数提取 ====================

function getDriversByType(drivers: Driver[], typeId: string): Driver[] {
  return drivers.filter(d => d.type_id === typeId && d.enabled)
}

function getTypesByCategory(types: DataSourceType[], category: string): DataSourceType[] {
  return types.filter(t => t.category === category && t.enabled)
}

function getGroupedTypes(types: DataSourceType[]): Record<string, DataSourceType[]> {
  const groups: Record<string, DataSourceType[]> = {}
  for (const t of types) {
    if (!t.enabled) continue
    if (!groups[t.category]) {
      groups[t.category] = []
    }
    groups[t.category].push(t)
  }
  return groups
}

// ==================== 测试数据 ====================

const drivers: Driver[] = [
  {
    id: 'mysql',
    type_id: 'relational',
    name: 'MySQL',
    driver_kind: 'native',
    is_file: false,
    default_port: 3306,
    config_schema: '{}',
    enabled: true,
  },
  {
    id: 'postgresql',
    type_id: 'relational',
    name: 'PostgreSQL',
    driver_kind: 'native',
    is_file: false,
    default_port: 5432,
    config_schema: '{}',
    enabled: true,
  },
  {
    id: 'sqlite',
    type_id: 'file',
    name: 'SQLite',
    driver_kind: 'native',
    is_file: true,
    config_schema: '{}',
    enabled: true,
  },
  {
    id: 'duckdb',
    type_id: 'file',
    name: 'DuckDB',
    driver_kind: 'native',
    is_file: true,
    config_schema: '{}',
    enabled: true,
  },
  {
    id: 'oracle-disabled',
    type_id: 'relational',
    name: 'Oracle (disabled)',
    driver_kind: 'native',
    is_file: false,
    config_schema: '{}',
    enabled: false,
  },
  {
    id: 'redis',
    type_id: 'nosql',
    name: 'Redis',
    driver_kind: 'native',
    is_file: false,
    config_schema: '{}',
    enabled: true,
  },
]

const dataSourceTypes: DataSourceType[] = [
  {
    id: 'relational',
    category: 'relational',
    name: '关系型',
    enabled: true,
    icon: 'database',
    created_at: '',
  },
  {
    id: 'file',
    category: 'file',
    name: '文件型',
    enabled: true,
    icon: 'file',
    created_at: '',
  },
  {
    id: 'nosql',
    category: 'nosql',
    name: 'NoSQL',
    enabled: true,
    icon: 'nosql',
    created_at: '',
  },
  {
    id: 'cloud',
    category: 'cloud',
    name: '云数据库',
    enabled: false,
    icon: 'cloud',
    created_at: '',
  },
]

// ==================== getDriversByType ====================

describe('getDriversByType 按类型过滤驱动', () => {
  it('relational → MySQL + PostgreSQL', () => {
    const result = getDriversByType(drivers, 'relational')
    expect(result).toHaveLength(2)
    expect(result.map(d => d.id)).toEqual(['mysql', 'postgresql'])
  })

  it('file → SQLite + DuckDB', () => {
    const result = getDriversByType(drivers, 'file')
    expect(result).toHaveLength(2)
    expect(result.map(d => d.id)).toEqual(['sqlite', 'duckdb'])
  })

  it('nosql → Redis', () => {
    const result = getDriversByType(drivers, 'nosql')
    expect(result).toHaveLength(1)
    expect(result[0].id).toBe('redis')
  })

  it('disabled 驱动 → 不包含', () => {
    const result = getDriversByType(drivers, 'relational')
    const oracle = result.find(d => d.id === 'oracle-disabled')
    expect(oracle).toBeUndefined()
  })

  it('不存在的 type_id → 空数组', () => {
    const result = getDriversByType(drivers, 'nonexistent')
    expect(result).toHaveLength(0)
  })

  it('空 drivers 列表 → 空数组', () => {
    const result = getDriversByType([], 'relational')
    expect(result).toHaveLength(0)
  })
})

// ==================== getTypesByCategory ====================

describe('getTypesByCategory 按分类过滤数据源类型', () => {
  it('relational → 关系型', () => {
    const result = getTypesByCategory(dataSourceTypes, 'relational')
    expect(result).toHaveLength(1)
    expect(result[0].id).toBe('relational')
  })

  it('file → 文件型', () => {
    const result = getTypesByCategory(dataSourceTypes, 'file')
    expect(result).toHaveLength(1)
    expect(result[0].id).toBe('file')
  })

  it('cloud (disabled) → 不包含', () => {
    const result = getTypesByCategory(dataSourceTypes, 'cloud')
    expect(result).toHaveLength(0)
  })

  it('不存在的 category → 空数组', () => {
    const result = getTypesByCategory(dataSourceTypes, 'bigdata')
    expect(result).toHaveLength(0)
  })
})

// ==================== getGroupedTypes ====================

describe('getGroupedTypes 按 category 分组', () => {
  it('三个分组：relational/file/nosql', () => {
    const result = getGroupedTypes(dataSourceTypes)
    expect(Object.keys(result)).toEqual(['relational', 'file', 'nosql'])
  })

  it('relational 组 → 1 个类型', () => {
    const result = getGroupedTypes(dataSourceTypes)
    expect(result.relational).toHaveLength(1)
  })

  it('cloud (disabled) → 不在分组中', () => {
    const result = getGroupedTypes(dataSourceTypes)
    expect(result.cloud).toBeUndefined()
  })

  it('空列表 → 空对象', () => {
    const result = getGroupedTypes([])
    expect(Object.keys(result)).toHaveLength(0)
  })

  it('同一 category 多个类型 → 正确分组', () => {
    const types: DataSourceType[] = [
      {
        id: 'relational',
        category: 'relational',
        name: '关系型',
        enabled: true,
        icon: 'db',
        created_at: '',
      },
      {
        id: 'timeseries',
        category: 'relational',
        name: '时序',
        enabled: true,
        icon: 'clock',
        created_at: '',
      },
    ]
    const result = getGroupedTypes(types)
    expect(result.relational).toHaveLength(2)
  })
})
