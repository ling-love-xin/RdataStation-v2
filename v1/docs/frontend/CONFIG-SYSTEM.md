# RdataStation 配置系统 — 设计文档

> 版本：v1.1
> 更新日期：2026-05-07
> 状态：✅ Phase 1 完成，Phase 2 API 就绪

---

## 目录

1. [设计目标](#设计目标)
2. [三层优先级](#三层优先级)
3. [可覆盖性矩阵](#可覆盖性矩阵)
4. [架构全景图](#架构全景图)
5. [数据流](#数据流)
6. [文件清单](#文件清单)
7. [Store 生命周期](#store-生命周期)
8. [初始化时序](#初始化时序)
9. [关键设计决策](#关键设计决策)
10. [迁移路径](#迁移路径)
11. [验收标准](#验收标准)

---

## 设计目标

| 目标              | 约束                                                  |
| ----------------- | ----------------------------------------------------- |
| **10 年生命周期** | 接口稳定，语义化版本；新增键不删旧键                  |
| **迁移就绪**      | `saveConfig()` 抽象层隔离存储实现，未来替换零前端改动 |
| **扁平化**        | 配置嵌套 ≤ 2 层，兼容 JSON 文件 / SQLite 键值对       |
| **单一数据入口**  | 所有读写通过 `useAppStore`，组件不直接访问底层        |
| **可测试**        | 每个方法纯函数可独立测试                              |

---

## 三层优先级

```
查询路径（单向 fallback）：

  Level 1: projectConfig.value[key]      ← 项目覆盖值
       ↓ undefined
  Level 2: globalConfig.value[key]        ← 全局默认值
       ↓ undefined
  Level 3: DEFAULT_GLOBAL_CONFIG[key]     ← 系统硬编码

规则:
  - 只向下查找，不存在反向继承
  - 项目覆盖可以显式删除（resetProjectOverride），删除后退回 Level 2
  - Level 3 值在首次启动时写入 Level 2，之后 Level 2 永远存在
```

---

## 可覆盖性矩阵

| 配置键           | Level 2 全局 | 项目可覆盖 | 仅项目 | 说明                   |
| ---------------- | :----------: | :--------: | :----: | ---------------------- |
| `theme`          |      ✅      |     ✅     |   —    | 多项目用不同主题       |
| `language`       |      ✅      |     ❌     |   —    | 个人偏好               |
| `editorSettings` |      ✅      |     ✅     |   —    | 不同项目不同缩进       |
| `defaultEngine`  |      ✅      |     ✅     |   —    | 生产原生 / 探索 DuckDB |
| `recentProjects` |      ✅      |     ❌     |   —    | 仅个人历史             |
| `dockviewLayout` |      —       |     —      |   ✅   | 每项目独立布局         |
| `sidebarState`   |      —       |     —      |   ✅   | 每项目独立侧边栏       |

---

## 架构全景图

```
┌─ Components ────────────────────────────────────────┐
│  MainLayout.vue  SettingsPanel.vue  QueryEditor.vue  │
│       │                  │                 │          │
│       └──────────────────┼─────────────────┘          │
│                          │                            │
│              useAppStore (Pinia)                      │
│              ┌───────────────────┐                    │
│              │ globalStoreRef    │ shallowRef<Store>  │
│              │ projectStoreRef   │ shallowRef<Store>  │
│              │                   │                    │
│              │ getConfig(key)    │ → priority merge   │
│              │ saveConfig(k,v,s) │ → write abstract   │
│              └───────┬───────────┘                    │
└──────────────────────┼────────────────────────────────┘
                       │
         ┌─────────────┴─────────────┐
         │   tauri-plugin-store       │
         │   Store.load / set / save  │
         └─────────────┬─────────────┘
                       │
         ┌─────────────┴─────────────┐
         │   JSON Files               │
         │   global-settings.json     │
         │   project-{path}-*.json    │
         └───────────────────────────┘

未来:
  tauri-plugin-store → invoke('set_config') → Rust SQLite
```

### 配套 Stores

```
useUiStore (shared/stores/ui.ts)
  └─ isDark / theme / applyTheme → 委托到 useAppStore
  └─ sidebarCollapsed / showHistoryPanel → 自有状态

useLayoutStore (workbench/ui/stores/)
  └─ dockview 布局 → 读 useAppStore.effectiveDockviewLayout
  └─ 布局变更 → 调 useAppStore.saveDockviewLayout()
```

---

## 数据流

### 读路径

```
Component
  → useAppStore().effectiveTheme       (computed, reactive)
  → useAppStore().effectiveEditorSettings
  → useAppStore().getConfig('theme')
      → projectConfig?.theme ?? globalConfig.theme ?? DEFAULT.theme
```

### 写路径（全局）

```
SettingsPanel
  → localTheme = 'light'                (本地 ref, 不持久化)
  → Click [应用所有设置]
  → appStore.setTheme('light', 'global')
  → saveConfig('theme', 'light', 'global')
      → globalStoreRef.value.set('theme', 'light')
      → globalStoreRef.value.save()     → 写入 JSON 文件
      → globalConfig.value.theme = 'light'
  → effectiveTheme reactive → App.vue NConfigProvider → UI 更新
```

### 写路径（项目覆盖 diff）

```
SettingsPanel
  → localEditorSettings.fontSize = 18
  → Click [应用所有设置]
  → appStore.setEditorSettings({ fontSize: 18 }, 'project')
      → 计算 diff: { fontSize: 18 }（tabSize 等与全局一致则忽略）
      → saveConfig('editorSettings', diff, 'project')
           → projectStoreRef.value.set('editorSettings', diff)
           → projectStoreRef.value.save()  → project-{id}-settings.json
      → projectConfig.value.editorSettings = diff
  → effective → 编辑器 fontSize 更新，tabSize 保持全局值
```

### 恢复全局默认值

```
SettingsPanel
  → Click [恢复全局默认值] (theme)
  → appStore.resetProjectOverride('theme')
      → projectStoreRef.value.delete('theme')
      → delete projectConfig.value.theme
  → localTheme = appStore.effectiveTheme  (自动退到 global.theme)
  → UI 恢复按钮消失
```

---

## 文件清单

```
src/stores/
├── config.ts              # 类型 + 注册表 + 默认值 + SeedKeys
└── useAppStore.ts         # Pinia Store 核心（391 行）

src/app/
├── main.ts                # Pinia → init → extension → mount
└── App.vue                # NConfigProvider: theme + locale + i18n

src/shared/stores/
└── ui.ts                  # useUiStore（委派给 useAppStore）

src/extensions/builtin/
├── settings/ui/components/SettingsPanel.vue    # 外观/编辑器/引擎/布局/操作
└── workbench/ui/components/panels/SettingsPanel.vue  # 连接池/历史/监控

docs/frontend/
├── CONFIG-SYSTEM.md       # 本文件（设计文档）
├── CONFIG-API.md          # 完整 API 参考
└── CONFIG-PROGRESS.md     # 开发进度追踪
```

---

## Store 生命周期

| Store             | 创建                       | 释放                        | 存储位置                                  |
| ----------------- | -------------------------- | --------------------------- | ----------------------------------------- |
| `globalStoreRef`  | `main.ts` → `initialize()` | 应用关闭                    | `global-settings.json` (app data)         |
| `projectStoreRef` | `openProject(path)`        | `closeProject()` / 切换项目 | `project-{path}-settings.json` (app data) |

切换协议：

```
openProject(B)  ← 用户点击切换
  1. closeProject()
     → projectStoreRef.value.save()      持久化 A 的覆盖
     → projectStoreRef.value = null      释放 A Store
     → projectConfig.value = {}          清空内存
  2. Store.load(project-{B}-settings.json)
  3. projectStoreRef.value = new Store
  4. projectConfig.value = B 的覆盖
```

---

## 初始化时序

```
main():  ← 阻塞型 async
  ┌──────────────────────────────────┐
  │ 1. appStore.initialize()         │  加载 global-settings.json
  │    ├─ Store.load(global-*.json)  │  首次 → seed DEFAULT 到磁盘
  │    ├─ GLOBAL_SEED_KEYS.forEach   │
  │    ├─ check _schemaVersion       │
  │    └─ store.save() if needed     │
  │                                  │
  │ 2. appStore.applyTheme()         │  <body> + theme-dark/light
  │                                  │
  │ 3. extensionHost.activate()      │  扩展系统
  │                                  │
  │ 4. panelRegistry → app.component │  全局组件注册
  │                                  │
  │ 5. app.mount('#app')             │  Vue 挂载
  └──────────────────────────────────┘

App.vue onMounted:
  - watch( effectiveLanguage ) → i18n.locale.value
  - setupSystemThemeListener()  → matchMedia change
  - appStore.applyTheme()
```

---

## 关键设计决策

### 1. 为什么用 `shallowRef` 而非模块级 `let`？

```typescript
// 之前: Pinia devtools 不可见, $reset() 不清理
let globalStore: Store | null = null

// 现在: devtools 可见, $reset() 同步
const globalStoreRef = shallowRef<Store | null>(null)
```

### 2. 项目编辑器设置用 diff 而非全量存储

当用户覆盖 `fontSize=18` 时：

- diff 模式存 `{ fontSize: 18 }`
- 全局 `tabSize` 从 2 改为 4 → 项目自动跟随
- 全量模式 `{ fontSize: 18, tabSize: 2 }` → 全局变更不传递

### 3. saveConfig 事务回滚

```
prevValue = snapshot
try: store.set → store.save → memory = value
catch: memory = prevValue → return SaveResult{error}
```

### 4. 新增配置键只需修改 CONFIG_REGISTRY

```typescript
// 只需在 CONFIG_REGISTRY 加一个条目，其余自动派生
const CONFIG_REGISTRY = {
  autoSave: {
    key: 'autoSave' as const,
    default: true,
    rule: { globalDefault: true, ... },
  },
}
```

---

## 迁移路径

| 时间          | 步骤                                    | 修改文件                       |
| ------------- | --------------------------------------- | ------------------------------ |
| **当前**      | tauri-plugin-store JSON                 | config.ts, useAppStore.ts      |
| **Phase 2**   | AppLayout 集成 openProject/closeProject | MainLayout.vue                 |
| **Phase 3-4** | 完善 settings UI + 全局生效             | SettingsPanel.vue, Editor.vue  |
| **未来**      | JSON → Rust SQLite                      | **仅** useAppStore.ts 3 个方法 |

未来替换详情：

```typescript
// 唯一修改点 — saveConfig()
// before:
await store.set(key, value)
await store.save()

// after:
await invoke('set_config', { key, value, scope })

// 同理 initialize() 和 openProject() 内部
```

| 工作                                            | 工时       |
| ----------------------------------------------- | ---------- |
| Rust 建表 (`app_config` / `project_config`)     | 0.5 天     |
| Rust Tauri Command (`set_config`, `get_config`) | 0.5 天     |
| 前端替换内部实现                                | 0.5 天     |
| 测试                                            | 0.5 天     |
| **合计**                                        | **≤ 2 天** |

---

## 验收标准

| #   | 验收项        | 验证方式                                  | 状态 |
| --- | ------------- | ----------------------------------------- | :--: |
| 1   | 配置持久化    | 改主题 → 关闭 → 重开 → 保留               |  ✅  |
| 2   | 优先级合并    | 全局:14 → 项目:16 → 删除项目:16 → 退回:14 |  ✅  |
| 3   | 设置面板联动  | 改设置 → 面板显示新值 → UI 反映           |  ✅  |
| 4   | 项目隔离      | 项目 A: light → 切项目 B → B 不变         |  ⬜  |
| 5   | 迁移就绪      | saveConfig() 唯一入口                     |  ✅  |
| 6   | devtools 可见 | Pinia Vue devtools → appConfig 可查看     |  ✅  |
| 7   | diff 模式     | 项目覆盖 editorSettings 仅存差异字段      |  ✅  |

---

## 故障排查

### Q1: 重启后配置丢失

**原因**：tauri-plugin-store JSON 文件损坏或权限不足。

**排查步骤**：

1. 查看 App.vue 顶部是否有红色错误横幅（`initError`）
2. 打开 Tauri WebView 控制台（F12），搜索 `useAppStore`
3. 手动删除 `global-settings.json`（位置: Tauri app_data_dir），重启应用自动重建

### Q2: 修改设置后其他组件未反应

**原因**：未通过 `useAppStore` 读写配置（绕过了响应式链路）。

**排查步骤**：

1. 确认组件从 `useAppStore.effective*` 读取（不是从 `localStorage` 或自有 ref）
2. 确认写入通过 `appStore.setXxx()` 而非直接修改 `globalConfig`
3. 在 Pinia devtools 中查看 `appConfig` 状态是否变化了

### Q3: 项目覆盖值异常（修改全局后项目未继承）

**原因**：项目覆盖值是静态快照，不会自动跟踪全局变化。

**排查步骤**：

1. 调用 `appStore.hasProjectOverride(key)` 检查是否有覆盖
2. 调用 `appStore.resetProjectOverride(key)` 清除覆盖退回全局值

### Q4: 退出应用时未保存的修改丢失

**影响版本**：≤ v2.5.3

**现象**：最后 500ms 内的修改可能未被 auto-persist watcher 保存。

**已修复**：v2.5.4 注册 `beforeunload` 事件，退出时主动 flush 所有 pending save。

### Q5: typecheck 报 globalConfig/projectConfig 只读

**原因**：v2.5.4 将 `globalConfig` / `projectConfig` 返回为 `readonly()` 包装。

**解决**：组件不应直接写这些 ref。使用 `setTheme()` / `saveConfig()` 等方法来修改配置。

---

### Q6: `saveConfig` 返回 validation 错误

**原因**：v2.5.5 新增写入时 zod 校验，值格式不合法。

**常见场景**：

- `setTheme('blue')` → `'blue'` 不是合法 Theme 枚举值
- `setEditorSettings({ fontSize: 32 })` → fontSize 超过 24 上限
- `saveDockviewLayout({ panels: 'invalid' })` → panels 必须是数组

**解决**：检查值是否符合配置项类型约束。TypeScript 编译期可捕获大部分错误。

---

## 变更历史

| 版本 | 日期       | 说明                                                                                                                                                                                                            |
| ---- | ---------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| v1.3 | 2026-05-08 | 架构完善：ConfigValueType 从注册表自动派生 / Schema 迁移函数 / 原子 schemas 提取 / saveConfig 写入时 zod 校验；故障排查新增 Q6（写入校验拒绝）                                                                  |
| v1.2 | 2026-05-08 | P0/P1/P2 修复：openProject 异常不设 projectOpen / beforeunload 保存 / reloadConfig zod 校验 / loadStoreWithDefaults 接入 / 死代码清理 / saveBatch 审计 / globalConfig 只读 / initError 可关闭；新增故障排查章节 |
| v1.1 | 2026-05-07 | 补充 JSDoc 注释；更新文档同步代码（Language='en'/applyTheme=body class/vue-i18n/themeOverrides 简化）；新增 CONFIG-API.md + CONFIG-PROGRESS.md                                                                  |
| v1.0 | 2026-05-07 | Phase 1 完成。包含 7 项重塑：统一注册表/种子表/Store进state/schema版本/diff覆盖/事务回滚/SaveResult                                                                                                             |
