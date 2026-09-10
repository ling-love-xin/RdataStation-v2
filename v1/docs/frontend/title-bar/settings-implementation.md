# 设置页面与标题栏/状态栏融合实现文档

> 版本: v1.0
> 日期: 2026-05-10
> 状态: 已完成

---

## 一、变更摘要

本次实现将标题栏/状态栏的可配置项全面接入现有设置系统，实现统一配置入口、实时响应、分层配置。

### 1.1 新增配置项 (14 项)

| 配置项                                   | 类型                              | 默认值   | 作用域  |
| ---------------------------------------- | --------------------------------- | -------- | ------- |
| `titleBar.menuStyle`                     | `'full' \| 'compact' \| 'hidden'` | `'full'` | global  |
| `titleBar.toolbarTools`                  | `string[]`                        | `[]`     | project |
| `titleBar.showProjectSelector`           | `boolean`                         | `true`   | global  |
| `titleBar.showCommandCenter`             | `boolean`                         | `true`   | global  |
| `titleBar.recentProjectCount`            | `number`                          | `5`      | global  |
| `statusBar.visible`                      | `boolean`                         | `true`   | project |
| `statusBar.showConnectionStatus`         | `boolean`                         | `true`   | global  |
| `statusBar.showExecutionTime`            | `boolean`                         | `true`   | global  |
| `statusBar.showRowCount`                 | `boolean`                         | `true`   | global  |
| `statusBar.showDuckDBIndicator`          | `boolean`                         | `true`   | global  |
| `statusBar.showEncoding`                 | `boolean`                         | `true`   | global  |
| `statusBar.showVersion`                  | `boolean`                         | `true`   | global  |
| `commandPalette.maxRecentCommands`       | `number`                          | `5`      | global  |
| `commandPalette.includeDisabledCommands` | `boolean`                         | `false`  | global  |

### 1.2 修改文件清单

| 文件                                                                    | 变更类型 | 说明                                 |
| ----------------------------------------------------------------------- | -------- | ------------------------------------ |
| `src/stores/config.ts`                                                  | 修改     | 新增 3 个配置 schema + 类型 + 默认值 |
| `src/stores/useAppStore.ts`                                             | 修改     | 新增 3 个 computed + 3 个 action     |
| `src/extensions/builtin/settings/ui/components/SettingsPanel.vue`       | 修改     | 新增 "界面" Tab + 设置 UI            |
| `src/extensions/builtin/workbench/ui/components/WorkbenchTitleBar.vue`  | 修改     | 从配置系统读取设置                   |
| `src/extensions/builtin/workbench/ui/components/WorkbenchStatusBar.vue` | 修改     | 从配置系统读取设置                   |
| `src/extensions/builtin/workbench/ui/stores/command-store.ts`           | 修改     | 从配置系统读取设置                   |
| `src/shared/locales/zh-CN.json`                                         | 修改     | 新增 23 个翻译键                     |
| `src/shared/locales/en.json`                                            | 修改     | 新增 23 个翻译键                     |

---

## 二、架构设计

### 2.1 数据流

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

### 2.2 配置 Schema

```typescript
// TitleBarSettings
interface TitleBarSettings {
  menuStyle: 'full' | 'compact' | 'hidden'
  toolbarTools: string[]
  showProjectSelector: boolean
  showCommandCenter: boolean
  recentProjectCount: number
}

// StatusBarSettings
interface StatusBarSettings {
  visible: boolean
  showConnectionStatus: boolean
  showExecutionTime: boolean
  showRowCount: boolean
  showDuckDBIndicator: boolean
  showEncoding: boolean
  showVersion: boolean
}

// CommandPaletteSettings
interface CommandPaletteSettings {
  maxRecentCommands: number
  includeDisabledCommands: boolean
}
```

---

## 三、组件改造详情

### 3.1 WorkbenchTitleBar.vue

**改造前**:

- 工具栏状态从 `localStorage` 读取
- 菜单始终显示
- 项目选择器始终显示
- 命令中心始终显示
- 最近项目数量固定

**改造后**:

```typescript
// 从配置系统读取
const titleBarSettings = computed(() => appStore.effectiveTitleBarSettings)

// 条件渲染
const showMenuBar = computed(() => titleBarSettings.value.menuStyle !== 'hidden')
const showCompactMenu = computed(() => titleBarSettings.value.menuStyle === 'compact')
const showProjectSelector = computed(() => titleBarSettings.value.showProjectSelector)
const showCommandCenter = computed(() => titleBarSettings.value.showCommandCenter)

// 工具栏从配置读取启用状态
const toolbarTools = computed<ToolbarTool[]>(() => {
  const enabledIds = new Set(titleBarSettings.value.toolbarTools)
  return createToolbarConfig(t, handleOpenCommandPalette).map(tool => ({
    ...tool,
    enabled: enabledIds.has(tool.id),
  }))
})

// 最近项目数量限制
const recentProjectsLimited = computed(() => {
  return titleBar.recentProjects.slice(0, titleBarSettings.value.recentProjectCount)
})
```

### 3.2 WorkbenchStatusBar.vue

**改造前**:

- 所有显示项固定显示
- 无可见性控制

**改造后**:

```typescript
// 从配置系统读取
const statusBarSettings = computed(() => appStore.effectiveStatusBarSettings)

// 条件渲染
<div v-if="statusBarSettings.visible" class="status-bar">
  <span v-if="statusBarSettings.showConnectionStatus">...</span>
  <span v-if="statusBarSettings.showDuckDBIndicator">...</span>
  <span v-if="statusBarSettings.showExecutionTime">...</span>
  <span v-if="statusBarSettings.showRowCount">...</span>
  <span v-if="statusBarSettings.showEncoding">...</span>
  <span v-if="statusBarSettings.showVersion">...</span>
</div>
```

### 3.3 CommandStore.ts

**改造前**:

- `maxRecent` 固定为 5
- 搜索包含所有命令

**改造后**:

```typescript
// 从配置系统读取
const paletteSettings = computed(() => appStore.effectiveCommandPaletteSettings)
const maxRecent = computed(() => paletteSettings.value.maxRecentCommands)

// 搜索时过滤禁用命令
function search(query: string): Command[] {
  const includeDisabled = paletteSettings.value.includeDisabledCommands
  return allCommands.value.filter(cmd => {
    if (!includeDisabled && cmd.disabled) return false
    // ...
  })
}
```

---

## 四、设置页面 UI

### 4.1 新增 "界面" Tab

```
┌─────────────────────────────────────────┐
│  外观  |  界面  |  其他 Tab...           │
├─────────────────────────────────────────┤
│  标题栏                                  │
│    [完整] [紧凑] [隐藏]  ← 菜单样式      │
│    [✓] 显示项目选择器                    │
│    [✓] 显示命令中心                      │
│    最近项目数量: [━━━●━━━━] 5            │
│                                          │
│  工具栏                                  │
│    [✓] 设置  [✓] 历史  [ ] 文档...      │
│                                          │
│  状态栏                                  │
│    [✓] 显示状态栏                        │
│    [✓] 显示连接状态                      │
│    [✓] 显示执行时间                      │
│    [✓] 显示行数                          │
│    [✓] 显示 DuckDB 加速指示器            │
│    [✓] 显示编码格式                      │
│    [✓] 显示版本信息                      │
│                                          │
│  命令面板                                │
│    最近命令最大数量: [━━━●━━━━] 5        │
│    [ ] 搜索包含禁用命令                  │
│                                          │
│  [应用所有设置] [重置为默认] [恢复出厂]   │
└─────────────────────────────────────────┘
```

---

## 五、接口文档

### 5.1 Store 新增 API

```typescript
// useAppStore 新增 computed
const effectiveTitleBarSettings: ComputedRef<TitleBarSettings>
const effectiveStatusBarSettings: ComputedRef<StatusBarSettings>
const effectiveCommandPaletteSettings: ComputedRef<CommandPaletteSettings>

// useAppStore 新增 actions
async function setTitleBarSettings(settings: TitleBarSettings): Promise<SaveResult>
async function setStatusBarSettings(settings: StatusBarSettings): Promise<SaveResult>
async function setCommandPaletteSettings(settings: CommandPaletteSettings): Promise<SaveResult>
```

### 5.2 配置持久化格式

```json
// global-settings.json
{
  "titleBarSettings": {
    "menuStyle": "full",
    "toolbarTools": ["settings", "history"],
    "showProjectSelector": true,
    "showCommandCenter": true,
    "recentProjectCount": 5
  },
  "statusBarSettings": {
    "visible": true,
    "showConnectionStatus": true,
    "showExecutionTime": true,
    "showRowCount": true,
    "showDuckDBIndicator": true,
    "showEncoding": true,
    "showVersion": true
  },
  "commandPaletteSettings": {
    "maxRecentCommands": 5,
    "includeDisabledCommands": false
  }
}
```

---

## 六、质量验证

| 检查项               | 结果                  |
| -------------------- | --------------------- |
| Lint (修改文件)      | 0 errors, 仅 warnings |
| Typecheck (修改文件) | 0 errors              |
| 新引入错误           | 无                    |

---

## 七、向后兼容

1. **工具栏配置迁移**: 原有 `localStorage` 中的 `toolbar-tools` 配置仍可用，但建议迁移到新的配置系统
2. **默认值**: 所有新增配置项均有合理的默认值，不影响现有用户
3. **schema 校验**: zod safeParse 失败时自动回退到默认值

---

## 八、待办事项

- [ ] 实现 `migrateToolbarSettings()` 将 `localStorage` 配置迁移到统一配置系统
- [ ] 为新增配置项添加单元测试
- [ ] 验证项目级覆盖功能
