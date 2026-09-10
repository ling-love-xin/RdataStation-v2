import { tauriInvoke } from './index'

// ==================== Mock 类型定义 ====================

export type ColumnDataType =
  | 'integer'
  | 'bigint'
  | 'float'
  | 'double'
  | 'decimal'
  | 'boolean'
  | 'varchar'
  | 'text'
  | 'date'
  | 'datetime'
  | 'timestamp'
  | 'uuid'
  | 'blob'

export type GeneratorType =
  | 'auto_increment'
  | 'random_int'
  | 'random_float'
  | 'random_decimal'
  | 'normal'
  | 'log_normal'
  | 'random_walk'
  | 'digit'
  | 'number_with_format'
  | 'boolean'
  | 'constant'
  | 'words'
  | 'word'
  | 'sentence'
  | 'sentences'
  | 'paragraph'
  | 'paragraphs'
  | 'regex'
  | 'template'
  | 'md_italic'
  | 'md_bold'
  | 'md_link'
  | 'md_bullet'
  | 'md_list'
  | 'md_blockquote_single'
  | 'md_blockquote_multi'
  | 'md_code'
  | 'name'
  | 'name_with_title'
  | 'first_name'
  | 'last_name'
  | 'title'
  | 'suffix'
  | 'email'
  | 'safe_email'
  | 'free_email_provider'
  | 'domain_suffix'
  | 'free_email'
  | 'username'
  | 'password'
  | 'phone_number'
  | 'cell_number'
  | 'country'
  | 'country_code'
  | 'country_name'
  | 'city'
  | 'city_prefix'
  | 'city_suffix'
  | 'state_name'
  | 'state_abbr'
  | 'street_name'
  | 'street_suffix'
  | 'zip_code'
  | 'post_code'
  | 'building_number'
  | 'secondary_address'
  | 'secondary_address_type'
  | 'latitude'
  | 'longitude'
  | 'geohash'
  | 'timezone'
  | 'ip_address'
  | 'ipv4'
  | 'ipv6'
  | 'ip'
  | 'mac_address'
  | 'datetime'
  | 'datetime_before'
  | 'datetime_after'
  | 'datetime_between'
  | 'sequential_date'
  | 'sequential_date_with_gaps'
  | 'date'
  | 'time'
  | 'duration'
  | 'company_name'
  | 'company_suffix'
  | 'job_title'
  | 'profession'
  | 'industry'
  | 'seniority'
  | 'field'
  | 'position'
  | 'buzzword'
  | 'buzzword_middle'
  | 'buzzword_tail'
  | 'catch_phrase'
  | 'bs_verb'
  | 'bs_adj'
  | 'bs_noun'
  | 'bs'
  | 'currency_code'
  | 'currency_name'
  | 'currency_symbol'
  | 'bic'
  | 'isin'
  | 'credit_card_number'
  | 'uuid_v1'
  | 'uuid_v3'
  | 'uuid_v4'
  | 'uuid_v5'
  | 'url'
  | 'user_agent'
  | 'mime_type'
  | 'semver'
  | 'semver_stable'
  | 'semver_unstable'
  | 'file_path'
  | 'file_name'
  | 'file_extension'
  | 'dir_path'
  | 'image_url'
  | 'image_url_with_seed'
  | 'image_url_grayscale'
  | 'image_url_blur'
  | 'image_url_custom'
  | 'hex_color'
  | 'rgb_color'
  | 'rgba_color'
  | 'hsl_color'
  | 'hsla_color'
  | 'color'
  | 'ferroid_ulid'
  | 'ferroid_twitter_id'
  | 'ferroid_instagram_id'
  | 'ferroid_mastodon_id'
  | 'ferroid_discord_id'
  | 'isbn'
  | 'isbn10'
  | 'isbn13'
  | 'rfc_status'
  | 'valid_status'
  | 'licence_plate'
  | 'health_insurance'
  | 'foreign_key'
  | 'sequence'
  | 'weighted'

export interface GeneratorConfig {
  type: GeneratorType
  params?: Record<string, unknown>
}

export interface ColumnDef {
  name: string
  dataType: ColumnDataType
  generator: GeneratorConfig
  nullableRatio: number
  unique: boolean
  varcharLength?: number
  decimalPrecision?: number
  decimalScale?: number
}

export type Locale =
  | 'ZH_CN'
  | 'EN'
  | 'JA_JP'
  | 'ZH_TW'
  | 'FR_FR'
  | 'DE_DE'
  | 'IT_IT'
  | 'PT_BR'
  | 'PT_PT'
  | 'NL_NL'
  | 'AR_SA'
  | 'TR_TR'
  | 'FA_IR'

export interface MockConfig {
  tableName: string
  rowCount: number
  seed: number | null
  locale: Locale
  columns: ColumnDef[]
}

export interface QueryResultPreview {
  columns: string[]
  rows: unknown[][]
  affected_rows: number
  is_read_only: boolean
  total_rows: number
}

export interface MockGenerateResult {
  tableName: string
  tempTableName: string
  rowCount: number
  preview: QueryResultPreview
  columns: string[]
  elapsedMs: number
}

export interface ColumnMappingResponse {
  columnName: string
  generator: GeneratorConfig
  confidence: string
  sampleValue: string
}

export type MockExportFormat = 'Csv' | 'Parquet' | 'Xlsx' | 'Table' | 'SqlInsert'

export interface MockExportInput {
  tempTableName: string
  format: MockExportFormat
  outputPath?: string
  tableName?: string
}

export interface TemplateTableRemote {
  name: string
  rowCount: number
  columns: ColumnDef[]
}

export interface ScenarioTemplate {
  id: string
  name: string
  description: string
  category: string
  locale: string
  tables: TemplateTableRemote[]
}

export interface MockPersistAssetResult {
  tableName: string
  rowCount: number
  columnCount: number
}

export interface MockScenarioTableResult {
  tableName: string
  tempTableName: string
  rowCount: number
  columns: string[]
  elapsedMs: number
}

export interface MockScenarioResult {
  templateId: string
  templateName: string
  tableCount: number
  totalRows: number
  totalElapsedMs: number
  tables: MockScenarioTableResult[]
}

// ==================== 列依赖模型 ====================

export type DependencyType =
  | 'Expression'
  | 'ForeignKey'
  | 'Template'
  | 'Sequence'
  | 'Weighted'

export interface ColumnDependency {
  depType: DependencyType
  sourceColumns: string[]
  expression: string | null
  refTable: string | null
  refColumn: string | null
  weights: [string, number][] | null
}

export interface DependencyConfig {
  sortedColumns: string[]
  dependencies: Record<string, ColumnDependency>
}

// ==================== 持久化层类型 ====================

export interface MockGenerationTask {
  id: string
  tableName: string
  tableAlias: string | null
  rowCount: number
  seed: number | null
  locale: string
  sceneId: string | null
  saveFormat: string | null
  status: string
  errorMessage: string | null
  generatedRows: number | null
  generationTimeMs: number | null
  createdAt: string | null
  updatedAt: string | null
}

export interface MockGenerationColumn {
  id: string
  taskId: string
  columnName: string
  columnType: string
  generator: string
  generatorParams: string | null
  nullRatio: number
  isUnique: boolean
  isPrimaryKey: boolean
  isForeignKey: boolean
  refTable: string | null
  refColumn: string | null
  comment: string | null
  confidence: string | null
  sortOrder: number
}

export interface MockColumnInput {
  id: string
  columnName: string
  columnType: string
  generator: string
  generatorParams: string | null
  nullRatio: number
  isUnique: boolean
  isPrimaryKey: boolean
  isForeignKey: boolean
  refTable: string | null
  refColumn: string | null
  comment: string | null
  confidence: string | null
  sortOrder: number
}

export interface MockUserTemplate {
  id: string
  name: string
  description: string | null
  rowCount: number
  seed: number | null
  locale: string
  createdAt: string | null
  updatedAt: string | null
}

export interface MockTemplateColumn {
  id: string
  templateId: string
  columnName: string
  columnType: string
  generator: string
  generatorParams: string | null
  nullRatio: number
  isUnique: boolean
  isPrimaryKey: boolean
  isForeignKey: boolean
  refTable: string | null
  refColumn: string | null
  comment: string | null
  confidence: string | null
  sortOrder: number
}

// ==================== 前端 → 后端外部标签枚举格式转换 ====================

/**
 * 后端使用 serde 外部标签枚举（externally tagged enum）：
 *   ColumnDataType::Integer → { "integer": {} }
 *   GeneratorConfig::RandomInt { min: 1, max: 100 } → { "randomInt": { "min": 1, "max": 100 } }
 *
 * 前端内部使用扁平格式方便 UI 操作：
 *   dataType: 'integer'
 *   generator: { type: 'random_int', params: { min: 1, max: 100 } }
 *
 * 以下函数在 API 边界做转换，后端不动一行。
 */

/** snake_case → camelCase（仅处理下划线边界，不含下划线的单词不做转换） */
function snakeToCamel(s: string): string {
  return s.replace(/_([a-z0-9])/g, (_m: string, c: string) => c.toUpperCase())
}

/** 生成器命名覆盖：前端 snake_case 与后端 serde camelCase 不一致的 15 个变体 */
const OVERRIDE_VARIANT: Record<string, string> = {
  md_italic: 'markdownItalicWord',
  md_bold: 'markdownBoldWord',
  md_link: 'markdownLink',
  md_bullet: 'markdownBulletPoints',
  md_list: 'markdownListItems',
  md_blockquote_single: 'markdownBlockQuoteSingle',
  md_blockquote_multi: 'markdownBlockQuoteMulti',
  md_code: 'markdownCode',
  rfc_status: 'rfcStatusCode',
  valid_status: 'validStatusCode',
  datetime_before: 'dateTimeBefore',
  datetime_after: 'dateTimeAfter',
  datetime_between: 'dateTimeBetween',
  timezone: 'timeZone',
  health_insurance: 'healthInsuranceCode',
}

function backendVariantName(type: GeneratorType): string {
  return OVERRIDE_VARIANT[type] ?? snakeToCamel(type)
}

/** ColumnDataType 命名覆盖（不含下划线，snakeToCamel 不出力） */
const DT_VARIANT_MAP: Record<string, string> = {
  bigint: 'bigInt',
  datetime: 'dateTime',
}

function toBackendDataType(dt: ColumnDataType, col?: ColumnDef): Record<string, unknown> {
  const camel = DT_VARIANT_MAP[dt] ?? snakeToCamel(dt)
  if (dt === 'decimal')
    return { [camel]: { precision: col?.decimalPrecision ?? 18, scale: col?.decimalScale ?? 2 } }
  if (dt === 'varchar') return { [camel]: { length: col?.varcharLength ?? 255 } }
  return { [camel]: {} }
}

function toBackendGenerator(g: GeneratorConfig): Record<string, unknown> {
  return { [backendVariantName(g.type)]: g.params ?? {} }
}

function toBackendColumn(c: ColumnDef): Record<string, unknown> {
  return {
    name: c.name,
    dataType: toBackendDataType(c.dataType, c),
    generator: toBackendGenerator(c.generator),
    nullableRatio: c.nullableRatio,
    unique: c.unique,
  }
}

function toBackendConfig(config: MockConfig): Record<string, unknown> {
  return {
    tableName: config.tableName,
    rowCount: config.rowCount,
    seed: config.seed,
    locale: config.locale,
    columns: config.columns.map(toBackendColumn),
  }
}

// ==================== 后端 → 前端扁平格式转换（读取方向） ====================

/** camelCase → snake_case */
function camelToSnake(s: string): string {
  return s.replace(/([A-Z])/g, '_$1').toLowerCase()
}

/** 生成器反向映射：后端变体名 → 前端 GeneratorType */
const VARIANT_TO_TYPE: Record<string, string> = {}
for (const [t, v] of Object.entries(OVERRIDE_VARIANT)) {
  VARIANT_TO_TYPE[v] = t
}
/** ColumnDataType 反向映射 */
const DT_VARIANT_TO_FRONTEND: Record<string, string> = {
  bigInt: 'bigint',
  dateTime: 'datetime',
}

function parseBackendGenerator(raw: Record<string, unknown>): GeneratorConfig {
  const keys = Object.keys(raw)
  if (keys.length === 0) return { type: 'constant' as GeneratorType, params: {} }
  const variantKey = keys[0]
  const type = (VARIANT_TO_TYPE[variantKey] ?? camelToSnake(variantKey)) as GeneratorType
  return { type, params: (raw[variantKey] as Record<string, unknown>) ?? {} }
}

function parseBackendDataType(raw: Record<string, unknown>): {
  dataType: ColumnDataType
  varcharLength?: number
  decimalPrecision?: number
  decimalScale?: number
} {
  const keys = Object.keys(raw)
  const variantKey = keys[0] ?? ''
  const dt = (DT_VARIANT_TO_FRONTEND[variantKey] ?? camelToSnake(variantKey)) as ColumnDataType
  const body = (raw[variantKey] ?? {}) as Record<string, unknown>
  return {
    dataType: dt,
    varcharLength: dt === 'varchar' ? (body.length as number | undefined) : undefined,
    decimalPrecision: dt === 'decimal' ? (body.precision as number | undefined) : undefined,
    decimalScale: dt === 'decimal' ? (body.scale as number | undefined) : undefined,
  }
}

function parseBackendColumn(raw: Record<string, unknown>): ColumnDef {
  const dtParsed = parseBackendDataType((raw.dataType ?? {}) as Record<string, unknown>)
  return {
    name: (raw.name ?? '') as string,
    dataType: dtParsed.dataType,
    generator: parseBackendGenerator((raw.generator ?? {}) as Record<string, unknown>),
    nullableRatio: (raw.nullableRatio ?? 0) as number,
    unique: (raw.unique ?? false) as boolean,
    varcharLength: dtParsed.varcharLength,
    decimalPrecision: dtParsed.decimalPrecision,
    decimalScale: dtParsed.decimalScale,
  }
}

function parseBackendColumns(raw: Record<string, unknown>[]): ColumnDef[] {
  return raw.map(parseBackendColumn)
}

// ==================== Mock 相关 API ====================

export const mockApi = {
  generate(config: MockConfig) {
    return tauriInvoke<MockGenerateResult>('mock_generate', { config: toBackendConfig(config) })
  },

  preview(tableName: string, limit: number) {
    return tauriInvoke<QueryResultPreview>('mock_preview', { tableName, limit })
  },

  exportData(input: MockExportInput) {
    return tauriInvoke<string>('mock_export', { input })
  },

  mapColumn(columnName: string, dataType: string) {
    return tauriInvoke<ColumnMappingResponse>('mock_map_column', { columnName, dataType })
  },

  mapColumnsBatch(columns: Array<[string, string]>) {
    return tauriInvoke<ColumnMappingResponse[]>('mock_map_columns_batch', { columns })
  },

  listTemplates() {
    return tauriInvoke<ScenarioTemplate[]>('mock_list_templates')
  },

  async applyTemplate(templateId: string) {
    const raw = await tauriInvoke<Record<string, unknown>>('mock_apply_template', { templateId })
    const tables = ((raw.tables ?? []) as Record<string, unknown>[]).map(
      (t: Record<string, unknown>) => ({
        ...t,
        columns: parseBackendColumns((t.columns ?? []) as Record<string, unknown>[]),
      })
    )
    return { ...raw, tables } as unknown as ScenarioTemplate
  },

  async importSchema(input: {
    connId: string
    database: string
    schema?: string
    tables: string[]
    connectionType?: string
    projectPath?: string
  }) {
    const raw = await tauriInvoke<Record<string, unknown>[]>('mock_import_schema', { input })
    return parseBackendColumns(raw)
  },

  saveToScratchpad(input: { tempTableName: string; format: MockExportFormat }) {
    return tauriInvoke<string>('mock_save_to_scratchpad', { input })
  },

  persistAsAsset(input: { tempTableName: string; name: string }) {
    return tauriInvoke<MockPersistAssetResult>('mock_persist_as_asset', { input })
  },

  cancelGenerate() {
    return tauriInvoke<void>('mock_cancel_generate')
  },

  generateScenario(templateId: string) {
    return tauriInvoke<MockScenarioResult>('mock_generate_scenario', { templateId })
  },

  // ==================== 持久化 API ====================

  saveTask(projectPath: string, task: MockGenerationTask, columns: MockColumnInput[]) {
    return tauriInvoke<string>('save_mock_generation_task', {
      projectPath,
      task,
      columns: columns.map(c => ({
        id: c.id,
        taskId: task.id,
        columnName: c.columnName,
        columnType: c.columnType,
        generator: c.generator,
        generatorParams: c.generatorParams ?? null,
        nullRatio: c.nullRatio ?? 0,
        isUnique: c.isUnique ?? false,
        isPrimaryKey: c.isPrimaryKey ?? false,
        isForeignKey: c.isForeignKey ?? false,
        refTable: c.refTable ?? null,
        refColumn: c.refColumn ?? null,
        comment: c.comment ?? null,
        confidence: c.confidence ?? null,
        sortOrder: c.sortOrder,
      })),
    })
  },

  getHistoryV2(projectPath: string, limit = 20) {
    return tauriInvoke<MockGenerationTask[]>('get_mock_generation_history', {
      projectPath,
      limit,
    })
  },

  getDetail(projectPath: string, taskId: string) {
    return tauriInvoke<{
      task: MockGenerationTask
      columns: MockGenerationColumn[]
    }>('get_mock_generation_detail', { projectPath, taskId })
  },

  deleteTask(projectPath: string, taskId: string) {
    return tauriInvoke<void>('delete_mock_generation_task', { projectPath, taskId })
  },

  deleteTemplate(projectPath: string, templateId: string) {
    return tauriInvoke<void>('delete_mock_template', { projectPath, templateId })
  },

  // ==================== 模板持久化 API ====================

  saveTemplate(projectPath: string, template: MockUserTemplate, columns: MockTemplateColumn[]) {
    return tauriInvoke<string>('save_mock_template', {
      projectPath,
      template,
      columns: columns.map(c => ({
        id: c.id,
        templateId: template.id,
        columnName: c.columnName,
        columnType: c.columnType,
        generator: c.generator,
        generatorParams: c.generatorParams ?? null,
        nullRatio: c.nullRatio ?? 0,
        isUnique: c.isUnique ?? false,
        isPrimaryKey: c.isPrimaryKey ?? false,
        isForeignKey: c.isForeignKey ?? false,
        refTable: c.refTable ?? null,
        refColumn: c.refColumn ?? null,
        comment: c.comment ?? null,
        confidence: c.confidence ?? null,
        sortOrder: c.sortOrder,
      })),
    })
  },

  getTemplates(projectPath: string) {
    return tauriInvoke<MockUserTemplate[]>('get_mock_templates', { projectPath })
  },

  getTemplateDetail(projectPath: string, templateId: string) {
    return tauriInvoke<[MockUserTemplate, MockTemplateColumn[]]>('get_mock_template_detail', {
      projectPath,
      templateId,
    })
  },
}
