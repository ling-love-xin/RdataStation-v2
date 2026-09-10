/**
 * 驱动适配器 — 将 SQLite drivers 表的 config_schema JSON 转换为前端 DriverDescriptor / DriverFormSchema
 *
 * 数据流：
 *   global.db.drivers.config_schema (JSON Schema 格式)
 *     → parseConfigSchema() → DbConfigSchema
 *     → backendDriverToDescriptor() → DriverDescriptor
 *     → configSchemaToFormSchema() → DriverFormSchema → DynamicFormRenderer
 */

import type {
  Driver,
  DataSourceType,
  DriverDescriptor,
  DriverField,
  DriverOption,
} from '../../domain/types'
import type { DriverFormSchema, FormSectionConfig, FormFieldConfig } from '../types/form-schema'

// Re-export for convenience
export type { Driver as BackendDriver, DataSourceType as BackendDataSourceType }

// ==================== config_schema JSON 结构 ====================

/**
 * SQLite seed data 使用的是 JSON Schema 格式:
 *   { "type":"object", "properties":{ "host":{ "type":"string", "title":"主机" } }, "required":[...] }
 *
 * 也兼容自定义格式:
 *   { "fields":[...], "options":[...] }
 */

/** JSON Schema 格式的单个属性 */
interface JsonSchemaProperty {
  type: string // "string" | "integer" | "boolean"
  title?: string // 中文标签
  default?: string | number | boolean
  format?: string // "password" | "file"
  enum?: string[] // 下拉选项
}

/** JSON Schema 根对象 */
interface JsonSchemaRoot {
  type: 'object'
  properties: Record<string, JsonSchemaProperty>
  required?: string[]
}

/** 自定义格式的字段定义 */
interface CustomFieldDef {
  key: string
  label: string
  type: string
  required?: boolean
  default?: string
  placeholder?: string
  values?: string[]
}

/** 自定义格式的 config_schema */
interface CustomConfigSchema {
  fields?: CustomFieldDef[]
  options?: CustomFieldDef[]
}

/** 统一内部字段表示 */
interface NormalizedField {
  key: string
  label: string
  type: string
  required: boolean
  default?: string | number | boolean
  placeholder?: string
  values?: string[]
}

// ==================== 转换函数 ====================

/**
 * 将后端 Driver 转为前端 DriverDescriptor
 * 从 config_schema JSON 中提取 fields/options 构建 fields 和 extraOptions
 */
export function backendDriverToDescriptor(driver: Driver): DriverDescriptor {
  const { fields, options } = parseConfigSchema(driver.config_schema)

  const driverFields = fields.map(f => ({
    name: f.key,
    label: f.label,
    fieldType: mapFieldType(f.type),
    required: f.required,
    defaultValue: String(f.default ?? ''),
    placeholder: f.placeholder,
    options: f.values?.map(v => ({ label: v, value: v })),
  })) as DriverField[]

  const extraOptions = options.map(o => ({
    name: o.key,
    label: o.label,
    optionType: mapOptionType(o.type),
    defaultValue: o.default,
    description: undefined,
  })) as DriverOption[]

  const _features = tryParseJsonArray(driver.capabilities)

  return {
    id: driver.id,
    name: driver.name,
    description: driver.name,
    defaultPort: driver.default_port ?? undefined,
    requiresDatabase: !driver.is_file,
    requiresFile: driver.is_file,
    supportsSsl: tryParseJsonArray(driver.supported_auth_types).includes('ssl'),
    supportsSshTunnel: false,
    supportsHttpProxy: false,
    supportsSocksProxy: false,
    fields: driverFields.length > 0 ? driverFields : undefined,
    extraOptions: extraOptions.length > 0 ? extraOptions : undefined,
  }
}

/**
 * 将 config_schema JSON 字符串转为 DriverFormSchema
 * 用于 DynamicFormRenderer 渲染表单
 */
export function configSchemaToFormSchema(driver: Driver): DriverFormSchema {
  const { fields, options } = parseConfigSchema(driver.config_schema)

  const sections: FormSectionConfig[] = []

  const connectionFields: FormFieldConfig[] = fields.map(mapNormalizedFieldToFormField)
  if (connectionFields.length > 0) {
    sections.push({
      key: 'connection',
      title: '连接参数',
      description: '数据库连接必需参数',
      fields: connectionFields,
    })
  }

  const optionFields: FormFieldConfig[] = options.map(mapNormalizedFieldToFormField)
  if (optionFields.length > 0) {
    sections.push({
      key: 'options',
      title: '高级选项',
      icon: 'settings',
      collapsible: true,
      collapsed: false,
      fields: optionFields,
    })
  }

  return {
    driverId: driver.id,
    driverName: driver.name,
    version: driver.version || undefined,
    sections,
    metadata: {
      category: driver.type_id,
      description: driver.name,
      features: tryParseJsonArray(driver.capabilities),
      defaultPort: driver.default_port ?? undefined,
      driverKind: driver.driver_kind,
      urlTemplate: driver.url_template || undefined,
      requireFile: driver.is_file,
      requireDatabase: !driver.is_file,
      supportsSsl: tryParseJsonArray(driver.supported_auth_types).includes('ssl'),
      supportsSshTunnel: false,
      supportsHttpProxy: false,
      supportsSocksProxy: false,
    },
  }
}

/**
 * 解析 config_schema JSON 为规范化字段列表
 *
 * 支持两种格式：
 *   JSON Schema 格式: { type:"object", properties:{...}, required:[...] }
 *   自定义格式:       { fields:[...], options:[...] }
 */
export function parseConfigSchema(raw: string): {
  fields: NormalizedField[]
  options: NormalizedField[]
} {
  if (!raw) return { fields: [], options: [] }

  try {
    const parsed = JSON.parse(raw) as Record<string, unknown>

    // 检测自定义格式 { fields, options }
    if ('fields' in parsed || 'options' in parsed) {
      return {
        fields: normalizeCustomFields((parsed as unknown as CustomConfigSchema).fields || []),
        options: normalizeCustomFields((parsed as unknown as CustomConfigSchema).options || []),
      }
    }

    // 检测 JSON Schema 格式 { type:"object", properties }
    if (
      (parsed as unknown as JsonSchemaRoot).type === 'object' &&
      (parsed as unknown as JsonSchemaRoot).properties
    ) {
      return normalizeJsonSchema(parsed as unknown as JsonSchemaRoot)
    }

    return { fields: [], options: [] }
  } catch (err) {
    console.warn(
      '[driver-adapter] 驱动元数据 JSON 解析失败:',
      err instanceof Error ? err.message : String(err)
    )
    return { fields: [], options: [] }
  }
}

/**
 * 判断是否为文件数据库
 */
export function isFileDatabase(driver: Driver): boolean {
  return driver.is_file
}

// ==================== 内部辅助 ====================

/** 将 JSON Schema 格式转为 NormalizedField[] */
function normalizeJsonSchema(schema: JsonSchemaRoot): {
  fields: NormalizedField[]
  options: NormalizedField[]
} {
  const required = new Set(schema.required || [])
  const fields: NormalizedField[] = []
  const options: NormalizedField[] = []

  for (const [key, prop] of Object.entries(schema.properties)) {
    const normalized: NormalizedField = {
      key,
      label: prop.title || key,
      type: mapJsonSchemaType(prop.type),
      required: required.has(key),
      default: prop.default !== undefined ? String(prop.default) : undefined,
      placeholder: undefined,
      values: prop.enum,
    }

    // format: "password" | "file" 的字段归为连接参数
    // enum 字段归为高级选项
    if (prop.enum || prop.type === 'boolean') {
      options.push(normalized)
    } else {
      fields.push(normalized)
    }
  }

  return { fields, options }
}

/** 将 JSON Schema type 映射到 UI 字段类型 */
function mapJsonSchemaType(jsType: string): string {
  switch (jsType) {
    case 'integer':
      return 'number'
    case 'boolean':
      return 'checkbox'
    case 'string':
      return 'text'
    default:
      return 'text'
  }
}

/** 将自定义格式字段转为 NormalizedField[] */
function normalizeCustomFields(raw: CustomFieldDef[]): NormalizedField[] {
  return raw.map(f => ({
    key: f.key,
    label: f.label,
    type: f.type,
    required: f.required ?? false,
    default: f.default,
    placeholder: f.placeholder,
    values: f.values,
  }))
}

function mapNormalizedFieldToFormField(f: NormalizedField): FormFieldConfig {
  const fieldType = mapFieldType(f.type)
  const formField: FormFieldConfig = {
    key: f.key,
    label: f.label,
    type: fieldType as FormFieldConfig['type'],
    required: f.required,
    default: f.default !== undefined ? String(f.default) : undefined,
    placeholder: f.placeholder,
  }

  if (fieldType === 'select' && f.values) {
    formField.options = f.values.map(v => ({ label: v, value: v }))
  }

  return formField
}

function mapFieldType(dbType: string): string {
  const typeMap: Record<string, string> = {
    text: 'text',
    password: 'password',
    number: 'number',
    select: 'select',
    checkbox: 'checkbox',
    file: 'file',
    textarea: 'textarea',
  }
  return typeMap[dbType] || 'text'
}

function mapOptionType(dbType: string): 'string' | 'number' | 'boolean' | 'select' {
  const typeMap: Record<string, 'string' | 'number' | 'boolean' | 'select'> = {
    text: 'string',
    password: 'string',
    number: 'number',
    select: 'select',
    checkbox: 'boolean',
  }
  return typeMap[dbType] || 'string'
}

function tryParseJsonArray(raw: string | null | undefined): string[] {
  if (!raw) return []
  try {
    const parsed = JSON.parse(raw)
    if (Array.isArray(parsed)) {
      return parsed.filter((item: unknown): item is string => typeof item === 'string')
    }
    return []
  } catch {
    return raw
      .split(',')
      .map(s => s.trim())
      .filter(Boolean)
  }
}
