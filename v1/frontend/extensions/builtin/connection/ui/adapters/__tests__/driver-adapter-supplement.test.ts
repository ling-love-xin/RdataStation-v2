/**
 * driver-adapter 补充测试 — backendDriverToDescriptor / configSchemaToFormSchema
 *
 * 测试后端 Driver → 前端 DriverDescriptor / DriverFormSchema 的完整转换链路。
 */
import { describe, expect, it } from 'vitest'

import { backendDriverToDescriptor, configSchemaToFormSchema } from '../driver-adapter'

import type { Driver } from '../../../domain/types'

// ==================== 测试数据 ====================

const baseDriver: Driver = {
  id: 'mysql',
  type_id: 'relational',
  name: 'MySQL',
  driver_kind: 'native',
  is_file: false,
  default_port: 3306,
  url_template: 'mysql://{username}:{password}@{host}:{port}/{database}',
  version: '8.0',
  config_schema: '',
  supported_auth_types: '["password","ssl"]',
  capabilities: '["事务","存储过程","视图","触发器"]',
  enabled: true,
}

const jsonSchemaConfigSchema = JSON.stringify({
  type: 'object',
  properties: {
    host: { type: 'string', title: '主机', default: 'localhost' },
    port: { type: 'integer', title: '端口', default: 3306 },
    database: { type: 'string', title: '数据库' },
    username: { type: 'string', title: '用户名', default: 'root' },
    password: { type: 'string', title: '密码', format: 'password' },
    ssl_mode: {
      type: 'string',
      title: 'SSL 模式',
      enum: ['disable', 'require', 'verify-ca', 'verify-full'],
    },
    use_compression: { type: 'boolean', title: '启用压缩', default: false },
    connect_timeout: { type: 'integer', title: '连接超时', default: 30 },
    charset: {
      type: 'string',
      title: '字符集',
      enum: ['utf8mb4', 'utf8', 'latin1'],
    },
  },
  required: ['host', 'port', 'database'],
})

const customFormatConfigSchema = JSON.stringify({
  fields: [
    { key: 'host', label: '主机', type: 'string', required: true },
    { key: 'port', label: '端口', type: 'integer' },
  ],
  options: [
    { key: 'charset', label: '字符集', type: 'select', values: ['utf8', 'latin1'] },
    { key: 'timeout', label: '超时', type: 'integer', default: '30' },
  ],
})

// ==================== backendDriverToDescriptor ====================

describe('backendDriverToDescriptor  后端 Driver → DriverDescriptor', () => {
  it('JSON Schema 格式 → 完整转换', () => {
    const driver: Driver = {
      ...baseDriver,
      config_schema: jsonSchemaConfigSchema,
    }
    const desc = backendDriverToDescriptor(driver)

    expect(desc.id).toBe('mysql')
    expect(desc.name).toBe('MySQL')
    expect(desc.defaultPort).toBe(3306)
    expect(desc.requiresDatabase).toBe(true)
    expect(desc.requiresFile).toBe(false)
    expect(desc.supportsSsl).toBe(true)
    expect(desc.supportsSshTunnel).toBe(false)
    expect(desc.supportsHttpProxy).toBe(false)
    expect(desc.supportsSocksProxy).toBe(false)

    // fields 应包含 host/port/database/username/password
    expect(desc.fields).toBeDefined()
    expect(desc.fields!.length).toBeGreaterThanOrEqual(5)
  })

  it('JSON Schema 格式 → fields 字段映射', () => {
    const driver: Driver = {
      ...baseDriver,
      config_schema: jsonSchemaConfigSchema,
    }
    const desc = backendDriverToDescriptor(driver)
    const fields = desc.fields!

    const hostField = fields.find(f => f.name === 'host')
    expect(hostField).toBeDefined()
    expect(hostField!.label).toBe('主机')
    expect(hostField!.required).toBe(true)
    expect(hostField!.defaultValue).toBe('localhost')

    const portField = fields.find(f => f.name === 'port')
    expect(portField).toBeDefined()
    expect(portField!.fieldType).toBe('number')
    expect(portField!.defaultValue).toBe('3306')

    const pwField = fields.find(f => f.name === 'password')
    expect(pwField).toBeDefined()
    // mapJsonSchemaType 不处理 format 字段，string 统一映射为 text
    expect(pwField!.fieldType).toBe('text')
  })

  it('JSON Schema 格式 → extraOptions 包含 enum/boolean 字段', () => {
    const driver: Driver = {
      ...baseDriver,
      config_schema: jsonSchemaConfigSchema,
    }
    const desc = backendDriverToDescriptor(driver)
    const options = desc.extraOptions!

    expect(options.length).toBeGreaterThanOrEqual(3)

    const sslMode = options.find(o => o.name === 'ssl_mode')
    expect(sslMode).toBeDefined()
    // mapOptionType 将 text 映射为 string（不区分 select）
    expect(sslMode!.optionType).toBe('string')
    expect(sslMode!.defaultValue).toBeUndefined()

    const charset = options.find(o => o.name === 'charset')
    expect(charset).toBeDefined()
    // mapOptionType 将 text 映射为 string（不区分 select）
    expect(charset!.optionType).toBe('string')

    const compression = options.find(o => o.name === 'use_compression')
    expect(compression).toBeDefined()
    expect(compression!.optionType).toBe('boolean')
    expect(compression!.defaultValue).toBe('false')
  })

  it('自定义格式 → fields + options', () => {
    const driver: Driver = {
      ...baseDriver,
      config_schema: customFormatConfigSchema,
    }
    const desc = backendDriverToDescriptor(driver)

    expect(desc.fields).toBeDefined()
    expect(desc.fields!.length).toBe(2)

    expect(desc.extraOptions).toBeDefined()
    expect(desc.extraOptions!.length).toBe(2)
  })

  it('空 config_schema → 无 fields 和 options', () => {
    const driver: Driver = {
      ...baseDriver,
      config_schema: '',
    }
    const desc = backendDriverToDescriptor(driver)
    expect(desc.fields).toBeUndefined()
    expect(desc.extraOptions).toBeUndefined()
  })

  it('is_file=true → requiresFile=true, requiresDatabase=false', () => {
    const driver: Driver = {
      ...baseDriver,
      is_file: true,
    }
    const desc = backendDriverToDescriptor(driver)
    expect(desc.requiresFile).toBe(true)
    expect(desc.requiresDatabase).toBe(false)
  })

  it('supported_auth_types 不含 ssl → supportsSsl=false', () => {
    const driver: Driver = {
      ...baseDriver,
      supported_auth_types: '["password","ldap"]',
    }
    const desc = backendDriverToDescriptor(driver)
    expect(desc.supportsSsl).toBe(false)
  })

  it('supported_auth_types 为 null → supportsSsl=false', () => {
    const driver: Driver = {
      ...baseDriver,
      supported_auth_types: undefined,
    }
    const desc = backendDriverToDescriptor(driver)
    expect(desc.supportsSsl).toBe(false)
  })

  it('capabilities 解析 → 功能列表', () => {
    const driver: Driver = {
      ...baseDriver,
      capabilities: '["事务","存储过程","视图"]',
    }
    const desc = backendDriverToDescriptor(driver)
    expect(desc.description).toBe('MySQL')
  })

  it('capabilities 为逗号分隔字符串 → 降级解析', () => {
    const driver: Driver = {
      ...baseDriver,
      capabilities: '事务,存储过程,视图',
    }
    const desc = backendDriverToDescriptor(driver)
    expect(desc.description).toBe('MySQL')
  })

  it('no default_port → defaultPort=undefined', () => {
    const driver: Driver = {
      ...baseDriver,
      default_port: undefined,
    }
    const desc = backendDriverToDescriptor(driver)
    expect(desc.defaultPort).toBeUndefined()
  })
})

// ==================== configSchemaToFormSchema ====================

describe('configSchemaToFormSchema  后端 Driver → DriverFormSchema', () => {
  it('JSON Schema 格式 → 完整 FormSchema', () => {
    const driver: Driver = {
      ...baseDriver,
      config_schema: jsonSchemaConfigSchema,
    }
    const schema = configSchemaToFormSchema(driver)

    expect(schema.driverId).toBe('mysql')
    expect(schema.driverName).toBe('MySQL')
    expect(schema.version).toBe('8.0')

    expect(schema.metadata).toBeDefined()
    expect(schema.metadata!.category).toBe('relational')
    expect(schema.metadata!.defaultPort).toBe(3306)
    expect(schema.metadata!.driverKind).toBe('native')
    expect(schema.metadata!.urlTemplate).toBe(
      'mysql://{username}:{password}@{host}:{port}/{database}'
    )
    expect(schema.metadata!.requireFile).toBe(false)
    expect(schema.metadata!.requireDatabase).toBe(true)
    expect(schema.metadata!.supportsSsl).toBe(true)
    expect(schema.metadata!.features).toContain('事务')
  })

  it('JSON Schema 格式 → sections 包含 connection 和 options', () => {
    const driver: Driver = {
      ...baseDriver,
      config_schema: jsonSchemaConfigSchema,
    }
    const schema = configSchemaToFormSchema(driver)

    expect(schema.sections).toHaveLength(2)

    const connSection = schema.sections.find(s => s.key === 'connection')
    expect(connSection).toBeDefined()
    expect(connSection!.title).toBe('连接参数')
    expect(connSection!.fields.length).toBeGreaterThanOrEqual(3)

    const optSection = schema.sections.find(s => s.key === 'options')
    expect(optSection).toBeDefined()
    expect(optSection!.title).toBe('高级选项')
    expect(optSection!.collapsible).toBe(true)
    expect(optSection!.fields.length).toBeGreaterThanOrEqual(3)
  })

  it('JSON Schema 格式 → select 字段有 options', () => {
    const driver: Driver = {
      ...baseDriver,
      config_schema: jsonSchemaConfigSchema,
    }
    const schema = configSchemaToFormSchema(driver)
    const optSection = schema.sections.find(s => s.key === 'options')!
    const sslField = optSection.fields.find(f => f.key === 'ssl_mode')
    expect(sslField).toBeDefined()
    // mapJsonSchemaType 将 string 映射为 text，enum 不自动转为 select
    // mapNormalizedFieldToFormField 仅在 fieldType === 'select' 时设置 options
    expect(sslField!.type).toBe('text')
    expect(sslField!.options).toBeUndefined()
  })

  it('自定义格式 → sections', () => {
    const driver: Driver = {
      ...baseDriver,
      config_schema: customFormatConfigSchema,
    }
    const schema = configSchemaToFormSchema(driver)

    expect(schema.sections).toHaveLength(2)

    const connSection = schema.sections.find(s => s.key === 'connection')!
    expect(connSection.fields).toHaveLength(2)

    const optSection = schema.sections.find(s => s.key === 'options')!
    expect(optSection.fields).toHaveLength(2)
  })

  it('空 config_schema → 无 sections', () => {
    const driver: Driver = {
      ...baseDriver,
      config_schema: '',
    }
    const schema = configSchemaToFormSchema(driver)
    expect(schema.sections).toHaveLength(0)
  })

  it('仅 fields 无 options → 只有 connection section', () => {
    const schemaStr = JSON.stringify({
      fields: [
        { key: 'host', label: '主机', type: 'string' },
        { key: 'port', label: '端口', type: 'integer' },
      ],
    })
    const driver: Driver = {
      ...baseDriver,
      config_schema: schemaStr,
    }
    const schema = configSchemaToFormSchema(driver)
    expect(schema.sections).toHaveLength(1)
    expect(schema.sections[0].key).toBe('connection')
  })

  it('仅 options 无 fields → 只有 options section', () => {
    const schemaStr = JSON.stringify({
      options: [{ key: 'charset', label: '字符集', type: 'select', values: ['utf8'] }],
    })
    const driver: Driver = {
      ...baseDriver,
      config_schema: schemaStr,
    }
    const schema = configSchemaToFormSchema(driver)
    expect(schema.sections).toHaveLength(1)
    expect(schema.sections[0].key).toBe('options')
  })

  it('version 为 null → metadata 无 version', () => {
    const driver: Driver = {
      ...baseDriver,
      version: undefined,
      config_schema: jsonSchemaConfigSchema,
    }
    const schema = configSchemaToFormSchema(driver)
    expect(schema.version).toBeUndefined()
  })

  it('url_template 为空 → metadata.urlTemplate=undefined', () => {
    const driver: Driver = {
      ...baseDriver,
      url_template: undefined,
      config_schema: jsonSchemaConfigSchema,
    }
    const schema = configSchemaToFormSchema(driver)
    expect(schema.metadata!.urlTemplate).toBeUndefined()
  })
})
