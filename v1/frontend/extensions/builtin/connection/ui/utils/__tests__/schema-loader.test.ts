/**
 * schema-loader 单元测试
 *
 * 测试 getDefaultNavigationConfig、clearSchemaCache 等纯函数。
 */
import { describe, expect, it } from 'vitest'

import { clearSchemaCache, getDefaultNavigationConfig } from '../schema-loader'

// ==================== getDefaultNavigationConfig ====================

describe('getDefaultNavigationConfig 默认导航配置', () => {
  it('返回完整结构', () => {
    const config = getDefaultNavigationConfig()

    expect(config).toBeDefined()
    expect(config.hasCatalogs).toBe(true)
    expect(config.hasSchemas).toBe(false)
    expect(config.systemSchemas).toEqual([])
  })

  it('hasCatalogs 与 hasSchemas 区分', () => {
    const config = getDefaultNavigationConfig()
    // 默认配置：有 catalogs、无 schemas
    expect(config.hasCatalogs).toBe(true)
    expect(config.hasSchemas).toBe(false)
  })

  it('folders 包含 6 个文件夹', () => {
    const config = getDefaultNavigationConfig()
    const keys = Object.keys(config.folders)
    expect(keys).toEqual(['tables', 'views', 'functions', 'procedures', 'sequences', 'triggers'])
  })

  it('tables 文件夹配置', () => {
    const config = getDefaultNavigationConfig()
    const tables = config.folders.tables
    expect(tables.enabled).toBe(true)
    expect(tables.label).toBe('Tables')
    expect(tables.icon).toBe('table')
    expect(tables.childTypes).toEqual(['table'])
  })

  it('views 文件夹配置', () => {
    const config = getDefaultNavigationConfig()
    const views = config.folders.views
    expect(views.enabled).toBe(true)
    expect(views.label).toBe('Views')
    expect(views.icon).toBe('eye')
    expect(views.childTypes).toEqual(['view'])
  })

  it('functions 文件夹配置', () => {
    const config = getDefaultNavigationConfig()
    const funcs = config.folders.functions
    expect(funcs.enabled).toBe(true)
    expect(funcs.label).toBe('Functions')
    expect(funcs.icon).toBe('function')
    expect(funcs.childTypes).toEqual([])
  })

  it('procedures 文件夹配置', () => {
    const config = getDefaultNavigationConfig()
    const procs = config.folders.procedures
    expect(procs.enabled).toBe(true)
    expect(procs.label).toBe('Procedures')
    expect(procs.icon).toBe('terminal')
    expect(procs.childTypes).toEqual([])
  })

  it('sequences 文件夹配置', () => {
    const config = getDefaultNavigationConfig()
    const seqs = config.folders.sequences
    expect(seqs.enabled).toBe(true)
    expect(seqs.label).toBe('Sequences')
    expect(seqs.icon).toBe('hash')
    expect(seqs.childTypes).toEqual([])
  })

  it('triggers 文件夹配置', () => {
    const config = getDefaultNavigationConfig()
    const trigs = config.folders.triggers
    expect(trigs.enabled).toBe(true)
    expect(trigs.label).toBe('Triggers')
    expect(trigs.icon).toBe('zap')
    expect(trigs.childTypes).toEqual([])
  })

  it('tableChildren 配置', () => {
    const config = getDefaultNavigationConfig()
    const tc = config.tableChildren
    expect(tc.columns).toBe(true)
    expect(tc.indexes).toBe(true)
    expect(tc.constraints).toBe(true)
    expect(tc.triggers).toBe(false)
    expect(tc.foreignKeys).toBe(false)
    expect(tc.references).toBe(false)
  })

  it('每次调用返回新对象（不可变）', () => {
    const a = getDefaultNavigationConfig()
    const b = getDefaultNavigationConfig()
    expect(a).not.toBe(b)
    expect(a).toEqual(b)
  })
})

// ==================== clearSchemaCache ====================

describe('clearSchemaCache 缓存清理', () => {
  it('调用不抛出异常', () => {
    expect(() => clearSchemaCache()).not.toThrow()
  })

  it('多次调用仍安全', () => {
    clearSchemaCache()
    clearSchemaCache()
    clearSchemaCache()
    expect(() => clearSchemaCache()).not.toThrow()
  })
})
