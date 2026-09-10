# RdataStation 前端布局架构文档

## 1. 布局概述

RdataStation 采用 **100% Dockview 6.0 驱动的完整布局架构**，所有面板（左侧栏、中心区、右侧栏、底部栏）都通过 Dockview 统一管理。
侧边栏使用 **Edge Group API**，中心工作区和底部栏使用 **普通 Group**。

### 1.1 布局结构（VSCode 风格）

```
┌─────────────────────────────────────────────┐
│                                             │
│  A 区域 (Edge Group) │  B 区域 (普通 Group)  │ C 区域 (Edge Group) │
│      左侧边栏        │      中心工作区        │      右侧边栏       │
│                     │                       │                    │
│  • 数据库导航        │  ┌──────────────┐    │  • SQL 历史         │
│  • 分析资源管理      │  │   欢迎页      │    │  • 列洞察           │
│  • 插件             │  │              │    │                    │
│                     │  └──────────────┘    │                    │
│                     │                       │                    │
│                     │  (SQL 编辑器和结果     │                    │
│                     │   面板在用户操作时     │                    │
│                     │   动态创建)            │                    │
├─────────────────────┴───────────────────────┴────────────────────┤
│             底部栏（动态创建）                                    │
│    queryResult / multiTabResult / output                         │
├──────────────────────────────────────────────────────────────────┤
│                 状态栏 (WorkbenchStatusBar)                       │
└──────────────────────────────────────────────────────────────────┘
```

### 1.2 区域说明

| 区域                 | 技术实现   | 说明                                                                        |
| -------------------- | ---------- | --------------------------------------------------------------------------- |
| A 区域（左侧边栏）   | Edge Group | 数据库导航、分析资源管理、插件，支持折叠/展开                               |
| B 区域（中心工作区） | 普通 Group | **初始只显示欢迎页**（EmptyWorkbenchPanel），SQL 编辑器在用户操作时动态创建 |
| C 区域（右侧边栏）   | Edge Group | SQL 历史、列洞察，支持折叠/展开                                             |
| 底部栏               | 普通 Group | 查询结果、输出等，在用户执行 SQL 时动态创建                                 |
| 状态栏               | Vue 组件   | 独立的 Vue 组件，不在 Dockview 中                                           |

### 1.3 核心设计原则

**初始状态**：

- ✅ B 区域默认只显示欢迎页（EmptyWorkbenchPanel），不自动创建 SQL 编辑器
- ✅ SQL 编辑器和底部面板在用户点击"新建查询"或创建连接时动态创建
- ✅ SQL 编辑器打开时空内容区域显示快捷键水印提示，无独立欢迎页

**侧边栏（Edge Group）**：

- ✅ 左侧和右侧边栏使用 Dockview 6.0 Edge Group API
- ✅ 支持折叠/展开
- ✅ 侧边栏面板不可关闭

**自由拖拽**：

- ✅ B 区域内的面板可以相互拖拽
- ✅ B 区域面板支持最大化、弹出为浮动窗口
- ✅ 底部面板支持拖拽调整

---

## 2. 核心组件

### 2.1 组件层级

```
WorkbenchView (Vue 根布局组件，管理 Dockview 实例)
├── DockviewVue (Dockview 主布局，100% 面板管理)
├── WorkbenchStatusBar (状态栏 Vue 组件)
├── AddDataSourceDialog (新增数据源对话框)
└── CustomizeLayoutDialog (自定义布局对话框)
```

### 2.2 关键文件

| 文件路径                                                                        | 职责                                       |
| ------------------------------------------------------------------------------- | ------------------------------------------ |
| `src/extensions/builtin/workbench/ui/views/WorkbenchView.vue`                   | 核心布局组件，创建 Dockview 实例和所有面板 |
| `src/extensions/builtin/workbench/ui/components/WorkbenchStatusBar.vue`         | 状态栏组件                                 |
| `src/extensions/builtin/workbench/ui/components/panels/EmptyWorkbenchPanel.vue` | 欢迎页组件（B 区域默认显示）               |
| `src/extensions/builtin/workbench/ui/components/panels/SqlEditorPanel.vue`      | SQL 编辑器（动态创建，空时显示水印）       |
| `src/extensions/builtin/workbench/ui/stores/layout-store.ts`                    | 布局状态管理（Pinia）                      |
| `src/core/panel-registry.ts`                                                    | 面板注册表                                 |
| `src/core/window-api.ts`                                                        | 窗口 API，提供面板注册功能                 |
| `src/app/main.ts`                                                               | 应用入口，负责扩展激活和全局组件注册       |

---

## 3. 面板注册和加载

### 3.1 注册流程

```
extension.ts 注册面板
  ↓
window-api.ts.registerViewProvider(id, config)
  ↓
panel-registry 保存面板描述符（含 Vue 组件引用）
  ↓
main.ts 扩展激活后，遍历 panelRegistry 全局注册组件
  ↓
app.component(panel.id, component) → appContext.components 可用
  ↓
WorkbenchView.onReady() 时通过 api.addPanel() 创建面板
```

### 3.2 组件注册方式

Dockview-vue 6.0 通过 `findComponent()` 查找组件（查看源码 `dockview-vue.es.js:7`）：

```javascript
function findComponent(parent, name) {
  let instance = parent
  let component = null
  while (!component && instance) {
    component = instance.components[name] // 局部注册
    instance = instance.parent
  }
  if (!component) {
    component = parent.appContext.components[name] // 全局注册
  }
  if (!component) {
    throw new Error(`Failed to find Vue Component '${name}'`)
  }
  return component
}
```

**必须使用 `app.component(id, component)` 全局注册**，`<template #xxx>` 命名插槽不会被识别。

---

## 4. 布局初始化（onReady 执行顺序）

布局初始化在 `WorkbenchView.vue` 的 `onReady` 函数中。执行顺序非常重要：

```typescript
const onReady = (event: DockviewReadyEvent) => {
  const api = event.api

  // 第 1 步：B 区域欢迎页（普通 Group）
  api.addPanel({
    id: 'panel_emptyWorkbench',
    component: 'emptyWorkbench',
    title: '欢迎',
    position: { direction: 'right' },
  })

  // 第 2 步：左侧 Edge Group（自动包裹在中心内容左侧）
  api.addEdgeGroup('left', {
    id: 'left-edge',
    initialSize: 280,
    minimumSize: 200,
    maximumSize: 500,
  })
  // 向左侧 Edge Group 添加面板
  api.addPanel({
    id: 'panel_databaseNavigator',
    component: 'databaseNavigator',
    title: '数据库导航',
    position: { referenceGroup: 'left-edge' },
  })

  // 第 3 步：右侧 Edge Group（自动包裹在中心内容右侧）
  api.addEdgeGroup('right', {
    id: 'right-edge',
    initialSize: 280,
    minimumSize: 200,
    maximumSize: 500,
  })
}
```

### 4.1 SQL 编辑器和结果面板动态创建

SQL 编辑器和底部结果面板 **不在 onReady 中创建**，而是在用户操作时通过事件处理函数动态创建：

```typescript
// 用户点击"新建查询"时
const handleOpenSqlEditor = event => {
  dockviewApi.addPanel({
    id: `panel_sqlEditor_${counter}`,
    component: 'sqlEditor',
    title: `SQL ${counter}`,
    position: { direction: 'center' },
    params: { connectionId, databaseName, initialSql },
  })
}

// 用户创建连接时（handleSaveConnection）
dockviewApi.addPanel({
  /* SQL 编辑器 */
})
ensureResultPanel() // 动态创建结果面板在编辑器下方
```

### 4.2 SQL 编辑器水印提示

当 SQL 编辑器打开但没有编写 SQL 时，不再显示独立的欢迎页（原 `welcome-overlay`），而是在编辑器上方显示半透明水印提示：

```
┌─────────────────────────────────┐
│  工具栏                         │
├─────────────────────────────────┤
│  (编辑器水印，透明度 0.35)       │
│  SQL 编辑器                      │
│  Ctrl+Enter 执行                │
│  Ctrl+Shift+F 格式化            │
│  Ctrl+/ 注释                    │
│  F5 执行全部                    │
├─────────────────────────────────┤
│  状态栏                         │
└─────────────────────────────────┘
```

水印特点：

- `opacity: 0.35` 半透明显示
- `pointer-events: none` 不阻塞编辑器交互
- 用户开始输入后自动消失

---

## 5. 主要变更记录

| 日期       | 变更内容                                              | 原因                                                   |
| ---------- | ----------------------------------------------------- | ------------------------------------------------------ |
| 2026-05-06 | B 区默认只显示欢迎页，不自动创建 SQL 编辑器和底部面板 | 优化初始加载体验，降低启动复杂度                       |
| 2026-05-06 | SQL 编辑器欢迎页改为水印提示                          | 节省空间，更简洁，不干扰用户                           |
| 2026-05-06 | 侧边栏改为 Edge Group API                             | 支持折叠/展开/自动隐藏                                 |
| 2026-05-06 | 全局组件注册方式改为 app.component()                  | dockview-vue 6.0 的 findComponent 不识别 template slot |
| 2026-05-06 | 扩展激活时序改为先激活再挂载 Vue                      | 避免竞态条件导致面板注册延迟                           |
