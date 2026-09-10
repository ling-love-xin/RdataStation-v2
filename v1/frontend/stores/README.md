# RdataStation 前端 Stores 目录

> 版本：v1.0
> 更新日期：2026-05-08
> 对应目录：`src/stores/`

---

## 概览

本目录包含应用级 Pinia Store —— RdataStation 配置系统的单一数据入口。

```
src/stores/
├── README.md          ← 本文件
├── config.ts           ← 类型定义 + 配置注册表 + 种子数据
└── useAppStore.ts       ← 核心 Pinia Store（配置读写/生命周期）
```

> **注意**：`src/shared/stores/ui.ts` 中的 `useUiStore` 不在此目录，其主题相关字段已全部委派给 `useAppStore`。

---

## 架构位置

```
UI 组件层
  ├── theme/locale 读取 → useAppStore.effective* (computed)
  └── 设置面板写入 → useAppStore.setTheme() → saveConfig()
                        ↓
                  useAppStore (单一入口)
                        ↓
               saveConfig(key, value, scope)  ← 迁移唯一修改点
                        ↓
            tauri-plugin-store (JSON 文件)    ← 当前存储
                        ↓
                 Rust SQLite (规划)           ← 目标存储
```

---

## 文件速查

| 文件                                                                                               | 职责                           | 核心导出                                                                                                           |
| -------------------------------------------------------------------------------------------------- | ------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| [config.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/stores/config.ts)           | 类型定义、配置注册表、种子数据 | `CONFIG_REGISTRY`, `OVERRIDE_RULES`, `DEFAULT_GLOBAL_CONFIG`, `GLOBAL_SEED_KEYS`, `PROJECT_SEED_KEYS`, zod schemas |
| [useAppStore.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/stores/useAppStore.ts) | 核心 Pinia Store               | `useAppStore()` — initialize / saveConfig / openProject / closeProject / effective* / set*                         |

---

## 依赖关系

```
config.ts (纯函数，无依赖)
    ↓
useAppStore.ts
    ├── @tauri-apps/plugin-store (Store.load/set/get/save/delete)
    ├── config.ts (类型 + 常量 + zod schemas)
    ├── Pinia (defineStore)
    └── Vue (ref/computed/watch)
```

---

## 配置三层模型

```
┌──────────────────────────────────────┐
│ Level 1: 项目覆盖 (Project Override)  │  ← project-{path}-settings.json
│   dockviewLayout, sidebarState,       │     打开项目时加载
│   theme, editorSettings, defaultEngine│
├──────────────────────────────────────┤
│ Level 2: 全局默认 (Global Default)    │  ← global-settings.json
│   theme, language, editorSettings,    │     启动时初始化
│   defaultEngine, recentProjects       │
├──────────────────────────────────────┤
│ Level 3: 系统硬编码 (System Defaults) │  ← DEFAULT_GLOBAL_CONFIG
│   代码中定义，不存文件               │     config.ts 中定义
└──────────────────────────────────────┘
```

---

## 数据流

### 读：优先级合并

```
组件 → useAppStore.effectiveTheme
         → projectConfig.theme ?? globalConfig.theme
```

### 写：saveConfig 抽象层

```
组件 → setTheme('dark', 'project')
         → saveConfig('theme', 'dark', 'project')
              → projectStore.set(key, value)
              → projectStore.save()
              → 内存同步: projectConfig.theme = 'dark'
              → 响应式更新: effectiveTheme → 组件重渲染
```

### 初始化

```
main.ts → appStore.initialize()
            → Store.load('global-settings.json')
            → 逐字段 zod 校验 (一个字段损坏不丢全局)
            → seed 缺失/无效字段
            → 写入 _schemaVersion
            → initialized = true
```

---

## 迁移路径

当前使用 `tauri-plugin-store` 写入 JSON 文件，目标迁移至 Rust SQLite：

```
当前: Store.set(key, value) + Store.save()
  ↓ 改动仅限 useAppStore.ts 内部
目标: invoke('set_config', { key, value, scope }) → Rust SQLite
```

受影响的函数（迁移唯一修改点）：

- `saveConfig()` — 写入
- `initialize()` — 初始化
- `openProject()` — 项目加载
- `closeProject()` — 项目保存
- `reloadConfig()` — 重新加载
- `resetToFactory()` — 出厂重置

`config.ts` 的所有类型/常量/规则 **不需要任何修改**。

---

## 使用方式

```typescript
import { useAppStore } from '@/stores/useAppStore'

const appStore = useAppStore()

// 读（推荐 computed 属性）
const theme = appStore.effectiveTheme
const isDark = appStore.isDark
const lang = appStore.effectiveLanguage
const editor = appStore.effectiveEditorSettings
const engine = appStore.effectiveDefaultEngine

// 写
await appStore.setTheme('dark')
await appStore.setLanguage('zh-CN')
await appStore.setEditorSettings({ fontSize: 14, minimap: false })
await appStore.setDefaultEngine('duckdb', 'project')

// 生命周期
await appStore.initialize() // main.ts 中调用
await appStore.openProject(path) // 打开项目时
await appStore.closeProject() // 关闭项目时

// 工具方法
const result = await appStore.saveConfig('theme', 'dark', 'global')
if (!result.success) console.error(result.error)

await appStore.saveBatch([
  { key: 'theme', value: 'dark', scope: 'global' },
  { key: 'language', value: 'en', scope: 'global' },
])

await appStore.resetToFactory()
await appStore.reloadConfig()
appStore.clearInitError()
```

---

## 自维护特性

| 特性        | 说明                                                                               |
| ----------- | ---------------------------------------------------------------------------------- |
| 自动持久化  | `projectConfig` 变化 500ms 后自动 save（deep watch）                               |
| 退出保存    | `window.beforeunload` 事件 flush 所有 pending save                                 |
| 写入校验    | `saveConfig` / `saveBatch` 通过 `VALUE_SCHEMAS` 校验值，非法值拒绝写入             |
| Schema 迁移 | `initialize()` 检测 `_schemaVersion` < `SCHEMA_VERSION`，运行 `migrateConfig()` 链 |
| 写保护      | `globalConfig` / `projectConfig` 返回 `readonly()` 包装，编译期阻止直接写          |
| 懒加载      | `_schemaVersion` 校验，旧版本自动升级                                              |
| 部分恢复    | 一个字段 zod 校验失败，仅 reseed 该字段，保留其余合法数据                          |
| 事务性写入  | `saveConfig` / `saveBatch` 写前 snapshot，失败时内存回滚                           |
| 审计预留    | `setConfigChangeHandler()` 注入审计回调，`saveConfig` / `saveBatch` 触发           |

---

## 参考文档

| 文档          | 路径                                                                                                                        |
| ------------- | --------------------------------------------------------------------------------------------------------------------------- |
| 配置系统设计  | [docs/frontend/CONFIG-SYSTEM.md](file:///e:/myapps/tauirapps/RdataStation/rdata-station/docs/frontend/CONFIG-SYSTEM.md)     |
| 配置 API 参考 | [docs/frontend/CONFIG-API.md](file:///e:/myapps/tauirapps/RdataStation/rdata-station/docs/frontend/CONFIG-API.md)           |
| 开发进度追踪  | [docs/frontend/CONFIG-PROGRESS.md](file:///e:/myapps/tauirapps/RdataStation/rdata-station/docs/frontend/CONFIG-PROGRESS.md) |
| 前端架构索引  | [docs/frontend/INDEX.md](file:///e:/myapps/tauirapps/RdataStation/rdata-station/docs/frontend/INDEX.md)                     |
| 变更日志      | [docs/CHANGELOG.md](file:///e:/myapps/tauirapps/RdataStation/rdata-station/docs/CHANGELOG.md)                               |
