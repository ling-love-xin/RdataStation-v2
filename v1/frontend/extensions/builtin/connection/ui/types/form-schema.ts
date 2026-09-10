import type { DriverDescriptor } from './connection'

export interface FormFieldConfig {
  key: string
  label: string
  type: 'text' | 'password' | 'number' | 'select' | 'checkbox' | 'file' | 'textarea'
  placeholder?: string
  required?: boolean
  default?: unknown
  options?: Array<{ label: string; value: string | number }>
  validation?: {
    pattern?: string
    min?: number
    max?: number
    minLength?: number
    maxLength?: number
    message?: string
  }
  tooltip?: string
  hidden?: boolean
  dependsOn?: {
    field: string
    value: unknown
  }
  inline?: boolean
  flex?: number
}

export interface FormSectionConfig {
  key: string
  title: string
  icon?: string
  description?: string
  fields: FormFieldConfig[]
  collapsible?: boolean
  collapsed?: boolean
  enableField?: string
}

export interface NavigationFolderConfig {
  enabled: boolean
  label: string
  icon: string
  childTypes?: string[]
}

export interface NavigationConfig {
  hasCatalogs: boolean
  hasSchemas: boolean
  systemSchemas: string[]
  folders: {
    tables: NavigationFolderConfig
    views: NavigationFolderConfig
    functions: NavigationFolderConfig
    procedures: NavigationFolderConfig
    sequences: NavigationFolderConfig
    triggers: NavigationFolderConfig
  }
  tableChildren: {
    columns: boolean
    indexes: boolean
    constraints: boolean
    triggers: boolean
    foreignKeys: boolean
    references: boolean
  }
}

export interface DriverFormSchema {
  driverId: string
  driverName: string
  version?: string
  sections: FormSectionConfig[]
  metadata?: {
    category: string
    description: string
    features: string[]
    defaultPort?: number
    driverKind?: string
    urlTemplate?: string
    requireFile?: boolean
    requireDatabase?: boolean
    supportsSsl?: boolean
    supportsSshTunnel?: boolean
    supportsHttpProxy?: boolean
    supportsSocksProxy?: boolean
  }
  navigation?: NavigationConfig
}

export function parseDriverSchema(schema: DriverFormSchema): DriverDescriptor {
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

export function generateDefaultFormData(schema: DriverFormSchema): Record<string, unknown> {
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

export function validateFormData(
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
