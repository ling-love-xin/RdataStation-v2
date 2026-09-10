# RdataStation 配置系统 — 完整 API 参考

> 版本：v1.3
> 更新日期：2026-05-08
> 对应源码：`src/stores/config.ts` + `src/stores/useAppStore.ts`

---

## 目录

1. [类型定义](#类型定义)
2. [常量](#常量)
3. [useAppStore — Computed 属性](#useappstore--computed-属性)
4. [useAppStore — 生命周期方法](#useappstore--生命周期方法)
5. [useAppStore — 配置读写方法](#useappstore--配置读写方法)
6. [useAppStore — 便捷方法](#useappstore--便捷方法)
7. [SaveResult 类型](#saveresult-类型)
8. [useUiStore — 委派接口](#useuistore--委派接口)
9. [JSON 文件格式](#json-文件格式)
10. [组件使用示例](#组件使用示例)
11. [写入校验行为](#写入校验行为)
12. [新增方法（v1.1）](#新增方法v11)
13. [自维护特性](#自维护特性)

---

## 类型定义

> 定义于 [config.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/stores/config.ts)

### 基础枚举

```typescript
type Theme = 'light' | 'dark' | 'system'
type Language = 'zh-CN' | 'en'
type DefaultEngine = 'native' | 'duckdb'
type ConfigKey =
  | 'theme'
  | 'language'
  | 'editorSettings'
  | 'defaultEngine'
  | 'recentProjects'
  | 'dockviewLayout'
  | 'sidebarState'
type ConfigScope = 'global' | 'project'
```

### EditorSettings

```typescript
interface EditorSettings {
  fontSize: number // 默认 14, 范围 10-24
  tabSize: number // 默认 2, 可选 2/4/8
  wordWrap: boolean // 默认 true
  minimap: boolean // 默认 true
  lineNumbers: boolean // 默认 true
  fontFamily: string // 默认 "'Cascadia Code', 'Fira Code', 'Consolas', monospace"
}
```

### 布局/侧边栏

```typescript
interface SerializedPanelState {
  id: string
  component: string
  params?: Record<string, string>
  title: string
}

interface SerializedDockviewLayout {
  panels: SerializedPanelState[]
  activePanel: string | null
}

interface SerializedSidebarState {
  collapsed: boolean
  width: number
  activeItem: string | null
}
```

### 核心状态类型

```typescript
interface GlobalConfig {
  theme: Theme
  language: Language
  editorSettings: EditorSettings
  defaultEngine: DefaultEngine
  recentProjects: string[] // 最多 10 条
}

interface ProjectConfig {
  theme?: Theme
  editorSettings?: Partial<EditorSettings>
  defaultEngine?: DefaultEngine
  dockviewLayout?: SerializedDockviewLayout
  sidebarState?: SerializedSidebarState
}

interface ConfigOverrideRule {
  globalDefault: boolean
  projectOverridable: boolean
  projectOnly: boolean
}

interface SeedEntry {
  key: string
  default: unknown
}
```

---

## 常量

> 定义于 [config.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/stores/config.ts)

### CONFIG_KEYS

```typescript
const CONFIG_KEYS = {
  THEME: 'theme',
  LANGUAGE: 'language',
  EDITOR_SETTINGS: 'editorSettings',
  DEFAULT_ENGINE: 'defaultEngine',
  RECENT_PROJECTS: 'recentProjects',
  DOCKVIEW_LAYOUT: 'dockviewLayout',
  SIDEBAR_STATE: 'sidebarState',
}
```

### 规则表

```typescript
const OVERRIDE_RULES: Record<string, ConfigOverrideRule>
// 示例: OVERRIDE_RULES['theme'] → { globalDefault: true, projectOverridable: true, projectOnly: false }
```

### 默认值

```typescript
const DEFAULT_GLOBAL_CONFIG: GlobalConfig
const DEFAULT_EDITOR_SETTINGS: EditorSettings
```

### 种子键

```typescript
const GLOBAL_SEED_KEYS: SeedEntry[] // globalDefault === true 的条目
const PROJECT_SEED_KEYS: SeedEntry[] // projectOnly || projectOverridable 的条目
```

### 系统常量

```typescript
const SCHEMA_VERSION = 1
const STORE_FILENAME_GLOBAL = 'global-settings.json'
const STORE_FILENAME_PROJECT = 'project-settings.json'
```

| 常量                     | 类型 | 值                                    | 说明                       |
| ------------------------ | ---- | ------------------------------------- | -------------------------- |
| `SCHEMA_VERSION`         | 常量 | `1`                                   | 配置 schema 版本号         |
| `STORE_FILENAME_GLOBAL`  | 常量 | `'global-settings.json'`              | 全局配置文件               |
| `STORE_FILENAME_PROJECT` | 常量 | `'project-settings.json'`             | 项目配置文件               |
| `RECENT_PROJECTS_MAX`    | 常量 | `10`                                  | 最近项目列表上限           |
| `SIDEBAR_WIDTH_MIN`      | 常量 | `200`                                 | 侧边栏最小宽度             |
| `SIDEBAR_WIDTH_MAX`      | 常量 | `400`                                 | 侧边栏最大宽度             |
| `VALUE_SCHEMAS`          | 对象 | `Partial<Record<ConfigKey, ZodType>>` | 写入校验 zod schema 查找表 |

---

## useAppStore — Computed 属性

> 定义于 [useAppStore.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/stores/useAppStore.ts)

### 优先级合并属性

这些属性自动执行三层 fallback，组件直接读取即可。

| 属性                      | 类型                                    | 合并逻辑                                        |
| ------------------------- | --------------------------------------- | ----------------------------------------------- |
| `effectiveTheme`          | `Theme`                                 | `project.theme ?? global.theme`                 |
| `isDark`                  | `boolean`                               | 含 `system` 主题系统检测                        |
| `effectiveLanguage`       | `Language`                              | `global.language`（不可覆盖）                   |
| `effectiveEditorSettings` | `EditorSettings`                        | `DEFAULT ← global ← project` 三层 merge         |
| `effectiveDefaultEngine`  | `DefaultEngine`                         | `project.defaultEngine ?? global.defaultEngine` |
| `recentProjects`          | `string[]`                              | `global.recentProjects`（不可覆盖）             |
| `effectiveDockviewLayout` | `SerializedDockviewLayout \| undefined` | 仅项目级                                        |
| `effectiveSidebarState`   | `SerializedSidebarState \| undefined`   | 仅项目级                                        |

### 状态字段

| 字段              | 类型                        | 说明                            |
| ----------------- | --------------------------- | ------------------------------- |
| `globalConfig`    | `Ref<GlobalConfig>`         | Level 2 全局配置                |
| `projectConfig`   | `Ref<ProjectConfig>`        | Level 1 项目覆盖                |
| `initialized`     | `Ref<boolean>`              | initialize() 完成的标志         |
| `initError`       | `Ref<string \| null>`       | 初始化失败错误消息，成功为 null |
| `projectOpen`     | `Ref<boolean>`              | 当前项目是否已打开              |
| `projectPath`     | `Ref<string \| null>`       | 当前项目路径                    |
| `globalStoreRef`  | `ShallowRef<Store \| null>` | Pinia devtools 可见             |
| `projectStoreRef` | `ShallowRef<Store \| null>` | Pinia devtools 可见             |

---

## useAppStore — 生命周期方法

### initialize()

```typescript
/**
 * 初始化全局配置（幂等）
 *
 * @async
 * @returns Promise<void>
 * @sideeffect  加载 global-settings.json → globalStoreRef
 *             首次启动 → seed DEFAULT 到磁盘 → store.save()
 *             校验 _schemaVersion 并写入当前版本
 *
 * @example
 * // main.ts
 * await useAppStore().initialize()
 */
async function initialize(): Promise<void>
```

### openProject(path)

```typescript
/**
 * 打开项目配置（自动关闭当前项目）
 *
 * @async
 * @param path  - 项目路径
 * @returns Promise<void>
 * @sideeffect  先 closeProject() → Store.load(project-{path}-settings.json)
 *             遍历种子键加载覆盖值 → projectConfig.value = newConfig
 *
 * @example
 * await appStore.openProject('/Users/me/my-project')
 */
async function openProject(path: string): Promise<void>
```

### closeProject()

```typescript
/**
 * 关闭项目配置
 *
 * @async
 * @returns Promise<void>
 * @sideeffect  projectStoreRef.value.save() → projectStoreRef = null
 *             projectConfig.value = {} → projectPath = null
 *
 * @example
 * await appStore.closeProject()
 */
async function closeProject(): Promise<void>
```

### applyTheme()

```typescript
/**
 * 应用主题到 DOM
 *
 * @returns void
 * @sideeffect  body.classList.add('theme-dark') 或 'theme-light'
 *
 * @example
 * appStore.applyTheme()
 */
function applyTheme(): void
```

---

## useAppStore — 配置读写方法

### getConfig(key)

```typescript
/**
 * 通用配置读取（含优先级合并）
 *
 * @param key   - 配置键（CONFIG_KEYS.THEME 等）
 * @returns 合并后的配置值
 *
 * @example
 * const theme = appStore.getConfig<Theme>(CONFIG_KEYS.THEME)
 * const fontSize = appStore.getConfig<EditorSettings>(CONFIG_KEYS.EDITOR_SETTINGS).fontSize
 */
function getConfig<T>(key: ConfigKey): T
```

### saveConfig(key, value, scope)

```typescript
/**
 * 配置写入抽象层 —— 迁移唯一修改点
 *
 * 事务保证: 写前 snapshot → save → 成功更新内存 / 失败回滚
 *
 * @param key    - 配置键
 * @param value  - 新值
 * @param scope  - 'global' | 'project'
 * @returns Promise<SaveResult>
 *
 * @example
 * const result = await appStore.saveConfig('theme', 'light', 'global')
 * if (!result.success) console.error(result.error)
 */
async function saveConfig(key: ConfigKey, value: unknown, scope: ConfigScope): Promise<SaveResult>
```

### saveBatch(entries)

```typescript
/**
 * 批量保存配置条目（减少文件 I/O）
 *
 * 自动按 store 分组，每组最后统一 save 一次。
 * 任一 entry 失败则跳过该 entry，继续处理剩余。
 *
 * @param entries  - [{ key, value, scope }, ...]
 * @returns Promise<SaveResult[]>
 *
 * @example
 * const results = await appStore.saveBatch([
 *   { key: CONFIG_KEYS.THEME, value: 'dark', scope: 'global' },
 *   { key: CONFIG_KEYS.LANGUAGE, value: 'en', scope: 'global' },
 *   { key: CONFIG_KEYS.EDITOR_SETTINGS, value: { fontSize: 14 }, scope: 'project' },
 * ])
 */
async function saveBatch(entries: BatchSaveEntry[]): Promise<SaveResult[]>
```

### resetToFactory()

```typescript
/**
 * 恢复全局配置到出厂设置
 *
 * 清除 global-settings.json，重新 seed Level 3 默认值。
 *
 * @returns Promise<SaveResult[]>
 *
 * @example
 * const results = await appStore.resetToFactory()
 */
async function resetToFactory(): Promise<SaveResult[]>
```

### resetProjectOverride(key)

```typescript
/**
 * 删除项目覆盖值，退回 Level 2
 *
 * @param key   - 配置键
 * @returns Promise<void>
 * @sideeffect  projectStoreRef.delete(key) → delete projectConfig[key]
 *
 * @example
 * await appStore.resetProjectOverride(CONFIG_KEYS.THEME)
 */
async function resetProjectOverride(key: ConfigKey): Promise<void>
```

### hasProjectOverride(key)

```typescript
/**
 * 检查项目是否有覆盖值
 *
 * @param key   - 配置键
 * @returns boolean
 *
 * @example
 * const needsResetBtn = appStore.hasProjectOverride(CONFIG_KEYS.THEME)
 */
function hasProjectOverride(key: ConfigKey): boolean
```

---

## useAppStore — 便捷方法

### setTheme(theme, scope?)

```typescript
/**
 * @param theme  - 'light' | 'dark' | 'system'
 * @param scope  - 'global' (default) | 'project'
 * @returns Promise<SaveResult>
 *
 * @example
 * await appStore.setTheme('dark')
 * await appStore.setTheme('light', 'project')  // 仅当前项目
 */
async function setTheme(theme: Theme, scope?: ConfigScope): Promise<SaveResult>
```

### setLanguage(language)

```typescript
/**
 * 仅全局（不允许项目覆盖）
 *
 * @param language  - 'zh-CN' | 'en'
 * @returns Promise<SaveResult>
 *
 * @example
 * await appStore.setLanguage('en')
 */
async function setLanguage(language: Language): Promise<SaveResult>
```

### setEditorSettings(settings, scope?)

```typescript
/**
 * 全局模式: 完整对象覆盖
 * 项目模式: diff 模式（仅存与全局不同的字段）
 *
 * @param settings  - 部分或全部编辑器设置
 * @param scope     - 'global' (default) | 'project'
 * @returns Promise<SaveResult>
 *
 * @example
 * await appStore.setEditorSettings({ fontSize: 16 })
 * await appStore.setEditorSettings({ fontSize: 20 }, 'project')
 */
async function setEditorSettings(
  settings: Partial<EditorSettings>,
  scope?: ConfigScope
): Promise<SaveResult>
```

### setDefaultEngine(engine, scope?)

```typescript
/**
 * @param engine  - 'native' | 'duckdb'
 * @param scope   - 'global' (default) | 'project'
 * @returns Promise<SaveResult>
 *
 * @example
 * await appStore.setDefaultEngine('duckdb')
 */
async function setDefaultEngine(engine: DefaultEngine, scope?: ConfigScope): Promise<SaveResult>
```

### addRecentProject(projectId)

```typescript
/**
 * 添加最近项目（自动去重，限制 10 条）
 *
 * @param projectId  - 项目 ID
 * @returns Promise<void>
 *
 * @example
 * await appStore.addRecentProject('project-123')
 */
async function addRecentProject(projectId: string): Promise<void>
```

### removeRecentProject(projectId)

```typescript
/**
 * @param projectId  - 项目 ID
 * @returns Promise<void>
 *
 * @example
 * await appStore.removeRecentProject('project-123')
 */
async function removeRecentProject(projectId: string): Promise<void>
```

### saveDockviewLayout(layout)

```typescript
/**
 * @param layout  - dockview-vue 序列化布局
 * @returns Promise<SaveResult>
 *
 * @example
 * const serialized = dockviewApi.toJSON()
 * await appStore.saveDockviewLayout(serialized)
 */
async function saveDockviewLayout(layout: SerializedDockviewLayout): Promise<SaveResult>
```

### saveSidebarState(state)

```typescript
/**
 * @param state   - 侧边栏序列化状态
 * @returns Promise<SaveResult>
 *
 * @example
 * await appStore.saveSidebarState({ collapsed: false, width: 280, activeItem: 'connections' })
 */
async function saveSidebarState(state: SerializedSidebarState): Promise<SaveResult>
```

---

## SaveResult 类型

```typescript
interface SaveResult {
  /** 操作是否成功 */
  success: boolean
  /** 配置键 */
  key: ConfigKey
  /** 作用域 */
  scope: ConfigScope
  /** 失败时的错误消息 */
  error?: string
}
```

### 典型用法

```typescript
// 设置面板中
async function applyAllSettings() {
  const results = await Promise.all([
    appStore.setTheme(localTheme.value),
    appStore.setLanguage(localLanguage.value),
    appStore.setEditorSettings(localEditorSettings),
  ])

  const failed = results.filter(r => !r.success)
  if (failed.length > 0) {
    // 展示 toast: `${failed.length} 项设置保存失败`
  } else {
    // 展示 toast: '设置已应用'
  }
}
```

---

## useUiStore — 委派接口

> 定义于 [ui.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/shared/stores/ui.ts)

useUiStore 的主题相关字段全部委托给 useAppStore，保持向后兼容。

| 字段/方法       | 实现                                      |
| --------------- | ----------------------------------------- |
| `isDark`        | `computed → useAppStore().isDark`         |
| `theme` (get)   | `computed → useAppStore().effectiveTheme` |
| `theme` (set)   | `appStore.setTheme(value)`                |
| `applyTheme()`  | `appStore.applyTheme()`                   |
| `setTheme(t)`   | `theme.value = t; applyTheme()`           |
| `toggleTheme()` | `theme.value = isDark ? 'light' : 'dark'` |
| `initTheme()`   | `applyTheme()`（不再注册重复系统监听器）  |

自有状态（不持久化）：

```typescript
sidebarCollapsed: boolean // 默认 false
sidebarWidth: number // 默认 280
showHistoryPanel: boolean // 默认 false
showConnectionPanel: boolean // 默认 true
```

---

## JSON 文件格式

### global-settings.json

```json
{
  "_schemaVersion": 1,
  "theme": "dark",
  "language": "zh-CN",
  "editorSettings": {
    "fontSize": 14,
    "tabSize": 2,
    "wordWrap": true,
    "minimap": true,
    "lineNumbers": true,
    "fontFamily": "'Cascadia Code', 'Fira Code', 'Consolas', monospace"
  },
  "defaultEngine": "native",
  "recentProjects": ["/Users/me/projects/my-db", "/Users/me/projects/analytics"]
}
```

### project-{path}-settings.json（示例）

```json
{
  "theme": "light",
  "editorSettings": {
    "fontSize": 16
  },
  "dockviewLayout": {
    "panels": [{ "id": "query-1", "component": "SqlEditor", "title": "查询-1" }],
    "activePanel": "query-1"
  }
}
```

---

## 组件使用示例

### 读配置

```typescript
import { useAppStore } from '@/stores/useAppStore'

const appStore = useAppStore()

// 方式 1：用 computed 属性（推荐，自动合并）
const theme = appStore.effectiveTheme
const isDark = appStore.isDark
const fontSize = appStore.effectiveEditorSettings.fontSize

// 方式 2：用 getConfig（通用）
const engine = appStore.getConfig<DefaultEngine>(CONFIG_KEYS.DEFAULT_ENGINE)
```

### 写配置

```typescript
// 直接设置
await appStore.setTheme('dark')

// 带 scope
await appStore.setTheme('light', 'project')

// 用底层 saveConfig
const result = await appStore.saveConfig('theme', 'system', 'global')
if (!result.success) {
  message.error(`保存失败: ${result.error}`)
}
```

### 项目生命周期

```typescript
// 打开项目时
await appStore.openProject('/path/to/project')
appStore.applyTheme()

// 关闭项目时
await appStore.closeProject()

// 最近项目
await appStore.addRecentProject('/path/to/project')
```

### 设置面板"应用"模式

```typescript
// 1. 本地 ref（不持久化，UI 即改即显示）
const localTheme = ref(appStore.effectiveTheme)

// 2. 用户修改本地 ref
localTheme.value = 'light'

// 3. 点击"应用" → 持久化 + 全局生效
await appStore.setTheme(localTheme.value)
```

### 恢复全局默认值

```typescript
// 用户点击"恢复全局默认值"
await appStore.resetProjectOverride(CONFIG_KEYS.THEME)

// 设置面板回退显示
localTheme.value = appStore.effectiveTheme
// hasProjectOverride → false → 按钮消失
```

---

### 写入校验行为（v1.2）

每次 `saveConfig()` 写入前，值必须通过 zod 校验：

| 配置键           | 校验 schema                                                                  | 说明                                  |
| ---------------- | ---------------------------------------------------------------------------- | ------------------------------------- |
| `theme`          | `ThemeSchema`                                                                | `'light' \| 'dark' \| 'system'`       |
| `language`       | `LanguageSchema`                                                             | `'zh-CN' \| 'en'`                     |
| `editorSettings` | `EditorSettingsSchema` (global) / `EditorSettingsSchema.partial()` (project) | 14 ≤ fontSize ≤ 24, tabSize ∈ {2,4,8} |
| `defaultEngine`  | `DefaultEngineSchema`                                                        | `'native' \| 'duckdb'`                |
| `recentProjects` | `RecentProjectsSchema`                                                       | string[] 且 ≤ 10 条                   |
| `dockviewLayout` | `DockviewLayoutSchema`                                                       | 含 panels + activePanel               |
| `sidebarState`   | `SidebarStateSchema`                                                         | 200 ≤ width ≤ 400                     |

校验失败时直接拒绝写入，返回：

```typescript
{ success: false, key, scope, error: 'validation: Invalid enum value. Expected ...' }
```

---

## 新增方法（v1.1）

> 新增于 v2.5.3，v2.5.4 追加

### reloadConfig()

```typescript
/**
 * 重新加载配置（外部修改同步）
 *
 * 从 JSON 文件重新读取 + 逐字段 zod 校验。
 * 适用场景：用户手动修改 global-settings.json 后无需重启。
 *
 * @async
 * @returns Promise<void>
 *
 * @example
 * await appStore.reloadConfig()
 */
async function reloadConfig(): Promise<void>
```

### setConfigChangeHandler(handler)

```typescript
/**
 * 设置配置变更审计回调
 *
 * @param handler  - (key, newValue, oldValue, scope) => void 或 null 取消
 *
 * @example
 * // 迁移 Rust 后端后
 * appStore.setConfigChangeHandler(async (key, newVal, oldVal, scope) => {
 *   await invoke('audit_log', { action: 'config_changed', key, scope })
 * })
 */
function setConfigChangeHandler(handler: ConfigChangeHandler | null): void
```

### clearInitError()

```typescript
/**
 * 清除初始化错误（用户关闭错误横幅）
 *
 * @example
 * // App.vue
 * <NAlert closable @close="appStore.clearInitError()" />
 */
function clearInitError(): void
```

### onConfigChanged

```typescript
/**
 * 配置变更审计 hook
 *
 * 当前为 null（占位），迁移 Rust 后端后赋值日志审计函数。
 *
 * 签名: (key: ConfigKey, newValue: unknown, oldValue: unknown, scope: ConfigScope) => void
 *
 * @example
 * // 迁移后
 * appStore.onConfigChanged = async (key, newVal, oldVal, scope) => {
 *   await invoke('audit_log', { action: 'config_changed', key, scope })
 * }
 */
const onConfigChanged: ConfigChangeHandler | null
```

---

## 自维护特性

| 特性              | 触发条件                                    | 行为                                                   |
| ----------------- | ------------------------------------------- | ------------------------------------------------------ |
| **自动持久化**    | `projectConfig` 深度变化                    | 500ms debounce 后自动 `projectStore.save()`            |
| **退出保存**      | `window.beforeunload` 事件                  | 取消 autoSaveTimer + 同步保存 global/project store     |
| **逐字段校验**    | `initialize()` / `reloadConfig()` 每个字段  | 一个字段损坏仅 reseed 该字段，其余合法数据保留         |
| **写保护**        | 组件直接写 `globalConfig` / `projectConfig` | `readonly()` 包装，TypeScript 编译期阻止               |
| **懒升级**        | `_schemaVersion` 与 `SCHEMA_VERSION` 不一致 | 自动写入新版本号                                       |
| **事务写入**      | 每次 `saveConfig()` 调用                    | 写前 snapshot 内存值，`store.save()` 失败时内存回滚    |
| **批量 I/O 优化** | 多条目同时写入                              | `saveBatch()` 按 store 分组，每组只 save 一次          |
| **外部同步**      | 用户手动调用                                | `reloadConfig()` 从文件重新读取所有字段 + 校验         |
| **审计预留**      | 外部注入                                    | `setConfigChangeHandler()` → saveConfig/saveBatch 触发 |

### auto-persist watcher 内部实现

```typescript
// useAppStore.ts 内部
let autoSaveTimer: ReturnType<typeof setTimeout> | null = null
watch(
  projectConfig,
  () => {
    if (!projectOpen.value || !projectStoreRef.value) return
    if (autoSaveTimer) clearTimeout(autoSaveTimer)
    autoSaveTimer = setTimeout(() => {
      projectStoreRef.value?.save().catch(() => {})
    }, 500)
  },
  { deep: true }
)
```

### loadStoreWithDefaults 辅助函数

```typescript
/**
 * 通用 Store 加载 + 校验 + seed 辅助函数
 *
 * 对每个种子键：
 *   - store.get(key) 读已有值
 *   - zod 子 schema safeParse 校验
 *   - 缺失/校验失败 → seed 默认值
 *
 * @param store       - tauri-plugin-store Store 实例
 * @param seedKeys    - 种子键列表 [{ key, default }]
 * @param fullSchema  - zod schema.shape 用于逐字段校验
 * @param target      - 内存目标对象
 * @param scope       - 'global' | 'project'（日志用）
 * @returns needsSave - 是否需要 store.save()
 */
async function loadStoreWithDefaults(
  store: Store,
  seedKeys: readonly { key: string; default: unknown }[],
  fullSchema: Record<string, unknown>,
  target: Record<string, unknown>,
  scope: ConfigScope
): Promise<boolean>
```

### computeDiff 泛型深比较

```typescript
/**
 * 计算对象 T 中与 base 不同的字段 (deep diff)
 *
 * 递归比较对象属性，生成仅含差异的 partial 对象。
 * 用于 setEditorSettings project 模式 — 仅存储与全局不同的字段。
 *
 * @param candidate  - 候选对象
 * @param base       - 基准对象
 * @returns Partial<T> — 仅含差异字段
 */
function computeDiff<T extends Record<string, unknown>>(candidate: T, base: T): Partial<T>
```
