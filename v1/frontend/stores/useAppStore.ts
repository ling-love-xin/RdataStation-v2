/**
 * 应用配置 Pinia Store —— 单一配置数据入口
 *
 * ─── 数据流 ───
 *   Component → setTheme(...) → saveConfig(key, value, scope)
 *            → tauri-plugin-store (JSON 文件)
 *            → 内存同步 → Vue 响应式 → 组件更新
 *
 * ─── 迁移路径 ───
 *   当前: tauri-plugin-store Store.load/set/save
 *   目标: invoke('set_config', { key, value, scope }) → Rust SQLite
 *   改动: 仅 modify saveConfig / initialize / openProject 内部
 *
 * @module useAppStore
 * @version 1.5
 */

import { Store } from '@tauri-apps/plugin-store'
import { defineStore } from 'pinia'
import { ref, computed, shallowRef, watch, readonly } from 'vue'

import {
  CONFIG_KEYS,
  OVERRIDE_RULES,
  DEFAULT_GLOBAL_CONFIG,
  DEFAULT_EDITOR_SETTINGS,
  SCHEMA_VERSION,
  STORE_FILENAME_GLOBAL,
  STORE_FILENAME_PROJECT,
  GLOBAL_SEED_KEYS,
  PROJECT_SEED_KEYS,
  RECENT_PROJECTS_MAX,
  VALUE_SCHEMAS,
  EditorSettingsSchema,
  GlobalConfigSchema,
  ProjectConfigSchema,
  migrateConfig,
} from './config'

import type {
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
  ResultSettings,
  TitleBarSettings,
  StatusBarSettings,
  CommandPaletteSettings,
  GlobalConfig,
  ProjectConfig,
  SerializedDockviewLayout,
  SerializedSidebarState,
} from './config'

type ConfigScope = 'global' | 'project'

interface SaveResult {
  success: boolean
  key: ConfigKey
  scope: ConfigScope
  error?: string
}

interface BatchSaveEntry {
  key: ConfigKey
  value: unknown
  scope: ConfigScope
}

type ConfigChangeHandler = (
  key: ConfigKey,
  newValue: unknown,
  oldValue: unknown,
  scope: ConfigScope
) => void

function buildProjectStoreFilename(projectPath: string): string {
  const normalized = projectPath.replace(/[/\\:]/g, '_')
  return `project-${normalized}-${STORE_FILENAME_PROJECT}`
}

function safeErrorMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

/** 计算对象 T 中与 base 不同的字段 (deep diff) */
function computeDiff<T extends Record<string, unknown>>(candidate: T, base: T): Partial<T> {
  const diff: Partial<T> = {}
  for (const key of Object.keys(candidate) as (keyof T)[]) {
    const cv = candidate[key]
    const bv = base[key]
    if (typeof cv === 'object' && cv !== null && typeof bv === 'object' && bv !== null) {
      const subDiff = computeDiff(cv as Record<string, unknown>, bv as Record<string, unknown>)
      if (Object.keys(subDiff).length > 0) {
        ;(diff as Record<string, unknown>)[key as string] = subDiff
      }
    } else if (cv !== bv) {
      ;(diff as Record<string, unknown>)[key as string] = cv
    }
  }
  return diff
}

/**
 * 通用 Store 加载 + 逐字段校验 + seed 辅助函数
 *
 * 对每个种子键：
 *   - 先从 store 读已有值
 *   - 用单个 zod 子 schema 校验（不因一个字段损坏丢弃全局）
 *   - 校验失败 / 不存在 → seed 默认值
 */
async function loadStoreWithDefaults(
  store: Store,
  seedKeys: readonly { key: string; default: unknown }[],
  fullSchema: typeof GlobalConfigSchema.shape,
  target: Record<string, unknown>,
  scope: ConfigScope
): Promise<boolean> {
  let needsSave = false

  for (const { key, default: defaultVal } of seedKeys) {
    const stored = await store.get<unknown>(key)

    if (stored === null || stored === undefined) {
      await store.set(key, defaultVal)
      target[key] = defaultVal
      needsSave = true
      continue
    }

    const subValidator = (fullSchema as Record<string, unknown>)[key]
    if (subValidator && typeof subValidator === 'object' && 'safeParse' in subValidator) {
      const subResult = (
        subValidator as { safeParse: (v: unknown) => { success: boolean } }
      ).safeParse(stored)
      if (subResult.success) {
        target[key] = stored
      } else {
        console.warn(`[useAppStore] Validation failed for ${scope}.${key}, reseeding`)
        await store.set(key, defaultVal)
        target[key] = defaultVal
        needsSave = true
      }
    } else {
      target[key] = stored
    }
  }

  return needsSave
}

export const useAppStore = defineStore('appConfig', () => {
  const globalConfig = ref<GlobalConfig>(structuredClone(DEFAULT_GLOBAL_CONFIG))
  const projectConfig = ref<ProjectConfig>({})
  const initialized = ref(false)
  const initError = ref<string | null>(null)
  const projectOpen = ref(false)
  const projectPath = ref<string | null>(null)

  const globalStoreRef = shallowRef<Store | null>(null)
  const projectStoreRef = shallowRef<Store | null>(null)

  let onConfigChanged: ConfigChangeHandler | null = null

  const systemDark = ref(
    typeof window !== 'undefined'
      ? window.matchMedia('(prefers-color-scheme: dark)').matches
      : false
  )

  const effectiveTheme = computed<Theme>(() => {
    if (projectConfig.value.theme !== undefined) {
      return projectConfig.value.theme
    }
    return globalConfig.value.theme
  })

  const isDark = computed(() => {
    const t = effectiveTheme.value
    if (t === 'system') {
      return systemDark.value
    }
    return t === 'dark'
  })

  const effectiveLanguage = computed<Language>(() => globalConfig.value.language)

  const effectiveEditorSettings = computed<EditorSettings>(() => {
    const base = structuredClone(DEFAULT_EDITOR_SETTINGS)
    Object.assign(base, globalConfig.value.editorSettings)
    if (projectConfig.value.editorSettings) {
      Object.assign(base, projectConfig.value.editorSettings)
    }
    return base
  })

  const effectiveDefaultEngine = computed<DefaultEngine>(() => {
    if (projectConfig.value.defaultEngine !== undefined) {
      return projectConfig.value.defaultEngine
    }
    return globalConfig.value.defaultEngine
  })

  const recentProjects = computed(() => globalConfig.value.recentProjects)

  const effectiveDockviewLayout = computed<SerializedDockviewLayout | undefined>(
    () => projectConfig.value.dockviewLayout
  )

  const effectiveSidebarState = computed<SerializedSidebarState | undefined>(
    () => projectConfig.value.sidebarState
  )

  const effectiveConnectionPool = computed<ConnectionPoolSettings>(
    () => globalConfig.value.connectionPool
  )

  const effectiveHistorySettings = computed<HistorySettings>(
    () => globalConfig.value.historySettings
  )

  const effectiveMonitoringSettings = computed<MonitoringSettings>(
    () => globalConfig.value.monitoringSettings
  )

  const effectivePerformanceSettings = computed<PerformanceSettings>(
    () => globalConfig.value.performanceSettings
  )

  const effectiveAppearanceSettings = computed<AppearanceSettings>(() => {
    const base = structuredClone(DEFAULT_GLOBAL_CONFIG.appearanceSettings)
    Object.assign(base, globalConfig.value.appearanceSettings)
    if (projectConfig.value.appearanceSettings) {
      Object.assign(base, projectConfig.value.appearanceSettings)
    }
    return base
  })

  const effectiveResultSettings = computed<ResultSettings>(() => {
    const base = globalConfig.value.resultSettings
    const projectOverride = projectConfig.value.resultSettings
    if (projectOverride) {
      return { ...base, ...projectOverride }
    }
    return base
  })

  const effectiveTitleBarSettings = computed<TitleBarSettings>(() => {
    const base = structuredClone(DEFAULT_GLOBAL_CONFIG.titleBarSettings)
    Object.assign(base, globalConfig.value.titleBarSettings)
    if (projectConfig.value.titleBarSettings) {
      Object.assign(base, projectConfig.value.titleBarSettings)
    }
    return base
  })

  const effectiveStatusBarSettings = computed<StatusBarSettings>(() => {
    const base = structuredClone(DEFAULT_GLOBAL_CONFIG.statusBarSettings)
    Object.assign(base, globalConfig.value.statusBarSettings)
    if (projectConfig.value.statusBarSettings) {
      Object.assign(base, projectConfig.value.statusBarSettings)
    }
    return base
  })

  const effectiveCommandPaletteSettings = computed<CommandPaletteSettings>(() => {
    const base = structuredClone(DEFAULT_GLOBAL_CONFIG.commandPaletteSettings)
    Object.assign(base, globalConfig.value.commandPaletteSettings)
    if (projectConfig.value.commandPaletteSettings) {
      Object.assign(base, projectConfig.value.commandPaletteSettings)
    }
    return base
  })

  function getConfig<T>(key: ConfigKey): T {
    return getConfigInternal(key) as T
  }

  function getConfigInternal(key: string): unknown {
    const rawGlobal = globalConfig.value as Record<string, unknown>
    const rawProject = projectConfig.value as Record<string, unknown>
    const rule = OVERRIDE_RULES[key]

    if (!rule) {
      return undefined
    }

    if (rule.projectOnly) {
      return rawProject[key]
    }

    if (!rule.projectOverridable) {
      const gv = rawGlobal[key]
      return gv !== undefined
        ? gv
        : (DEFAULT_GLOBAL_CONFIG as unknown as Record<string, unknown>)[key]
    }

    const pv = rawProject[key]
    if (pv !== undefined) {
      return pv
    }

    const gv = rawGlobal[key]
    if (gv !== undefined) {
      return gv
    }

    return (DEFAULT_GLOBAL_CONFIG as unknown as Record<string, unknown>)[key]
  }

  async function saveConfig<K extends ConfigKey>(
    key: K,
    value: ConfigValueType<K>,
    scope: ConfigScope
  ): Promise<SaveResult> {
    const store = scope === 'global' ? globalStoreRef.value : projectStoreRef.value
    if (!store) {
      return { success: false, key, scope, error: `${scope} store not initialized` }
    }

    const rawTarget =
      scope === 'global'
        ? (globalConfig.value as Record<string, unknown>)
        : (projectConfig.value as Record<string, unknown>)

    const prevValue = rawTarget[key]

    const schema = VALUE_SCHEMAS[key]
    if (schema) {
      const validator =
        key === 'editorSettings' && scope === 'project' ? EditorSettingsSchema.partial() : schema
      const vr = validator.safeParse(value)
      if (!vr.success) {
        return {
          success: false,
          key,
          scope,
          error: `validation: ${vr.error.issues[0]?.message ?? 'invalid value'}`,
        }
      }
    }

    try {
      await store.set(key, value)
      await store.save()
      rawTarget[key] = value

      onConfigChanged?.(key, value, prevValue, scope)
      return { success: true, key, scope }
    } catch (e) {
      rawTarget[key] = prevValue
      console.error(`[useAppStore] saveConfig failed: ${key} (${scope})`, e)
      return { success: false, key, scope, error: safeErrorMessage(e) }
    }
  }

  async function saveBatch(entries: BatchSaveEntry[]): Promise<SaveResult[]> {
    const results: SaveResult[] = []

    const storeEntries = new Map<Store, BatchSaveEntry[]>()
    for (const entry of entries) {
      const store = entry.scope === 'global' ? globalStoreRef.value : projectStoreRef.value
      if (!store) {
        results.push({
          success: false,
          key: entry.key,
          scope: entry.scope,
          error: 'store not initialized',
        })
        continue
      }
      const list = storeEntries.get(store) ?? []
      list.push(entry)
      storeEntries.set(store, list)
    }

    for (const [store, storeEntriesList] of storeEntries) {
      const snapshot: Map<string, unknown> = new Map()
      const scope = store === globalStoreRef.value ? 'global' : 'project'
      const rawTarget =
        scope === 'global'
          ? (globalConfig.value as Record<string, unknown>)
          : (projectConfig.value as Record<string, unknown>)

      for (const entry of storeEntriesList) {
        snapshot.set(entry.key, rawTarget[entry.key])
      }

      const written: BatchSaveEntry[] = []

      try {
        for (const entry of storeEntriesList) {
          const schema = VALUE_SCHEMAS[entry.key]
          if (schema) {
            const validator =
              entry.key === 'editorSettings' && scope === 'project'
                ? EditorSettingsSchema.partial()
                : schema
            const vr = validator.safeParse(entry.value)
            if (!vr.success) {
              results.push({
                success: false,
                key: entry.key,
                scope: entry.scope,
                error: `validation: ${vr.error.issues[0]?.message ?? 'invalid value'}`,
              })
              continue
            }
          }

          await store.set(entry.key, entry.value)
          rawTarget[entry.key] = entry.value
          written.push(entry)
        }
        await store.save()
        for (const entry of written) {
          const prevValue = snapshot.get(entry.key)
          onConfigChanged?.(entry.key, entry.value, prevValue, scope)
          results.push({ success: true, key: entry.key, scope: entry.scope })
        }
      } catch (e) {
        for (const entry of storeEntriesList) {
          rawTarget[entry.key] = snapshot.get(entry.key)
        }
        const message = safeErrorMessage(e)
        for (const entry of storeEntriesList) {
          results.push({ success: false, key: entry.key, scope: entry.scope, error: message })
        }
      }
    }

    return results
  }

  async function resetProjectOverride(key: ConfigKey): Promise<void> {
    const store = projectStoreRef.value
    if (!store) {
      return
    }

    try {
      await store.delete(key)
      await store.save()
      delete (projectConfig.value as Record<string, unknown>)[key]
    } catch (e) {
      console.error(`[useAppStore] resetProjectOverride failed: ${key}`, e)
    }
  }

  function hasProjectOverride(key: ConfigKey): boolean {
    return (projectConfig.value as Record<string, unknown>)[key] !== undefined
  }

  async function setTheme(theme: Theme, scope: ConfigScope = 'global'): Promise<SaveResult> {
    return saveConfig(CONFIG_KEYS.THEME, theme, scope)
  }

  async function setLanguage(language: Language): Promise<SaveResult> {
    return saveConfig(CONFIG_KEYS.LANGUAGE, language, 'global')
  }

  async function setEditorSettings(
    settings: Partial<EditorSettings>,
    scope: ConfigScope = 'global'
  ): Promise<SaveResult> {
    if (scope === 'global') {
      const current = structuredClone(globalConfig.value.editorSettings)
      const merged = { ...current, ...settings }
      return saveConfig(CONFIG_KEYS.EDITOR_SETTINGS, merged, scope)
    }

    const current = structuredClone(projectConfig.value.editorSettings ?? {})
    const globalCurrent = structuredClone(globalConfig.value.editorSettings)
    const candidate = { ...globalCurrent, ...current, ...settings }
    const diff = computeDiff(candidate, globalCurrent)

    return saveConfig(
      CONFIG_KEYS.EDITOR_SETTINGS,
      diff as EditorSettings | Partial<EditorSettings>,
      scope
    )
  }

  async function setDefaultEngine(
    engine: DefaultEngine,
    scope: ConfigScope = 'global'
  ): Promise<SaveResult> {
    return saveConfig(CONFIG_KEYS.DEFAULT_ENGINE, engine, scope)
  }

  async function addRecentProject(projectId: string): Promise<SaveResult> {
    const list = [...globalConfig.value.recentProjects]
    const idx = list.indexOf(projectId)
    if (idx !== -1) {
      list.splice(idx, 1)
    }
    list.unshift(projectId)
    if (list.length > RECENT_PROJECTS_MAX) {
      list.length = RECENT_PROJECTS_MAX
    }
    return saveConfig(CONFIG_KEYS.RECENT_PROJECTS, list, 'global')
  }

  async function removeRecentProject(projectId: string): Promise<SaveResult> {
    const list = globalConfig.value.recentProjects.filter(id => id !== projectId)
    return saveConfig(CONFIG_KEYS.RECENT_PROJECTS, list, 'global')
  }

  async function saveDockviewLayout(layout: SerializedDockviewLayout): Promise<SaveResult> {
    return saveConfig(CONFIG_KEYS.DOCKVIEW_LAYOUT, layout, 'project')
  }

  async function saveSidebarState(state: SerializedSidebarState): Promise<SaveResult> {
    return saveConfig(CONFIG_KEYS.SIDEBAR_STATE, state, 'project')
  }

  async function setConnectionPool(settings: ConnectionPoolSettings): Promise<SaveResult> {
    return saveConfig(CONFIG_KEYS.CONNECTION_POOL, settings, 'global')
  }

  async function setHistorySettings(settings: HistorySettings): Promise<SaveResult> {
    return saveConfig(CONFIG_KEYS.HISTORY_SETTINGS, settings, 'global')
  }

  async function setMonitoringSettings(settings: MonitoringSettings): Promise<SaveResult> {
    return saveConfig(CONFIG_KEYS.MONITORING_SETTINGS, settings, 'global')
  }

  async function setPerformanceSettings(settings: PerformanceSettings): Promise<SaveResult> {
    return saveConfig(CONFIG_KEYS.PERFORMANCE_SETTINGS, settings, 'global')
  }

  async function setAppearanceSettings(
    settings: AppearanceSettings | Partial<AppearanceSettings>,
    scope: ConfigScope = 'global'
  ): Promise<SaveResult> {
    if (scope === 'global') {
      const current = structuredClone(globalConfig.value.appearanceSettings)
      const merged = { ...current, ...settings }
      return saveConfig(CONFIG_KEYS.APPEARANCE_SETTINGS, merged, scope)
    }

    const current = structuredClone(projectConfig.value.appearanceSettings ?? {})
    const globalCurrent = structuredClone(globalConfig.value.appearanceSettings)
    const candidate = { ...globalCurrent, ...current, ...settings }
    const diff = computeDiff(candidate, globalCurrent)

    return saveConfig(
      CONFIG_KEYS.APPEARANCE_SETTINGS,
      diff as AppearanceSettings | Partial<AppearanceSettings>,
      scope
    )
  }

  async function setResultSettings(settings: ResultSettings): Promise<SaveResult> {
    return saveConfig(CONFIG_KEYS.RESULT_SETTINGS, settings, 'global')
  }

  async function setTitleBarSettings(settings: TitleBarSettings): Promise<SaveResult> {
    return saveConfig(CONFIG_KEYS.TITLE_BAR_SETTINGS, settings, 'global')
  }

  async function setStatusBarSettings(settings: StatusBarSettings): Promise<SaveResult> {
    return saveConfig(CONFIG_KEYS.STATUS_BAR_SETTINGS, settings, 'global')
  }

  async function setCommandPaletteSettings(settings: CommandPaletteSettings): Promise<SaveResult> {
    return saveConfig(CONFIG_KEYS.COMMAND_PALETTE_SETTINGS, settings, 'global')
  }

  async function initialize(): Promise<void> {
    if (initialized.value) {
      return
    }

    try {
      const store = await Store.load(STORE_FILENAME_GLOBAL)
      globalStoreRef.value = store

      const rawGlobal = globalConfig.value as Record<string, unknown>

      let shouldSave = false

      const storedVersion = await store.get<number>('_schemaVersion')
      if (storedVersion !== undefined && storedVersion < SCHEMA_VERSION) {
        const migrated = migrateConfig(storedVersion, rawGlobal)
        Object.assign(rawGlobal, migrated)
        shouldSave = true
      }

      const needsSeed = await loadStoreWithDefaults(
        store,
        GLOBAL_SEED_KEYS,
        GlobalConfigSchema.shape,
        rawGlobal,
        'global'
      )

      shouldSave = shouldSave || needsSeed

      if (shouldSave) {
        await store.set('_schemaVersion', SCHEMA_VERSION)
        await store.save()
      }

      // 迁移旧版 localStorage 工具栏配置
      await migrateToolbarSettings()

      registerBeforeUnloadHandler()

      initialized.value = true
      initError.value = null
    } catch (e) {
      const message = safeErrorMessage(e)
      console.error('[useAppStore] Failed to initialize global store:', e)
      initError.value = message
      initialized.value = true
    }
  }

  async function migrateToolbarSettings(): Promise<void> {
    try {
      const stored = localStorage.getItem('toolbar-tools')
      if (!stored) return

      const tools = JSON.parse(stored) as Array<{ id: string; enabled?: boolean }>
      const enabledIds = tools.filter(t => t.enabled).map(t => t.id)

      const current = structuredClone(globalConfig.value.titleBarSettings)
      if (enabledIds.length > 0) {
        current.toolbarTools = enabledIds
        await saveConfig(CONFIG_KEYS.TITLE_BAR_SETTINGS, current, 'global')
        localStorage.removeItem('toolbar-tools')
        // eslint-disable-next-line no-console
        console.info('[useAppStore] Migrated toolbar settings from localStorage:', enabledIds)
      }
    } catch {
      // ignore migration errors
    }
  }

  async function reloadConfig(): Promise<void> {
    const store = globalStoreRef.value
    if (!store) {
      return
    }

    try {
      const rawGlobal = globalConfig.value as Record<string, unknown>

      const storedVersion = await store.get<number>('_schemaVersion')
      if (storedVersion !== undefined && storedVersion < SCHEMA_VERSION) {
        const migrated = migrateConfig(storedVersion, rawGlobal)
        Object.assign(rawGlobal, migrated)
      }

      const needsSave = await loadStoreWithDefaults(
        store,
        GLOBAL_SEED_KEYS,
        GlobalConfigSchema.shape,
        rawGlobal,
        'global'
      )

      if (needsSave) {
        await store.set('_schemaVersion', SCHEMA_VERSION)
        await store.save()
      }
    } catch (e) {
      console.error('[useAppStore] reloadConfig failed:', e)
    }
  }

  async function resetToFactory(): Promise<SaveResult[]> {
    const store = globalStoreRef.value
    if (!store) {
      return [
        {
          success: false,
          key: CONFIG_KEYS.THEME,
          scope: 'global',
          error: 'global store not initialized',
        },
      ]
    }

    try {
      await store.clear()

      if (projectOpen.value) {
        await closeProject()
      }

      const results: SaveResult[] = []

      for (const { key, default: defaultVal } of GLOBAL_SEED_KEYS) {
        await store.set(key, defaultVal)
        ;(globalConfig.value as Record<string, unknown>)[key] = defaultVal
        results.push({ success: true, key, scope: 'global' })
      }

      await store.set('_schemaVersion', SCHEMA_VERSION)
      await store.save()

      setupSystemThemeListener()
      applyTheme()
      return results
    } catch (e) {
      const message = safeErrorMessage(e)
      return GLOBAL_SEED_KEYS.map(({ key }) => ({
        success: false,
        key,
        scope: 'global' as ConfigScope,
        error: message,
      }))
    }
  }

  async function openProject(p: string): Promise<{ success: boolean; error?: string }> {
    if (projectOpen.value) {
      await closeProject()
    }

    try {
      const filename = buildProjectStoreFilename(p)
      const store = await Store.load(filename)
      projectStoreRef.value = store

      const newConfig: ProjectConfig = {}

      for (const { key } of PROJECT_SEED_KEYS) {
        const stored = await store.get<unknown>(key)
        if (stored !== null && stored !== undefined) {
          ;(newConfig as Record<string, unknown>)[key] = stored
        }
      }

      const validation = ProjectConfigSchema.safeParse(newConfig)
      if (validation.success) {
        projectConfig.value = validation.data
      } else {
        console.warn(
          '[useAppStore] Project config validation failed, using partial',
          validation.error.issues
        )
        const partial: ProjectConfig = {}
        for (const { key } of PROJECT_SEED_KEYS) {
          const v = (newConfig as Record<string, unknown>)[key]
          if (v !== undefined) {
            ;(partial as Record<string, unknown>)[key] = v
          }
        }
        projectConfig.value = partial
      }

      projectPath.value = p
      projectOpen.value = true
      applyTheme()
      return { success: true }
    } catch (e) {
      console.error('[useAppStore] Failed to open project store:', e)
      projectStoreRef.value = null
      projectPath.value = null
      projectOpen.value = false
      applyTheme()
      return { success: false, error: safeErrorMessage(e) }
    }
  }

  async function closeProject(): Promise<void> {
    if (!projectOpen.value) {
      return
    }

    cancelAutoSaveTimer()

    const store = projectStoreRef.value
    if (store) {
      try {
        await store.save()
      } catch (e) {
        console.error('[useAppStore] Failed to save project store on close:', e)
      }
    }

    projectStoreRef.value = null
    projectConfig.value = {}
    projectPath.value = null
    projectOpen.value = false
    applyTheme()
  }

  let autoSaveTimer: ReturnType<typeof setTimeout> | null = null

  function cancelAutoSaveTimer() {
    if (autoSaveTimer) {
      clearTimeout(autoSaveTimer)
      autoSaveTimer = null
    }
  }

  watch(
    projectConfig,
    () => {
      if (!projectOpen.value || !projectStoreRef.value) {
        return
      }
      cancelAutoSaveTimer()
      autoSaveTimer = setTimeout(() => {
        projectStoreRef.value?.save().catch(() => {})
      }, 500)
    },
    { deep: true }
  )

  let beforeUnloadRegistered = false

  function registerBeforeUnloadHandler() {
    if (beforeUnloadRegistered) {
      return
    }
    beforeUnloadRegistered = true

    window.addEventListener('beforeunload', () => {
      cancelAutoSaveTimer()
      const saveOps: Promise<void>[] = []

      if (globalStoreRef.value) {
        saveOps.push(globalStoreRef.value.save().catch(() => {}))
      }
      if (projectOpen.value && projectStoreRef.value) {
        saveOps.push(projectStoreRef.value.save().catch(() => {}))
      }

      if (saveOps.length > 0) {
        Promise.all(saveOps).catch(() => {})
      }
    })
  }

  function setConfigChangeHandler(handler: ConfigChangeHandler | null) {
    onConfigChanged = handler
  }

  function clearInitError() {
    initError.value = null
  }

  let systemThemeListenerSetup = false

  function setupSystemThemeListener() {
    if (typeof window === 'undefined' || systemThemeListenerSetup) return
    systemThemeListenerSetup = true
    const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)')
    const handler = () => {
      systemDark.value = mediaQuery.matches
      if (effectiveTheme.value === 'system') {
        applyTheme()
      }
    }
    mediaQuery.addEventListener('change', handler)
  }

  function applyTheme() {
    const body = document.body
    const themeClass = isDark.value ? 'theme-dark' : 'theme-light'
    body.classList.remove('theme-dark', 'theme-light')
    body.classList.add(themeClass)
  }

  return {
    globalConfig: readonly(globalConfig),
    projectConfig: readonly(projectConfig),
    initialized: readonly(initialized),
    initError: readonly(initError),
    projectOpen: readonly(projectOpen),
    projectPath: readonly(projectPath),

    globalStoreRef,
    projectStoreRef,

    effectiveTheme,
    isDark,
    effectiveLanguage,
    effectiveEditorSettings,
    effectiveDefaultEngine,
    recentProjects,
    effectiveDockviewLayout,
    effectiveSidebarState,
    effectiveConnectionPool,
    effectiveHistorySettings,
    effectiveMonitoringSettings,
    effectivePerformanceSettings,
    effectiveAppearanceSettings,
    effectiveResultSettings,
    effectiveTitleBarSettings,
    effectiveStatusBarSettings,
    effectiveCommandPaletteSettings,

    getConfig,
    saveConfig,
    saveBatch,
    resetProjectOverride,
    hasProjectOverride,

    setTheme,
    setLanguage,
    setEditorSettings,
    setDefaultEngine,
    addRecentProject,
    removeRecentProject,
    saveDockviewLayout,
    saveSidebarState,
    setConnectionPool,
    setHistorySettings,
    setMonitoringSettings,
    setPerformanceSettings,
    setAppearanceSettings,
    setResultSettings,
    setTitleBarSettings,
    setStatusBarSettings,
    setCommandPaletteSettings,

    initialize,
    reloadConfig,
    resetToFactory,
    openProject,
    closeProject,
    applyTheme,

    setConfigChangeHandler,
    clearInitError,
  }
})
