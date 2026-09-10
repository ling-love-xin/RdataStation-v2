# ⛔ 已废弃 — 编辑器与结果集管理架构

> **此文档已废弃，不再维护。**
> 原因：编辑器引擎已从 Monaco Editor 迁移至 CodeMirror 6（2026-05-17 完成）。
> Monaco Editor 相关文件（monaco-theme.ts、useMonacoEditor.ts、useEditorSettings.ts、SqlEditorPanel.vue、CodeEditorPanel.vue、CodeEditorStatusbar.vue、sql-dialect-highlight.ts）已全部删除。
> 替代文档：[编辑器架构设计文档 v3.0](../editor-architecture-v3.md)
>
> 以下为历史存档内容，仅供参考。

---

# 编辑器与结果集管理架构 — 完整设计文档（已废弃）

> 版本：v2.0 ⛔ 已废弃
> 创建日期：2026-05-16
> 最后更新：2026-05-17
> 状态：✅ 架构重构完成（ITextModel 单例 + dockview 原生 Float Tab + 每文件 Editor）
> 关联：[sql-editor/design.md](../sql-editor/design.md) · [query-result/design.md](../query-result/design.md)

---

## 📖 目录

- [0. 概念说明](#0-概念说明)
- [1. 背景与原始问题](#1-背景与原始问题)
- [2. 核心原则：ITextModel 单例 + dockview 原生 Float Tab + 按需 AG Grid](#2-核心原则itextmodel-单例--dockview-原生-float-tab--按需-ag-grid)
- [3. 核心组件职责划分](#3-核心组件职责划分)
- [4. 文件 → Group 的映射关系](#4-文件--group-的映射关系)
- [5. 两种编辑器状态：普通文件 vs 分析面板](#5-两种编辑器状态普通文件-vs-分析面板)
- [6. 结果集管理机制（路线 C 混合方案）](#6-结果集管理机制路线-c-混合方案)
- [7. 结果集的独立管理（钉住与拖拽）](#7-结果集的独立管理钉住与拖拽)
- [8. 弹出功能](#8-弹出功能)
- [9. 全局快捷键管理](#9-全局快捷键管理)
- [10. 与 Vue 响应式的集成](#10-与-vue-响应式的集成)
- [11. 数据流架构](#11-数据流架构)
- [12. 组件树与职责矩阵](#12-组件树与职责矩阵)
- [13. 接口契约](#13-接口契约)
- [14. 文件结构清单](#14-文件结构清单)
- [15. 开发路线图](#15-开发路线图)

---

## 0. 概念说明

### 0.1 模块名称演进

| 阶段                  | 称呼                    | 说明                                           |
| --------------------- | ----------------------- | ---------------------------------------------- |
| Phase 1-3             | SQL 编辑器（sqlEditor） | 仅支持 SQL 文件的编辑和执行                    |
| Phase 4-5             | 编辑面板模块            | 加入 CodeEditorPanel，支持通用文本文件         |
| **Phase 6（本文档）** | **统一编辑器系统**      | 单 Editor 实例 + 多 Model，融合 SQL 与通用编辑 |

### 0.2 三种架构方案对比

```
方案 A（Phase 1-4）：N 个 Panel → N 个 Editor → 内存线性增长
方案 B（Phase 5 草案）：1 个 Panel → 1 个 Editor → DOM 迁移 → 结果集手动管理
方案 C（本文档 v2.0）：N 个 Panel（同 Group Tab）→ N 个 Editor（懒创建）→ dockview 原生 Float Tab + AG Grid 按需
```

### 0.3 什么是"结果集独立管理"

结果集不仅可以从属于文件，还可以脱离文件成为独立的数据资产。用户可将某个结果集 Tab 从文件 Group 中拖出，形成独立的浮动 Group。拖出后的结果集面板不再跟随原文件，即使原文件关闭，该结果集依然保留。

### 0.4 本次重构要点

- 🔴 **破坏性变更**：废弃 SqlEditorPanel.vue、CodeEditorPanel.vue，由 EditorPanel.vue 统一取代
- 🟡 **Store 重构**：sql-execution-store、result-store 合并简化
- 🟢 **向后兼容**：Tauri IPC 层不变，草稿箱 API 不变，前端 CustomEvent 接口保留
- 🔵 **v2.0 架构升级**：ITextModel 单例 + 每文件 Editor + dockview 原生 Float Tab + dockview direction:'within' 同 Group Tab

---

## 1. 背景与原始问题

### 1.1 原始 Bug

最初的前端编辑器架构存在"多实例"问题——每个打开的文件都创建一个独立的 Monaco Editor 实例。这导致三个核心问题：

1. **焦点争夺**：多个 Editor 实例互相抢占焦点，导致全局快捷键失效。具体表现为：打开编辑器后，草稿箱的"新建"按钮和标题栏的"更多功能 → 新建"按钮均无法使用。

2. **快捷键冲突**：每个 Editor 实例各自注册快捷键，与业务面板的快捷键互相干扰，且无法按面板作用域隔离。

3. **资源浪费**：每个 Editor 实例都加载独立的 DOM、内核和 Worker 线程，内存和 CPU 消耗随打开文件数量线性增长。

```
当前内存模型（10 个文件打开）：
┌──────────────────────────────────────────────────────┐
│ Monaco Editor  ×10  = ~800MB                        │
│ AG Grid        ×10  = ~400MB                        │
│ Vue 组件壳     ×20  = ~40MB                         │
│ 合计                 ≈ 1.2GB                        │
└──────────────────────────────────────────────────────┘
```

### 1.2 问题根因分析

```
焦点争夺链路：
  Editor #1 获得焦点 → 注册快捷键 Ctrl+S, Ctrl+Enter
  Editor #2 获得焦点 → 注册快捷键 Ctrl+S, Ctrl+Enter（覆盖 #1）
  Editor #3 获得焦点 → 注册快捷键 Ctrl+S, Ctrl+Enter（覆盖 #2）
  → 全局 keydown 被 Editor 容器拦截
  → 草稿箱按钮绑定的事件收不到
  → 按钮"表现失效"
```

---

## 2. 核心原则：ITextModel 单例 + dockview 原生 Float Tab + 按需 AG Grid

### 2.1 Editor / Model 层

```
每个打开的文件对应一个 ITextModel（全局唯一）：
  - 包含：文件内容、光标位置、撤销栈、语言模式
  - 轻量级（~3MB/个），创建和销毁成本极低
  - 使用 markRaw 标记，阻止 Vue 深度代理

每个文件对应一个 dockview Panel + 自己的 Monaco Editor：
  - Panel 作为 dockview tab 存在于主 Group 中
  - Monaco Editor 在 Panel 首次可见时懒创建
  - Editor 创建后持续复用，切换文件不重建
  - 使用 shallowRef + markRaw 包裹，阻止 Vue 深度代理

切换文件 = dockview 原生 tab 激活：
  - dockviewApi.getPanel(pid).focus()
  - EditorPanel 组件实例不变，Monaco Editor 不变
  - 毫秒级响应
```

### 2.2 AG Grid 层（路线 C 混合方案）

```
组内结果集 Tab（dockview 原生，AG Grid 按需创建/销毁）：
  - 同 Group 内始终最多 1 个活跃 AG Grid
  - 非活跃 Tab 的 AG Grid 自动销毁，释放 ~40MB/个
  - 销毁前保存 Grid 状态（排序、筛选、滚动位置），切换回时恢复
  - 切换 delay ~200ms（AG Grid 重建时间）

拖出为独立结果集（始终持有 AG Grid）：
  - 脱离 Group 的 FileResultPanel 始终保留 AG Grid 实例
  - 可与其他结果集并排观看（真正的 dockview 多 Panel 布局）
  - 独立 Panel 不受原文件 Group 显隐影响
```

### 2.3 内存模型对比

```
目标内存（10 个文件，每文件 1 个可见结果集，2 个独立浮动结果集）：
┌──────────────────────────────────────────────────────┐
│ Monaco Editor  ×1   = ~80MB                         │
│ ITextModel     ×10  = ~30MB                         │
│ AG Grid        ×3   = ~120MB  (1 活跃 + 2 浮动)     │
│ Vue 组件壳     ×15  = ~15MB                         │
│ 合计                ≈ 245MB  ← 相比当前节省 80%     │
└──────────────────────────────────────────────────────┘
```

---

## 3. 核心组件职责划分

### 3.1 全局管理器（3 个）

```
┌──────────────────────────────────────────────────────────────────┐
│                        全局管理器层                               │
│                                                                  │
│  EditorManager（全局单例）                                       │
│  ├─ 管理 fileEditors Map（每个文件一个 Monaco Editor，懒创建）    │
│  ├─ 管理所有 ITextModel 的创建、切换、销毁（ITextModel 单例）     │
│  ├─ 管理文件路径 → dockview Panel ID 的映射                       │
│  ├─ 处理文件打开（openFile）、关闭（closeFile）                   │
│  ├─ 根据文件后缀判断文件类型（sql / python / plaintext / ...）    │
│  ├─ 管理活跃文件的 isDirty 状态（通过 Model.onDidChangeContent）  │
│  └─ onPanelActivated：Panel 激活时同步 activeFilePath、editorRef、结果集可见性 │
│                                                                  │
│  ResultPanelManager（全局单例）                                  │
│  ├─ 在指定文件的 Group 中添加新的结果集 Panel                     │
│  ├─ 管理结果集 Panel 的创建、销毁和迁移                           │
│  ├─ 处理结果集的独立管理（钉住、拖拽）                            │
│  └─ 维护 filePath → ResultSetMetadata[] 映射                     │
│                                                                  │
│  ShortcutManager（全局单例）                                     │
│  ├─ 按面板作用域注册和注销快捷键                                  │
│  ├─ activeScope: 'editor' | 'scratchpad' | 'result' | 'global'  │
│  ├─ 确保快捷键只在当前激活的面板中生效                            │
│  └─ dockview onDidActivePanelChange → 更新 activeScope           │
└──────────────────────────────────────────────────────────────────┘
```

### 3.2 视图组件（2 个 Panel 组件 + 1 个 Result 组件）

```
┌──────────────────────────────────────────────────────────────────┐
│                       视图组件层                                  │
│                                                                  │
│  EditorPanel.vue（dockview Panel 组件）                          │
│  ├─ 内部包含自定义文件标签栏（n-tabs），数据源 EditorManager.openFiles │
│  ├─ 自己的 Monaco Editor（首次可见时懒创建，EditorManager.registerFileEditor） │
│  ├─ 根据当前文件类型动态显示/隐藏工具栏区域                       │
│  ├─ 标签栏点击 → EditorManager.switchToFile()                   │
│  ├─ 标签栏关闭 → EditorManager.closeFile()                      │
│  ├─ 标签栏拖出 → dockview 原生 Float Tab（无需自定义 popOutFile） │
│  └─ 与当前活跃结果集的分割条（splitRatio）                       │
│                                                                  │
│  FileResultPanel.vue（dockview Panel 组件）                      │
│  ├─ 单个结果集的展示面板                                          │
│  ├─ AG Grid 按需创建/销毁（组内 Tab 切换 + 独立浮动判断）        │
│  ├─ 销毁前保存 Grid 状态，重建时恢复（columnState, filterModel, sortModel） │
│  ├─ Props: resultSetId, columns, rows, rowCount, elapsedMs      │
│  └─ 支持导出（CSV / JSON / Excel）                               │
│                                                                  │
│  ResultSubTab.vue（结果集子 Tab 栏组件，嵌套在 EditorPanel 内）   │
│  ├─ 显示当前文件的所有结果集 Tab（[结果1 100行] [结果2 50行]）   │
│  ├─ 只渲染在当前文件的 Group 内，不暴露到 dockview 层面           │
│  └─ 点击切换 → EditorManager.activeFile.activeResultIndex 变化   │
└──────────────────────────────────────────────────────────────────┘
```

### 3.3 组件定位策略

```
dockview 布局示意（新架构：dockview 原生 Tab + 各自 Editor）：

┌─ left-edge ────────────────────┬─ center（主 Tab Group） ────────────────────────┬─ right-edge ──┐
│                                │                                                │               │
│  ScratchpadPanel               │  Group: 主编辑区（dockview 原生 Tab 切换）      │  属性面板      │
│  (dockview Panel，固定)        │  ┌─ EditorPanel ────────────────────────────┐  │               │
│                                │  │ n-tabs: [users●] [orders] [report]       │  │               │
│                                │  │ ┌─ Monaco Editor（当前文件）────────────┐ │  │               │
│                                │  │ │ SELECT * FROM users                   │ │  │               │
│                                │  │ └───────────────────────────────────────┘ │  │               │
│                                │  │ ── split handle ──                        │  │               │
│                                │  │ ┌─ ResultSubTab ───────────────────────┐ │  │               │
│                                │  │ │ [结果1 100行] [结果2 50行]            │ │  │               │
│                                │  │ ├─ dockview Tab 区 ────────────────────┤ │  │               │
│                                │  │ │ FileResultPanel (结果1) ●            │ │  │               │
│                                │  │ │ FileResultPanel (结果2)              │ │  │               │
│                                │  │ └──────────────────────────────────────┘ │  │               │
│                                │  └───────────────────────────────────────────┘  │               │
│                                │                                                │               │
│                                │  （dockview Tab: users.sql / orders.sql / ...） │               │
│                                │  每个 Tab 是独立的 EditorPanel，持有自己的       │               │
│                                │  Monaco Editor + n-tabs + 结果集区域            │               │
└────────────────────────────────┴────────────────────────────────────────────────┴───────────────┘

dockview 原生 Float Tab（拖出/弹出）：

┌─ Floating Group（dockview 自动创建）─────────────┐
│  EditorPanel（users.sql）                         │
│  ┌─ n-tabs: [users●] [orders] [report] ────────┐ │
│  ├─ Monaco Editor ─────────────────────────────┤ │
│  ├─ ResultSubTab + FileResultPanel ─────────────┤ │
│  └──────────────────────────────────────────────┘ │
│  无需 DOM 迁移，dockview 原生管理 Panel 生命周期    │
└───────────────────────────────────────────────────┘

独立浮动结果集（从 Group 拖出）：

┌─ Floating Group ────────────────────────────┐
│  FileResultPanel (结果2 - 独立)              │
│  ┌─ AG Grid ──────────────────────────────┐ │
│  │ id │ name  │ email                     │ │
│  │ 1  │ Bob   │ bob@test.com              │ │
│  └────────────────────────────────────────┘ │
│  始终持有 AG Grid，不随文件关闭而销毁        │
└──────────────────────────────────────────────┘
```

---

## 4. 文件 → Group 的映射关系

### 4.1 数据结构

```typescript
// EditorManager 核心状态
interface OpenFileInfo {
  model: monaco.editor.ITextModel // Monaco TextModel（markRaw，全局单例）
  filePath: string // 草稿箱相对路径
  fileName: string // 显示名称
  language: string // sql | python | json | plaintext
  type: 'file' | 'analysis' // 文件类型
  isDirty: boolean // 未保存标记
  connectionId: string // 绑定的数据库连接（SQL 文件）
  databaseName: string // 绑定的数据库名
  resultSets: ResultSetMetadata[] // 结果集元数据
  activeResultIndex: number // 当前选中的结果集索引
  resultPanelIds: string[] // 文件 Group 内结果集 Panel ID 列表
}

interface ResultSetMetadata {
  id: string
  title: string // "结果1" | "2026-05-16 14:30"
  columns: string[]
  totalRowCount: number
  elapsedMs: number
  affectedRows: number
  messages: string
  sql: string
  timestamp: number
  gridState?: GridStateSnapshot // AG Grid 状态快照
  rows?: unknown[][] // 按需加载的数据
}

// EditorManager 全局状态
const openFiles = shallowRef<Map<string, OpenFileInfo>>(new Map())
const activeFilePath = ref<string | null>(null)
const fileEditors = new Map<string, monaco.editor.IStandaloneCodeEditor>()
const editorRef = shallowRef<monaco.editor.IStandaloneCodeEditor | null>(null)
const tabGroupId: Ref<string | null> = ref(null)
const dockviewApi = shallowRef<DockviewVueApi | null>(null)
```

### 4.2 文件切换流程

```
用户点击 n-tabs 中 orders.sql 标签
  │
  ▼
EditorPanel.onTabClick('orders.sql')
  │
  ▼
EditorManager.switchToFile('orders.sql')
  │
  ├─► 1. dockview 原生 tab 激活
  │     通过 dockviewApi.getPanel(pid)?.focus() 切换
  │     dockview 自动处理 Panel 显隐
  │
  ├─► 2. 切换 ITextModel
  │     新的 EditorPanel 已持有自己的 Editor（懒创建）
  │     Editor 在 onMounted 时自动 setModel(targetFileInfo.model)
  │     或通过 onPanelActivated 同步
  │
  ├─► 3. 更新活跃文件引用
  │     activeFilePath.value = 'orders.sql'
  │
  ├─► 4. 更新 editorRef
  │     editorRef.value = fileEditors.get('orders.sql')
  │
  ├─► 5. 切换结果集可见性
  │     当前文件的 resultPanelIds → setVisible(true)
  │     其他文件的 resultPanelIds → setVisible(false)
  │
  └─► 6. n-tabs 响应式更新（所有 EditorPanel 共享 openFiles 数据源）
```

### 4.3 dockview 原生 Tab 管理

```
新架构不使用 Group 显隐，而是利用 dockview 原生 tab 机制：

1. 所有 EditorPanel 注册进同一个 Group（tabGroupId）
2. dockview tab 切换自动处理 Panel 显隐（原生 CSS）
3. dockview 原生 Float Tab 处理弹出（无需自定义 popOutFile）
4. 每个 EditorPanel 持有自己的 Monaco Editor（懒创建，不复用）

优点：
  - 零 DOM 迁移：切换文件 = dockview tab 激活，毫秒级
  - 零自定义代码：浮动弹出完全由 dockview 管理
  - 内存可控：切换出时 Editor 保持但 AG Grid 销毁，切回时恢复
```

### 4.4 n-tabs 数据源

```
所有 Group 的 EditorPanel 共享同一个 EditorManager.openFiles 作为 n-tabs 数据源

原因：
  1. 数据一致性：用户需从任意 Group 切换到任意文件，必须共享文件列表
  2. 切换逻辑统一：onTabClick → EditorManager.switchToFile()，全局统一入口
  3. 关闭逻辑统一：onTabClose → EditorManager.closeFile()，所有 Group 同步更新

实现：
  // EditorPanel.vue
  const editorManager = useEditorManager()
  const tabs = computed(() =>
    Array.from(editorManager.openFiles.entries()).map(([path, info]) => ({
      key: path,
      label: info.fileName,
      isDirty: info.isDirty,
      isActive: path === editorManager.activeFilePath.value,
    }))
  )
```

### 4.5 文件打开流程

```
handleOpenSqlEditor(event)  // CustomEvent 'open-sql-editor'
  │
  ▼
EditorManager.openFile({
  filePath: event.detail.scratchpadRelativePath,
  fileName: event.detail.scratchpadFileName || 'Untitled',
  language: event.detail.language || 'sql',
  sql: event.detail.sql || '',
  connectionId: event.detail.connectionId,
  databaseName: event.detail.databaseName,
})
  │
  ├─► 1. 去重检查
  │     if (openFiles.has(filePath)) {
  │       switchToFile(filePath)  // 已打开 → 切换
  │       return
  │     }
  │
  ├─► 2. 创建 ITextModel
  │     const model = monaco.editor.createModel(sql, language)
  │     markRaw(model)  // 阻止 Vue 深度代理
  │
  ├─► 3. 创建 dockview Panel（同一个 Group 内 Tab）
  │     const pid = filePanelId(filePath)
  │     const isFirstFile = openFiles.value.size === 0
  │     const refGroup = tabGroupId.value
  │     dockviewApi.addPanel({
  │       id: pid,
  │       component: 'editorPanel',
  │       title: fileName,
  │       position: isFirstFile || !refGroup
  │         ? { direction: 'right' }
  │         : { referenceGroup: refGroup, direction: 'within' },
  │     })
  │
  │     // 首文件创建后捕获 Group ID（后续文件使用 direction: 'within'）
  │     if (isFirstFile) {
  │       captureGroup(pid)  // 轮询直到获取 group.id → tabGroupId
  │     }
  │
  ├─► 4. 注册到 openFiles
  │     openFiles.set(filePath, {
  │       model, filePath, fileName, language,
  │       type: 'file', isDirty: false,
  │       connectionId, databaseName,
  │       resultSets: [], activeResultIndex: -1, resultPanelIds: [],
  │     })
  │
  └─► 5. 自动切换到新文件
        switchToFile(filePath)
```

### 4.6 文件关闭流程

```
EditorManager.closeFile(filePath)
  │
  ├─► 1. 获取文件信息
  │     const info = openFiles.get(filePath)
  │
  ├─► 2. 销毁跟随文件的结果集（已独立拖出的不受影响）
  │     for (const resultPanelId of info.resultPanelIds) {
  │       const panel = dockviewApi.getPanel(resultPanelId)
  │       if (panel && !isPanelDetached(panel)) {
  │         panel.api.close()  // dockview 原生关闭
  │       }
  │     }
  │
  ├─► 3. 销毁 ITextModel
  │     info.model.dispose()
  │
  ├─► 4. 关闭 dockview Panel
  │     const pid = filePanelId(filePath)
  │     dockviewApi.getPanel(pid)?.api.close()
  │
  ├─► 5. 清理 fileEditors Map
  │     const ed = fileEditors.get(filePath)
  │     if (ed) { ed.dispose(); fileEditors.delete(filePath) }
  │
  ├─► 6. 从 openFiles 移除
  │     openFiles.delete(filePath)
  │
  ├─► 7. 如果还有打开的文件，切换到第一个
  │     if (openFiles.size > 0) {
  │       switchToFile(openFiles.keys().next().value)
  │     }
  │
  └─► 8. 清理 draft（useEditorPersistence.remove）
```

---

## 5. 两种编辑器状态：普通文件 vs 分析面板

### 5.1 状态定义

```
每个 EditorPanel 持有自己的 Monaco Editor，支持两种不同的使用场景：

┌─ 普通文件编辑 ──────────────────────────────────┐
│                                                  │
│ 触发：打开草稿箱 .sql/.py/.r/.json 文件           │
│ 工具栏：完全隐藏                                  │
│ 功能：纯文本编辑                                  │
│ 布局：n-tabs + Editor + split + ResultSubTab     │
│                                                  │
└──────────────────────────────────────────────────┘

┌─ 分析面板 ───────────────────────────────────────┐
│                                                  │
│ 触发：结果集"分析"入口 / "打开分析面板"入口        │
│ 工具栏：完整显示                                  │
│  ├─ SQL 执行按钮                                 │
│  ├─ DuckDB 加速按钮                              │
│  ├─ 模式切换器（查询/分析/智能）                  │
│  ├─ 格式化/验证/转译按钮                         │
│  └─ 连接选择器                                   │
│ 功能：完整 SQL 分析和执行能力                     │
│ 布局：工具栏 + n-tabs + Editor + split + ResultSubTab │
│                                                  │
└──────────────────────────────────────────────────┘
```

### 5.2 状态判断

```typescript
// EditorPanel.vue
const showToolbar = computed(() => {
  const active = editorManager.activeFileInfo.value
  if (!active) return false
  return active.type === 'analysis' || active.language === 'sql'
})

const showSqlActions = computed(() => {
  const active = editorManager.activeFileInfo.value
  return active?.type === 'analysis'
})
```

### 5.3 分析面板创建流程

```
用户点击"打开分析面板"入口
  │
  ▼
EditorManager.openFile({
  filePath: '__analysis__',         // 特殊虚拟路径
  fileName: '分析面板',
  language: 'sql',
  sql: '',
  type: 'analysis',                // 标记为分析面板类型
  connectionId: lastConnectionId,
})
  │
  ▼
EditorPanel 检测到 type === 'analysis'
  → showToolbar = true
  → showSqlActions = true
  → 渲染完整工具栏
```

### 5.4 分析面板与普通文件的交互

```
分析面板执行 SQL 生成结果集
  │
  ▼
ResultPanelManager.addResultSet('__analysis__', resultData)
  │
  ▼
分析面板 Group 内创建 FileResultPanel
  │
  ▼
如果用户手动切换 n-tabs 到普通文件
  → 工具栏隐藏（检测到 type !== 'analysis'）
  → 结果集 Tab 区刷新为该文件的结果集
  → Editor Model 切换为该文件的 Model
```

---

## 6. 结果集管理机制（路线 C 混合方案）

### 6.1 两层结果集架构

```
┌─ 第一层：ResultSubTab（n-tabs 逻辑 Tab）────────────────────────┐
│                                                                  │
│  显示当前文件的所有结果集列表                                     │
│  定位：EditorPanel 内部，分割条下方                               │
│  数据源：editorManager.activeFileInfo.resultSets                 │
│  交互：点击切换 → activeResultIndex 变化 → dockview Tab 联动     │
│                                                                  │
│  [结果1 100行] [结果2 50行] [结果3 0行] [消息]                   │
│        ●                     ↑                                   │
│   当前选中              （dockview Panel 同步高亮）               │
│                                                                  │
└──────────────────────────────────────────────────────────────────┘

┌─ 第二层：dockview 原生 Tab（FileResultPanel）────────────────────┐
│                                                                  │
│  每个结果集对应一个 FileResultPanel                               │
│  以 dockview Tab 形式堆叠在 Group 的下方区域                      │
│  Panel ID：panel_result_{filePath_hash}_{resultId}               │
│  标题：显示查询时间或序号                                         │
│                                                                  │
│  ┌─ Tab: [结果1 100行] ─── Tab: [结果2 50行] ─── Tab: [结果3] ─┐ │
│  │  FileResultPanel (active → 持有 AG Grid)                     │ │
│  │  FileResultPanel (hidden → AG Grid 已销毁)                   │ │
│  │  FileResultPanel (hidden → AG Grid 已销毁)                   │ │
│  └──────────────────────────────────────────────────────────────┘ │
│                                                                  │
└──────────────────────────────────────────────────────────────────┘
```

### 6.2 结果集创建流程

```typescript
// 用户执行 SQL 后
function storeResult(
  filePath: string,
  data: {
    columns: string[]
    rows: unknown[][]
    totalRows: number
    elapsedMs: number
    affectedRows: number
    sql: string
    error: string | null
  }
) {
  const fileInfo = openFiles.get(filePath)
  if (!fileInfo) return

  // 1. 创建结果集元数据
  const resultId = `result_${Date.now()}_${Math.random().toString(36).slice(2, 7)}`
  const metadata: ResultSetMetadata = {
    id: resultId,
    title: data.error ? '错误' : `结果${fileInfo.resultSets.length + 1}`,
    columns: data.columns,
    totalRowCount: data.totalRows,
    elapsedMs: data.elapsedMs,
    affectedRows: data.affectedRows,
    messages: data.error || `Query OK, ${data.totalRows} rows`,
    sql: data.sql,
    timestamp: Date.now(),
  }

  // 2. 添加到文件的结果集列表
  fileInfo.resultSets.push(metadata)
  fileInfo.activeResultIndex = fileInfo.resultSets.length - 1

  // 3. 在 dockview Group 中创建 FileResultPanel
  const panelId = `panel_result_${sanitizeFilePath(filePath)}_${resultId}`
  const editorPanelId = filePanelId(filePath)
  const editorPanel = dockviewApi.value?.getPanel(editorPanelId)
  const group = editorPanel?.group
  if (group) {
    // 找到 Group 下方的结果区域 Panel 作为参考
    const refPanelId =
      fileInfo.resultPanelIds.length > 0
        ? fileInfo.resultPanelIds[fileInfo.resultPanelIds.length - 1]
        : editorPanelId

    dockviewApi.value?.addPanel({
      id: panelId,
      component: 'fileResultPanel',
      title: metadata.title,
      position: { referencePanel: refPanelId, direction: 'below' },
      params: {
        resultSetId: resultId,
        columns: data.columns,
        rows: data.rows,
        totalRowCount: data.totalRows,
        elapsedMs: data.elapsedMs,
        filePath,
      },
    })
    fileInfo.resultPanelIds.push(panelId)
  }

  // 4. 栅格化数据（可以延迟到 Panel 激活时做）
  // rows 可以暂存在 metadata 中，也可以通过 ResultPanelManager.getRows(resultId) 懒加载
}
```

### 6.3 AG Grid 生命周期（按需创建/销毁）

```typescript
// FileResultPanel.vue
const panelApi = usePanelApi()
const isActive = ref(false)
const isDetached = ref(false) // 是否已脱离文件 Group
const gridInstance = shallowRef<AgGridVue | null>(null)
const gridState = shallowRef<GridStateSnapshot | null>(null)

// 判断是否需要保留 AG Grid
const shouldKeepGrid = computed(() => isDetached.value)
// 独立 Panel 始终保留 AG Grid，组内 Tab 按需创建/销毁

watch([() => panelApi?.isActive, isDetached], ([active, detached]) => {
  isActive.value = active

  if (detached) {
    // 独立 Panel：确保 AG Grid 存活
    if (!gridInstance.value) {
      nextTick(() => createGrid())
    }
    return
  }

  if (active && !gridInstance.value) {
    // 组内激活：创建 AG Grid
    nextTick(() => createGrid())
  }

  if (!active && gridInstance.value) {
    // 组内非激活：保存状态 + 销毁 AG Grid
    gridState.value = saveGridState()
    gridInstance.value.destroy()
    gridInstance.value = null
  }
})

function createGrid() {
  gridInstance.value = createAgGrid(domRef.value!, {
    columnDefs: toAgGridColDefs(props.columns),
    rowData: props.rows,
  })

  // 恢复之前保存的状态
  if (gridState.value) {
    nextTick(() => restoreGridState(gridState.value!))
    gridState.value = null
  }
}

function saveGridState(): GridStateSnapshot {
  if (!gridApi.value) return {}
  return {
    columnState: gridApi.value.getColumnState(),
    filterModel: gridApi.value.getFilterModel(),
    sortModel: gridApi.value.getSortModel(),
  }
}

function restoreGridState(state: GridStateSnapshot) {
  if (!gridApi.value || !state.columnState) return
  gridApi.value.applyColumnState({ state: state.columnState, applyOrder: true })
  if (state.filterModel) gridApi.value.setFilterModel(state.filterModel)
}
```

### 6.4 与草稿稿箱的交互

```
草稿箱文件被打开 → EditorManager.openFile() → 创建 Group + EditorPanel
草稿箱文件被删除（外部） → EditorManager.closeFile() → 销毁 Group + Model
草稿箱文件内容变更（外部） → 重新加载 Model 内容
```

---

## 7. 结果集的独立管理（钉住与拖拽）

### 7.1 拖出为独立结果集

```
用户拖拽结果集 Tab 从 Group 中拖出
  │
  ▼
dockview 原生拖拽事件触发
  │  onDidRemovePanel（从原 Group 移除）
  │  onDidAddPanel（添加到新浮动 Group）
  │
  ▼
FileResultPanel 检测到自己变为独立 Panel
  │  isDetached.value = true
  │  → 永不销毁 AG Grid
  │  → 标题变为可编辑（重命名功能）
  │
  ▼
该结果集从原 fileInfo.resultPanelIds 中移除（不再跟随文件）
```

### 7.2 关闭文件时只销毁跟随结果集

```typescript
function closeFile(filePath: string) {
  const info = openFiles.get(filePath)
  if (!info) return

  // 只关闭仍在 Group 内的结果集 Panel
  for (const panelId of info.resultPanelIds) {
    const panel = dockviewApi.value?.getPanel(panelId)
    if (panel) {
      panel.api.close()
    }
  }

  // 已拖出为独立的 FileResultPanel 不受影响
  // 它们已从 resultPanelIds 中移除，不属于该文件

  // 销毁 Model + Panel
  info.model.dispose()
  const pid = filePanelId(filePath)
  dockviewApi.value?.getPanel(pid)?.api.close()
  openFiles.delete(filePath)
}
```

### 7.3 重命名独立结果集

```
用户右键独立结果集 Tab → "重命名"
  │
  ▼
显示内联输入框（n-input），默认值 = Panel 标题
  │
  ▼
用户确认 → panel.api.setTitle(newName)
  │
  ▼
后续可支持：归档到项目资产（存储到项目 DuckDB）
```

---

## 8. 弹出功能（dockview 原生 Float Tab）

### 8.1 设计原则

```
弹出功能完全由 dockview 原生 Float Tab 机制接管，不编写任何自定义 popOutFile 代码：

┌─ dockview 原生能力 ────────────────────────────────────┐
│                                                        │
│  1. 拖拽 Tab 到窗口外 → dockview 自动创建浮动 Group     │
│  2. addPanel({ floating: { x, y, width, height } })    │
│  3. dockview 负责 Panel 的 DOM 生命周期管理              │
│  4. 浮动 Panel 支持拖回（merge back）                    │
│                                                        │
│  无需：DOM 迁移（removeChild → appendChild）            │
│  无需：自定义 popOutFile / mergeBackFile 方法           │
│  无需：isPoppedOut 状态追踪                              │
└────────────────────────────────────────────────────────┘

两层 tab 结构配合：
  - dockview Panel tab（上层）：显示当前激活文件名，支持 Float Tab 弹出
  - n-tabs 自定义标签栏（下层）：文件切换，不受弹出影响
```

### 8.2 弹出流程（dockview 原生）

```
用户拖拽 dockview Tab 到窗口外（或使用"弹出为独立窗口"菜单）
  │
  ▼
dockview API 自动处理：
  ├─► 1. 从原 Group 移除 Panel（onDidRemovePanel 事件）
  ├─► 2. 创建浮动 Group（独立的浏窗口或主窗口内浮动）
  ├─► 3. EditorPanel 组件保持挂载，Monaco Editor 不重建
  ├─► 4. ITextModel 不受影响（全局单例，仍然有效）
  │
  EditorManager 只需响应 dockview 事件：
  ├─► 无额外操作：EditorPanel 组件实例不变
  ├─► 无额外操作：fileEditors Map 中的 Editor 仍然有效
  └─► 浮动后 EditorPanel 仍可正常编辑、执行 SQL、显示结果集
```

### 8.3 与旧方案对比

```
旧方案（已废弃）：
  EditorManager.popOutFile(filePath)
  → DOM 迁移（removeChild → appendChild → layout）
  → 结果集 Panel 逐个迁移
  → isPoppedOut 状态追踪
  → 关闭浮动窗口需 mergeBackFile
  ❌ 大量自定义代码、DOM 操作脆弱、状态追踪复杂

新方案（dockview 原生）：
  用户拖拽 dockview Tab
  → dockview 自动创建浮动 Group
  → EditorPanel 保持不变（Component 不重新挂载）
  → Monaco Editor 保持引用不变（fileEditors Map 无变化）
  → 关闭浮动窗口只需关闭 Tab（dockview 原生清理）
  ✅ 零自定义代码、零 DOM 操作、dockview 原生管理
```

---

## 9. 全局快捷键管理

### 9.1 ShortcutManager 作用域模型

```typescript
// ShortcutManager.ts
type ShortcutScope = 'global' | 'editor' | 'scratchpad' | 'result' | 'none'

interface ShortcutRegistration {
  key: string // 'Ctrl+S' | 'Ctrl+Enter' | ...
  scope: ShortcutScope // 绑定作用域
  handler: () => void
  description: string
}

class ShortcutManagerImpl {
  private registrations: ShortcutRegistration[] = []
  private activeScope: ShortcutScope = 'none'

  register(key: string, scope: ShortcutScope, handler: () => void, desc: string) {
    this.registrations.push({ key, scope, handler, description: desc })
  }

  unregister(key: string) {
    this.registrations = this.registrations.filter(r => r.key !== key)
  }

  setActiveScope(scope: ShortcutScope) {
    this.activeScope = scope
  }

  // 全局 keydown 处理
  handleKeydown(e: KeyboardEvent) {
    const combo = buildKeyCombo(e)

    // 优先匹配 activeScope，其次匹配 global
    const match = this.registrations.find(
      r => r.key === combo && (r.scope === this.activeScope || r.scope === 'global')
    )

    if (match) {
      e.preventDefault()
      match.handler()
    }
  }
}

export const ShortcutManager = new ShortcutManagerImpl()
```

### 9.2 作用域切换逻辑

```typescript
// WorkbenchView.vue onReady
dockviewApi.onDidActivePanelChange(panel => {
  if (!panel) {
    ShortcutManager.setActiveScope('none')
    return
  }

  // 编辑器 Panel（包括所有 Group 的 EditorPanel）
  if (panel.id.startsWith('panel_editor_')) {
    ShortcutManager.setActiveScope('editor')
    return
  }

  // 结果集 Panel
  if (panel.id.startsWith('panel_result_')) {
    ShortcutManager.setActiveScope('result')
    return
  }

  // 草稿箱 Panel
  if (panel.id === 'scratchpad' || panel.id.startsWith('panel_scratchpad')) {
    ShortcutManager.setActiveScope('scratchpad')
    return
  }

  ShortcutManager.setActiveScope('none')
})
```

### 9.3 快捷键注册表

```typescript
// 注册时机：EditorManager 初始化时
ShortcutManager.register(
  'Ctrl+S',
  'editor',
  () => {
    EditorManager.saveCurrentFile()
  },
  '保存当前文件'
)

ShortcutManager.register(
  'Ctrl+Enter',
  'editor',
  () => {
    EditorManager.executeCurrentSQL()
  },
  '执行当前 SQL'
)

ShortcutManager.register(
  'Ctrl+/',
  'editor',
  () => {
    EditorManager.toggleComment()
  },
  '切换注释'
)

ShortcutManager.register(
  'Ctrl+Shift+E',
  'global',
  () => {
    EditorManager.openNewQuery()
  },
  '新建查询'
)

ShortcutManager.register(
  'Ctrl+Alt+N',
  'scratchpad',
  () => {
    // 委托草稿箱的 handleCreateFile
  },
  '草稿箱新建文件'
)

ShortcutManager.register(
  'Delete',
  'scratchpad',
  () => {
    // 委托草稿箱的 handleDelete
  },
  '草稿箱删除文件'
)

ShortcutManager.register(
  'Ctrl+C',
  'result',
  () => {
    // 复制选中的结果行
  },
  '复制结果数据'
)

// 关键：编辑器快捷键在所有 Group 中共享同一个 'editor' scope
// 用户从 Group A 的编辑器切换到 Group B 的编辑器
// → onDidActivePanelChange 触发
// → 新 Panel id = 'panel_editor_group_orders_sql'（仍然是 'editor' scope）
// → 快捷键无需重新注册，Ctrl+S/Ctrl+Enter 直接生效
```

### 9.4 与 useDockviewKeyboard 的关系

```
旧方案：useDockviewKeyboard（dockview 全局 keydown 监听，无作用域概念）
  → 替换为 ShortcutManager.handleKeydown 作为全局唯一的 keydown 处理器
  → useDockviewKeyboard 的侧边栏切换、最大化退出等功能迁移到 ShortcutManager 注册表
  → useDockviewKeyboard.ts 标记为 @deprecated，最终移除
```

---

## 10. 与 Vue 响应式的集成

### 10.1 防止 Vue 深度代理

```
Monaco Editor 实例：
  const editor = shallowRef<monaco.editor.IStandaloneCodeEditor | null>(null)
  → shallowRef 只追踪 .value 的替换，不深度代理内部属性
  → 避免 Monaco 内部循环引用导致 Vue Proxy 性能问题

ITextModel：
  const model = monaco.editor.createModel(content, language)
  markRaw(model)  // 标记为"原始对象"，阻止 Vue 深度代理
  → Model 对象被 MarkRaw 包裹，Vue 不再代理其内部属性

openFiles Map：
  const openFiles = shallowRef<Map<string, OpenFileInfo>>(new Map())
  → shallowRef 只触发 Map 对象的替换（openFiles.value = new Map(...)）
  → Map 内部增删不触发响应式 → 需手动触发更新

手动触发 Map 变化：
  openFiles.value.set(filePath, info)
  openFiles.value = new Map(openFiles.value)  // 替换引用以触发响应式
```

### 10.2 isDirty 状态的更新机制

```
不依赖 Vue 响应式追踪文件内容变化。
使用 Monaco 原生事件：

  model.onDidChangeContent(() => {
    const info = openFiles.get(filePath)
    if (!info) return

    const isClean = model.getValue() === originalContent
    info.isDirty = !isClean

    // 触发 Vue 响应式更新（替换 openFiles 引用）
    openFiles.value = new Map(openFiles.value)

    // 同时同步到 draft（用于崩溃恢复）
    if (info.isDirty) {
      draft.save(model.getValue())
    }
  })

优点：
  - Monaco 原生事件链路，不受 Vue 调度影响
  - 即使 Monaco Editor 在 shallowRef 中，事件回调正常工作
  - n-tabs 通过 computed → openFiles 的引用替换自动更新
```

### 10.3 AG Grid 的响应式边界

```
AG Grid 实例：shallowRef（与 Monaco 相同的理由）
行数据：传递原始数组，不在 Vue 响应式系统内追踪
  → 10 万行数据不触发 Vue Proxy 的深度遍历
  → AG Grid 内部通过 rowData 直接操作 DOM

columns / rowCount 等元数据：放在 ResultSetMetadata 中
  → 使用 ref 包裹（轻量数据，响应式无害）
```

---

## 11. 数据流架构

### 11.1 SQL 执行全链路

```
用户点击执行（Ctrl+Enter 或工具栏按钮）
  │
  ▼
ShortcutManager 接收到 editor scope → handler()
  │
  ▼
EditorManager.executeCurrentSQL()
  │  ├─ 从 activeFileInfo 获取 connectionId
  │  ├─ 从 editorRef.value?.getModel() 获取 SQL 文本
  │  ├─ 处理选区（选中的文本优先于全文）
  │  └─ 调用 queryService.executeSql(sql, connId)
  │
  ▼
queryService.executeSql(sql, connId)
  │  invoke<ExecuteSqlResponse>('execute_sql', { conn_id, sql })
  │
  ▼
Rust Backend (sqlx / duckdb / rusqlite)
  │  → QueryResult { columns, rows, rowCount, elapsedMs }
  │
  ▼
EditorManager.storeResult(activeFilePath.value, resultData)
  │
  ├─► 1. fileInfo.resultSets.push(metadata)
  │
  ├─► 2. ResultPanelManager.createResultPanel(filePath, metadata, data)
  │      ├─ dockviewApi.addPanel(FileResultPanel)
  │      ├─ position: { referencePanel: lastResultPanel, direction: 'below' }
  │      └─ fileInfo.resultPanelIds.push(panelId)
  │
  └─► 3. activeResultIndex = fileInfo.resultSets.length - 1
         → ResultSubTab 自动切换到最新结果
```

### 11.2 文件保存全链路

```
用户按 Ctrl+S（Editor scope 激活）
  │
  ▼
EditorManager.saveCurrentFile()
  │  ├─ 从 activeFileInfo 获取 filePath, model
  │  ├─ 获取 model.getValue()（当前编辑器内容）
  │  └─ 调用 scratchpadApi.saveScratchpadFile(filePath, content)
  │
  ▼
scratchpadApi.saveScratchpadFile(relativePath, content)
  │  invoke('save_scratchpad_file', { path, content })
  │
  ▼
文件写入磁盘 → Tauri 'scratchpad-changed' 事件
  │
  ▼
ScratchpadPanel 监听到变化 → 刷新文件树
  │
  ▼
EditorManager 标记 isDirty = false
  │  → n-tabs 脏标记消失
  │  → useEditorPersistence.draft.remove()
```

### 11.3 DuckDB 加速链路

```
用户点击 DuckDB 加速按钮（分析面板工具栏）
  │
  ▼
EditorManager.executeDuckDBAccelerated()
  │
  ▼
分析路径：
  1. 本地缓存命中 → 直接返回 DuckDB 计算结果
  2. 本地缓存未命中 → invoke execute_duckdb_accelerated
     → Rust Backend 将数据桥接到 DuckDB → 执行分析 SQL → 返回结果
  │
  ▼
结果作为新的 ResultSet 添加到分析面板的 resultSets
```

---

## 12. 组件树与职责矩阵

### 12.1 完整组件树

```
WorkbenchView.vue（dockview 宿主）
├── EditorManager（全局单例，非 Vue 组件）
│   ├── fileEditors: Map<path, MonacoEditor>
│   ├── editorRef: ShallowRef<MonacoEditor | null>
│   ├── openFiles: ShallowRef<Map<path, OpenFileInfo>>
│   ├── activeFilePath: Ref<string | null>
│   └── tabGroupId: Ref<string | null>
│
├── ResultPanelManager（全局单例，非 Vue 组件）
│   └── resultPanels: Map<panelId, ResultPanelInfo>
│
├── ShortcutManager（全局单例，非 Vue 组件）
│   ├── registrations: ShortcutRegistration[]
│   └── activeScope: ShortcutScope
│
├── Group: 主编辑区（所有 EditorPanel 的 Tab Group）
│   ├── EditorPanel.vue ×N（dockview Panel，每个文件一个 Tab）
│   │   ├── n-tabs（数据源 EditorManager.openFiles）
│   │   │   └── TabItem[{key, label, isDirty, isActive}]
│   │   ├── div.editor-container（自己的 Monaco Editor，懒创建）
│   │   ├── EditorToolbar（v-if="showToolbar"）
│   │   │   ├── 连接选择器
│   │   │   ├── Execute / ExecuteNew / BatchExecute 按钮
│   │   │   ├── DuckDB 加速按钮
│   │   │   ├── Format / Validate / Transpile 按钮
│   │   │   └── 模式切换器（查询/分析/智能）
│   │   ├── ResultSubTab.vue（v-if="resultSets.length > 0"）
│   │   │   └── [结果1] [结果2] [消息] Tab 栏
│   │   ├── FileResultPanel.vue ×N（dockview Panel，Tab 堆叠）
│   │   │   ├── AG Grid（active → 创建，hidden → 销毁）
│   │   │   ├── GridToolbar（导出、筛选模式）
│   │   │   └── GridStatusbar（行数、耗时、affected rows）
│   │   └── EditorStatusbar（光标位置、语言模式、编码）
│
├── Floating Group（dockview 原生 Float Tab）
│   ├── EditorPanel.vue（弹出文件，Editor 不变）
│   │   └── ...（同上，dockview 原生 DOM 管理）
│   │
├── Floating Group（独立结果集）
│   └── FileResultPanel.vue（始终持有 AG Grid）
│       ├── AG Grid（活跃，不销毁）
│       └── GridToolbar（重命名、导出、归档）
│
└── ScratchpadPanel（dockview Panel，left-edge）
    ├── n-tree（文件树）
    ├── 内联创建 input
    └── 刷新 → 通过 'scratchpad-changed' 事件
```

### 12.2 组件职责矩阵

| 组件 / 管理器             | 行数（估）     | 职责                                                                                                  | 依赖                                       |
| ------------------------- | -------------- | ----------------------------------------------------------------------------------------------------- | ------------------------------------------ |
| **EditorManager.ts**      | ~350           | Editor 生命周期、Model 管理、Panel ID 映射、文件 CRUD、fileEditors 管理、Dirty 追踪、onPanelActivated | dockviewApi, monaco, useEditorPersistence  |
| **ResultPanelManager.ts** | ~200           | 结果集 Panel CRUD、独立/跟随判断、Grid 状态缓存                                                       | dockviewApi, EditorManager                 |
| **ShortcutManager.ts**    | ~150           | 快捷键注册/注销、作用域管理、keydown 事件分发                                                         | dockviewApi.onDidActivePanelChange         |
| **EditorPanel.vue**       | ~280           | n-tabs 标签栏、Editor DOM 容器、工具栏显隐、ResultSubTab 集成                                         | EditorManager, EditorToolbar, ResultSubTab |
| **FileResultPanel.vue**   | ~250           | AG Grid 按需创建/销毁、Grid 状态保存恢复、导出                                                        | AG Grid, props(columns, rows), panelApi    |
| **ResultSubTab.vue**      | ~80            | 结果集子 Tab 栏、激活索引切换                                                                         | EditorManager.activeFileInfo               |
| **EditorToolbar.vue**     | ~180           | 连接选择器、执行按钮组、模式切换、方言转译入口                                                        | EditorManager, useSqlExecution             |
| **WorkbenchView.vue**     | ~200（重构后） | dockview 初始化、Panel 注册、Group 布局、事件监听初始化                                               | EditorManager, ShortcutManager             |
| **废弃文件**              | —              | SqlEditorPanel.vue、CodeEditorPanel.vue、useDockviewKeyboard.ts（功能迁移）                           | —                                          |

### 12.3 架构层级对比

```
重构前：
  UI Layer: SqlEditorPanel / CodeEditorPanel（~2000 行总）
  Composable Layer: useMonacoEditor, useSqlExecution, useEditorSettings, ...
  Store Layer: sql-execution-store, result-store, scratchpad-editor-store
  Service Layer: sql-editor-service, query-service, scratchpad-api

重构后：
  UI Layer: EditorPanel / FileResultPanel / ResultSubTab / EditorToolbar（~800 行总）
  Manager Layer: EditorManager / ResultPanelManager / ShortcutManager（~700 行总）
  Composable Layer: useMonacoEditor, useEditorPersistence（保留，精简）
  Store Layer: 精简为统一的 editor-runtime-store（替代 3 个 Store）
  Service Layer: 不变
```

---

## 13. 接口契约

### 13.1 EditorManager 公开接口

```typescript
interface IEditorManager {
  // 生命周期
  init(dockviewApi: DockviewVueApi): void
  destroy(): void
  onPanelActivated(panelId: string): void

  // 文件操作
  openFile(params: OpenFileParams): void
  closeFile(filePath: string): void
  switchToFile(filePath: string): void
  saveCurrentFile(): Promise<void>
  openNewQuery(): Promise<void>

  // Editor 注册（每文件一个）
  registerFileEditor(filePath: string, editor: monaco.editor.IStandaloneCodeEditor): void
  unregisterFileEditor(filePath: string): void

  // SQL 执行
  executeCurrentSQL(): Promise<void>
  executeNewTabSQL(): Promise<void>
  executeDuckDBAccelerated(): Promise<void>
  cancelExecution(): void

  // 编辑器操作
  formatSQL(): void
  validateSQL(): void
  toggleComment(): void

  // 状态查询
  readonly openFiles: ShallowRef<Map<string, OpenFileInfo>>
  readonly activeFilePath: Ref<string | null>
  readonly activeFileInfo: ComputedRef<OpenFileInfo | null>
  readonly editorRef: ShallowRef<monaco.editor.IStandaloneCodeEditor | null>
  readonly isExecuting: Ref<boolean>
}

interface OpenFileParams {
  filePath: string
  fileName: string
  language: string
  sql: string
  connectionId?: string
  databaseName?: string
  type?: 'file' | 'analysis'
}
```

### 13.2 ResultPanelManager 公开接口

```typescript
interface IResultPanelManager {
  // 结果集管理
  addResultSet(filePath: string, data: ResultSetCreateParams): string
  removeResultSet(filePath: string, resultSetId: string): void
  getResultSetRows(filePath: string, resultSetId: string): unknown[][]

  // Panel 管理
  createResultPanel(
    filePath: string,
    panelId: string,
    metadata: ResultSetMetadata,
    rows: unknown[][]
  ): void

  // 独立管理
  detachResultPanel(panelId: string): void // Panel 被拖出时调用
  attachResultPanel(panelId: string, filePath: string): void // Panel 被拖回时调用

  // 状态查询
  getAllResultSets(filePath: string): ResultSetMetadata[]
  getActiveResultSet(filePath: string): ResultSetMetadata | null
}

interface ResultSetCreateParams {
  columns: string[]
  rows: unknown[][]
  totalRows: number
  elapsedMs: number
  affectedRows: number
  sql: string
  error: string | null
}
```

### 13.3 ShortcutManager 公开接口

```typescript
interface IShortcutManager {
  register(key: string, scope: ShortcutScope, handler: () => void, desc: string): void
  unregister(key: string): void
  setActiveScope(scope: ShortcutScope): void
  handleKeydown(e: KeyboardEvent): void
}

type ShortcutScope = 'global' | 'editor' | 'scratchpad' | 'result' | 'none'
```

### 13.4 向 Tauri 的 IPC 接口（不变）

```typescript
// 现有接口保持不变，只是调用方从 SqlEditorPanel 变为 EditorManager
execute_sql(input: { conn_id: string, sql: string, timeout_ms?: number }): ExecuteSqlResponse
cancel_sql_query(connId: string): boolean
execute_duckdb_accelerated(params: DuckDBAcceleratedParams): DuckDBAcceleratedResult
begin_transaction(connId?: string): TransactionStatus
commit_transaction(connId?: string): TransactionStatus
rollback_transaction(connId?: string): TransactionStatus
create_scratchpad_entry(name: string, isFolder: boolean, parentPath?: string): ScratchpadEntry
save_scratchpad_file(path: string, content: string): void
load_scratchpad_file(path: string): string | null
```

### 13.5 向外的 CustomEvent 接口

```typescript
// 保留现有事件格式，emit 方从 SqlEditorPanel 迁移到 EditorManager

// open-sql-editor（接收，由 WorkbenchView 转发给 EditorManager.openFile）
interface OpenSqlEditorEvent {
  detail: {
    connectionId?: string
    databaseName?: string
    sql?: string
    scratchpadRelativePath?: string
    scratchpadFileName?: string
    language?: string
  }
}
```

---

## 14. 文件结构清单

### 14.1 目标文件树

```
src/extensions/builtin/workbench/
├── manager/                                    ← 新增
│   ├── EditorManager.ts                        ← 新建
│   ├── ResultPanelManager.ts                   ← 新建
│   └── ShortcutManager.ts                      ← 新建
│
├── ui/
│   ├── components/
│   │   ├── panels/
│   │   │   ├── EditorPanel.vue                 ← 新建（替代 SqlEditorPanel + CodeEditorPanel）
│   │   │   ├── FileResultPanel.vue             ← 新建
│   │   │   ├── EditorToolbar.vue               ← 新建（从 SqlEditorPanel 模板拆出）
│   │   │   ├── EditorStatusbar.vue             ← 保留（共享）
│   │   │   ├── EditorWelcome.vue               ← 保留
│   │   │   ├── ParamBindingModal.vue           ← 保留
│   │   │   ├── TranspileModal.vue              ← 保留
│   │   │   ├── EditorSettingsPopup.vue         ← 保留
│   │   │   ├── ResultSubTab.vue                ← 新建
│   │   │   ├── SqlEditorPanel.vue              ← 标记 @deprecated，后续删除
│   │   │   ├── CodeEditorPanel.vue             ← 标记 @deprecated，后续删除
│   │   │   ├── CodeEditorStatusbar.vue         ← 标记 @deprecated（合并到 EditorStatusbar）
│   │   │   ├── QueryResultPanel.vue            ← 保留（作为 FileResultPanel 的参考）
│   │   │   └── GridToolbar.vue                 ← 新建（AG Grid 工具栏）
│   │   └── ...
│   │
│   ├── composables/
│   │   ├── useMonacoEditor.ts                  ← 保留，精简
│   │   ├── useEditorPersistence.ts             ← 保留
│   │   ├── useEditorSettings.ts                ← 保留
│   │   ├── useFileSave.ts                      ← 保留
│   │   ├── useTabDirtyState.ts                 ← 保留
│   │   ├── useSqlExecution.ts                  ← 重构（结果写入变更）
│   │   ├── useConnectionBinding.ts             ← 保留
│   │   ├── useDialectSync.ts                   ← 保留
│   │   ├── useDockviewKeyboard.ts              ← 标记 @deprecated
│   │   └── useEditorManager.ts                 ← 新建（依赖注入入口）
│   │
│   ├── stores/
│   │   ├── editor-runtime-store.ts             ← 新建（替代 sql-execution-store + result-store）
│   │   ├── sql-execution-store.ts              ← 标记 @deprecated
│   │   ├── result-store.ts                     ← 保留（独立结果集管理）
│   │   └── layout-store.ts                     ← 保留
│   │
│   ├── services/
│   │   ├── sql-editor-service.ts               ← 保留
│   │   └── ...
│   │
│   └── views/
│       └── WorkbenchView.vue                   ← 重构
│
└── types/
    └── editor-types.ts                         ← 新建（OpenFileInfo, ResultSetMetadata, GridStateSnapshot）
```

### 14.2 文件统计

| 文件                                       | 操作     | 行数（估）   |
| ------------------------------------------ | -------- | ------------ |
| `manager/EditorManager.ts`                 | **新建** | ~350         |
| `manager/ResultPanelManager.ts`            | **新建** | ~200         |
| `manager/ShortcutManager.ts`               | **新建** | ~150         |
| `ui/components/panels/EditorPanel.vue`     | **新建** | ~280         |
| `ui/components/panels/FileResultPanel.vue` | **新建** | ~250         |
| `ui/components/panels/ResultSubTab.vue`    | **新建** | ~80          |
| `ui/components/panels/EditorToolbar.vue`   | **新建** | ~180         |
| `ui/components/panels/GridToolbar.vue`     | **新建** | ~80          |
| `ui/composables/useEditorManager.ts`       | **新建** | ~30          |
| `ui/stores/editor-runtime-store.ts`        | **新建** | ~120         |
| `types/editor-types.ts`                    | **新建** | ~60          |
| `ui/views/WorkbenchView.vue`               | **重构** | ~200（精简） |
| `ui/composables/useSqlExecution.ts`        | **重构** | ~50（精简）  |
| **新建总计**                               |          | **~1800 行** |
| `SqlEditorPanel.vue`                       | **废弃** | -~1200       |
| `CodeEditorPanel.vue`                      | **废弃** | -~350        |
| `CodeEditorStatusbar.vue`                  | **废弃** | -~480        |
| `sql-execution-store.ts`                   | **废弃** | -~150        |
| `useDockviewKeyboard.ts`                   | **废弃** | -~70         |
| **废弃总计**                               |          | **~2250 行** |
| **净变化**                                 |          | **-450 行**  |

---

## 15. 开发路线图

### Phase 1：核心基础设施（2 天）

```
目标：搭建三大管理器 + 依赖注入

任务：
  □ 1.1 新建 types/editor-types.ts
         OpenFileInfo, ResultSetMetadata, GridStateSnapshot,
         OpenFileParams, ResultSetCreateParams

  □ 1.2 新建 manager/EditorManager.ts
         初始化逻辑：创建 fileEditors Map、dockviewApi 绑定、tabGroupId 追踪
         文件 CRUD：openFile（direction: 'right' → 'within'）、closeFile、switchToFile
         状态管理：openFiles Map, activeFilePath, editorRef, fileEditors
         onPanelActivated：dockview Panel 激活时同步状态
         registerFileEditor / unregisterFileEditor：EditorPanel 生命周期回调
         通过 onDidChangeContent 管理 isDirty

  □ 1.3 新建 manager/ShortcutManager.ts
         作用域模型：global | editor | scratchpad | result | none
         注册表：key → { scope, handler, description }
         keydown 事件统一处理
         activeScope 自动切换（绑定 dockview onDidActivePanelChange）

  □ 1.4 新建 ui/composables/useEditorManager.ts
         依赖注入入口，提供 EditorManager 单例给 Vue 组件
         export function useEditorManager(): IEditorManager

  □ 1.5 POC 验证：dockview 原生 Tab 切换 + 每文件 Editor 懒创建
         创建三个文件 → 三个 EditorPanel Tab 在同一 Group
         验证：dockview tab 切换 → Editor 无闪烁 → 光标定位正确 → 自动补全正常工作
         验证：fileEditors Map 正确管理 Editor 生命周期

  验收标准：
    - EditorManager 可在同一 Group 内通过 direction: 'within' 创建多个 Tab
    - tabGroupId 正确捕获，后续文件不需要 direction: 'right'
    - ShortcutManager 可注册作用域快捷键
    - POC 中多个 Editor Panel 切换无渲染异常
```

### Phase 2：EditorPanel 组件（2 天）

```
目标：用 EditorPanel.vue 替代 SqlEditorPanel.vue + CodeEditorPanel.vue

任务：
  □ 2.1 新建 EditorPanel.vue
         n-tabs 标签栏（数据源 EditorManager.openFiles）
         自己的 Monaco Editor（onMounted 时懒创建，registerFileEditor 注册）
         工具栏显隐逻辑（showToolbar / showSqlActions）
         标签栏点击 → EditorManager.switchToFile()
         标签栏关闭 → EditorManager.closeFile()
         标签栏拖出 → dockview 原生 Float Tab

  □ 2.2 新建 EditorToolbar.vue
         从 SqlEditorPanel 模板中拆出工具栏区域
         Props: showAdvanced, toolbarPosition, isDuckDb
         连接选择器、执行按钮组、模式切换器
         方言转译入口
         Events: @execute, @execute-new, @format, @validate, ...

  □ 2.3 重构 WorkbenchView.vue
         dockview Panel 注册：移除 sqlEditor / codeEditor，注册 editorPanel
         handleOpenSqlEditor → 改为调用 EditorManager.openFile()
         handleWorkbenchNewQuery → 保留并优化
         onDidActivePanelChange → 绑定 ShortcutManager.setActiveScope()

  □ 2.4 验证 SqlEditorPanel 功能完整性
         确保所有 SQL 编辑功能在 EditorPanel 中可用
         分析面板状态切换正确
         工具栏根据文件类型显隐正确

  验收标准：
    - Ctrl+N 新建 SQL 文件 → EditorPanel 打开
    - 草稿箱双击 .sql 文件 → EditorPanel 打开
    - 分析法面板入口 → EditorPanel 显示工具栏
    - 切换 n-tabs → 文件内容正确切换，Editor 无闪烁
```

### Phase 3：结果集管理（2-3 天）

```
目标：实现路线 C 混合方案的结果集管理

任务：
  □ 3.1 新建 manager/ResultPanelManager.ts
         addResultSet: 创建结果集元数据 + 创建 FileResultPanel
         detachResultPanel: Panel 被拖出时标记为独立
         attachResultPanel: Panel 被拖回时标记为跟随
         getResultSetRows: 懒加载行数据
         Grid 状态缓存管理

  □ 3.2 新建 FileResultPanel.vue
         AG Grid 按需创建/销毁逻辑
         独立 Panel 判断（isDetached）
         Grid 状态保存/恢复（columnState, filterModel, sortModel）
         Props: resultSetId, columns, rows, totalRowCount, elapsedMs
         导出功能（CSV / JSON / Excel）

  □ 3.3 新建 ResultSubTab.vue
         结果集子 Tab 栏（n-tabs）
         数据源：activeFileInfo.resultSets
         激活索引切换：activeFileInfo.activeResultIndex

  □ 3.4 新建 ui/stores/editor-runtime-store.ts
         替代 sql-execution-store 的执行状态管理
         executing, lastExecutionTime, resultVersion
         与 EditorManager 的集成

  □ 3.5 验证多结果集场景
         执行两条 SELECT → 两个结果集 Tab
         Tab 切换 → AG Grid 数据切换
         dockview Tab 切换 → AG Grid 销毁/重建
         独立拖出 → AG Grid 保持

  验收标准：
    - 执行多条 SELECT → 多个结果集正确显示
    - 切换结果 Tab → 数据切换正确，Grid 状态保持
    - 拖出结果集 → 独立 Panel 正常工作
    - 10 文件 × 3 结果集 → 内存 < 400MB
```

### Phase 4：清理与集成（1 天）

```
目标：删除废弃代码，运行测试，文档更新

任务：
  □ 4.1 标记废弃文件
         SqlEditorPanel.vue → @deprecated
         CodeEditorPanel.vue → @deprecated
         CodeEditorStatusbar.vue → @deprecated
         sql-execution-store.ts → @deprecated
         useDockviewKeyboard.ts → @deprecated

  □ 4.2 清理 import 引用
         确保无任何活跃代码引用废弃文件

  □ 4.3 删除废弃文件（确认无引用后）
         物理删除标记为 @deprecated 的文件

  □ 4.4 更新设计文档
         docs/frontend/sql-editor/design.md → v1.14（添加指向本文档的链接）
         docs/frontend/editor/design.md → 本文档 v1.0

  □ 4.5 运行全量验证
         pnpm run lint → 零新增
         pnpm run typecheck → 通过
         npx vitest run → 无新增失败

  验收标准：
    - 零废弃代码引用
    - lint / typecheck 零新增错误
    - 所有现有测试通过
    - 手动回归：新建文件、打开文件、保存、执行 SQL、结果集展示
```

### 总体时间线

```
Week 1:
  Day 1-2: Phase 1（核心基础设施 + POC）
  Day 3-4: Phase 2（EditorPanel 组件 + EditorToolbar）
  Day 5:   Phase 2 收尾 + 缓冲

Week 2:
  Day 1-3: Phase 3（结果集管理 + FileResultPanel + ResultSubTab）
  Day 4:   Phase 4（清理 + 验证）
  Day 5:   缓冲 / Bug 修复
```

---

### Phase 5：类型安全清洗与旧流程迁移（0.5 天）✅ 完成

**目标**：消除脆弱的 `as unknown` 类型断言，将 Ctrl+N 和文件打开流程迁移到新架构。

**结果**：
| 指标 | 优化前 | 优化后 |
|------|--------|--------|
| EditorManager.ts `as unknown` cast | 7 | 0 |
| EditorPanel.vue `as unknown` cast | 3 | 0 |
| ResultSubTab/ResultPanelManager cast | 2 | 0 |
| WorkbenchView dead code | — | -52 行 |
| Ctrl+N 实现行数 | ~30 行 | 4 行 |
| FileResultPanel AG Grid | 挂载即创建 | **dockview Tab 激活时按需创建** |

**修改清单**：

- EditorManager.ts：新增 `DockviewApiFacade` 接口、`setEditor()`、`setActiveResultIndex()`、`dockviewApi` getter；移除 4 个死函数
- EditorPanel.vue：`(EditorManager as unknown).editor = ed` → `EditorManager.setEditor(ed)`
- ResultSubTab.vue：`(EditorManager as Record).activeResultIndex = idx` → `EditorManager.setActiveResultIndex()`
- ResultPanelManager.ts：`(EditorManager as unknown).dockviewApi` → `EditorManager.dockviewApi`
- WorkbenchView.vue：`handleWorkbenchNewQuery` 从 30 行内联创建 → `EditorManager.openNewQuery()`
- WorkbenchView.vue：`handleOpenSqlEditor` 委托到 `EditorManager.openFile()` / `openNewQuery()`
- WorkbenchView.vue：删除 `getScratchpadBasePath`、`ensureMultiTabResultPanel` 死代码
- FileResultPanel.vue：+ `watch(panelApi.isActive)` → 按需创建/销毁 AG Grid + GridState 持久化
- query/extension.ts：4 个旧组件 `@deprecated` 标注 + `FileResultPanel` 注册

**Lint**：3 errors（全预存 / cache-error-handler.ts）、348 warnings（全预存）、0 新增

---

### Phase 6（已完成 ✅）：dockview 原生 Float Tab + ITextModel 单例 + 每文件 Editor

**目标**：用 dockview 原生 Float Tab 替代自定义 popOutFile/mergeBackFile，每文件一个 Editor（懒创建），ITextModel 单例。

**当前状态**：已完成。所有文件位于同一 tabGroupId 的 dockview Tab 中，每个 EditorPanel 持有自己的 Monaco Editor + ITextModel。
dockview 原生 Float Tab 处理弹出，无需 DOM 迁移，无需 isPoppedOut 状态追踪。

---

## 附录

### A. 风险矩阵

| 风险                                    | 概率 | 影响 | 缓解措施                                              |
| --------------------------------------- | ---- | ---- | ----------------------------------------------------- |
| 多 Editor Panel 内存增长超预期          | 低   | 中   | fileEditors Map 管理，懒创建 + 统一 dispose，监控内存 |
| AG Grid 创建/销毁延迟用户感知           | 中   | 中   | 骨架屏预热 + Grid 状态持久化                          |
| ShortcutManager 与现有 keydown 监听冲突 | 低   | 低   | 逐步迁移，新旧并存过渡期                              |
| 结果集 Panel 数量过多 dockview 性能下降 | 低   | 中   | 限制每文件结果集上限（如 5 个），超限提示清理         |
| dockview 原生 Float Tab 兼容性问题      | 低   | 低   | dockview-vue 6.1 已验证，保留降级方案                 |

### B. 依赖项版本锁定

| 依赖          | 版本   | 约束            |
| ------------- | ------ | --------------- |
| monaco-editor | 0.55.x | 禁止 major 升级 |
| dockview-vue  | 6.1.x  | 禁止 major 升级 |
| ag-grid-vue3  | 35.3.x | 禁止 major 升级 |
| naive-ui      | 2.44.x | 允许 minor 升级 |
| Vue           | 3.5.x  | 禁止 major 升级 |
| TypeScript    | 6.0.x  | 允许 minor 升级 |

### C. 版本历史

| 版本 | 日期       | 说明                                                                                                                                                                                             |
| ---- | ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| v2.0 | 2026-05-17 | 架构重构：ITextModel 单例 + dockview 原生 Float Tab + 每文件 Editor。移除 popOutFile/mergeBackFile/DOM 迁移，用 fileEditors Map 替代单一 editorRef，dockview direction:'within' 实现同 Group Tab |
| v1.1 | 2026-05-16 | Phase 1-5 实现完成：类型安全清洗、Ctrl+N/文件打开迁移新架构、FileResultPanel 按需 AG Grid、旧组件 @deprecated 标注                                                                               |
| v1.0 | 2026-05-16 | 初始版本，完整架构设计文档                                                                                                                                                                       |

### D. 相关文档

| 文档               | 路径                                                              |
| ------------------ | ----------------------------------------------------------------- |
| SQL 编辑器设计文档 | [docs/frontend/sql-editor/design.md](../sql-editor/design.md)     |
| 结果集面板设计文档 | [docs/frontend/query-result/design.md](../query-result/design.md) |
| 前端架构概述       | [docs/frontend/ARCHITECTURE.md](../ARCHITECTURE.md)               |
| 前端文档索引       | [docs/frontend/INDEX.md](../INDEX.md)                             |
