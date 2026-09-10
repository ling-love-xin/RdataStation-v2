/**
 * 配置系统类型定义与注册表
 *
 * ─── 三层优先级（单向 fallback）───
 *   Level 1: 项目覆盖值  (project-settings.json)
 *   Level 2: 全局默认值  (global-settings.json)
 *   Level 3: 系统硬编码   (DEFAULT_GLOBAL_CONFIG)
 * ─────────────────────────────
 *
 * ─── 新增配置项流程 ───
 *   1. 定义类型（Theme / Language 等）
 *   2. 在 CONFIG_REGISTRY 添加条目（含 key / default / writeType / rule）
 *   3. 追加到 GlobalConfig / ProjectConfig 接口
 *   4. 追加 zod inner schema + 注册到 VALUE_SCHEMAS
 *   5. ConfigKey / ConfigValueType / CONFIG_KEYS / OVERRIDE_RULES / SEED_KEYS 自动派生
 *
 * @module config
 * @version 1.1
 * @status Phase 1 complete / Phase 2 API ready
 */

import { z } from 'zod'

/** 主题枚举 */
type Theme = 'light' | 'dark' | 'system'

/** 语言枚举（对应 naive-ui locale + vue-i18n） */
type Language = 'zh-CN' | 'en'

/** 默认查询执行引擎 */
type DefaultEngine = 'native' | 'duckdb'

/** 编辑器设置（第 2 层嵌套，整体作为 JSON 值存取） */
interface EditorSettings {
  fontSize: number
  tabSize: number
  wordWrap: boolean
  minimap: boolean
  lineNumbers: boolean
  fontFamily: string
}

/** 连接池设置 */
interface ConnectionPoolSettings {
  maxConnections: number
  minIdleConnections: number
  connectionTimeout: number
  idleTimeout: number
  autoReconnect: boolean
  healthCheck: boolean
  healthCheckInterval: number
}

/** 历史记录设置 */
interface HistorySettings {
  maxHistoryItems: number
  retentionDays: number
  enableHistory: boolean
  includeSQL: boolean
  enableUndo: boolean
}

/** 监控设置 */
interface MonitoringSettings {
  enableMonitoring: boolean
  updateInterval: number
  enableAlerts: boolean
  alertOnDisconnect: boolean
  alertOnSlowQuery: boolean
  slowQueryThreshold: number
}

/** 性能设置 */
interface PerformanceSettings {
  virtualScrollBuffer: number
  maxCacheSize: number
  cacheExpireMinutes: number
  enableLazyLoad: boolean
  enablePreload: boolean
}

/** 外观密度枚举 */
type AppearanceDensity = 'compact' | 'comfortable' | 'spacious'

/** 外观设置 */
interface AppearanceSettings {
  uiFontSize: number
  compactMode: boolean
  accentColor: string | null
  fontFamily: string
  borderRadius: number
  density: AppearanceDensity
}

/** 结果面板设置 */
interface ResultSettings {
  pageSize: number
  defaultViewMode: 'grid' | 'text' | 'chart'
  nullDisplay: string
  dateFormat: string
}

/** 标题栏设置 */
interface TitleBarSettings {
  menuStyle: 'full' | 'compact' | 'hidden'
  toolbarTools: string[]
  showProjectSelector: boolean
  showCommandCenter: boolean
  recentProjectCount: number
}

/** 状态栏设置 */
interface StatusBarSettings {
  visible: boolean
  showConnectionStatus: boolean
  showExecutionTime: boolean
  showRowCount: boolean
  showDuckDBIndicator: boolean
  showEncoding: boolean
  showVersion: boolean
}

/** 命令面板设置 */
interface CommandPaletteSettings {
  maxRecentCommands: number
  includeDisabledCommands: boolean
}

/** dockview-vue 序列化面板状态 */
interface SerializedPanelState {
  id: string
  component: string
  params?: Record<string, string>
  title: string
}

/** dockview-vue 序列化布局 */
interface SerializedDockviewLayout {
  panels: SerializedPanelState[]
  activePanel: string | null
}

/** 侧边栏/布局序列化状态（项目级持久化） */
interface SerializedSidebarState {
  leftActivityBarVisible: boolean
  rightActivityBarVisible: boolean
  primarySideBarVisible: boolean
  secondarySideBarVisible: boolean
  panelVisible: boolean
  statusBarVisible: boolean
  primarySideBarExpanded: boolean
  secondarySideBarExpanded: boolean
  selectedLeftItem: string | null
  selectedRightItem: string | null
  primarySideBarWidth: number
  secondarySideBarWidth: number
  panelHeight: number
  bottomPanelMode: 'editor' | 'full'
  openPanelIds: string[]
  bottomInsightTab: 'multi' | 'history' | 'schema' | 'table'
}

/** 配置项可覆盖性规则 */
interface ConfigOverrideRule {
  /** Level 2 是否提供全局默认值 */
  globalDefault: boolean
  /** 是否允许项目覆盖 Level 2 值 */
  projectOverridable: boolean
  /** 是否仅项目级存在（不在 global-settings.json 中） */
  projectOnly: boolean
}

/** 全局配置对象（存储在 global-settings.json） */
interface GlobalConfig {
  theme: Theme
  language: Language
  editorSettings: EditorSettings
  defaultEngine: DefaultEngine
  recentProjects: string[]
  connectionPool: ConnectionPoolSettings
  historySettings: HistorySettings
  monitoringSettings: MonitoringSettings
  performanceSettings: PerformanceSettings
  appearanceSettings: AppearanceSettings
  resultSettings: ResultSettings
  titleBarSettings: TitleBarSettings
  statusBarSettings: StatusBarSettings
  commandPaletteSettings: CommandPaletteSettings
}

/** 项目配置对象（存储在 project-{id}-settings.json，所有字段可选） */
interface ProjectConfig {
  theme?: Theme
  editorSettings?: Partial<EditorSettings>
  defaultEngine?: DefaultEngine
  dockviewLayout?: SerializedDockviewLayout
  sidebarState?: SerializedSidebarState
  resultSettings?: Partial<ResultSettings>
  titleBarSettings?: Partial<TitleBarSettings>
  statusBarSettings?: Partial<StatusBarSettings>
  commandPaletteSettings?: Partial<CommandPaletteSettings>
  appearanceSettings?: Partial<AppearanceSettings>
}

// ============================================
// 原子 zod schemas（供组合 + 写时校验复用）
// ============================================

const ThemeSchema = z.enum(['light', 'dark', 'system'])
const LanguageSchema = z.enum(['zh-CN', 'en'])
const DefaultEngineSchema = z.enum(['native', 'duckdb'])

const EditorSettingsSchema = z.object({
  fontSize: z.number().min(10).max(24),
  tabSize: z.union([z.literal(2), z.literal(4), z.literal(8)]),
  wordWrap: z.boolean(),
  minimap: z.boolean(),
  lineNumbers: z.boolean(),
  fontFamily: z.string().min(1),
})

const SerializedPanelStateSchema = z.object({
  id: z.string(),
  component: z.string(),
  params: z.record(z.string(), z.string()).optional(),
  title: z.string(),
})

const DockviewLayoutSchema = z.object({
  panels: z.array(SerializedPanelStateSchema),
  activePanel: z.string().nullable(),
})

const SidebarStateSchema = z.object({
  leftActivityBarVisible: z.boolean().default(true),
  rightActivityBarVisible: z.boolean().default(true),
  primarySideBarVisible: z.boolean().default(true),
  secondarySideBarVisible: z.boolean().default(true),
  panelVisible: z.boolean().default(true),
  statusBarVisible: z.boolean().default(true),
  primarySideBarExpanded: z.boolean().default(true),
  secondarySideBarExpanded: z.boolean().default(true),
  selectedLeftItem: z.string().nullable().default('database'),
  selectedRightItem: z.string().nullable().default('column-insights'),
  primarySideBarWidth: z.number().min(200).max(600).default(300),
  secondarySideBarWidth: z.number().min(200).max(600).default(300),
  panelHeight: z.number().min(100).max(600).default(250),
  bottomPanelMode: z.enum(['editor', 'full']).default('editor'),
  openPanelIds: z.array(z.string()).default([]),
  bottomInsightTab: z.enum(['multi', 'history', 'schema', 'table']).default('multi'),
})

const RecentProjectsSchema = z.array(z.string()).max(10)

const ConnectionPoolSettingsSchema = z.object({
  maxConnections: z.number().min(1).max(100),
  minIdleConnections: z.number().min(0).max(50),
  connectionTimeout: z.number().min(1).max(300),
  idleTimeout: z.number().min(10).max(3600),
  autoReconnect: z.boolean(),
  healthCheck: z.boolean(),
  healthCheckInterval: z.number().min(10).max(300),
})

const HistorySettingsSchema = z.object({
  maxHistoryItems: z.number().min(10).max(1000),
  retentionDays: z.number().min(1).max(365),
  enableHistory: z.boolean(),
  includeSQL: z.boolean(),
  enableUndo: z.boolean(),
})

const MonitoringSettingsSchema = z.object({
  enableMonitoring: z.boolean(),
  updateInterval: z.number().min(1).max(60),
  enableAlerts: z.boolean(),
  alertOnDisconnect: z.boolean(),
  alertOnSlowQuery: z.boolean(),
  slowQueryThreshold: z.number().min(100).max(30000),
})

const PerformanceSettingsSchema = z.object({
  virtualScrollBuffer: z.number().min(1).max(20),
  maxCacheSize: z.number().min(10).max(500),
  cacheExpireMinutes: z.number().min(5).max(1440),
  enableLazyLoad: z.boolean(),
  enablePreload: z.boolean(),
})

const AppearanceSettingsSchema = z.object({
  uiFontSize: z.number().min(10).max(24),
  compactMode: z.boolean(),
  accentColor: z
    .string()
    .regex(/^#[0-9a-fA-F]{6}$/)
    .nullable(),
  fontFamily: z.string().min(1),
  borderRadius: z.number().min(4).max(12),
  density: z.enum(['compact', 'comfortable', 'spacious']),
})

const ResultSettingsSchema = z.object({
  pageSize: z.number().min(10).max(10000),
  defaultViewMode: z.enum(['grid', 'text', 'chart']),
  nullDisplay: z.string().max(20),
  dateFormat: z.string().max(32),
})

const TitleBarSettingsSchema = z.object({
  menuStyle: z.enum(['full', 'compact', 'hidden']),
  toolbarTools: z.array(z.string()),
  showProjectSelector: z.boolean(),
  showCommandCenter: z.boolean(),
  recentProjectCount: z.number().min(1).max(10),
})

const StatusBarSettingsSchema = z.object({
  visible: z.boolean(),
  showConnectionStatus: z.boolean(),
  showExecutionTime: z.boolean(),
  showRowCount: z.boolean(),
  showDuckDBIndicator: z.boolean(),
  showEncoding: z.boolean(),
  showVersion: z.boolean(),
})

const CommandPaletteSettingsSchema = z.object({
  maxRecentCommands: z.number().min(1).max(20),
  includeDisabledCommands: z.boolean(),
})

// ============================================
// 配置注册表 —— 单一事实来源
// ============================================

const CONFIG_REGISTRY = {
  theme: {
    key: 'theme' as const,
    default: 'dark' as Theme,
    writeType: 'dark' as Theme,
    valueSchema: ThemeSchema,
    rule: {
      globalDefault: true,
      projectOverridable: true,
      projectOnly: false,
    } satisfies ConfigOverrideRule,
  },
  language: {
    key: 'language' as const,
    default: 'zh-CN' as Language,
    writeType: 'zh-CN' as Language,
    valueSchema: LanguageSchema,
    rule: {
      globalDefault: true,
      projectOverridable: false,
      projectOnly: false,
    } satisfies ConfigOverrideRule,
  },
  editorSettings: {
    key: 'editorSettings' as const,
    default: {
      fontSize: 14,
      tabSize: 2,
      wordWrap: true,
      minimap: true,
      lineNumbers: true,
      fontFamily: "'Cascadia Code', 'Fira Code', 'Consolas', monospace",
    } satisfies EditorSettings,
    writeType: {} as EditorSettings | Partial<EditorSettings>,
    valueSchema: EditorSettingsSchema,
    rule: {
      globalDefault: true,
      projectOverridable: true,
      projectOnly: false,
    } satisfies ConfigOverrideRule,
  },
  defaultEngine: {
    key: 'defaultEngine' as const,
    default: 'native' as DefaultEngine,
    writeType: 'native' as DefaultEngine,
    valueSchema: DefaultEngineSchema,
    rule: {
      globalDefault: true,
      projectOverridable: true,
      projectOnly: false,
    } satisfies ConfigOverrideRule,
  },
  recentProjects: {
    key: 'recentProjects' as const,
    default: [] as string[],
    writeType: [] as string[],
    valueSchema: RecentProjectsSchema,
    rule: {
      globalDefault: true,
      projectOverridable: false,
      projectOnly: false,
    } satisfies ConfigOverrideRule,
  },
  dockviewLayout: {
    key: 'dockviewLayout' as const,
    default: { panels: [], activePanel: null } satisfies SerializedDockviewLayout,
    writeType: {} as SerializedDockviewLayout,
    valueSchema: DockviewLayoutSchema,
    rule: {
      globalDefault: false,
      projectOverridable: false,
      projectOnly: true,
    } satisfies ConfigOverrideRule,
  },
  sidebarState: {
    key: 'sidebarState' as const,
    default: {
      leftActivityBarVisible: true,
      rightActivityBarVisible: true,
      primarySideBarVisible: true,
      secondarySideBarVisible: true,
      panelVisible: true,
      statusBarVisible: true,
      primarySideBarExpanded: true,
      secondarySideBarExpanded: true,
      selectedLeftItem: 'database',
      selectedRightItem: 'column-insights',
      primarySideBarWidth: 300,
      secondarySideBarWidth: 300,
      panelHeight: 250,
      bottomPanelMode: 'editor',
      openPanelIds: [],
      bottomInsightTab: 'multi',
    } satisfies SerializedSidebarState,
    writeType: {} as SerializedSidebarState,
    valueSchema: SidebarStateSchema,
    rule: {
      globalDefault: false,
      projectOverridable: false,
      projectOnly: true,
    } satisfies ConfigOverrideRule,
  },
  connectionPool: {
    key: 'connectionPool' as const,
    default: {
      maxConnections: 10,
      minIdleConnections: 2,
      connectionTimeout: 30,
      idleTimeout: 300,
      autoReconnect: true,
      healthCheck: true,
      healthCheckInterval: 60,
    } satisfies ConnectionPoolSettings,
    writeType: {} as ConnectionPoolSettings,
    valueSchema: ConnectionPoolSettingsSchema,
    rule: {
      globalDefault: true,
      projectOverridable: false,
      projectOnly: false,
    } satisfies ConfigOverrideRule,
  },
  historySettings: {
    key: 'historySettings' as const,
    default: {
      maxHistoryItems: 100,
      retentionDays: 30,
      enableHistory: true,
      includeSQL: true,
      enableUndo: true,
    } satisfies HistorySettings,
    writeType: {} as HistorySettings,
    valueSchema: HistorySettingsSchema,
    rule: {
      globalDefault: true,
      projectOverridable: false,
      projectOnly: false,
    } satisfies ConfigOverrideRule,
  },
  monitoringSettings: {
    key: 'monitoringSettings' as const,
    default: {
      enableMonitoring: true,
      updateInterval: 5,
      enableAlerts: true,
      alertOnDisconnect: true,
      alertOnSlowQuery: true,
      slowQueryThreshold: 1000,
    } satisfies MonitoringSettings,
    writeType: {} as MonitoringSettings,
    valueSchema: MonitoringSettingsSchema,
    rule: {
      globalDefault: true,
      projectOverridable: false,
      projectOnly: false,
    } satisfies ConfigOverrideRule,
  },
  performanceSettings: {
    key: 'performanceSettings' as const,
    default: {
      virtualScrollBuffer: 5,
      maxCacheSize: 100,
      cacheExpireMinutes: 60,
      enableLazyLoad: true,
      enablePreload: true,
    } satisfies PerformanceSettings,
    writeType: {} as PerformanceSettings,
    valueSchema: PerformanceSettingsSchema,
    rule: {
      globalDefault: true,
      projectOverridable: false,
      projectOnly: false,
    } satisfies ConfigOverrideRule,
  },
  appearanceSettings: {
    key: 'appearanceSettings' as const,
    default: {
      uiFontSize: 13,
      compactMode: false,
      accentColor: null,
      fontFamily: "'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif",
      borderRadius: 6,
      density: 'comfortable' as AppearanceDensity,
    } satisfies AppearanceSettings,
    writeType: {} as AppearanceSettings | Partial<AppearanceSettings>,
    valueSchema: AppearanceSettingsSchema,
    rule: {
      globalDefault: true,
      projectOverridable: true,
      projectOnly: false,
    } satisfies ConfigOverrideRule,
  },
  resultSettings: {
    key: 'resultSettings' as const,
    default: {
      pageSize: 200,
      defaultViewMode: 'grid' as const,
      nullDisplay: 'NULL',
      dateFormat: 'YYYY-MM-DD HH:mm:ss',
    } satisfies ResultSettings,
    writeType: {} as ResultSettings,
    valueSchema: ResultSettingsSchema,
    rule: {
      globalDefault: true,
      projectOverridable: true,
      projectOnly: false,
    } satisfies ConfigOverrideRule,
  },
  titleBarSettings: {
    key: 'titleBarSettings' as const,
    default: {
      menuStyle: 'full' as const,
      toolbarTools: ['settings'],
      showProjectSelector: true,
      showCommandCenter: true,
      recentProjectCount: 5,
    } satisfies TitleBarSettings,
    writeType: {} as TitleBarSettings,
    valueSchema: TitleBarSettingsSchema,
    rule: {
      globalDefault: true,
      projectOverridable: true,
      projectOnly: false,
    } satisfies ConfigOverrideRule,
  },
  statusBarSettings: {
    key: 'statusBarSettings' as const,
    default: {
      visible: true,
      showConnectionStatus: true,
      showExecutionTime: true,
      showRowCount: true,
      showDuckDBIndicator: true,
      showEncoding: true,
      showVersion: true,
    } satisfies StatusBarSettings,
    writeType: {} as StatusBarSettings,
    valueSchema: StatusBarSettingsSchema,
    rule: {
      globalDefault: true,
      projectOverridable: true,
      projectOnly: false,
    } satisfies ConfigOverrideRule,
  },
  commandPaletteSettings: {
    key: 'commandPaletteSettings' as const,
    default: {
      maxRecentCommands: 5,
      includeDisabledCommands: false,
    } satisfies CommandPaletteSettings,
    writeType: {} as CommandPaletteSettings,
    valueSchema: CommandPaletteSettingsSchema,
    rule: {
      globalDefault: true,
      projectOverridable: false,
      projectOnly: false,
    } satisfies ConfigOverrideRule,
  },
}

/** 联合类型：所有合法配置键（从注册表自动派生） */
type ConfigKey = (typeof CONFIG_REGISTRY)[keyof typeof CONFIG_REGISTRY]['key']

/** 配置键→值类型映射（从注册表 writeType 自动派生，新增键无需改动此处） */
type ConfigValueType<K extends ConfigKey> = (typeof CONFIG_REGISTRY)[K]['writeType']

/** 配置键→zod schema 查找表 */
type ValueSchemaLookup = {
  [K in ConfigKey]?: z.ZodType<unknown>
}

const VALUE_SCHEMAS: ValueSchemaLookup = {}
for (const entry of Object.values(CONFIG_REGISTRY)) {
  VALUE_SCHEMAS[entry.key] = entry.valueSchema as z.ZodType<unknown>
}

/** 从注册表派生的键名常量集合 */
const CONFIG_KEYS = {
  THEME: CONFIG_REGISTRY.theme.key,
  LANGUAGE: CONFIG_REGISTRY.language.key,
  EDITOR_SETTINGS: CONFIG_REGISTRY.editorSettings.key,
  DEFAULT_ENGINE: CONFIG_REGISTRY.defaultEngine.key,
  RECENT_PROJECTS: CONFIG_REGISTRY.recentProjects.key,
  DOCKVIEW_LAYOUT: CONFIG_REGISTRY.dockviewLayout.key,
  SIDEBAR_STATE: CONFIG_REGISTRY.sidebarState.key,
  CONNECTION_POOL: CONFIG_REGISTRY.connectionPool.key,
  HISTORY_SETTINGS: CONFIG_REGISTRY.historySettings.key,
  MONITORING_SETTINGS: CONFIG_REGISTRY.monitoringSettings.key,
  PERFORMANCE_SETTINGS: CONFIG_REGISTRY.performanceSettings.key,
  APPEARANCE_SETTINGS: CONFIG_REGISTRY.appearanceSettings.key,
  RESULT_SETTINGS: CONFIG_REGISTRY.resultSettings.key,
  TITLE_BAR_SETTINGS: CONFIG_REGISTRY.titleBarSettings.key,
  STATUS_BAR_SETTINGS: CONFIG_REGISTRY.statusBarSettings.key,
  COMMAND_PALETTE_SETTINGS: CONFIG_REGISTRY.commandPaletteSettings.key,
}

/** 可覆盖性规则查找表：key → ConfigOverrideRule */
const OVERRIDE_RULES: Record<string, ConfigOverrideRule> = {}
for (const entry of Object.values(CONFIG_REGISTRY)) {
  OVERRIDE_RULES[entry.key] = entry.rule
}

/** 全局默认配置（Level 3 → seed 到 Level 2 首次启动） */
const DEFAULT_GLOBAL_CONFIG: GlobalConfig = {
  theme: CONFIG_REGISTRY.theme.default,
  language: CONFIG_REGISTRY.language.default,
  editorSettings: { ...CONFIG_REGISTRY.editorSettings.default },
  defaultEngine: CONFIG_REGISTRY.defaultEngine.default,
  recentProjects: [...CONFIG_REGISTRY.recentProjects.default],
  connectionPool: { ...CONFIG_REGISTRY.connectionPool.default },
  historySettings: { ...CONFIG_REGISTRY.historySettings.default },
  monitoringSettings: { ...CONFIG_REGISTRY.monitoringSettings.default },
  performanceSettings: { ...CONFIG_REGISTRY.performanceSettings.default },
  appearanceSettings: { ...CONFIG_REGISTRY.appearanceSettings.default },
  resultSettings: { ...CONFIG_REGISTRY.resultSettings.default },
  titleBarSettings: { ...CONFIG_REGISTRY.titleBarSettings.default },
  statusBarSettings: { ...CONFIG_REGISTRY.statusBarSettings.default },
  commandPaletteSettings: { ...CONFIG_REGISTRY.commandPaletteSettings.default },
}

/** 编辑器默认设置独立副本 */
const DEFAULT_EDITOR_SETTINGS: EditorSettings = { ...CONFIG_REGISTRY.editorSettings.default }

/** 配置 schema 主版本号（变更时追加迁移函数） */
const SCHEMA_VERSION = 1

/** tauri-plugin-store 文件名 */
const STORE_FILENAME_GLOBAL = 'global-settings.json'
const STORE_FILENAME_PROJECT = 'project-settings.json'

/** 最近项目列表最大条数 */
const RECENT_PROJECTS_MAX = 10

/** 侧边栏宽度限制 */
const SIDEBAR_WIDTH_MIN = 200
const SIDEBAR_WIDTH_MAX = 400

// ============================================
// 复合 zod schemas（供 loadStoreWithDefaults 校验）
// ============================================

const GlobalConfigSchema = z.object({
  _schemaVersion: z.number().optional(),
  theme: ThemeSchema,
  language: LanguageSchema,
  editorSettings: EditorSettingsSchema,
  defaultEngine: DefaultEngineSchema,
  recentProjects: RecentProjectsSchema,
  connectionPool: ConnectionPoolSettingsSchema,
  historySettings: HistorySettingsSchema,
  monitoringSettings: MonitoringSettingsSchema,
  performanceSettings: PerformanceSettingsSchema,
  appearanceSettings: AppearanceSettingsSchema,
  resultSettings: ResultSettingsSchema,
  titleBarSettings: TitleBarSettingsSchema,
  statusBarSettings: StatusBarSettingsSchema,
  commandPaletteSettings: CommandPaletteSettingsSchema,
})

const ProjectConfigSchema = z.object({
  theme: ThemeSchema.optional(),
  editorSettings: EditorSettingsSchema.partial().optional(),
  defaultEngine: DefaultEngineSchema.optional(),
  dockviewLayout: DockviewLayoutSchema.optional(),
  sidebarState: SidebarStateSchema.optional(),
  resultSettings: ResultSettingsSchema.partial().optional(),
  titleBarSettings: TitleBarSettingsSchema.partial().optional(),
  statusBarSettings: StatusBarSettingsSchema.partial().optional(),
  commandPaletteSettings: CommandPaletteSettingsSchema.partial().optional(),
  appearanceSettings: AppearanceSettingsSchema.partial().optional(),
})

// ============================================
// Schema 版本迁移
// ============================================

type MigrationFn = (data: Record<string, unknown>) => Record<string, unknown>

/**
 * 迁移函数注册表
 *
 * key: fromVersion（迁移前版本号）
 * value: 将数据迁移到 key+1 版本的纯函数
 *
 * @example
 * // SCHEMA_VERSION 从 1 升到 2 时：
 * // 1: (data) => {
 * //   // v2: 'locale' → 'language'
 * //   if ('locale' in data) {
 * //     data.language = data.locale
 * //     delete data.locale
 * //   }
 * //   return data
 * // }
 */
const MIGRATIONS: Record<number, MigrationFn> = {}

function migrateConfig(
  fromVersion: number,
  data: Record<string, unknown>
): Record<string, unknown> {
  let current = fromVersion
  let result = { ...data }

  while (current < SCHEMA_VERSION) {
    const migration = MIGRATIONS[current]
    if (migration) {
      result = migration(result)
      // eslint-disable-next-line no-console
      console.info(`[config] Migrated schema ${current} → ${current + 1}`)
    }
    current++
  }

  return result
}

/** 种子条目（initialize 驱动循环用） */
interface SeedEntry {
  key: ConfigKey
  default: unknown
}

/** 全局级种子键（globalDefault === true） */
const GLOBAL_SEED_KEYS: SeedEntry[] = Object.values(CONFIG_REGISTRY)
  .filter(e => e.rule.globalDefault)
  .map(e => ({ key: e.key, default: e.default }))

/** 项目级种子键（projectOnly || projectOverridable） */
const PROJECT_SEED_KEYS: SeedEntry[] = Object.values(CONFIG_REGISTRY)
  .filter(e => e.rule.projectOnly || e.rule.projectOverridable)
  .map(e => ({ key: e.key, default: e.default }))

export {
  CONFIG_KEYS,
  CONFIG_REGISTRY,
  OVERRIDE_RULES,
  DEFAULT_GLOBAL_CONFIG,
  DEFAULT_EDITOR_SETTINGS,
  SCHEMA_VERSION,
  STORE_FILENAME_GLOBAL,
  STORE_FILENAME_PROJECT,
  GLOBAL_SEED_KEYS,
  PROJECT_SEED_KEYS,
  RECENT_PROJECTS_MAX,
  SIDEBAR_WIDTH_MIN,
  SIDEBAR_WIDTH_MAX,
  VALUE_SCHEMAS,
  EditorSettingsSchema,
  ConnectionPoolSettingsSchema,
  HistorySettingsSchema,
  MonitoringSettingsSchema,
  PerformanceSettingsSchema,
  AppearanceSettingsSchema,
  ResultSettingsSchema,
  TitleBarSettingsSchema,
  StatusBarSettingsSchema,
  CommandPaletteSettingsSchema,
  GlobalConfigSchema,
  ProjectConfigSchema,
  migrateConfig,
}

export type {
  ConfigKey,
  ConfigValueType,
  Theme,
  Language,
  DefaultEngine,
  EditorSettings,
  ConnectionPoolSettings,
  HistorySettings,
  MonitoringSettings,
  PerformanceSettings,
  AppearanceSettings,
  AppearanceDensity,
  ResultSettings,
  TitleBarSettings,
  StatusBarSettings,
  CommandPaletteSettings,
  GlobalConfig,
  ProjectConfig,
  SerializedDockviewLayout,
  SerializedSidebarState,
}
