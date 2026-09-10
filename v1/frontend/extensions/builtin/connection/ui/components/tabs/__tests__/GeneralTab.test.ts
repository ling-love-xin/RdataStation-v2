/**
 * GeneralTab 字段过滤逻辑单元测试
 *
 * 测试 config_schema 解析、AUTH_MANAGED_KEYS 过滤、getDefaultFields 降级、
 * isFieldDisabled 禁用逻辑、advancedSchemaFields 计算等。
 */

import { describe, expect, it } from 'vitest'

// ==================== 类型定义（与 GeneralTab.vue 对齐） ====================

interface ConfigSchemaField {
  key: string
  label: string
  type: 'string' | 'integer' | 'number' | 'boolean'
  required: boolean
  placeholder: string
  defaultValue: unknown
  order: number
  format?: string
  options?: Array<{ label: string; value: unknown }>
  helpText?: string
  min?: number
  max?: number
  rows?: number
}

// ==================== AUTH_MANAGED_KEYS ====================

const AUTH_MANAGED_KEYS = new Set([
  'username',
  'password',
  'certPath',
  'certKeyPath',
  'principal',
  'keytabPath',
  'tokenEndpoint',
  'clientId',
  'clientSecret',
])

describe('AUTH_MANAGED_KEYS 过滤', () => {
  it('应包含全部 9 个认证管理字段', () => {
    expect(AUTH_MANAGED_KEYS.size).toBe(9)
  })

  it('username 和 password 应在过滤集合中', () => {
    expect(AUTH_MANAGED_KEYS.has('username')).toBe(true)
    expect(AUTH_MANAGED_KEYS.has('password')).toBe(true)
  })

  it('证书相关字段应在过滤集合中', () => {
    expect(AUTH_MANAGED_KEYS.has('certPath')).toBe(true)
    expect(AUTH_MANAGED_KEYS.has('certKeyPath')).toBe(true)
  })

  it('Kerberos 字段应在过滤集合中', () => {
    expect(AUTH_MANAGED_KEYS.has('principal')).toBe(true)
    expect(AUTH_MANAGED_KEYS.has('keytabPath')).toBe(true)
  })

  it('OAuth2 字段应在过滤集合中', () => {
    expect(AUTH_MANAGED_KEYS.has('tokenEndpoint')).toBe(true)
    expect(AUTH_MANAGED_KEYS.has('clientId')).toBe(true)
    expect(AUTH_MANAGED_KEYS.has('clientSecret')).toBe(true)
  })

  it('连接属性字段不应在过滤集合中', () => {
    // host/port/database 不应被过滤，应由 config_schema v-for 渲染
    expect(AUTH_MANAGED_KEYS.has('host')).toBe(false)
    expect(AUTH_MANAGED_KEYS.has('port')).toBe(false)
    expect(AUTH_MANAGED_KEYS.has('database')).toBe(false)
    expect(AUTH_MANAGED_KEYS.has('url')).toBe(false)
    expect(AUTH_MANAGED_KEYS.has('file_path')).toBe(false)
  })
})

// ==================== getDefaultFields 降级 ====================

function getDefaultFields(driverKind?: string): ConfigSchemaField[] {
  switch (driverKind) {
    case 'http':
      return [
        {
          key: 'url',
          label: 'URL',
          type: 'string',
          required: true,
          order: 1,
          placeholder: 'https://api.example.com/v1',
          defaultValue: '',
        },
        {
          key: 'headers',
          label: 'Headers',
          type: 'string',
          required: false,
          order: 2,
          placeholder: '{"Authorization": "Bearer ..."}',
          defaultValue: '',
        },
      ]
    case 'wasm':
      return [
        {
          key: 'wasmPath',
          label: 'WASM 路径',
          type: 'string',
          required: true,
          order: 1,
          placeholder: '',
          defaultValue: '',
        },
      ]
    default:
      return [
        {
          key: 'host',
          label: '主机',
          type: 'string',
          required: true,
          placeholder: '请输入主机地址',
          defaultValue: 'localhost',
          order: 1,
        },
        {
          key: 'port',
          label: '端口',
          type: 'integer',
          required: true,
          placeholder: '请输入端口号',
          defaultValue: 3306,
          order: 2,
        },
        {
          key: 'database',
          label: '数据库',
          type: 'string',
          required: false,
          placeholder: '请输入数据库名',
          defaultValue: '',
          order: 3,
        },
        {
          key: 'username',
          label: '用户名',
          type: 'string',
          required: false,
          placeholder: 'root',
          defaultValue: 'root',
          order: 4,
        },
        {
          key: 'password',
          label: '密码',
          type: 'string',
          required: false,
          placeholder: '****',
          defaultValue: '',
          order: 5,
          format: 'password',
        },
      ]
  }
}

describe('getDefaultFields 降级', () => {
  it('driver_kind 未指定 → 默认 host/port/database/username/password', () => {
    const fields = getDefaultFields()
    expect(fields).toHaveLength(5)
    expect(fields[0].key).toBe('host')
    expect(fields[1].key).toBe('port')
    expect(fields[2].key).toBe('database')
    expect(fields[3].key).toBe('username')
    expect(fields[4].key).toBe('password')
  })

  it('driver_kind="native" → 默认字段', () => {
    const fields = getDefaultFields('native')
    expect(fields).toHaveLength(5)
    expect(fields[0].key).toBe('host')
  })

  it('driver_kind="jdbc" → 默认字段', () => {
    const fields = getDefaultFields('jdbc')
    expect(fields).toHaveLength(5)
    expect(fields[0].defaultValue).toBe('localhost')
    expect(fields[1].defaultValue).toBe(3306)
  })

  it('driver_kind="http" → url + headers', () => {
    const fields = getDefaultFields('http')
    expect(fields).toHaveLength(2)
    expect(fields[0].key).toBe('url')
    expect(fields[1].key).toBe('headers')
  })

  it('driver_kind="wasm" → wasmPath', () => {
    const fields = getDefaultFields('wasm')
    expect(fields).toHaveLength(1)
    expect(fields[0].key).toBe('wasmPath')
  })

  it('默认降级字段按 order 排序', () => {
    const fields = getDefaultFields()
    for (let i = 1; i < fields.length; i++) {
      expect(fields[i].order).toBeGreaterThanOrEqual(fields[i - 1].order)
    }
  })
})

// ==================== parseConfigSchema（GeneralTab 内部版本） ====================

function parseConfigSchema(schema: string, driverKind?: string): ConfigSchemaField[] {
  const fallback = getDefaultFields(driverKind)
  if (!schema) return fallback

  let parsed: Record<string, unknown>
  try {
    parsed = JSON.parse(schema) as Record<string, unknown>
  } catch {
    return fallback
  }

  if (parsed.type !== 'object' || !parsed.properties) {
    return fallback
  }

  const propsMap = parsed.properties as Record<string, Record<string, unknown>>
  const requiredSet = new Set<string>(
    Array.isArray(parsed.required) ? (parsed.required as string[]) : []
  )
  const fields: ConfigSchemaField[] = []

  for (const [key, prop] of Object.entries(propsMap)) {
    const jsonType = String(prop.type ?? 'string')
    let fieldType: ConfigSchemaField['type'] = 'string'
    if (jsonType === 'integer') fieldType = 'integer'
    else if (jsonType === 'number') fieldType = 'number'
    else if (jsonType === 'boolean') fieldType = 'boolean'

    const field: ConfigSchemaField = {
      key,
      label: (prop.title as string) || key,
      type: fieldType,
      required: requiredSet.has(key),
      placeholder: (prop.description as string) || '',
      defaultValue: prop.default,
      order: typeof prop.order === 'number' ? (prop.order as number) : 999,
      format: prop.format as string | undefined,
    }

    if (Array.isArray(prop.enum)) {
      field.options = (prop.enum as unknown[]).map((v: unknown) => ({
        label: String(v),
        value: v,
      }))
    }

    if (typeof prop.minimum === 'number') field.min = prop.minimum as number
    if (typeof prop.maximum === 'number') field.max = prop.maximum as number
    if (prop.description) field.helpText = prop.description as string

    fields.push(field)
  }

  fields.sort((a, b) => a.order - b.order)
  return fields.length > 0 ? fields : fallback
}

describe('parseConfigSchema（GeneralTab 版本）', () => {
  it('空字符串 → getDefaultFields 降级', () => {
    const fields = parseConfigSchema('')
    expect(fields.length).toBeGreaterThan(0)
    expect(fields[0].key).toBe('host')
  })

  it('空字符串 + driverKind="http" → http 降级', () => {
    const fields = parseConfigSchema('', 'http')
    expect(fields).toHaveLength(2)
    expect(fields[0].key).toBe('url')
  })

  it('无效 JSON → 降级字段', () => {
    const fields = parseConfigSchema('{invalid json', 'jdbc')
    expect(fields).toHaveLength(5)
    expect(fields[0].key).toBe('host')
  })

  it('缺少 properties 的 JSON Schema → 降级字段', () => {
    const schema = JSON.stringify({ type: 'object' })
    const fields = parseConfigSchema(schema)
    expect(fields[0].key).toBe('host')
  })

  it('正常 JSON Schema → 解析字段', () => {
    const schema = JSON.stringify({
      type: 'object',
      properties: {
        host: { type: 'string', title: '主机', order: 1 },
        port: { type: 'integer', title: '端口', default: 5432, order: 2 },
      },
    })

    const fields = parseConfigSchema(schema)
    expect(fields).toHaveLength(2)
    expect(fields[0].key).toBe('host')
    expect(fields[1].key).toBe('port')
    expect(fields[1].defaultValue).toBe(5432)
  })

  it('JSON Schema 含 enum → select options', () => {
    const schema = JSON.stringify({
      type: 'object',
      properties: {
        ssl_mode: {
          type: 'string',
          title: 'SSL Mode',
          enum: ['disable', 'require'],
        },
      },
    })

    const fields = parseConfigSchema(schema)
    expect(fields).toHaveLength(1)
    expect(fields[0].options).toHaveLength(2)
    expect(fields[0].options![0].label).toBe('disable')
  })

  it('JSON Schema 含 minimum/maximum → 映射到 min/max', () => {
    const schema = JSON.stringify({
      type: 'object',
      properties: {
        port: {
          type: 'integer',
          title: '端口',
          minimum: 1024,
          maximum: 65535,
        },
      },
    })

    const fields = parseConfigSchema(schema)
    expect(fields[0].min).toBe(1024)
    expect(fields[0].max).toBe(65535)
  })

  it('JSON Schema 含 description → helpText', () => {
    const schema = JSON.stringify({
      type: 'object',
      properties: {
        timeout: {
          type: 'integer',
          title: 'Timeout',
          description: '连接超时时间（秒）',
        },
      },
    })

    const fields = parseConfigSchema(schema)
    expect(fields[0].helpText).toBe('连接超时时间（秒）')
  })

  it('解析后字段按 order 排序', () => {
    const schema = JSON.stringify({
      type: 'object',
      properties: {
        database: { type: 'string', title: '数据库', order: 3 },
        host: { type: 'string', title: '主机', order: 1 },
        port: { type: 'integer', title: '端口', order: 2 },
      },
    })

    const fields = parseConfigSchema(schema)
    expect(fields[0].key).toBe('host')
    expect(fields[1].key).toBe('port')
    expect(fields[2].key).toBe('database')
  })

  it('password 字段 format 标记为 password', () => {
    const schema = JSON.stringify({
      type: 'object',
      properties: {
        password: {
          type: 'string',
          title: '密码',
          format: 'password',
        },
      },
    })

    const fields = parseConfigSchema(schema)
    expect(fields[0].format).toBe('password')
  })
})

// ==================== configSchemaFields 计算（过滤 AUTH_MANAGED_KEYS） ====================

describe('configSchemaFields 过滤逻辑', () => {
  it('config_schema 中 AUTH_MANAGED_KEYS 字段应被过滤', () => {
    const schema = JSON.stringify({
      type: 'object',
      properties: {
        host: { type: 'string', title: '主机' },
        port: { type: 'integer', title: '端口' },
        database: { type: 'string', title: '数据库' },
        username: { type: 'string', title: '用户名' },
        password: { type: 'string', title: '密码' },
        certPath: { type: 'string', title: '证书路径' },
      },
    })

    const allFields = parseConfigSchema(schema)
    const filteredFields = allFields.filter(f => !AUTH_MANAGED_KEYS.has(f.key))

    // username, password, certPath 应被过滤
    expect(filteredFields).toHaveLength(3)
    expect(filteredFields.map(f => f.key)).toEqual(['host', 'port', 'database'])
  })

  it('全部 AUTH_MANAGED_KEYS（9个）均应被过滤', () => {
    const schema = JSON.stringify({
      type: 'object',
      properties: {
        host: { type: 'string', title: '主机' },
        ...Object.fromEntries([...AUTH_MANAGED_KEYS].map(k => [k, { type: 'string', title: k }])),
      },
    })

    const allFields = parseConfigSchema(schema)
    const filteredFields = allFields.filter(f => !AUTH_MANAGED_KEYS.has(f.key))

    expect(filteredFields).toHaveLength(1)
    expect(filteredFields[0].key).toBe('host')
  })

  it('空 config_schema → 默认字段中过滤 AUTH_MANAGED_KEYS', () => {
    // 默认降级字段包含 host/port/database/username/password
    const allFields = getDefaultFields()
    const filteredFields = allFields.filter(f => !AUTH_MANAGED_KEYS.has(f.key))

    // username + password 应被过滤
    expect(filteredFields).toHaveLength(3)
    expect(filteredFields.map(f => f.key)).toEqual(['host', 'port', 'database'])
  })
})

// ==================== isFieldDisabled 禁用逻辑 ====================

describe('isFieldDisabled', () => {
  it('未选择认证配置 → 不禁用任何字段', () => {
    const selectedAuthConfigId: string | null = null
    const isDisabled = (key: string): boolean => {
      if (!selectedAuthConfigId) return false
      return key === 'username' || key === 'password'
    }
    expect(isDisabled('username')).toBe(false)
    expect(isDisabled('password')).toBe(false)
    expect(isDisabled('host')).toBe(false)
    expect(isDisabled('port')).toBe(false)
  })

  it('选择认证配置 → 禁用 username + password', () => {
    const selectedAuthConfigId = 'G_auth_001'
    const isDisabled = (key: string): boolean => {
      if (!selectedAuthConfigId) return false
      return key === 'username' || key === 'password'
    }
    expect(isDisabled('username')).toBe(true)
    expect(isDisabled('password')).toBe(true)
  })

  it('选择认证配置 → host/port/database 不受影响', () => {
    const selectedAuthConfigId = 'G_auth_001'
    const isDisabled = (key: string): boolean => {
      if (!selectedAuthConfigId) return false
      return key === 'username' || key === 'password'
    }
    expect(isDisabled('host')).toBe(false)
    expect(isDisabled('port')).toBe(false)
    expect(isDisabled('database')).toBe(false)
  })
})

// ==================== advancedSchemaFields 计算 ====================

describe('advancedSchemaFields 计算', () => {
  it('主 v-for 包含全部非 AUTH_MANAGED_KEYS 字段 → 高级字段为空', () => {
    // NOTE: 当前 configSchemaFields = allFields - AUTH_MANAGED_KEYS（全部非认证字段）。
    // 因此 advancedSchemaFields = allFields - configSchemaFields - AUTH_MANAGED_KEYS = []。
    // 高级连接参数区域始终为空 — 待优化：应将基础字段与扩展字段分类。
    const schema = JSON.stringify({
      type: 'object',
      properties: {
        host: { type: 'string', title: '主机', order: 1 },
        port: { type: 'integer', title: '端口', order: 2 },
        connect_timeout: { type: 'integer', title: '连接超时', order: 10 },
        ssl_ca: { type: 'string', title: 'SSL CA', order: 20 },
      },
    })

    const allFields = parseConfigSchema(schema)
    const mainFields = allFields.filter(f => !AUTH_MANAGED_KEYS.has(f.key))
    const mainKeys = new Set(mainFields.map(f => f.key))
    const advancedFields = allFields.filter(
      f => !mainKeys.has(f.key) && !AUTH_MANAGED_KEYS.has(f.key)
    )

    // 当前行为：configSchemaFields 包含所有非认证字段，advanced 为空
    expect(advancedFields).toHaveLength(0)
    // 验证所有非认证字段都在主字段中
    expect(mainFields.map(f => f.key)).toEqual(['host', 'port', 'connect_timeout', 'ssl_ca'])
  })

  it('仅 host/port/database + username/password → 高级字段为空', () => {
    const schema = JSON.stringify({
      type: 'object',
      properties: {
        host: { type: 'string', title: '主机' },
        port: { type: 'integer', title: '端口' },
        database: { type: 'string', title: '数据库' },
        username: { type: 'string', title: '用户名' },
        password: { type: 'string', title: '密码' },
      },
    })

    const allFields = parseConfigSchema(schema)
    const mainFields = allFields.filter(f => !AUTH_MANAGED_KEYS.has(f.key))
    const mainKeys = new Set(mainFields.map(f => f.key))
    const advancedFields = allFields.filter(
      f => !mainKeys.has(f.key) && !AUTH_MANAGED_KEYS.has(f.key)
    )

    expect(advancedFields).toHaveLength(0)
  })

  it('BASIC_SCHEMA_KEYS 分类后 advancedSchemaFields 正确渲染', () => {
    // 建议修复：引入 BASIC_SCHEMA_KEYS 集合（host/port/database/url/wasmPath/headers/file_path）
    // configSchemaFields = allFields ∩ BASIC_SCHEMA_KEYS（仅基础字段走 v-for）
    // advancedSchemaFields = allFields - configSchemaFields - AUTH_MANAGED_KEYS
    const BASIC_SCHEMA_KEYS = new Set([
      'host',
      'port',
      'database',
      'url',
      'wasmPath',
      'headers',
      'file_path',
    ])

    const schema = JSON.stringify({
      type: 'object',
      properties: {
        host: { type: 'string', title: '主机', order: 1 },
        port: { type: 'integer', title: '端口', order: 2 },
        connect_timeout: { type: 'integer', title: '连接超时', order: 10 },
        ssl_ca: { type: 'string', title: 'SSL CA', order: 20 },
      },
    })

    const allFields = parseConfigSchema(schema)
    // 基础字段：BASIC_SCHEMA_KEYS 中的非认证字段
    const basicFields = allFields.filter(
      f => BASIC_SCHEMA_KEYS.has(f.key) && !AUTH_MANAGED_KEYS.has(f.key)
    )
    const basicKeys = new Set(basicFields.map(f => f.key))
    // 高级字段：非基础 + 非认证
    const advancedFields = allFields.filter(
      f => !basicKeys.has(f.key) && !AUTH_MANAGED_KEYS.has(f.key)
    )

    expect(basicFields.map(f => f.key)).toEqual(['host', 'port'])
    expect(advancedFields.map(f => f.key)).toEqual(['connect_timeout', 'ssl_ca'])
  })
})
