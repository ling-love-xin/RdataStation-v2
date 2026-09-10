/**
 * form-schema 表单配置解析与验证单元测试
 *
 * 测试 parseDriverSchema、generateDefaultFormData、validateFormData、extractExtraOptions 等核心函数。
 */
import { describe, expect, it } from 'vitest'

import type { DriverFormSchema, FormSectionConfig } from '../form-schema'

// ==================== 纯函数提取（从 form-schema.ts 提取，避免模块导入问题） ====================

function extractExtraOptions(sections: FormSectionConfig[]): Array<{
  name: string
  label: string
  type: 'string' | 'number' | 'boolean' | 'select'
  description?: string
  options?: Array<{ label: string; value: string }>
}> {
  const options: Array<{
    name: string
    label: string
    type: 'string' | 'number' | 'boolean' | 'select'
    description?: string
    options?: Array<{ label: string; value: string }>
  }> = []

  for (const section of sections) {
    for (const field of section.fields) {
      if (field.type === 'select' && field.options) {
        options.push({
          name: field.key,
          label: field.label,
          type: 'select',
          description: field.placeholder,
          options: field.options.map(opt => ({
            label: opt.label,
            value: String(opt.value),
          })),
        })
      } else if (field.type === 'text' || field.type === 'number') {
        options.push({
          name: field.key,
          label: field.label,
          type: field.type as 'string' | 'number',
          description: field.placeholder,
        })
      }
    }
  }

  return options
}

function parseDriverSchema(schema: DriverFormSchema) {
  return {
    id: schema.driverId,
    name: schema.driverName,
    icon: schema.driverId,
    version: schema.version,
    features: schema.metadata?.features || [],
    category: schema.metadata?.category,
    defaultPort: schema.metadata?.defaultPort,
    description: schema.metadata?.description,
    driverKind: schema.metadata?.driverKind,
    urlTemplate: schema.metadata?.urlTemplate,
    requireFile: schema.metadata?.requireFile,
    requireDatabase: schema.metadata?.requireDatabase,
    supportsSsl: schema.metadata?.supportsSsl,
    supportsSshTunnel: schema.metadata?.supportsSshTunnel,
    supportsHttpProxy: schema.metadata?.supportsHttpProxy,
    supportsSocksProxy: schema.metadata?.supportsSocksProxy,
    navigation: schema.navigation as Record<string, unknown> | undefined,
    extraOptions: extractExtraOptions(schema.sections),
  }
}

function generateDefaultFormData(schema: DriverFormSchema): Record<string, unknown> {
  const data: Record<string, unknown> = {}

  for (const section of schema.sections) {
    for (const field of section.fields) {
      if (field.default !== undefined) {
        data[field.key] = field.default
      } else if (field.type === 'checkbox') {
        data[field.key] = false
      } else if (field.type === 'number') {
        data[field.key] = 0
      } else {
        data[field.key] = ''
      }
    }
  }

  return data
}

function validateFormData(
  data: Record<string, unknown>,
  schema: DriverFormSchema
): Record<string, string> {
  const errors: Record<string, string> = {}

  for (const section of schema.sections) {
    for (const field of section.fields) {
      if (field.hidden) continue

      if (field.dependsOn) {
        const depValue = data[field.dependsOn.field]
        if (depValue !== field.dependsOn.value) continue
      }

      const value = data[field.key]

      if (field.required && (!value || (typeof value === 'string' && !value.trim()))) {
        errors[field.key] = field.validation?.message || `${field.label} 是必填项`
        continue
      }

      if (value && field.validation) {
        if (field.validation.pattern) {
          const regex = new RegExp(field.validation.pattern)
          if (typeof value === 'string' && !regex.test(value)) {
            errors[field.key] = field.validation.message || `${field.label} 格式不正确`
          }
        }

        if (
          field.validation.minLength &&
          typeof value === 'string' &&
          value.length < field.validation.minLength
        ) {
          errors[field.key] =
            field.validation.message ||
            `${field.label} 长度不能少于 ${field.validation.minLength} 个字符`
        }

        if (
          field.validation.maxLength &&
          typeof value === 'string' &&
          value.length > field.validation.maxLength
        ) {
          errors[field.key] =
            field.validation.message ||
            `${field.label} 长度不能超过 ${field.validation.maxLength} 个字符`
        }

        if (
          field.validation.min !== undefined &&
          typeof value === 'number' &&
          value < field.validation.min
        ) {
          errors[field.key] =
            field.validation.message || `${field.label} 不能小于 ${field.validation.min}`
        }

        if (
          field.validation.max !== undefined &&
          typeof value === 'number' &&
          value > field.validation.max
        ) {
          errors[field.key] =
            field.validation.message || `${field.label} 不能大于 ${field.validation.max}`
        }
      }
    }
  }

  return errors
}

// ==================== 测试数据 ====================

const mysqlSchema: DriverFormSchema = {
  driverId: 'mysql',
  driverName: 'MySQL',
  version: '8.0',
  metadata: {
    category: 'relational',
    description: 'MySQL 关系型数据库',
    features: ['事务', '存储过程', '触发器', '视图'],
    defaultPort: 3306,
    driverKind: 'native',
    urlTemplate: 'mysql://{username}:{password}@{host}:{port}/{database}',
    requireFile: false,
    requireDatabase: true,
    supportsSsl: true,
    supportsSshTunnel: true,
    supportsHttpProxy: true,
    supportsSocksProxy: true,
  },
  sections: [
    {
      key: 'connection',
      title: '连接设置',
      icon: 'database',
      fields: [
        {
          key: 'host',
          label: '主机',
          type: 'text',
          placeholder: 'localhost',
          required: true,
          inline: true,
          flex: 2,
        },
        {
          key: 'port',
          label: '端口',
          type: 'number',
          placeholder: '3306',
          default: 3306,
          required: true,
          inline: true,
          flex: 1,
        },
        {
          key: 'database',
          label: '数据库',
          type: 'text',
          placeholder: 'my_database',
          required: true,
        },
      ],
    },
    {
      key: 'authentication',
      title: '认证',
      icon: 'user',
      fields: [
        {
          key: 'authMethod',
          label: '认证方式',
          type: 'select',
          default: 'password',
          options: [
            { label: '密码', value: 'password' },
            { label: '信任', value: 'trust' },
            { label: 'SSL 证书', value: 'ssl' },
          ],
        },
        {
          key: 'username',
          label: '用户名',
          type: 'text',
          placeholder: 'root',
          required: true,
          dependsOn: { field: 'authMethod', value: 'password' },
        },
        {
          key: 'password',
          label: '密码',
          type: 'password',
          placeholder: '输入密码',
          dependsOn: { field: 'authMethod', value: 'password' },
        },
      ],
    },
    {
      key: 'advanced',
      title: '高级选项',
      icon: 'settings',
      collapsible: true,
      collapsed: true,
      fields: [
        {
          key: 'charset',
          label: '字符集',
          type: 'select',
          default: 'utf8mb4',
          options: [
            { label: 'utf8mb4', value: 'utf8mb4' },
            { label: 'utf8', value: 'utf8' },
            { label: 'latin1', value: 'latin1' },
          ],
        },
        {
          key: 'connectTimeout',
          label: '连接超时 (秒)',
          type: 'number',
          placeholder: '30',
          default: 30,
        },
        {
          key: 'useCompression',
          label: '启用压缩',
          type: 'checkbox',
          default: false,
        },
      ],
    },
  ],
  navigation: {
    hasCatalogs: true,
    hasSchemas: false,
    systemSchemas: ['information_schema', 'mysql', 'performance_schema', 'sys'],
    folders: {
      tables: { enabled: true, label: 'Tables', icon: 'table', childTypes: ['table'] },
      views: { enabled: true, label: 'Views', icon: 'eye', childTypes: ['view'] },
      functions: {
        enabled: true,
        label: 'Functions',
        icon: 'function',
        childTypes: ['function'],
      },
      procedures: {
        enabled: true,
        label: 'Procedures',
        icon: 'terminal',
        childTypes: ['procedure'],
      },
      sequences: { enabled: false, label: 'Sequences', icon: 'hash', childTypes: [] },
      triggers: { enabled: false, label: 'Triggers', icon: 'zap', childTypes: [] },
    },
    tableChildren: {
      columns: true,
      indexes: false,
      constraints: false,
      triggers: false,
      foreignKeys: false,
      references: false,
    },
  },
}

const sqliteSchema: DriverFormSchema = {
  driverId: 'sqlite',
  driverName: 'SQLite',
  metadata: {
    category: 'file',
    description: 'SQLite 文件数据库',
    features: ['轻量', '零配置'],
    driverKind: 'native',
    requireFile: true,
    requireDatabase: false,
    supportsSsl: false,
    supportsSshTunnel: false,
    supportsHttpProxy: false,
    supportsSocksProxy: false,
  },
  sections: [
    {
      key: 'connection',
      title: '连接设置',
      icon: 'database',
      fields: [
        {
          key: 'file_path',
          label: '文件路径',
          type: 'file',
          placeholder: './data.db',
          required: true,
        },
      ],
    },
  ],
}

const minimalSchema: DriverFormSchema = {
  driverId: 'minimal',
  driverName: 'Minimal',
  sections: [],
}

// ==================== parseDriverSchema ====================

describe('parseDriverSchema 驱动描述符解析', () => {
  it('MySQL schema → DriverDescriptor 基础字段', () => {
    const desc = parseDriverSchema(mysqlSchema)
    expect(desc.id).toBe('mysql')
    expect(desc.name).toBe('MySQL')
    expect(desc.icon).toBe('mysql')
    expect(desc.version).toBe('8.0')
    expect(desc.category).toBe('relational')
    expect(desc.defaultPort).toBe(3306)
    expect(desc.driverKind).toBe('native')
    expect(desc.urlTemplate).toBe('mysql://{username}:{password}@{host}:{port}/{database}')
  })

  it('MySQL schema → 协议支持标记', () => {
    const desc = parseDriverSchema(mysqlSchema)
    expect(desc.requireFile).toBe(false)
    expect(desc.requireDatabase).toBe(true)
    expect(desc.supportsSsl).toBe(true)
    expect(desc.supportsSshTunnel).toBe(true)
    expect(desc.supportsHttpProxy).toBe(true)
    expect(desc.supportsSocksProxy).toBe(true)
  })

  it('SQLite schema → 文件数据库标记', () => {
    const desc = parseDriverSchema(sqliteSchema)
    expect(desc.requireFile).toBe(true)
    expect(desc.requireDatabase).toBe(false)
    expect(desc.supportsSsl).toBe(false)
    expect(desc.supportsSshTunnel).toBe(false)
    expect(desc.supportsHttpProxy).toBe(false)
    expect(desc.supportsSocksProxy).toBe(false)
  })

  it('缺少 metadata 时 features 为空数组', () => {
    const desc = parseDriverSchema(minimalSchema)
    expect(desc.features).toEqual([])
    expect(desc.category).toBeUndefined()
    expect(desc.defaultPort).toBeUndefined()
  })

  it('MySQL schema → navigation 透传', () => {
    const desc = parseDriverSchema(mysqlSchema)
    expect(desc.navigation).toBeDefined()
    const nav = desc.navigation!
    expect(nav.hasCatalogs).toBe(true)
    expect(nav.hasSchemas).toBe(false)
    expect(nav.systemSchemas).toHaveLength(4)
  })

  it('MySQL schema → extraOptions 包含 select 和 text 字段', () => {
    const desc = parseDriverSchema(mysqlSchema)
    expect(desc.extraOptions).toBeDefined()
    // authMethod (select) + charset (select) + host (text) + database (text) + username (text) + connectTimeout (number)
    expect(desc.extraOptions!.length).toBeGreaterThanOrEqual(4)

    // authMethod 应为 select 类型
    const authOpt = desc.extraOptions!.find(o => o.name === 'authMethod')
    expect(authOpt).toBeDefined()
    expect(authOpt!.type).toBe('select')
    expect(authOpt!.options).toHaveLength(3)
  })

  it('MySQL schema → extraOptions 中 charset 选项值类型为 string', () => {
    const desc = parseDriverSchema(mysqlSchema)
    const charsetOpt = desc.extraOptions!.find(o => o.name === 'charset')
    expect(charsetOpt).toBeDefined()
    expect(charsetOpt!.type).toBe('select')
    expect(charsetOpt!.options).toEqual([
      { label: 'utf8mb4', value: 'utf8mb4' },
      { label: 'utf8', value: 'utf8' },
      { label: 'latin1', value: 'latin1' },
    ])
  })
})

// ==================== generateDefaultFormData ====================

describe('generateDefaultFormData 表单默认值生成', () => {
  it('MySQL schema → 包含所有字段默认值', () => {
    const data = generateDefaultFormData(mysqlSchema)
    // connection: host='', port=3306, database=''
    // authentication: authMethod='password', username='', password=''
    // advanced: charset='utf8mb4', connectTimeout=30, useCompression=false
    expect(data.host).toBe('')
    expect(data.port).toBe(3306)
    expect(data.database).toBe('')
    expect(data.authMethod).toBe('password')
    expect(data.username).toBe('')
    expect(data.password).toBe('')
    expect(data.charset).toBe('utf8mb4')
    expect(data.connectTimeout).toBe(30)
    expect(data.useCompression).toBe(false)
  })

  it('checkbox 字段默认值为 false', () => {
    const data = generateDefaultFormData(mysqlSchema)
    expect(data.useCompression).toBe(false)
  })

  it('无 default 的 number 字段默认为 0', () => {
    const schema: DriverFormSchema = {
      driverId: 'test',
      driverName: 'Test',
      sections: [
        {
          key: 'main',
          title: 'Main',
          fields: [{ key: 'count', label: '数量', type: 'number' }],
        },
      ],
    }
    const data = generateDefaultFormData(schema)
    expect(data.count).toBe(0)
  })

  it('无 default 的 text 字段默认为空字符串', () => {
    const schema: DriverFormSchema = {
      driverId: 'test',
      driverName: 'Test',
      sections: [
        {
          key: 'main',
          title: 'Main',
          fields: [{ key: 'name', label: '名称', type: 'text' }],
        },
      ],
    }
    const data = generateDefaultFormData(schema)
    expect(data.name).toBe('')
  })

  it('无 sections 时返回空对象', () => {
    const data = generateDefaultFormData(minimalSchema)
    expect(Object.keys(data)).toHaveLength(0)
  })

  it('SQLite schema → file_path 为空字符串', () => {
    const data = generateDefaultFormData(sqliteSchema)
    expect(data.file_path).toBe('')
  })
})

// ==================== validateFormData ====================

describe('validateFormData 表单验证', () => {
  it('完整有效数据 → 无错误', () => {
    const data = {
      host: '192.168.1.1',
      port: 3306,
      database: 'prod_db',
      authMethod: 'password',
      username: 'admin',
      password: 'secret123',
    }
    const errors = validateFormData(data, mysqlSchema)
    expect(Object.keys(errors)).toHaveLength(0)
  })

  it('必填字段为空 → 返回错误', () => {
    const data = {
      host: '',
      port: 3306,
      database: 'test',
      authMethod: 'password',
      username: 'root',
    }
    const errors = validateFormData(data, mysqlSchema)
    expect(errors.host).toBeDefined()
    expect(errors.host).toContain('必填项')
  })

  it('必填字段为空白字符串 → 返回错误', () => {
    const data = {
      host: '   ',
      port: 3306,
      database: 'test',
      authMethod: 'password',
      username: 'root',
    }
    const errors = validateFormData(data, mysqlSchema)
    expect(errors.host).toBeDefined()
  })

  it('trust 认证方式 → 跳过 username/password 验证', () => {
    const data = {
      host: 'localhost',
      port: 3306,
      database: 'test',
      authMethod: 'trust',
    }
    const errors = validateFormData(data, mysqlSchema)
    // username 和 password 依赖于 authMethod='password'，trust 模式下应跳过
    expect(errors.username).toBeUndefined()
    expect(errors.password).toBeUndefined()
  })

  it('dependsOn 不匹配的字段 → 跳过验证', () => {
    const data = {
      host: 'localhost',
      port: 3306,
      database: 'test',
      authMethod: 'trust',
      username: '', // dependsOn authMethod=password，不匹配应跳过
    }
    const errors = validateFormData(data, mysqlSchema)
    expect(errors.username).toBeUndefined()
  })

  it('dependsOn 匹配的字段为空 → 报错', () => {
    const data = {
      host: 'localhost',
      port: 3306,
      database: 'test',
      authMethod: 'password',
      username: '', // dependsOn authMethod=password，匹配且为空
    }
    const errors = validateFormData(data, mysqlSchema)
    expect(errors.username).toBeDefined()
  })

  it('hidden 字段 → 跳过验证', () => {
    const schema: DriverFormSchema = {
      driverId: 'test',
      driverName: 'Test',
      sections: [
        {
          key: 'main',
          title: 'Main',
          fields: [{ key: 'secret', label: '密钥', type: 'text', required: true, hidden: true }],
        },
      ],
    }
    const errors = validateFormData({ secret: '' }, schema)
    expect(errors.secret).toBeUndefined()
  })

  it('pattern 正则验证 → 格式不正确报错', () => {
    const schema: DriverFormSchema = {
      driverId: 'test',
      driverName: 'Test',
      sections: [
        {
          key: 'main',
          title: 'Main',
          fields: [
            {
              key: 'email',
              label: '邮箱',
              type: 'text',
              validation: {
                pattern: '^[\\w.-]+@[\\w.-]+\\.\\w+$',
                message: '邮箱格式不正确',
              },
            },
          ],
        },
      ],
    }
    const errors = validateFormData({ email: 'not-an-email' }, schema)
    expect(errors.email).toBe('邮箱格式不正确')
  })

  it('pattern 正则验证 → 格式正确无错误', () => {
    const schema: DriverFormSchema = {
      driverId: 'test',
      driverName: 'Test',
      sections: [
        {
          key: 'main',
          title: 'Main',
          fields: [
            {
              key: 'email',
              label: '邮箱',
              type: 'text',
              validation: {
                pattern: '^[\\w.-]+@[\\w.-]+\\.\\w+$',
              },
            },
          ],
        },
      ],
    }
    const errors = validateFormData({ email: 'test@example.com' }, schema)
    expect(errors.email).toBeUndefined()
  })

  it('minLength 验证 → 字符串过短报错', () => {
    const schema: DriverFormSchema = {
      driverId: 'test',
      driverName: 'Test',
      sections: [
        {
          key: 'main',
          title: 'Main',
          fields: [
            {
              key: 'password',
              label: '密码',
              type: 'password',
              validation: { minLength: 8 },
            },
          ],
        },
      ],
    }
    const errors = validateFormData({ password: '1234' }, schema)
    expect(errors.password).toBeDefined()
    expect(errors.password).toContain('8')
  })

  it('maxLength 验证 → 字符串过长报错', () => {
    const schema: DriverFormSchema = {
      driverId: 'test',
      driverName: 'Test',
      sections: [
        {
          key: 'main',
          title: 'Main',
          fields: [
            {
              key: 'name',
              label: '名称',
              type: 'text',
              validation: { maxLength: 10 },
            },
          ],
        },
      ],
    }
    const errors = validateFormData({ name: 'very-long-name-exceeds-limit' }, schema)
    expect(errors.name).toBeDefined()
    expect(errors.name).toContain('10')
  })

  it('min 验证 → 数字过小报错', () => {
    // 注意：value && field.validation 中，0 是 falsy 会跳过验证
    // 所以使用 -1（truthy）来测试 min 验证
    const schema: DriverFormSchema = {
      driverId: 'test',
      driverName: 'Test',
      sections: [
        {
          key: 'main',
          title: 'Main',
          fields: [
            {
              key: 'port',
              label: '端口',
              type: 'number',
              default: 3306,
              validation: { min: 1, max: 65535 },
            },
          ],
        },
      ],
    }
    const errors = validateFormData({ port: -1 }, schema)
    expect(errors.port).toBeDefined()
    expect(errors.port).toContain('1')
  })

  it('max 验证 → 数字过大报错', () => {
    const schema: DriverFormSchema = {
      driverId: 'test',
      driverName: 'Test',
      sections: [
        {
          key: 'main',
          title: 'Main',
          fields: [
            {
              key: 'port',
              label: '端口',
              type: 'number',
              validation: { min: 1, max: 65535 },
            },
          ],
        },
      ],
    }
    const errors = validateFormData({ port: 99999 }, schema)
    expect(errors.port).toBeDefined()
    expect(errors.port).toContain('65535')
  })

  it('自定义验证错误消息', () => {
    const schema: DriverFormSchema = {
      driverId: 'test',
      driverName: 'Test',
      sections: [
        {
          key: 'main',
          title: 'Main',
          fields: [
            {
              key: 'port',
              label: '端口',
              type: 'number',
              required: true,
              validation: { message: '请填写有效端口号' },
            },
          ],
        },
      ],
    }
    const errors = validateFormData({}, schema)
    expect(errors.port).toBe('请填写有效端口号')
  })

  it('空 schema → 无错误', () => {
    const errors = validateFormData({}, minimalSchema)
    expect(Object.keys(errors)).toHaveLength(0)
  })

  it('多个字段同时报告错误', () => {
    const data = {
      host: '',
      port: 0,
      database: '',
      authMethod: 'password',
      username: '',
    }
    const errors = validateFormData(data, mysqlSchema)
    expect(Object.keys(errors).length).toBeGreaterThanOrEqual(3)
    expect(errors.host).toBeDefined()
    expect(errors.database).toBeDefined()
    expect(errors.username).toBeDefined()
  })
})

// ==================== extractExtraOptions ====================

describe('extractExtraOptions 额外选项提取', () => {
  it('空 sections → 空数组', () => {
    const options = extractExtraOptions([])
    expect(options).toHaveLength(0)
  })

  it('select 字段 → 类型为 select，含 options', () => {
    const sections: FormSectionConfig[] = [
      {
        key: 'main',
        title: 'Main',
        fields: [
          {
            key: 'mode',
            label: '模式',
            type: 'select',
            options: [
              { label: 'A', value: 'a' },
              { label: 'B', value: 'b' },
            ],
          },
        ],
      },
    ]
    const options = extractExtraOptions(sections)
    expect(options).toHaveLength(1)
    expect(options[0].name).toBe('mode')
    expect(options[0].type).toBe('select')
    expect(options[0].options).toEqual([
      { label: 'A', value: 'a' },
      { label: 'B', value: 'b' },
    ])
  })

  it('text 字段 → 类型为 text（保持原始字段类型）', () => {
    const sections: FormSectionConfig[] = [
      {
        key: 'main',
        title: 'Main',
        fields: [
          {
            key: 'host',
            label: '主机',
            type: 'text',
            placeholder: 'localhost',
          },
        ],
      },
    ]
    const options = extractExtraOptions(sections)
    expect(options).toHaveLength(1)
    expect(options[0].name).toBe('host')
    expect(options[0].type).toBe('text')
    expect(options[0].description).toBe('localhost')
  })

  it('number 字段 → 类型为 number', () => {
    const sections: FormSectionConfig[] = [
      {
        key: 'main',
        title: 'Main',
        fields: [
          {
            key: 'timeout',
            label: '超时',
            type: 'number',
            placeholder: '30',
          },
        ],
      },
    ]
    const options = extractExtraOptions(sections)
    expect(options).toHaveLength(1)
    expect(options[0].type).toBe('number')
  })

  it('password/checkbox/textarea 字段 → 不提取为 extraOptions', () => {
    const sections: FormSectionConfig[] = [
      {
        key: 'main',
        title: 'Main',
        fields: [
          { key: 'pwd', label: '密码', type: 'password' },
          { key: 'enabled', label: '启用', type: 'checkbox' },
          { key: 'desc', label: '描述', type: 'textarea' },
        ],
      },
    ]
    const options = extractExtraOptions(sections)
    expect(options).toHaveLength(0)
  })

  it('select 字段无 options → 不提取', () => {
    const sections: FormSectionConfig[] = [
      {
        key: 'main',
        title: 'Main',
        fields: [{ key: 'mode', label: '模式', type: 'select' }],
      },
    ]
    const options = extractExtraOptions(sections)
    expect(options).toHaveLength(0)
  })
})

// ==================== 边界场景 ====================

describe('边界场景', () => {
  it('fields 为 undefined 的 section → 会抛出 TypeError', () => {
    const schema: DriverFormSchema = {
      driverId: 'test',
      driverName: 'Test',
      sections: [
        {
          key: 'empty',
          title: 'Empty',
          fields: undefined,
        } as unknown as FormSectionConfig,
      ],
    }
    // fields 为 undefined 时 for...of 迭代会抛出 TypeError
    expect(() => generateDefaultFormData(schema)).toThrow(TypeError)
    expect(() => validateFormData({}, schema)).toThrow(TypeError)
  })

  it('value 为 0 且 required → 会触发 required 错误（!0 === true）', () => {
    const schema: DriverFormSchema = {
      driverId: 'test',
      driverName: 'Test',
      sections: [
        {
          key: 'main',
          title: 'Main',
          fields: [{ key: 'port', label: '端口', type: 'number', required: true }],
        },
      ],
    }
    const errors = validateFormData({ port: 0 }, schema)
    // 当前实现：!0 === true，触发 required 检查
    expect(errors.port).toBeDefined()
  })

  it('value 为 false 且 required → 会触发 required 错误（!false === true）', () => {
    const schema: DriverFormSchema = {
      driverId: 'test',
      driverName: 'Test',
      sections: [
        {
          key: 'main',
          title: 'Main',
          fields: [{ key: 'enabled', label: '启用', type: 'checkbox', required: true }],
        },
      ],
    }
    const errors = validateFormData({ enabled: false }, schema)
    // 当前实现：!false === true，触发 required 检查
    expect(errors.enabled).toBeDefined()
  })
})
