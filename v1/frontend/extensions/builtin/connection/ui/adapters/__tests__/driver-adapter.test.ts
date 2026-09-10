/**
 * driver-adapter 解析器单元测试
 *
 * 测试 parseConfigSchema、normalize* 函数对各种 config_schema 格式的处理。
 */

import { describe, expect, it } from 'vitest'

import { parseConfigSchema, isFileDatabase } from '../driver-adapter'

interface TestDriver {
  id: string
  name: string
  driver: string
  driver_kind: string
  is_file: boolean
  auto_create: boolean
  default_port: number | null
  version: string
  url_template: string
  config_schema: string
  description: string
  icon: string
  category: string
  created_at: string
  updated_at: string
}

// ==================== parseConfigSchema ====================

describe('parseConfigSchema', () => {
  it('空字符串返回空结果', () => {
    const result = parseConfigSchema('')
    expect(result.fields).toHaveLength(0)
    expect(result.options).toHaveLength(0)
  })

  it('无效 JSON 返回空结果', () => {
    const result = parseConfigSchema('not-json')
    expect(result.fields).toHaveLength(0)
    expect(result.options).toHaveLength(0)
  })

  it('JSON Schema 格式: host + port + database', () => {
    const schema = JSON.stringify({
      type: 'object',
      properties: {
        host: { type: 'string', title: 'Host', default: 'localhost' },
        port: { type: 'integer', title: 'Port', default: 3306 },
        database: { type: 'string', title: 'Database' },
      },
      required: ['host', 'port'],
    })

    const result = parseConfigSchema(schema)

    expect(result.fields).toHaveLength(3)
    expect(result.options).toHaveLength(0)

    const hostField = result.fields.find(f => f.key === 'host')
    expect(hostField).toBeDefined()
    expect(hostField!.required).toBe(true)
    expect(hostField!.default).toBe('localhost')
  })

  it('JSON Schema 格式: enum 字段归入 options', () => {
    const schema = JSON.stringify({
      type: 'object',
      properties: {
        host: { type: 'string', title: 'Host' },
        ssl_mode: {
          type: 'string',
          title: 'SSL Mode',
          enum: ['disable', 'require', 'verify-ca'],
        },
      },
    })

    const result = parseConfigSchema(schema)

    expect(result.fields).toHaveLength(1) // host
    expect(result.options).toHaveLength(1) // ssl_mode (因为 enum)
    expect(result.options[0].key).toBe('ssl_mode')
    expect(result.options[0].values).toEqual(['disable', 'require', 'verify-ca'])
  })

  it('JSON Schema 格式: boolean 字段归入 options', () => {
    const schema = JSON.stringify({
      type: 'object',
      properties: {
        host: { type: 'string', title: 'Host' },
        use_ssl: { type: 'boolean', title: 'Use SSL', default: false },
      },
    })

    const result = parseConfigSchema(schema)

    expect(result.fields).toHaveLength(1) // host
    expect(result.options).toHaveLength(1) // use_ssl (boolean → option)
    expect(result.options[0].type).toBe('checkbox')
  })

  it('自定义格式: { fields, options }', () => {
    const schema = JSON.stringify({
      fields: [
        { key: 'host', label: 'Host', type: 'string', required: true },
        { key: 'port', label: 'Port', type: 'integer' },
      ],
      options: [
        {
          key: 'charset',
          label: 'Charset',
          type: 'select',
          values: ['utf8', 'latin1'],
        },
      ],
    })

    const result = parseConfigSchema(schema)

    expect(result.fields).toHaveLength(2)
    expect(result.options).toHaveLength(1)
    expect(result.fields[0].key).toBe('host')
    expect(result.fields[0].required).toBe(true)
    expect(result.options[0].key).toBe('charset')
  })

  it('format: password 字段仍在 fields 中（非 enum/boolean 即 field）', () => {
    const schema = JSON.stringify({
      type: 'object',
      properties: {
        username: { type: 'string', title: 'Username' },
        password: { type: 'string', title: 'Password', format: 'password' },
      },
    })

    const result = parseConfigSchema(schema)

    expect(result.fields).toHaveLength(2)
    // password 字段归入 fields（非 enum 且非 boolean）
    const pwField = result.fields.find(f => f.key === 'password')
    expect(pwField).toBeDefined()
  })

  it('空对象 → 空结果', () => {
    const result = parseConfigSchema('{}')
    expect(result.fields).toHaveLength(0)
    expect(result.options).toHaveLength(0)
  })

  it('缺少 properties 的 JSON Schema → 空结果', () => {
    const schema = JSON.stringify({ type: 'object' })
    const result = parseConfigSchema(schema)
    expect(result.fields).toHaveLength(0)
    expect(result.options).toHaveLength(0)
  })
})

// ==================== isFileDatabase ====================

describe('isFileDatabase', () => {
  it('is_file=true 返回 true', () => {
    const driver: TestDriver = {
      id: 'sqlite',
      name: 'SQLite',
      driver: 'sqlite',
      driver_kind: 'native',
      is_file: true,
      auto_create: false,
      default_port: null,
      version: '3.0.0',
      url_template: 'sqlite://{file_path}',
      config_schema: '',
      description: 'SQLite database',
      icon: '',
      category: 'relational',
      created_at: '',
      updated_at: '',
    }
    expect(isFileDatabase(driver as any)).toBe(true)
  })

  it('is_file=false 返回 false', () => {
    const driver: TestDriver = {
      id: 'mysql',
      name: 'MySQL',
      driver: 'mysql',
      driver_kind: 'native',
      is_file: false,
      auto_create: false,
      default_port: 3306,
      version: '8.0.0',
      url_template: 'mysql://{host}:{port}/{database}',
      config_schema: '',
      description: 'MySQL database',
      icon: '',
      category: 'relational',
      created_at: '',
      updated_at: '',
    }
    expect(isFileDatabase(driver as any)).toBe(false)
  })
})
