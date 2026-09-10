import type { DriverFormSchema, NavigationConfig } from '../types/form-schema'

export { generateDefaultFormData, validateFormData } from '../types/form-schema'

const schemaCache = new Map<string, DriverFormSchema>()

export async function loadDriverSchema(driverId: string): Promise<DriverFormSchema | null> {
  const cached = schemaCache.get(driverId)
  if (cached) {
    return cached
  }

  const idsToTry = [driverId]

  const dashIdx = driverId.indexOf('-')
  if (dashIdx > 0) {
    const baseDbType = driverId.substring(0, dashIdx)
    if (baseDbType !== driverId) {
      idsToTry.push(baseDbType)
    }
  }

  for (const id of idsToTry) {
    try {
      const schemaModule = await import(`../schemas/${id}.json`)
      const schema = schemaModule.default as DriverFormSchema
      schemaCache.set(driverId, schema)
      return schema
    } catch {
      // try next fallback
    }
  }

  console.warn(`未找到驱动 ${driverId} 的表单配置，使用默认配置`)
  return null
}

export async function loadAllSchemas(): Promise<DriverFormSchema[]> {
  const schemaFiles = import.meta.glob('../schemas/*.json')
  const schemas: DriverFormSchema[] = []

  for (const path of Object.keys(schemaFiles)) {
    try {
      const module = await schemaFiles[path]()
      const schema = (module as Record<string, unknown>).default as DriverFormSchema
      schemas.push(schema)
      schemaCache.set(schema.driverId, schema)
    } catch (e) {
      console.error(`加载表单配置失败: ${path}`, e)
    }
  }

  return schemas
}

export function clearSchemaCache(): void {
  schemaCache.clear()
}

export async function loadNavigationConfig(driverId: string): Promise<NavigationConfig | null> {
  const schema = await loadDriverSchema(driverId)
  if (!schema?.navigation) return null
  return schema.navigation
}

export function getDefaultNavigationConfig(): NavigationConfig {
  return {
    hasCatalogs: true,
    hasSchemas: false,
    systemSchemas: [],
    folders: {
      tables: { enabled: true, label: 'Tables', icon: 'table', childTypes: ['table'] },
      views: { enabled: true, label: 'Views', icon: 'eye', childTypes: ['view'] },
      functions: { enabled: true, label: 'Functions', icon: 'function', childTypes: [] },
      procedures: { enabled: true, label: 'Procedures', icon: 'terminal', childTypes: [] },
      sequences: { enabled: true, label: 'Sequences', icon: 'hash', childTypes: [] },
      triggers: { enabled: true, label: 'Triggers', icon: 'zap', childTypes: [] },
    },
    tableChildren: {
      columns: true,
      indexes: true,
      constraints: true,
      triggers: false,
      foreignKeys: false,
      references: false,
    },
  }
}
