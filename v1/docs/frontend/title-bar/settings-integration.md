# 设置页面与标题栏/状态栏融合打通方案

> 版本: v1.0
> 日期: 2026-05-10
> 状态: 设计阶段

---

## 一、现状分析

### 1.1 现有设置系统

```
src/stores/config.ts              # 配置注册表（zod schema + 默认值 + 覆盖规则）
src/stores/useAppStore.ts         # Pinia Store（持久化、校验、三层优先级）
src/extensions/builtin/settings/ui/components/SettingsPanel.vue  # 设置面板 UI
```

**现有配置项**:

- `theme` / `language` / `editorSettings`
- `connectionPool` / `historySettings` / `monitoringSettings`
- `performanceSettings` / `appearanceSettings` / `resultSettings`
- `dockviewLayout` / `sidebarState`

**持久化机制**: tauri-plugin-store → JSON 文件（global-settings.json / project-settings.json）

### 1.2 现有标题栏/状态栏系统

```
src/extensions/builtin/workbench/ui/components/WorkbenchTitleBar.vue   # 标题栏
src/extensions/builtin/workbench/ui/components/WorkbenchStatusBar.vue  # 状态栏
src/extensions/builtin/workbench/ui/stores/command-store.ts            # 命令注册表
src/extensions/builtin/workbench/ui/config/title-bar-config.ts         # 配置工厂
```

**当前问题**:

- 标题栏/状态栏的可配置项**未接入**设置系统
- 工具栏启用状态保存在 `localStorage`（`toolbar-tools`），而非统一配置系统
- 状态栏显示项（执行时间、行数、连接状态）**无法自定义**
- 菜单项的显示/隐藏**无法配置**
- 命令面板搜索范围**无法配置**

---

## 二、设计目标

1. **统一配置入口**: 所有标题栏/状态栏的可配置项纳入 `useAppStore` 管理
2. **实时响应**: 配置变更后标题栏/状态栏**无需刷新**即时生效
3. **分层配置**: 支持全局默认 + 项目覆盖（复用现有三层优先级）
4. **向后兼容**: 不影响现有设置项，仅新增配置键

---

## 三、可配置项梳理

### 3.1 标题栏可配置项

| 配置项                         | 类型                              | 默认值   | 作用域  | 说明                     |
| ------------------------------ | --------------------------------- | -------- | ------- | ------------------------ |
| `titleBar.menuStyle`           | `'full' \| 'compact' \| 'hidden'` | `'full'` | global  | 菜单显示模式             |
| `titleBar.toolbarTools`        | `string[]`                        | `[]`     | project | 启用的工具栏按钮 ID 列表 |
| `titleBar.showProjectSelector` | `boolean`                         | `true`   | global  | 是否显示项目选择器       |
| `titleBar.showCommandCenter`   | `boolean`                         | `true`   | global  | 是否显示命令中心         |
| `titleBar.recentProjectCount`  | `number`                          | `5`      | global  | 最近项目显示数量         |

### 3.2 状态栏可配置项

| 配置项                           | 类型      | 默认值 | 作用域  | 说明                   |
| -------------------------------- | --------- | ------ | ------- | ---------------------- |
| `statusBar.visible`              | `boolean` | `true` | project | 状态栏可见性           |
| `statusBar.showConnectionStatus` | `boolean` | `true` | global  | 显示连接状态           |
| `statusBar.showExecutionTime`    | `boolean` | `true` | global  | 显示执行时间           |
| `statusBar.showRowCount`         | `boolean` | `true` | global  | 显示行数               |
| `statusBar.showDuckDBIndicator`  | `boolean` | `true` | global  | 显示 DuckDB 加速指示器 |
| `statusBar.showEncoding`         | `boolean` | `true` | global  | 显示编码格式           |
| `statusBar.showVersion`          | `boolean` | `true` | global  | 显示版本信息           |

### 3.3 命令面板可配置项

| 配置项                                   | 类型      | 默认值  | 作用域 | 说明                 |
| ---------------------------------------- | --------- | ------- | ------ | -------------------- |
| `commandPalette.maxRecentCommands`       | `number`  | `5`     | global | 最近命令最大数量     |
| `commandPalette.includeDisabledCommands` | `boolean` | `false` | global | 搜索是否包含禁用命令 |

---

## 四、架构设计

### 4.1 数据流

```
SettingsPanel.vue (设置页面)
    ↓ 用户修改配置
useAppStore.saveConfig('titleBar.xxx', value, 'global')
    ↓ 持久化到 tauri-plugin-store
global-settings.json / project-settings.json
    ↓ 响应式更新
Vue Reactivity System
    ↓ computed 监听
WorkbenchTitleBar.vue / WorkbenchStatusBar.vue
    ↓ 即时生效
UI 更新（无需刷新）
```

### 4.2 配置 Schema 扩展

```typescript
// src/stores/config.ts

// 新增类型定义
interface TitleBarSettings {
  menuStyle: 'full' | 'compact' | 'hidden'
  toolbarTools: string[]
  showProjectSelector: boolean
  showCommandCenter: boolean
  recentProjectCount: number
}

interface StatusBarSettings {
  visible: boolean
  showConnectionStatus: boolean
  showExecutionTime: boolean
  showRowCount: boolean
  showDuckDBIndicator: boolean
  showEncoding: boolean
  showVersion: boolean
}

interface CommandPaletteSettings {
  maxRecentCommands: number
  includeDisabledCommands: boolean
}

// 新增 zod schemas
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

// 注册到 CONFIG_REGISTRY
titleBarSettings: {
  key: 'titleBarSettings' as const,
  default: { ... } satisfies TitleBarSettings,
  valueSchema: TitleBarSettingsSchema,
  rule: { globalDefault: true, projectOverridable: true, projectOnly: false },
},
statusBarSettings: {
  key: 'statusBarSettings' as const,
  default: { ... } satisfies StatusBarSettings,
  valueSchema: StatusBarSettingsSchema,
  rule: { globalDefault: true, projectOverridable: true, projectOnly: false },
},
commandPaletteSettings: {
  key: 'commandPaletteSettings' as const,
  default: { ... } satisfies CommandPaletteSettings,
  valueSchema: CommandPaletteSettingsSchema,
  rule: { globalDefault: true, projectOverridable: false, projectOnly: false },
}
```

### 4.3 Store 扩展

```typescript
// src/stores/useAppStore.ts

// 新增 computed
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

// 新增 actions
async function setTitleBarSettings(settings: Partial<TitleBarSettings>): Promise<SaveResult> {
  const current = structuredClone(globalConfig.value.titleBarSettings)
  const merged = { ...current, ...settings }
  return saveConfig(CONFIG_KEYS.TITLE_BAR_SETTINGS, merged, 'global')
}

async function setStatusBarSettings(settings: Partial<StatusBarSettings>): Promise<SaveResult> {
  const current = structuredClone(globalConfig.value.statusBarSettings)
  const merged = { ...current, ...settings }
  return saveConfig(CONFIG_KEYS.STATUS_BAR_SETTINGS, merged, 'global')
}
```

---

## 五、UI 设计

### 5.1 设置页面新增 Tab

在 `SettingsPanel.vue` 的 `tabs` 数组中新增 **"界面"** Tab：

```typescript
const tabs = [
  { id: 'connection-pool', label: t('settings.connectionPool'), icon: Database },
  { id: 'history', label: t('settings.history'), icon: History },
  { id: 'monitoring', label: t('settings.monitoring'), icon: Activity },
  { id: 'performance', label: t('settings.performance'), icon: Zap },
  { id: 'shortcuts', label: t('settings.shortcuts'), icon: Keyboard },
  { id: 'appearance', label: t('settings.appearance'), icon: Palette },
  { id: 'interface', label: t('settings.interface'), icon: LayoutTemplate }, // 新增
]
```

### 5.2 "界面" Tab 内容

```vue
<!-- 标题栏设置 -->
<div class="settings-section">
  <h3>{{ t('settings.titleBar') }}</h3>

  <div class="setting-item">
    <label>{{ t('settings.menuStyle') }}</label>
    <select v-model="settings.interface.titleBar.menuStyle">
      <option value="full">{{ t('settings.menuStyleFull') }}</option>
      <option value="compact">{{ t('settings.menuStyleCompact') }}</option>
      <option value="hidden">{{ t('settings.menuStyleHidden') }}</option>
    </select>
  </div>

  <div class="setting-item">
    <label>
      <input v-model="settings.interface.titleBar.showProjectSelector" type="checkbox" />
      {{ t('settings.showProjectSelector') }}
    </label>
  </div>

  <div class="setting-item">
    <label>
      <input v-model="settings.interface.titleBar.showCommandCenter" type="checkbox" />
      {{ t('settings.showCommandCenter') }}
    </label>
  </div>

  <div class="setting-item">
    <label>{{ t('settings.recentProjectCount') }}</label>
    <input v-model.number="settings.interface.titleBar.recentProjectCount" type="number" min="1" max="10" />
  </div>
</div>

<!-- 工具栏设置 -->
<div class="settings-section">
  <h3>{{ t('settings.toolbar') }}</h3>
  <div class="toolbar-tools-config">
    <div v-for="tool in availableTools" :key="tool.id" class="tool-toggle-item">
      <label>
        <input v-model="settings.interface.titleBar.toolbarTools" :value="tool.id" type="checkbox" />
        <component :is="tool.icon" :size="16" />
        <span>{{ tool.name }}</span>
      </label>
    </div>
  </div>
</div>

<!-- 状态栏设置 -->
<div class="settings-section">
  <h3>{{ t('settings.statusBar') }}</h3>

  <div class="setting-item">
    <label>
      <input v-model="settings.interface.statusBar.visible" type="checkbox" />
      {{ t('settings.showStatusBar') }}
    </label>
  </div>

  <div class="setting-item">
    <label>
      <input v-model="settings.interface.statusBar.showConnectionStatus" type="checkbox" />
      {{ t('settings.showConnectionStatus') }}
    </label>
  </div>

  <div class="setting-item">
    <label>
      <input v-model="settings.interface.statusBar.showExecutionTime" type="checkbox" />
      {{ t('settings.showExecutionTime') }}
    </label>
  </div>

  <div class="setting-item">
    <label>
      <input v-model="settings.interface.statusBar.showRowCount" type="checkbox" />
      {{ t('settings.showRowCount') }}
    </label>
  </div>

  <div class="setting-item">
    <label>
      <input v-model="settings.interface.statusBar.showDuckDBIndicator" type="checkbox" />
      {{ t('settings.showDuckDBIndicator') }}
    </label>
  </div>

  <div class="setting-item">
    <label>
      <input v-model="settings.interface.statusBar.showEncoding" type="checkbox" />
      {{ t('settings.showEncoding') }}
    </label>
  </div>

  <div class="setting-item">
    <label>
      <input v-model="settings.interface.statusBar.showVersion" type="checkbox" />
      {{ t('settings.showVersion') }}
    </label>
  </div>
</div>

<!-- 命令面板设置 -->
<div class="settings-section">
  <h3>{{ t('settings.commandPalette') }}</h3>

  <div class="setting-item">
    <label>{{ t('settings.maxRecentCommands') }}</label>
    <input v-model.number="settings.interface.commandPalette.maxRecentCommands" type="number" min="1" max="20" />
  </div>

  <div class="setting-item">
    <label>
      <input v-model="settings.interface.commandPalette.includeDisabledCommands" type="checkbox" />
      {{ t('settings.includeDisabledCommands') }}
    </label>
  </div>
</div>
```

---

## 六、组件改造

### 6.1 WorkbenchTitleBar.vue 改造

```typescript
// 注入配置
const appStore = useAppStore()
const titleBarSettings = computed(() => appStore.effectiveTitleBarSettings)

// 根据配置条件渲染
const showMenuBar = computed(() => titleBarSettings.value.menuStyle !== 'hidden')
const showCompactMenu = computed(() => titleBarSettings.value.menuStyle === 'compact')
const showProjectSelector = computed(() => titleBarSettings.value.showProjectSelector)
const showCommandCenter = computed(() => titleBarSettings.value.showCommandCenter)

// 工具栏从配置读取
const toolbarTools = computed<ToolbarTool[]>(() => {
  const enabledIds = new Set(titleBarSettings.value.toolbarTools)
  return createToolbarConfig(t, handleOpenCommandPalette).map(tool => ({
    ...tool,
    enabled: enabledIds.has(tool.id),
  }))
})

// 最近项目数量限制
const recentProjects = computed(() => {
  return titleBar.recentProjects.slice(0, titleBarSettings.value.recentProjectCount)
})
```

### 6.2 WorkbenchStatusBar.vue 改造

```typescript
// 注入配置
const appStore = useAppStore()
const statusBarSettings = computed(() => appStore.effectiveStatusBarSettings)

// 根据配置条件渲染
const visible = computed(() => statusBarSettings.value.visible)
const showConnectionStatus = computed(() => statusBarSettings.value.showConnectionStatus)
const showExecutionTime = computed(() => statusBarSettings.value.showExecutionTime)
const showRowCount = computed(() => statusBarSettings.value.showRowCount)
const showDuckDBIndicator = computed(() => statusBarSettings.value.showDuckDBIndicator)
const showEncoding = computed(() => statusBarSettings.value.showEncoding)
const showVersion = computed(() => statusBarSettings.value.showVersion)
```

### 6.3 CommandStore.ts 改造

```typescript
// 注入配置
const appStore = useAppStore()
const paletteSettings = computed(() => appStore.effectiveCommandPaletteSettings)

// 使用配置值
const maxRecent = computed(() => paletteSettings.value.maxRecentCommands)

// 搜索时过滤禁用命令
function search(query: string): Command[] {
  // ...
  const includeDisabled = paletteSettings.value.includeDisabledCommands
  return allCommands.value.filter(cmd => includeDisabled || !cmd.disabled)
  // ...
}
```

---

## 七、迁移策略

### 7.1 现有工具栏配置迁移

```typescript
// 在 useAppStore.initialize() 中添加迁移逻辑
async function migrateToolbarSettings() {
  const stored = localStorage.getItem('toolbar-tools')
  if (stored) {
    try {
      const tools = JSON.parse(stored)
      const current = structuredClone(globalConfig.value.titleBarSettings)
      current.toolbarTools = tools
        .filter((t: ToolbarTool) => t.enabled)
        .map((t: ToolbarTool) => t.id)
      await saveConfig(CONFIG_KEYS.TITLE_BAR_SETTINGS, current, 'global')
      localStorage.removeItem('toolbar-tools')
    } catch {
      // ignore
    }
  }
}
```

### 7.2 状态栏可见性迁移

`statusBarVisible` 目前在 `sidebarState` 中，建议保持现状或迁移到 `statusBarSettings.visible`：

```typescript
// 可选：迁移 sidebarState.statusBarVisible → statusBarSettings.visible
async function migrateStatusBarVisibility() {
  const sidebarState = globalConfig.value.sidebarState
  if (sidebarState && sidebarState.statusBarVisible !== undefined) {
    const current = structuredClone(globalConfig.value.statusBarSettings)
    current.visible = sidebarState.statusBarVisible
    await saveConfig(CONFIG_KEYS.STATUS_BAR_SETTINGS, current, 'global')
  }
}
```

---

## 八、实现步骤

### Phase 1: 配置系统扩展 (1 天)

1. [ ] 在 `config.ts` 中新增 `TitleBarSettings` / `StatusBarSettings` / `CommandPaletteSettings` 类型
2. [ ] 新增 zod schemas
3. [ ] 注册到 `CONFIG_REGISTRY`
4. [ ] 追加到 `GlobalConfig` / `DEFAULT_GLOBAL_CONFIG`
5. [ ] 在 `useAppStore.ts` 中新增 computed 和 actions

### Phase 2: 设置页面扩展 (1 天)

1. [ ] 在 `SettingsPanel.vue` 新增 "界面" Tab
2. [ ] 实现标题栏/工具栏/状态栏/命令面板设置 UI
3. [ ] 添加翻译键（zh-CN.json / en.json）
4. [ ] 实现保存/重置逻辑

### Phase 3: 组件改造 (1 天)

1. [ ] 改造 `WorkbenchTitleBar.vue` 读取配置
2. [ ] 改造 `WorkbenchStatusBar.vue` 读取配置
3. [ ] 改造 `command-store.ts` 读取配置
4. [ ] 移除 `localStorage` 工具栏配置逻辑

### Phase 4: 迁移与测试 (1 天)

1. [ ] 实现 `migrateToolbarSettings()`
2. [ ] 验证配置实时响应
3. [ ] 验证持久化正确性
4. [ ] 验证三层优先级（全局/项目覆盖）

---

## 九、风险与回滚

| 风险                               | 影响 | 缓解措施                         |
| ---------------------------------- | ---- | -------------------------------- |
| 配置 schema 变更导致旧数据无法解析 | 高   | zod safeParse 失败时回退到默认值 |
| 组件改造引入回归 bug               | 中   | 保留原有逻辑作为 fallback        |
| 设置页面性能下降                   | 低   | 使用 computed + shallowRef 优化  |

**回滚策略**: 保留原有 `localStorage` 逻辑 1 个版本，确认稳定后移除。

---

## 十、总结

本方案将标题栏/状态栏的可配置项全面接入现有设置系统，实现：

1. **统一配置入口**: 所有配置通过 `SettingsPanel.vue` 管理
2. **实时响应**: Vue 响应式系统自动同步配置变更
3. **分层配置**: 复用现有全局/项目覆盖机制
4. **向后兼容**: 平滑迁移旧配置，无破坏性变更

预计开发周期: **4 天**（Phase 1-4）
