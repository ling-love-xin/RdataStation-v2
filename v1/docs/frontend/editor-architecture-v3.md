# 编辑器架构设计文档 v3.0

> 版本：v3.1
> 创建日期：2026-05-17
> 最后更新：2026-05-18
> 状态：✅ 四轮审查 + 修复完成
>
> 废弃文档：[editor/design.md (v2.0)](./editor/design.md) · [sql-editor/design.md (v1.13)](./sql-editor/design.md) · [sql-editor/README.md (v1.9)](./sql-editor/README.md) · [sql-editor/optimization-plan.md (v2.4)](./sql-editor/optimization-plan.md)

---

## 目录

- [1. 概述与迁移状态](#1-概述与迁移状态)
- [2. CodeMirror 6 核心概念](#2-codemirror-6-核心概念)
- [3. 当前架构：多 EditorView 模式](#3-当前架构多-editorview-模式)
- [4. 实例模型演进](#4-实例模型演进)
- [5. 多实例场景建模](#5-多实例场景建模)
- [6. 状态管理设计](#6-状态管理设计)
- [7. DockView 集成与事件映射](#7-dockview-集成与事件映射)
- [8. 生命周期管理](#8-生命周期管理)
- [9. 跨窗口通信（Tauri）](#9-跨窗口通信tauri)
- [10. 状态序列化与持久化](#10-状态序列化与持久化)
- [11. 大文件处理策略](#11-大文件处理策略)
- [12. 性能优化](#12-性能优化)
- [13. 边缘情况与错误处理](#13-边缘情况与错误处理)
- [14. 文件结构与依赖](#14-文件结构与依赖)
- [15. 待办事项与路线图](#15-待办事项与路线图)

---

## 1. 概述与迁移状态

### 1.1 迁移完成

Monaco Editor → CodeMirror 6 全量迁移已于 2026-05-17 完成：

| 操作     | 数量 | 说明                                                                                                                                                  |
| -------- | ---- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| 删除文件 | 7    | monaco-theme.ts、useMonacoEditor.ts、useEditorSettings.ts、SqlEditorPanel.vue、CodeEditorPanel.vue、CodeEditorStatusbar.vue、sql-dialect-highlight.ts |
| 新建文件 | 3    | useCodeMirror.ts、cm-sql-extensions.ts、cm-theme.ts                                                                                                   |
| 修改文件 | 19   | 配置、Manager、Service、Panel、Store、i18n、Composables                                                                                               |
| 移除依赖 | 1    | monaco-editor (~15MB)                                                                                                                                 |
| 新增依赖 | 10   | @codemirror/\* 轻量包（总计 ~800KB）                                                                                                                  |

### 1.2 当前 CM6 依赖栈

```
@codemirror/autocomplete ^6.20.2    @codemirror/commands ^6.10.3
@codemirror/lang-sql ^6.10.0        @codemirror/language ^6.12.3
@codemirror/lint ^6.9.6             @codemirror/search ^6.7.0
@codemirror/state ^6.6.0            @codemirror/view ^6.43.0
@codemirror/theme-one-dark ^6.1.3
@lezer/highlight (via @codemirror/language)
```

### 1.3 核心数据流

```
EditorPanel.vue
  └─ useCodeMirror (Compartment 管理)
       ├─ EditorView (DOM 挂载)
       ├─ EditorState (文档 + 选区 + 历史)
       └─ Compartment: language / theme / extra
            ├─ cm-sql-extensions (方言 + 补全 + 折叠 + 诊断)
            └─ cm-theme (rdataDark / rdataLight)
  └─ EditorManager (全局状态)
       ├─ openFiles: Map<filePath, OpenFileInfo>
       ├─ fileEditors: Map<filePath, EditorView>
       └─ ShortcutManager
  └─ sql-editor-service
       ├─ SQL 执行 (invoke → Tauri)
       ├─ DuckDB 加速
       └─ 错误诊断注入
```

---

## 2. CodeMirror 6 核心概念

### 2.1 EditorState（不可变状态对象）

`EditorState` 是 CM6 的核心数据结构，包含：

- **文档内容**（字符串 + 变更历史）
- **选区范围**（主选区 + 多个副选区）
- **撤销/重做栈**（历史记录）
- **扩展状态**（通过 `StateField` 自定义，支持 `toJSON`/`fromJSON`）
- **语言模式配置**（语法树缓存）

关键特性：

- 不可变，任何修改都产生新的 EditorState
- 独立于 DOM，可序列化（`EditorState.toJSON()`）
- 体积约为文档内容的 2-3 倍（含历史记录栈）

### 2.2 EditorView（DOM 视图）

- 将 EditorState 渲染到 DOM
- 一个 EditorView 同时只关联一个 EditorState
- 切换状态：`view.setState(nextState)`（O(1) 操作）
- 管理滚动位置、焦点、光标闪烁等视图临时状态

### 2.3 Compartment（动态扩展切换）

用于运行时动态重新配置扩展而不重建 EditorView：

```typescript
const languageCompartment = new Compartment()
const themeCompartment = new Compartment()

// 切换语言不重建 View
languageCompartment.reconfigure(sql({ dialect: MySQL }))
```

当前 [useCodeMirror.ts](#) 使用 3 个 Compartment：`language` / `theme` / `extra`

### 2.4 Extension（扩展机制）

CM6 通过 Extension 组合功能，核心类型：

| Extension 类型 | 示例                 | 可序列化                  |
| -------------- | -------------------- | ------------------------- |
| ViewPlugin     | 光标闪烁、高亮当前行 | ❌ 视图临时状态           |
| StateField     | 折叠状态、语法树缓存 | ✅ 需实现 toJSON/fromJSON |
| Facet          | 编辑器配置           | ❌                        |
| 装饰器         | lint 标记、语法高亮  | ❌ 由外部服务管理         |

---

## 3. 当前架构：多 EditorView 模式

### 3.1 当前设计（Phase B：多实例映射）

[EditorManager.ts](../src/extensions/builtin/workbench/manager/EditorManager.ts) 核心数据结构：

```typescript
const editorInstances = new Map<string, EditorInstance>() // instanceId → EditorInstance
const panelGroupMap = new Map<string, string>() // panelId → groupId
const savedStates = new Map<string, EditorState>() // filePath → EditorState（缓存）
```

每个通过 dockview 打开的标签页都有独立的 `EditorView` 实例（Phase B：一对一映射）：

```
dockview Panel 1 (file A.sql) ─── EditorInstance #1 (EditorView #1)
dockview Panel 2 (file B.sql) ─── EditorInstance #2 (EditorView #2)
```

> **注意**：当前为 Phase B 阶段。Phase C（Group 级 EditorView 复用，`view.setState()` O(1) 切换）尚未实现。

### 3.2 OpenFileInfo 结构

```typescript
interface OpenFileInfo {
  filePath: string
  fileName: string
  language: string // 'sql' | 'python' | 'json' | ...
  type: FileType // 'file' | 'analysis'
  isDirty: boolean
  connectionId: string
  databaseName: string
  resultSets: ResultSetMetadata[]
  activeResultIndex: number
  resultPanelIds: string[]
  detachedResultIds: string[]
}
```

注意：`OpenFileInfo` 当前不包含 `EditorState` 快照。

### 3.3 当前模式的局限性

| 问题                    | 说明                                                                                                   | 严重程度  |
| ----------------------- | ------------------------------------------------------------------------------------------------------ | --------- |
| **一对一映射**          | `editorInstances` 是 `Map<instanceId, EditorInstance>`，每个 Instance 仍有独立 EditorView（Phase B）   | 🟡 P1     |
| **无 EditorState 保存** | 切换/关闭标签页时，选区位置和历史记录通过 `savedStates` 缓存保持，但非 Group 级 `view.setState()` 切换 | 🟢 已缓解 |
| **Tab 切换时重建 View** | 每次切换依赖 Panel `onMounted/onUnmounted` 生命周期，不是 `view.setState()`                            | 🟡 P1     |
| **分屏支持**            | 通过 `instanceId` 区分已支持，同一文件可在不同 Group 独立打开                                          | 🟢 已解决 |
| **无弹窗支持**          | Popout 端到端链路断裂                                                                                  | 🟠 P2     |

---

## 4. 实例模型演进

### 4.1 目标模型：Group 级单 EditorView + 多 EditorState

参考 VS Code 架构，核心思想：

> 每个可视区域（Group）维护一个 EditorView，该 Group 内的所有标签页共享此 View，通过 `view.setState()` 切换。不同 Group 之间 EditorView 独立。

```
Group A (左边)
  ├── EditorView #A
  │    ├── Tab 1: file A.sql  → EditorState #1
  │    ├── Tab 2: file B.sql  → EditorState #2
  │    └── Tab 3: file C.py   → EditorState #3
  │
Group B (右边，分屏)
  └── EditorView #B
       ├── Tab 1: file A.sql  → EditorState #4 (独立副本，只读)
       └── Tab 2: file D.json → EditorState #5
```

关键约束：

- **同一 Group 内**：EditorView 复用，`view.setState()` 切换，O(1)
- **跨 Group 同一文件**：每个 Group 独立副本（`EditorInstance`），一个可编辑、其余只读
- **状态保存**：每个 Tab 关闭或切换前，将其 `EditorState` 保存到映射

### 4.2 引入 EditorInstance

```typescript
interface EditorInstance {
  instanceId: string // `${groupId}_${filePath}`
  groupId: string // 所属 Group
  filePath: string
  view: EditorView // 该 Group 的共享 View
  state: EditorState // 当前激活状态（切换前保存到映射）
  writable: boolean // 是否可编辑（同文件首个实例为 true，其余只读）
}

const editorInstances = new Map<string, EditorInstance>()
const groupViews = new Map<string, EditorView>() // groupId → 共享 EditorView
const groupActiveState = new Map<string, EditorState>() // groupId → 当前活跃 EditorState
```

### 4.3 OpenFileInfo 扩展

```typescript
interface OpenFileInfo {
  // ... 现有字段

  // 新增：每个打开实例的 EditorState 完整快照
  states: Map<string, EditorState> // instanceId → EditorState

  // 新增：主实例 ID（可编辑的那个）
  primaryInstanceId: string

  // 新增：只读实例 ID 列表
  readonlyInstanceIds: string[]
}
```

### 4.4 演进路径

```
Phase A（当前）
  Map<filePath, EditorView>
  └── 缺陷：分屏覆盖、无状态保存

Phase B（P0 - 修补）
  Map<instanceId, EditorView>
  └── 支持分屏多实例 + 冲突检测

Phase C（P1 - 优化）
  Map<groupId, EditorView>
  Map<instanceId, EditorState>
  └── Group 级 View 复用 + Tab 切换 O(1)

Phase D（P2 - 弹窗）
  Tauri WebviewWindow + EventSystem 通信
  └── Group 弹出/合并
```

---

## 5. 多实例场景建模

### 5.1 场景分类

| 场景     | 触发方式              | EditorView             | EditorState            | 编辑权限                   |
| -------- | --------------------- | ---------------------- | ---------------------- | -------------------------- |
| 普通打开 | 双击文件树            | 新建/复用              | 新建                   | 可编辑                     |
| 同组切换 | 点击 Tab              | 复用                   | 切换                   | 可编辑                     |
| 分屏观看 | 拖拽 Tab 到另一 Group | 目标 Group 的共享 View | 新建副本               | **只读**                   |
| 弹出窗口 | 弹出 Group 按钮       | 新窗口内的共享 View    | 原 Group 的 State 迁移 | 弹出窗口可编辑，原窗口锁定 |

### 5.2 分屏冲突策略

当用户将已打开的 `file A.sql` 拖到另一个 Group 创建分屏：

```
方案一（推荐 - 锁定）：
  第二个实例标记为只读，显示提示"文件已在左侧编辑中"
  用户点击第二个实例 Tab 时 → 自动切换到第一个实例

方案二（不推荐 - 双向编辑）：
  允许两个实例独立编辑，但需要实现复杂的状态合并/冲突解决
  浏览器端 OT/CRDT 实现成本高，不符合项目定位
```

推荐方案一，理由：

- 符合用户预期（点击已打开的文件 = 导航到该文件）
- 实现成本低
- DataGrip / DBeaver 均采用类似策略

### 5.3 弹出窗口冲突策略

弹出 Group 时：

1. 主窗口中该 Group 内所有文件的编辑器**锁定为只读**
2. 弹出窗口中的副本获得**编辑权限**
3. 合并回主窗口时，覆盖原状态

用户关闭弹出窗口时的处理：

- 选择合并 → 序列化并回传，覆盖主窗口中的对应状态
- 选择放弃 → 主窗口中的只读锁定解除

---

## 6. 状态管理设计

### 6.1 状态管理器职责

| 职责     | 说明                                                      |
| -------- | --------------------------------------------------------- |
| 存储     | 维护 `instanceId → EditorState` 映射                      |
| CRUD     | 创建/读取/更新/删除标签页状态                             |
| 当前活动 | 跟踪 `(groupId, instanceId)` 当前活跃对                   |
| 序列化   | 提供 `serializeGroup(groupId)` / `deserializeGroup(data)` |
| 同步     | 编辑器内容变更后自动回写 EditorState                      |

### 6.2 状态与视图分离

```
EditorView (DOM)  ←→  EditorState (数据)
   视图层               状态层
   - 滚动位置            - 文档内容
   - 焦点状态            - 选区
   - 光标闪烁            - 历史栈
   - 视口大小            - 语言模式
   (不可序列化)          (可序列化)
```

### 6.3 状态持久化时机

| 时机         | 操作                                              |
| ------------ | ------------------------------------------------- |
| 每次编辑     | `updateListener` → 保存最新 `EditorState` 到映射  |
| 切换标签页前 | 先保存当前 State，再加载新 State（顺序不可逆）    |
| 弹出 Group   | 序列化整个 Group 的所有 State 并传输              |
| 合并关闭     | 接收序列化数据，反序列化后合并到映射              |
| 应用关闭     | `EditorState.toJSON()` → localStorage（崩溃恢复） |

### 6.4 状态同步合约

```typescript
// 切换标签页 - 必须遵循的顺序
function onTabSwitch(oldInstanceId: string, newInstanceId: string): void {
  // 1. 先保存当前
  const currentView = groupViews.get(groupId)
  if (currentView) {
    editorInstances.get(oldInstanceId)!.state = currentView.state
  }

  // 2. 再加载新状态
  const nextState = editorInstances.get(newInstanceId)!.state
  currentView!.setState(nextState)

  // 3. 更新活动记录
  groupActiveState.set(groupId, nextState)
}

// 关闭标签页 - 先切换再删除
function onTabClose(instanceId: string, groupId: string): void {
  const siblings = getSiblingsInGroup(groupId, instanceId)
  if (siblings.length > 0) {
    onTabSwitch(instanceId, siblings[0]) // 触发切换（内部先保存）
  }
  editorInstances.delete(instanceId) // 再删除
}
```

---

## 7. DockView 集成与事件映射

### 7.1 事件映射表

| DockView 事件            | 当前处理                                            | 目标处理                                                                                         |
| ------------------------ | --------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| `onDidAddPanel`          | `openFile()` → 创建 EditorView + OpenFileInfo       | 创建 EditorState + 注册 EditorInstance；若是同 Group 第二个 Tab，复用 EditorView                 |
| `onDidActivePanelChange` | `onPanelActivated()` → 更新 activeFilePath          | 保存当前 State → `view.setState(nextState)` → 更新活动记录                                       |
| `onDidRemovePanel`       | `closeFile()` → `view.destroy()` + 删除 fileEditors | 从映射删除；若是活动页则先切换到同组其他页                                                       |
| `onDidMovePanel`         | ✅ 已处理                                           | 更新 `instance.groupId`；若移动的是活动页，更新 Group→View 的关联                                |
| `onDidDockGroup`         | ⚠️ 仅日志                                           | 合并外部 Group：反序列化数据 → 注册实例 → dockview 重建（反序列化逻辑待实现）                    |
| `onDidUndockGroup`       | ✅ 已处理                                           | 弹出 Group：序列化状态 → Tauri EventSystem 发送 → 锁定主窗口文件（通过 `onDidRemoveGroup` 触发） |

### 7.2 时序约束

```
关闭标签页的正确顺序：
  onDidRemovePanel
    → 如果是当前活动页
      → onDidActivePanelChange (切换到同组其他页)
        → 保存当前 State
        → view.setState(nextState)
    → 删除状态映射
    → editorInstances.delete(id)
```

### 7.3 DOM 重新挂载处理

dockview 拖拽时会将 EditorView 的 DOM 节点从文档树中临时移除再插入：

```typescript
// 在 EditorPanel.vue 中监听 DOM 重新挂载
onMounted(() => {
  const observer = new MutationObserver(mutations => {
    for (const m of mutations) {
      if (m.type === 'childList' && editorContainerEl.value?.isConnected) {
        view.value?.requestMeasure()
      }
    }
  })
  observer.observe(parentEl, { childList: true })
})
```

或通过 dockview 的 `onDidLayoutChange` 批量触发。

---

## 8. 生命周期管理

### 8.1 EditorView 生命周期

```
createView(dom, doc, lang, theme)
  → EditorState.create({ doc, extensions })
  → new EditorView({ state, parent: dom })

运行时：
  view.setState(nextState)       // 切换内容
  view.dispatch(changes)         // 编辑操作
  compartment.reconfigure(ext)   // 动态扩展

销毁：
  view.destroy()                 // 解绑 DOM + 移除事件监听
  → view.dom.isConnected = false
```

### 8.2 EditorManager 生命周期

```
init(dockviewApi)
  → 注册 dockview 事件监听
  → 初始化 ShortcutManager
  → 注册 EditorPanel 组件
  → 显示欢迎页 / 空白编辑器

运行期间：
  → 响应用户操作（打开/关闭/切换/执行）
  → 持久化编辑器状态（localStorage）

destroy()
  → view.destroy()
  → 移除所有事件监听
  → 清空 openFiles / fileEditors
```

### 8.3 弹出窗口生命周期

```
创建：
  → Tauri WebviewWindow 加载前端 bundle（共享无需重新下载）
  → 初始化自己的 EditorManager（空状态）
  → 等待主窗口 INIT_GROUP 事件

接收到数据：
  → 反序列化 EditorState[]
  → 创建 EditorView，激活第一个 Tab
  → 监听编辑变更

关闭前（beforeunload）：
  → 询问用户"是否合并更改？"
  → 是：序列化 Group 数据 → Tauri emit MERGE_GROUP → 关闭
  → 否：直接关闭，主窗口的只读锁定解除
```

### 8.4 应用级生命周期

```
启动：
  → 检测 localStorage 中的未保存状态（上次崩溃恢复）
  → 检测 scratchpad 中的草稿文件
  → 初始化 EditorManager

退出：
  → 遍历所有 EditorInstance
  → 对 isDirty = true 的文件提示保存
  → EditorState.toJSON() → localStorage（崩溃恢复快照）
  → view.destroy()
```

---

## 9. 跨窗口通信（Tauri）

### 9.1 为何不是 postMessage

Tauri 应用中，每个 WebviewWindow 是独立的 webview 进程，不是浏览器 Tab。`window.postMessage` 不可用于跨 webview 通信。

### 9.2 Tauri EventSystem 协议

```typescript
import { emit, listen } from '@tauri-apps/api/event'

// 消息类型
type EditorWindowMessage =
  | { type: 'EDITOR_INIT_GROUP'; payload: SerializedGroup }
  | { type: 'EDITOR_MERGE_GROUP'; payload: SerializedGroup }
  | { type: 'EDITOR_READY' }
  | { type: 'EDITOR_ACK' }
  | { type: 'EDITOR_LOCK_FILES'; payload: { filePaths: string[]; locked: boolean } }
```

### 9.3 通信时序

```
主窗口                                弹出窗口
  │                                      │
  │── tauri::WebviewWindow::new() ──────→│ 创建
  │                                      │
  │                               listen('EDITOR_INIT_GROUP')
  │                               emit('EDITOR_READY')
  │←── EDITOR_READY ────────────────────│
  │                                      │
  │ 序列化 Group 数据                     │
  │ emit('EDITOR_INIT_GROUP', data)      │
  │─────────────────────────────────────→│ 反序列化 + 初始化
  │                                      │
  │ 锁定主窗口文件                         │ ...编辑中...
  │                                      │
  │                               beforeunload:
  │                               用户选择"合并"
  │                               emit('EDITOR_MERGE_GROUP', data)
  │←── EDITOR_MERGE_GROUP ─────────────│
  │                                      │ 窗口关闭
  │ 反序列化 + 合并                       │
  │ emit('EDITOR_ACK') ─────────────────→│
  │ 解锁主窗口文件                         │
```

### 9.4 可靠性保障

- **弹出窗口意外关闭**：beforeunload 中持久化到 localStorage，主窗口下次启动时检测并提示恢复
- **主窗口已关闭**：弹出窗口检测 ACK 超时（3s），将数据写入临时文件
- **多弹出窗口**：主窗口维护 `Map<windowLabel, groupId>`

---

## 10. 状态序列化与持久化

### 10.1 可序列化内容

`EditorState.toJSON()` 自动覆盖：

- ✅ 文档内容（doc.toString()）
- ✅ 选区范围（selection.main）
- ✅ 历史记录（undo/redo 栈）
- ✅ 使用了 `StateField` 且实现了 `toJSON`/`fromJSON` 的扩展数据

额外需序列化的元数据：

```typescript
interface SerializedGroup {
  groupId: string
  tabs: SerializedTab[]
}

interface SerializedTab {
  instanceId: string
  filePath: string
  fileName: string
  language: string
  title: string
  writable: boolean
  stateJSON: object // EditorState.toJSON()
  scrollTop: number // view.scrollDOM.scrollTop
  scrollLeft: number // view.scrollDOM.scrollLeft
}
```

### 10.2 不可序列化内容及处理

| 内容         | 原因                | 处理方式                                                                       |
| ------------ | ------------------- | ------------------------------------------------------------------------------ |
| 滚动位置     | ViewPlugin 临时状态 | 序列化前单独读取 `view.scrollDOM.scrollTop/Left`，反序列化后 `view.scrollTo()` |
| 代码折叠     | 取决于扩展实现      | `foldGutter` 的折叠状态是 StateField，可自动序列化                             |
| 当前高亮行   | ViewPlugin 临时状态 | 丢失（可接受）                                                                 |
| 数据库连接   | 运行时引用          | 不存储在 EditorState 中，通过外部服务管理                                      |
| LSP 连接     | 运行时引用          | 弹出窗口独立创建                                                               |
| 光标闪烁相位 | ViewPlugin 临时状态 | 丢失（可接受）                                                                 |

### 10.3 扩展兼容性约束

跨窗口（或保存→恢复）需要**两端扩展集完全一致**，否则 `EditorState.fromJSON()` 会失败。

实现策略：

- 主窗口和弹出窗口使用相同的 Extensions 构建函数
- Compartment 配置（language/theme/extra）在主窗口打包发送，弹出窗口按配置重建
- 扩展集 hash 校验：发送前取 `extensions.hash`，接收端比对，不匹配则拒绝恢复

---

## 11. 大文件处理策略

### 11.1 场景分析

RdataStation 编辑器承载的内容不止 SQL 脚本，还包括：

| 文件类型         | 典型大小           | 使用场景           |
| ---------------- | ------------------ | ------------------ |
| SQL 脚本         | 1KB - 500KB        | 日常查询、存储过程 |
| Python 脚本      | 1KB - 2MB          | 数据分析脚本       |
| JSON 配置        | 100B - 5MB         | 数据导出/导入      |
| CSV/TSV 数据文件 | 100KB - **100MB+** | 数据预览/编辑      |
| 日志文件         | 1MB - **500MB+**   | 调试/审计          |
| XML/HTML         | 1KB - 50MB         | 数据交换格式       |

### 11.2 分级处理策略

| 文件大小     | 策略                | 说明                                                       |
| ------------ | ------------------- | ---------------------------------------------------------- |
| < 1MB        | 全量加载            | 正常模式，完整语法高亮 + 历史记录 + 序列化                 |
| 1MB - 10MB   | 全量加载 + 限制历史 | 限制撤销栈深度（如 200 步），禁用代码折叠自动分析          |
| 10MB - 50MB  | 全量加载 + 禁用扩展 | 仅基础语法高亮，禁用补全/折叠/lint；提示"大文件模式"       |
| 50MB - 200MB | 分片加载            | 仅加载可视区域 + 前后各 5000 行缓冲区；语法高亮延迟        |
| > 200MB      | 拒绝或只读预览      | 显示"文件过大，建议使用外部编辑器"，或仅显示前 1000 行预览 |

### 11.3 大文件序列化策略

对于大文件（>10MB），`EditorState.toJSON()` 可能耗时数百毫秒：

- **仅传输文档内容**：`view.state.doc.toString()`，丢弃历史记录和选区
- **限制历史栈**：大文件模式下限制 `history()` 扩展的 `minDepth` 参数
- **懒序列化**：弹出/保存时才执行序列化，不频繁执行
- **Web Worker**：将 `EditorState.toJSON()` 放到 Worker 线程（需评估序列化成本 vs Worker 通信成本）

### 11.4 大文件检测与提示

```typescript
function openLargeFile(filePath: string, content: string): OpenStrategy {
  const sizeMB = new Blob([content]).size / (1024 * 1024)

  if (sizeMB > 200) {
    return { mode: 'rejected', reason: `文件过大 (${sizeMB.toFixed(1)}MB)，建议使用外部工具打开` }
  }
  if (sizeMB > 50) {
    return { mode: 'chunked', chunkSize: 10000 }
  }
  if (sizeMB > 10) {
    return { mode: 'large', noCompletion: true, noFold: true, noLint: true }
  }
  if (sizeMB > 1) {
    return { mode: 'reduced', historyDepth: 200 }
  }
  return { mode: 'normal' }
}
```

### 11.5 数据库结果集渲染

与编辑器不同，AG Grid 结果集的渲染策略（已独立于编辑器）：

- AG Grid 内置虚拟滚动 → 百万行数据流畅滚动
- 结果集按需分页（`LIMIT` / `FETCH`）
- 编辑器仅显示执行的 SQL，不加载结果数据到 EditorState

---

## 12. 性能优化

### 12.1 单 EditorView 内存优势

```
假设同时打开 30 个标签页，每个文件 200KB：

多 EditorView 方案（当前）：
  30 × 3MB (EditorView) + 30 × 0.4MB (EditorState) ≈ 102MB

Group 级单 EditorView（目标，2个 Group）：
  2 × 3MB (EditorView) + 30 × 0.4MB (EditorState) ≈ 18MB

节省：~82%
```

### 12.2 Tab 切换性能

| 方案                 | 操作                         | 延迟  |
| -------------------- | ---------------------------- | ----- |
| 当前（多 View）      | destroyView() + createView() | ~50ms |
| 目标（单 View 复用） | view.setState(nextState)     | <5ms  |

### 12.3 弹出窗口首屏加载

- Tauri WebviewWindow 共享前端 bundle，无需重新下载 JS/CSS
- 仅需传输序列化的 Group 数据（通常 <1MB）
- 首屏渲染：反序列化 EditorState + 创建 EditorView ≈ 100-300ms

### 12.4 Compartment 扩展懒加载

```typescript
// 大文件模式下不加载 SQL 补全等重扩展
function buildExtensions(fileInfo: OpenFileInfo): Extension[] {
  const base = [lineNumbers(), highlightActiveLine(), history()]
  if (fileInfo.sizeMB < 10) {
    base.push(sql({ dialect: MySQL }))
    base.push(autocompletion())
    base.push(foldGutter())
    base.push(lintGutter())
  }
  return base
}
```

---

## 13. 边缘情况与错误处理

### 13.1 同文件并发编辑冲突

| 场景                        | 处理                                           |
| --------------------------- | ---------------------------------------------- |
| 分屏打开已在编辑的文件      | 第二个实例设为只读，提示"文件已在左侧编辑中"   |
| 弹出 Group 后再打开同一文件 | 主窗口中文件锁定，提示"文件已在弹出窗口中编辑" |
| 弹出窗口合并时发现冲突      | 不实现自动合并——仅弹出窗口的更改覆盖主窗口     |

### 13.2 弹出窗口数据合并失败

- 发送 `EDITOR_MERGE_GROUP` 后等待 `EDITOR_ACK`
- 超时 3s 未收到 ACK → 将序列化数据写入 `localStorage`
- 下次主窗口启动时检测并提示："检测到未合并的编辑器更改，是否恢复？"

### 13.3 扩展版本不一致

主窗口和弹出窗口的扩展集可能因代码热更新而产生差异：

- 弹出窗口初始化时，主窗口发送 `{ extensions: string, hash: string }`
- 弹出窗口用本地扩展集生成 hash 对比
- 不匹配 → 提示"编辑器版本不一致，请刷新弹出窗口"

### 13.4 EditorState.fromJSON() 失败

反序列化失败的可能原因：

- 扩展集不一致
- JSON 数据损坏

处理：

- 降级为仅恢复文档内容（`EditorState.create({ doc: rawText })`），丢弃历史/选区
- 提示用户"编辑器状态恢复失败，仅恢复了文档内容"

### 13.5 localStorage 配额超限

大文件 + 历史记录可能导致 localStorage 超限（通常 5-10MB）：

- 保存前先 `try { localStorage.setItem } catch (e)`
- 配额不足 → 仅保存文件路径 + 脏标记，丢弃 EditorState JSON
- 提示用户"编辑器状态过大，未保存撤销历史"

### 13.6 Scratchpad 草稿箱的双层恢复

当前 scratchpad 已有的双层持久化机制保持不变：

1. **localStorage**：崩溃恢复，保存最近编辑的草稿内容
2. **`.scratchpad/` 目录**：正式存储，文件系统持久化

新增的 `EditorState` 序列化叠加在此之上：

- 编辑器关闭：`EditorState.toJSON()` → localStorage（选区 + 历史）
- 草稿保存：`doc.toString()` → `.scratchpad/` 文件系统

---

## 14. 文件结构与依赖

### 14.1 当前文件清单

```
src/
├── shared/
│   └── styles/
│       └── cm-theme.ts                          # rdataDark/rdataLight 主题
│
├── extensions/builtin/workbench/
│   ├── types/
│   │   └── editor-types.ts                      # IEditorManager 等类型定义
│   ├── manager/
    │   ├── EditorManager.ts                     # 编辑器全局管理器
    │   └── ShortcutManager.ts                   # 快捷键管理
    ├── services/
    │   ├── cm-sql-extensions.ts                 # SQL 方言/补全/折叠/诊断
    │   └── sql-editor-service.ts                # SQL 执行/格式化/DuckDB
    └── ui/
        ├── composables/
        │   ├── useCodeMirror.ts                 # CM6 生命周期管理
        │   ├── useCrossWindow.ts                # 跨窗口通信 (Tauri EventSystem)
        │   ├── useEditorPersistence.ts          # 编辑器配置持久化
        │   ├── useEditorRecovery.ts             # 崩溃恢复 (localStorage)
        │   ├── useKeyboardShortcuts.ts          # 键盘快捷键
        │   └── useLargeFile.ts                  # 大文件分级处理
        ├── stores/
        │   ├── workbench-store.ts               # 工作台全局状态
        │   └── editor-runtime-store.ts          # 编辑器运行时状态
        └── components/panels/
            └── EditorPanel.vue                  # 统一编辑器面板
```

### 14.2 依赖关系

```
EditorPanel.vue
 ├── useCodeMirror
 │    ├── @codemirror/state (EditorState, Compartment)
 │    ├── @codemirror/view (EditorView)
 │    ├── @codemirror/lang-sql (sql)
 │    ├── @codemirror/autocomplete
 │    └── cm-theme (主题)
 ├── EditorManager
 │    ├── editor-types
 │    ├── ShortcutManager
 │    └── useEditorPersistence
 └── sql-editor-service
      ├── cm-sql-extensions
      └── @tauri-apps/api (invoke)
```

---

## 15. 待办事项与路线图

### 15.1 优先级矩阵

| 优先级 | 任务                                                | 说明                                  | 预估改动 |
| ------ | --------------------------------------------------- | ------------------------------------- | -------- |
| **P0** | `fileEditors` 一对一 → `editorInstances` 多实例映射 | 修复分屏架构缺陷                      | ~150 行  |
| **P0** | DOM 重新挂载后 `requestMeasure()`                   | dockview 拖拽兼容                     | ~20 行   |
| **P0** | 同一文件打开冲突检测                                | 已打开时切换到已有实例                | ~30 行   |
| **P1** | EditorState 保存到 OpenFileInfo                     | 选区/历史不丢失                       | ~50 行   |
| **P1** | 同 Group 内 EditorView 复用                         | `view.setState()` 替代重建，切换 <5ms | ~100 行  |
| **P1** | 大文件分级处理                                      | 按 1MB/10MB/50MB/200MB 分级           | ~120 行  |
| **P2** | Tauri EventSystem 跨窗口通信                        | 弹出窗口基础设施                      | ~200 行  |
| **P2** | Group 级分组序列化/反序列化                         | 为弹出/合并做准备                     | ~150 行  |
| **P3** | 崩溃恢复（EditorState 持久化）                      | localStorage 快照                     | ~80 行   |
| **P3** | 弹出窗口生命周期                                    | WebviewWindow 创建/合并/销毁          | ~200 行  |

### 15.2 不变事项

以下当前架构设计保持不变：

- EditorPanel.vue 作为唯一编辑器面板（不区分 SQL/Code）
- dockview-vue 的原生 Float Tab 机制
- AG Grid 虚拟滚动（结果集渲染）
- Scratchpad 的双层持久化（localStorage + 文件系统）
- Tauri IPC 桥接（`invoke` 调用 Rust 后端）
- SQL 执行/格式化/DuckDB 加速流程

---

## 16. 实现进度

> 最后更新：2026-05-18

### 16.1 已完成

| 任务                                     | 状态                                                          | 改动文件                                                                                                                                              | 说明                                                                                                                                                                                                                     |
| ---------------------------------------- | ------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **P0-1 多实例映射**                      | ✅ 完成                                                       | `editor-types.ts`, `EditorManager.ts`                                                                                                                 | `fileEditors: Map<filePath, EditorView>` → `editorInstances: Map<instanceId, EditorInstance>`；新增 `EditorInstance` 接口                                                                                                |
| **P0-2 DOM re-attach**                   | ✅ 完成                                                       | `EditorPanel.vue`                                                                                                                                     | `MutationObserver` 监听 `.cm-container` 父元素 `childList`，dockview 拖拽后自动 `requestMeasure()`                                                                                                                       |
| **P0-3 冲突检测**                        | ✅ 完成                                                       | `EditorManager.ts`                                                                                                                                    | `isFileOpenElsewhere()` 跨 Group 检测；`openFile()` 增强                                                                                                                                                                 |
| **P1-2 同组 EditorView 复用**            | ⏳ Phase B（多实例映射）已完成，Phase C（Group 级复用）未实现 | `useCodeMirror.ts`, `EditorManager.ts`, `EditorPanel.vue`                                                                                             | `setEditorState()`/`getEditorState()` API；`savedStates` 缓存；生命周期状态保存/恢复；Group 级 `view.setState()` 切换待实现                                                                                              |
| **P1-1 EditorState 保存到 OpenFileInfo** | ✅ 完成                                                       | `editor-types.ts`, `EditorManager.ts`                                                                                                                 | `OpenFileInfo.states`/`primaryInstanceId`/`readonlyInstanceIds`；全生命周期同步                                                                                                                                          |
| **P1-3 大文件分级处理**                  | ✅ 完成                                                       | `useLargeFile.ts` (新建), `useCodeMirror.ts`, `EditorPanel.vue`                                                                                       | 5 级策略（normal/reduced/large/chunked/rejected）；按 tier 条件加载扩展                                                                                                                                                  |
| **P2 跨窗口通信**                        | ✅ 完成                                                       | `useCrossWindow.ts` (新建), `EditorManager.ts`, `WorkbenchView.vue`, `popout.ts`                                                                      | Tauri EventSystem 封装；`PopoutTransfer`/`MergeTransfer`/`StateSync`/`WindowReady`；`popoutActiveFile()` + `setupCrossWindowListeners()`（在 WorkbenchView.onReady 中调用）；popout.ts 完整初始化                        |
| **P3 崩溃恢复**                          | ✅ 完成                                                       | `useEditorRecovery.ts` (新建), `EditorManager.ts`, `WorkbenchView.vue`                                                                                | `saveSnapshot()`→localStorage；`loadSnapshots()`/`hasRecoveryData()`；恢复横幅 UI（恢复全部/忽略）；4MB 总额 + 2MB 单文件配额降级策略                                                                                    |
| **IPC 对齐 (6 P0)**                      | ✅ 完成                                                       | `lib.rs`, `metadata_commands.rs`, `metadata_cache_commands.rs`, `workbench-store.ts`, `use-connection-health.ts`, `project-connection.ts`, `query.ts` | 命令名修正/注册；参数名/类型对齐；缺少字段补充                                                                                                                                                                           |
| **代码清理 第一轮**                      | ✅ 完成                                                       | 5 删除 + 7 修改                                                                                                                                       | Monaco 残留：EditorSettingsPopup(361行)、3个Test\*Panel、useDialectSync(75行)、sql-editor-service死代码(185行)                                                                                                           |
| **P1-2 popout.ts**                       | ✅ 完成                                                       | `popout.ts`                                                                                                                                           | 填充完整 CodeMirror 初始化 + `PopoutTransfer` 监听 + `beforeunload` 合并回传                                                                                                                                             |
| **P3-3 IEditorManager**                  | ✅ 完成                                                       | `editor-types.ts`                                                                                                                                     | 补全 18 个缺失方法签名                                                                                                                                                                                                   |
| **any/magic/log 清理**                   | ✅ 完成                                                       | 6 文件                                                                                                                                                | any 7→2处；魔法字符串提取 4 常量 2 函数；console.log 移除 5 调试日志                                                                                                                                                     |
| **代码清理 第二轮**                      | ✅ 完成                                                       | 10 删除                                                                                                                                               | 死文件：menuActionHandlers.ts、useEditorManager.ts、useGridKeyboard.ts、useDockviewKeyboard.ts、useResultTabs.ts、useFilterModes.ts、RightSidebarPlaceholder.vue、ThreeColumnLayout.vue、ActivityBarPanel.vue、format.ts |
| **内存泄漏修复**                         | ✅ 完成                                                       | 5 文件                                                                                                                                                | MainContentArea resize onUnmounted、EditorManager UnlistenFn 保存/destroy、QuickFilterInput timer 清理、ToolbarActions setTimeout 竞态、project.ts Promise.race setTimeout                                               |
| **错误处理增强**                         | ✅ 完成                                                       | 3 文件                                                                                                                                                | toggleComment 废弃 API→@codemirror/commands；空 catch 块添加 warn 日志；emit 错误日志                                                                                                                                    |
| **popout 链路修复**                      | ✅ 完成                                                       | `WorkbenchView.vue`                                                                                                                                   | `setupCrossWindowListeners()` 在 onReady 中调用                                                                                                                                                                          |
| **any 类型清零**                         | ✅ 完成                                                       | `TableStructurePanel.vue`, `TableDataPanel.vue`                                                                                                       | ColumnInfo/IndexInfo/ConstraintInfo 接口定义；any[][]→unknown[][]                                                                                                                                                        |
| **静默错误修复**                         | ✅ 完成                                                       | `EmptyWorkbenchPanel.vue`, `TableStructurePanel.vue`                                                                                                  | 错误 catch 增加 message.error 用户反馈                                                                                                                                                                                   |
| **SQL 执行竞态修复**                     | ✅ 完成                                                       | `useSqlExecution.ts`                                                                                                                                  | executeSingleStatement/executeBatch 添加 executing 状态守卫                                                                                                                                                              |
| **规则文件更新**                         | ✅ 完成                                                       | 4 个 `.trae/rules/*.md`                                                                                                                               | Monaco Editor → CodeMirror 6                                                                                                                                                                                             |
| **旧文档废弃**                           | ✅ 完成                                                       | 4 个 `docs/frontend/*.md`                                                                                                                             | 加 ⛔ 废弃标记 + 指向 v3.0                                                                                                                                                                                               |

### 16.2 未完成

> 仅剩余跨窗口串行反序列化和光标同步：

| 方向                        | 说明                                                                                             |
| --------------------------- | ------------------------------------------------------------------------------------------------ |
| **onDidDockGroup 反序列化** | `WorkbenchView.vue` 中仅日志输出，合并外部 Group 时反序列化数据→注册实例→dockview 重建逻辑待实现 |
| **跨窗口光标实时同步**      | `StateSync` 事件已定义，`listenStateSync` 已就绪；EditorPanel 中 watch cursor position 后 emit   |

### 16.3 关键设计决策记录

1. **EditorInstance 的 writable 标志**：同一文件在多个 Group 中打开时，首个实例标记为可写，后续实例只读。避免实现复杂的 OT/CRDT 冲突合并。
2. **registerFileEditor 向后兼容**：保持 `(filePath, ed)` 签名不变，内部通过 dockview API 查找 groupId。
3. **getEditorView() 优先级**：先查找 `writable=true` 的实例，保证只有一个编辑源。
4. **MutationObserver 范围**：仅监听父元素的 `childList`，最小化性能开销。
5. **EditorState 保存策略**：内存中保存完整 `EditorState` 对象避免 JSON 序列化开销（P1），localStorage 持久化用于崩溃恢复（P3）。
6. **大文件分级策略**：>10MB 禁用自动补全/折叠/lint，>50MB 仅保留基础输入能力，>200MB 拒绝打开。
7. **崩溃恢复配额策略**：总 localStorage 额度 4MB，单文件 2MB。超限自动降级为仅保存元数据（文件路径+脏标记），丢弃 EditorState JSON。
8. **跨窗口通信方案**：基于 Tauri `emit`/`listen` 全局事件总线，不依赖 `window.postMessage`。事件名统一前缀 `editor:`，Payload 类型化。Popout 窗口通过 dockview 的 `popout.html` + `addPopoutGroup()` 创建，EditorManager 负责状态转移。

---

> **文档维护**：此文档为编辑器架构的唯一权威来源。后续任何编辑器相关架构变更必须先更新此文档。
