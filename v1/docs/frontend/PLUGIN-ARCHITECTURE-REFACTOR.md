# 插件化架构重构方案

> 版本：v1.0
> 最后更新：2026-05-03
> 状态：✅ 持续更新

---

## 项目概述

本文档记录 RdataStation 前端插件化架构重构的完整设计、实施路线和开发进度。

**核心理念：** 一切插件化，Dockview 负责布局，插件系统负责内容。

**设计原则：**

- ✅ Dockview 负责布局管理（分栏、标签、拖拽）
- ✅ 插件系统负责内容注册（面板、命令、视图）
- ✅ Workbench 作为容器，动态加载注册的面板
- ✅ 保持 VSCode 式扩展架构

---

## 架构现状分析

### ✅ 已有的基础设施

| 模块                 | 状态          | 文件位置                                |
| -------------------- | ------------- | --------------------------------------- |
| **ExtensionContext** | ✅ 已定义     | `src/extensions/core/types.ts`          |
| **PanelRegistry**    | ✅ 接口已定义 | `src/extensions/core/types.ts:46-48`    |
| **CommandRegistry**  | ✅ 接口已定义 | `src/extensions/core/types.ts:40-43`    |
| **内置扩展**         | ✅ 已实现     | `src/extensions/builtin/*/extension.ts` |
| **扩展生命周期**     | ✅ 已定义     | `ExtensionModule.activate/deactivate`   |

### ❌ 缺失的关键实现

| 模块                     | 状态      | 说明                             |
| ------------------------ | --------- | -------------------------------- |
| **PanelRegistry 实现**   | ❌ 未实现 | 只有接口，没有运行时实现         |
| **CommandRegistry 实现** | ❌ 未实现 | 只有接口，没有运行时实现         |
| **WindowAPI 实现**       | ❌ 未实现 | 只有接口，没有运行时实现         |
| **ExtensionHost**        | ❌ 未实现 | 没有扩展加载器                   |
| **Workbench 集成**       | ❌ 硬编码 | `WorkbenchView.vue` 直接写死组件 |

---

## 目标架构

```
┌─────────────────────────────────────────────────────────┐
│                    WorkbenchView                         │
│  ┌───────────────────────────────────────────────────┐  │
│  │              Dockview Container                    │  │
│  │  ┌─────────────┐  ┌─────────────────────────────┐  │  │
│  │  │  Panel A    │  │        Panel B              │  │  │
│  │  │ (动态加载)   │  │      (动态加载)             │  │  │
│  │  └─────────────┘  └─────────────────────────────┘  │  │
│  └───────────────────────────────────────────────────┘  │
│                                                          │
│  ┌──────────────────────────────────────────────────┐   │
│  │         PanelRegistry (实现类)                    │   │
│  │  - 存储插件注册的面板                              │   │
│  │  - 提供面板查询接口                                │   │
│  └──────────────────────────────────────────────────┘   │
│                                                          │
│  ┌──────────────────────────────────────────────────┐   │
│  │         Extension Host (扩展运行时)               │   │
│  │  - 加载扩展 (builtin + future plugins)           │   │
│  │  - 注入 ExtensionContext                         │   │
│  │  - 管理扩展生命周期                               │   │
│  └──────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────┘
```

---

## 核心设计

### 1. PanelRegistry 实现

**文件：** `src/core/panel-registry.ts`

```typescript
import type { PanelDescriptor, Disposable } from '@/extensions/core/types'

class PanelRegistryImpl {
  private panels = new Map<string, PanelDescriptor>()

  register(panel: PanelDescriptor): Disposable {
    this.panels.set(panel.id, panel)

    return {
      dispose: () => this.panels.delete(panel.id),
    }
  }

  get(id: string): PanelDescriptor | undefined {
    return this.panels.get(id)
  }

  getAll(): PanelDescriptor[] {
    return Array.from(this.panels.values())
  }

  getByLocation(location: string): PanelDescriptor[] {
    return this.getAll().filter(p => p.location === location)
  }
}

export const panelRegistry = new PanelRegistryImpl()
```

### 2. WindowAPI 实现

**文件：** `src/core/window-api.ts`

```typescript
import type { WindowAPI, Disposable } from '@/extensions/core/types'
import { panelRegistry } from './panel-registry'

export const windowAPI: WindowAPI = {
  registerViewProvider(
    id: string,
    provider: {
      component: unknown
      title: string
      location: 'left' | 'right' | 'bottom' | 'center'
      icon?: string
      order?: number
    }
  ): Disposable {
    return panelRegistry.register({
      id,
      name: provider.title,
      component: provider.component,
      location: provider.location,
      icon: provider.icon,
      order: provider.order,
    })
  },

  showNotification(message: string, type?: 'info' | 'warning' | 'error') {
    // 实现通知逻辑
  },
}
```

### 3. CommandRegistry 实现

**文件：** `src/core/command-registry.ts`

```typescript
import type { CommandRegistry, Disposable } from '@/extensions/core/types'

class CommandRegistryImpl implements CommandRegistry {
  private commands = new Map<string, (...args: unknown[]) => unknown>()

  registerCommand(id: string, handler: (...args: unknown[]) => unknown): Disposable {
    this.commands.set(id, handler)

    return {
      dispose: () => this.commands.delete(id),
    }
  }

  async executeCommand(id: string, ...args: unknown[]): Promise<unknown> {
    const handler = this.commands.get(id)
    if (!handler) {
      throw new Error(`Command '${id}' not found`)
    }
    return handler(...args)
  }
}

export const commandRegistry = new CommandRegistryImpl()
```

### 4. ExtensionHost 实现

**文件：** `src/core/extension-host.ts`

```typescript
import type {
  ExtensionContext,
  ExtensionModule,
  ExtensionAPI,
  Disposable,
  ProjectInfo,
} from '@/extensions/core/types'
import { commandRegistry } from './command-registry'
import { windowAPI } from './window-api'
import { eventBus } from '@/extensions/core/event-bus'

class ExtensionHost {
  private activatedExtensions = new Map<string, ExtensionAPI>()
  private subscriptions: Disposable[] = []

  async activateExtensions(
    extensions: Array<{ id: string; module: ExtensionModule }>,
    projectInfo: ProjectInfo
  ) {
    for (const { id, module } of extensions) {
      try {
        const context: ExtensionContext = {
          project: projectInfo,
          extension: { id, publisher: 'builtin', name: id, version: '1.0.0' },
          globalState: {
            /* 实现 */
          },
          workspaceState: {
            /* 实现 */
          },
          storagePath: '',
          globalStoragePath: '',
          logPath: '',
          subscriptions: this.subscriptions,
          commands: commandRegistry,
          window: windowAPI,
          workspace: {
            /* 实现 */
          },
          database: {
            /* 实现 */
          },
          sqlEditor: {
            /* 实现 */
          },
          events: eventBus,
          configuration: {
            /* 实现 */
          },
          utils: {
            /* 实现 */
          },
        }

        const api = await module.activate(context)
        this.activatedExtensions.set(id, api)
        console.log(`[ExtensionHost] Activated: ${id}`)
      } catch (error) {
        console.error(`[ExtensionHost] Failed to activate ${id}:`, error)
      }
    }
  }

  async deactivateExtensions() {
    for (const [id, api] of this.activatedExtensions) {
      try {
        api.dispose?.()
        console.log(`[ExtensionHost] Deactivated: ${id}`)
      } catch (error) {
        console.error(`[ExtensionHost] Failed to deactivate ${id}:`, error)
      }
    }
    this.activatedExtensions.clear()
  }

  getExtension(id: string): ExtensionAPI | undefined {
    return this.activatedExtensions.get(id)
  }
}

export const extensionHost = new ExtensionHost()
```

---

## 扩展注册面板示例

### Database 扩展

**文件：** `src/extensions/builtin/database/extension.ts`

```typescript
import DatabaseNavigator from './ui/components/database-navigator.vue'

const activate = (context: ExtensionContext): ExtensionAPI => {
  // 注册数据库导航面板
  const disposable = context.window.registerViewProvider('navigator', {
    component: DatabaseNavigator,
    title: '数据库导航',
    location: 'left',
    icon: 'database',
    order: 1
  })

  context.subscribe(disposable)

  return { ... }
}
```

### Query 扩展

**文件：** `src/extensions/builtin/query/extension.ts`

```typescript
import SqlEditorPanel from './ui/components/SqlEditorPanel.vue'
import ResultPanel from './ui/components/ResultPanel.vue'

const activate = (context: ExtensionContext): ExtensionAPI => {
  // 注册 SQL 编辑器面板
  const sqlDisposable = context.window.registerViewProvider('sqlEditor', {
    component: SqlEditorPanel,
    title: 'SQL 编辑器',
    location: 'center',
    order: 1
  })

  // 注册结果面板
  const resultDisposable = context.window.registerViewProvider('resultPanel', {
    component: ResultPanel,
    title: '查询结果',
    location: 'bottom',
    order: 2
  })

  context.subscribe(sqlDisposable)
  context.subscribe(resultDisposable)

  return { ... }
}
```

---

## Workbench 动态渲染

**文件：** `src/extensions/builtin/workbench/ui/views/WorkbenchView.vue`

```vue
<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { panelRegistry } from '@/core/panel-registry'
import { DockviewVueComponent, type DockviewApi } from 'dockview-vue'

const dockviewApi = ref<DockviewApi | null>(null)

onMounted(() => {
  // 从注册表读取所有面板并创建
  const panels = panelRegistry.getAll()

  panels.forEach(panel => {
    dockviewApi.value?.addPanel({
      id: panel.id,
      component: panel.id,
      title: panel.name,
      position: { direction: panel.location },
    })
  })
})
</script>

<template>
  <dockview :components="getComponentsFromRegistry()" @ready="onDockviewReady" />
</template>
```

---

## 实施路线

### 阶段 1：实现核心基础设施

| 任务                 | 文件                           | 状态      | 说明           |
| -------------------- | ------------------------------ | --------- | -------------- |
| 实现 PanelRegistry   | `src/core/panel-registry.ts`   | ⏳ 待开始 | 面板注册表实现 |
| 实现 WindowAPI       | `src/core/window-api.ts`       | ⏳ 待开始 | 窗口 API 实现  |
| 实现 CommandRegistry | `src/core/command-registry.ts` | ⏳ 待开始 | 命令注册表实现 |
| 实现 ExtensionHost   | `src/core/extension-host.ts`   | ⏳ 待开始 | 扩展加载器     |

### 阶段 2：重构内置扩展

| 任务                | 文件                    | 状态      | 说明                   |
| ------------------- | ----------------------- | --------- | ---------------------- |
| 注册导航面板        | `database/extension.ts` | ⏳ 待开始 | 注册 DatabaseNavigator |
| 注册 SQL 编辑器面板 | `query/extension.ts`    | ⏳ 待开始 | 注册 SqlEditorPanel    |
| 注册结果面板        | `query/extension.ts`    | ⏳ 待开始 | 注册 ResultPanel       |

### 阶段 3：重构 WorkbenchView

| 任务         | 文件                | 状态      | 说明             |
| ------------ | ------------------- | --------- | ---------------- |
| 动态加载面板 | `WorkbenchView.vue` | ⏳ 待开始 | 从注册表读取面板 |
| 移除硬编码   | `WorkbenchView.vue` | ⏳ 待开始 | 删除写死的组件   |

---

## 开发进度

| 日期       | 任务                      | 状态      | 备注                             |
| ---------- | ------------------------- | --------- | -------------------------------- |
| 2026-04-28 | 创建架构设计文档          | ✅ 完成   | 本文档                           |
| 2026-04-28 | 实现 PanelRegistry        | ✅ 完成   | `src/core/panel-registry.ts`     |
| 2026-04-28 | 实现 CommandRegistry      | ✅ 完成   | `src/core/command-registry.ts`   |
| 2026-04-28 | 实现 WindowAPI            | ✅ 完成   | `src/core/window-api.ts`         |
| 2026-04-28 | 实现 ExtensionHost        | ✅ 完成   | `src/core/extension-host.ts`     |
| 2026-04-28 | 创建内置扩展注册表        | ✅ 完成   | `src/core/builtin-extensions.ts` |
| 2026-04-28 | 重构 Database 扩展        | ✅ 完成   | 注册数据库导航面板               |
| 2026-04-28 | 重构 Query 扩展           | ✅ 完成   | 注册 SQL 编辑器和结果面板        |
| 2026-04-28 | 重构 WorkbenchView        | ✅ 完成   | 从注册表动态加载面板             |
| 2026-04-28 | 集成扩展到 main.ts        | ✅ 完成   | 应用启动时激活扩展               |
| 2026-04-28 | 统一类型定义文件          | ✅ 完成   | 合并两套类型定义                 |
| 2026-04-28 | 修复 DockviewApi 类型     | ✅ 完成   | 添加类型定义和类型断言           |
| 2026-04-28 | 修复 DatabaseAPI 缺失方法 | ✅ 完成   | 添加 registerConnectionProvider  |
| 2026-04-28 | 运行应用验证              | ✅ 完成   | 无报错，扩展正常激活             |
|            | 清理预存在类型错误        | ⏳ 待开始 | 约 30+ 个预存在错误              |

---

## 架构变更摘要

### 新增文件

| 文件                             | 说明              |
| -------------------------------- | ----------------- |
| `src/core/panel-registry.ts`     | 面板注册表实现    |
| `src/core/command-registry.ts`   | 命令注册表实现    |
| `src/core/window-api.ts`         | 窗口 API 实现     |
| `src/core/extension-host.ts`     | 扩展主机实现      |
| `src/core/builtin-extensions.ts` | 内置扩展注册表    |
| `src/core/index.ts`              | Core 模块统一导出 |

### 修改文件

| 文件                                                          | 变更                                                       |
| ------------------------------------------------------------- | ---------------------------------------------------------- |
| `src/extensions/core/types.ts`                                | 添加 DatabaseDriverContribution、ConnectionProvider 等类型 |
| `src/extensions/builtin/database/extension.ts`                | 添加面板注册                                               |
| `src/extensions/builtin/query/extension.ts`                   | 添加面板注册                                               |
| `src/extensions/builtin/workbench/extension.ts`               | 统一类型导入路径                                           |
| `src/extensions/builtin/mysql-driver/extension.ts`            | 统一类型导入路径                                           |
| `src/extensions/builtin/workbench/ui/views/WorkbenchView.vue` | 从注册表动态加载面板，修复类型                             |
| `src/app/main.ts`                                             | 添加扩展激活逻辑                                           |

---

## 当前状态

### ✅ 已完成的核心功能

1. **插件注册系统** - 面板、命令、视图提供者注册
2. **扩展生命周期管理** - 激活、停用、资源清理
3. **动态面板加载** - WorkbenchView 从注册表动态创建面板
4. **类型系统统一** - 合并两套类型定义，消除类型冲突

### ⚠️ 预存在的问题（非本次重构引入）

约 30+ 个类型错误存在于以下模块：

- `DatabaseManager.vue` - `db_type` 属性缺失
- `database-navigator.vue` - 可选类型传递问题
- `navigator-context-menu-v2.vue` - 接口定义不完整
- 其他 composables 和组件

这些是代码库中已存在的问题，需要单独修复。

### 🚀 下一步建议

1. **运行应用测试** - 验证插件系统核心功能
2. **逐步修复预存在类型错误** - 按模块优先级修复
3. **完善插件 API** - 实现配置、通知等完整功能

---

## 技术决策记录

### 决策 1：Dockview 与插件系统的关系

**问题：** Dockview 布局是否与插件化设计理念冲突？

**决策：** 不冲突，两者是正交关注点。

**理由：**

- Dockview 负责布局管理（分栏、标签、拖拽）
- 插件系统负责内容注册（面板、命令、视图）
- Workbench 作为容器，从插件注册表读取面板并交给 Dockview 渲染

### 决策 2：面板注册方式

**问题：** 面板应该通过配置文件注册还是通过代码注册？

**决策：** 通过代码注册（`context.window.registerViewProvider`）。

**理由：**

- 符合 VSCode 扩展架构
- 支持动态注册/注销
- 支持插件生命周期管理
- 配置文件可以作为补充（用于布局预设）

### 决策 3：扩展加载时机

**问题：** 扩展应该在何时加载？

**决策：** 在项目激活时加载。

**理由：**

- 扩展运行在 Project 上下文中
- 不同项目可以有不同的扩展配置
- 支持项目级别的扩展启用/禁用

---

## 参考资料

- [VSCode Extension API](https://code.visualstudio.com/api)
- [Dockview Documentation](https://dockview.dev/)
- [项目架构文档](./docs/frontend/ARCHITECTURE.md)
- [扩展系统类型定义](./src/extensions/core/types.ts)
