# ⛔ 已废弃 — SQL 编辑器优化开发计划

> **此文档已废弃，不再维护。**
> 原因：编辑器引擎已从 Monaco Editor 迁移至 CodeMirror 6（2026-05-17 完成）。
> 替代文档：[编辑器架构设计文档 v3.0](../editor-architecture-v3.md)
>
> 以下为历史存档内容，仅供参考。

---

# SQL 编辑器优化开发计划（已废弃）

> 版本：v2.4
> 创建日期：2026-05-08
> 状态：✅ P0-P6 全覆盖（全局配置持久化集成：fontSize/tabSize/wordWrap/minimap/lineNumbers/fontFamily 读写 + 编辑器启动应用）

---

## 📖 目录

- [优化目标](#优化目标)
- [现状分析](#现状分析)
- [Phase 1：类型与持久化统一](#phase-1类型与持久化统一)
- [Phase 2：组件拆分 + Composable 抽取](#phase-2组件拆分--composable-抽取)
- [Phase 3：通信机制重构](#phase-3通信机制重构)
- [Phase 4：体验增强](#phase-4体验增强)
- [Phase 8：DuckDB 加速桥接](#phase-8duckdb-加速桥接)
- [Phase 9：P1 功能补齐 (图表/导出/补全)](#phase-9p1-功能补齐-图表导出补全)
- [Phase 10：P2 批量执行 & 执行计划](#phase-10p2-批量执行--执行计划)
- [Phase 11：P3 细节增强（代码折叠/收藏/参数化/内联编辑）](#phase-11p3-细节增强代码折叠收藏参数化内联编辑)
- [Phase 12：P4 质量与打磨（重构/错误定位/Minimap/设置面板）](#phase-12p4-质量与打磨重构错误定位minimap设置面板)
- [Phase 13：P5 高级 UX 增强（右键菜单/自动保存/历史重执行/欢迎页连接）](#phase-13p5-高级-ux-增强右键菜单自动保存历史重执行欢迎页连接)
- [Phase 14：P6 全局配置集成（设置持久化到 useAppStore）](#phase-14p6-全局配置集成设置持久化到-useappstore)
- [文件变更清单](#文件变更清单)
- [验收标准](#验收标准)
- [风险控制](#风险控制)

---

## 优化目标

1. **组件轻量化**：将 1600+ 行的 `SqlEditorPanel.vue` 拆分为多个独立组件 + composables，单一文件不超过 300 行
2. **类型安全强化**：消除所有 `as any` 强制转换，统一连接/Dockview params/方言类型定义
3. **通信规范化**：用 Pinia Store + provide/inject 替代 7+ 个全局 CustomEvent
4. **持久化统一**：创建 `useEditorPersistence.ts` 统一管理 localStorage 读写
5. **可测试性提升**：每个 composable 可独立进行单元测试

---

## 现状分析

| 问题                                   | 严重程度 | 影响                             |
| -------------------------------------- | -------- | -------------------------------- |
| SqlEditorPanel.vue 1600+ 行，职责过多  | 🔴 P0    | 维护困难、新功能风险高           |
| 7+ 个 CustomEvent 在 window 上广播     | 🔴 P0    | 隐式耦合、调试困难、内存泄漏风险 |
| `(conn as any).dbType` 松散类型遍布    | 🟡 P1    | 类型安全缺失、IDE 提示失效       |
| localStorage key 散落三处，无统一管理  | 🟡 P1    | 持久化逻辑碎片化                 |
| 方言高亮每次完整重建 Monarch tokenizer | 🟢 P2    | 切换连接有微小性能开销           |
| 执行中的查询无法取消                   | 🟢 P2    | 用户体验欠缺                     |

---

## Phase 1：类型与持久化统一

> 🎯 目标：打好基础，不改变任何运行时行为，后续 Phase 的铺路石
> ⏱ 预估复杂度：低风险，2-3 天

### 1.1 统一方言类型定义

**问题**：`SqlDialect` 重复定义在 `sql-editor-service.ts` 和 `sql-dialect-highlight.ts`

**操作**：

- 新增 `src/shared/types/sql.ts`，定义 `SqlDialect`、`DatabaseType` 并统一导出
- 两个 service 文件改为 `import type { SqlDialect } from '@/shared/types/sql'`
- 删除各自文件中的重复定义

### 1.2 统一连接类型定义

**问题**：`Connection` 类型中 `dbType` 字段缺失，导致到处 `(conn as any).dbType`

**操作**：

- 在 `src/shared/types/index.ts` 中完善 `Connection` 接口，新增 `dbType: DatabaseType` 字段
- 确保 `connectionStore.connections` 类型为 `Connection[]`
- 逐步替换 `SqlEditorPanel.vue` 中的 `(conn as any).dbType` 为 `conn.dbType`

### 1.3 统一 Dockview Params 类型

**问题**：Dockview 嵌套 `params.params` 结构导致到处都是 `(props.params as any)?.params?.connectionId`

**操作**：

- 新增类型 `SqlEditorParams`：
  ```typescript
  interface SqlEditorParams {
    connectionId?: string
    databaseName?: string
    initialSql?: string
    panelId?: string
    schema?: string
  }
  ```
- 在 `SqlEditorPanel.vue` 中新增一个 `resolvedParams` computed 统一解包

### 1.4 统一 Tauri 响应类型

**操作**：

- 新增 `ExecuteSqlResponse` 接口，明确 `result` 结构
- 替换 `executeSingleStatement()` 中的 `result.result || result` 防御性写法

### 1.5 创建 useEditorPersistence.ts

**操作**：新增文件 `src/extensions/builtin/workbench/ui/composables/useEditorPersistence.ts`

**职责**：

- 统一 localStorage key 前缀 `rdata:workbench:`
- 提供 `draft.save()` / `draft.load()` / `draft.remove()`（替代散落的 `sql-draft-${panelId}` key）
- 提供 `clearOrphanDrafts()` 清理已关闭面板的过期草稿（7天 TTL）

---

## Phase 2：组件拆分 + Composable 抽取

> 🎯 目标：将大组件拆分为可独立维护的单元
> ⏱ 预估复杂度：中风险，3-4 天

### 2.1 拆分方案总览

```
SqlEditorPanel.vue              ← 仅保留布局编排 (~80行)
├── EditorToolbar.vue            ← 工具栏组件 (~120行)
├── EditorStatusbar.vue          ← 状态栏组件 (~80行)
├── EditorWelcome.vue            ← 欢迎页水印 (~40行)
├── TranspileModal.vue           ← 方言转换弹窗 (~50行)
├── composables/
│   ├── useEditorPersistence.ts  ← Phase 1 创建，Phase 2 集成
│   ├── useMonacoEditor.ts       ← Monaco 初始化/销毁/主题/快捷键 (~250行)
│   ├── useSqlExecution.ts       ← 执行逻辑 (单/多语句/Explain/DuckDB) (~200行)
│   ├── useConnectionBinding.ts  ← 连接选择/ensure/waitFor (~150行)
│   └── useDialectSync.ts        ← 方言监听 + 高亮补全注册/注销 (~60行)
```

### 2.2 步骤 2.1：抽取小组件（低风险先练手）

| 序号 | 操作             | 来源文件                     | 目标文件             |
| ---- | ---------------- | ---------------------------- | -------------------- |
| 1    | 提取欢迎页水印   | SqlEditorPanel.vue:L155-L167 | `EditorWelcome.vue`  |
| 2    | 提取方言转换弹窗 | SqlEditorPanel.vue:L108-L131 | `TranspileModal.vue` |

**EditorWelcome.vue 接口**：

```typescript
interface Props {
  visible: boolean
}
// 无 emits，纯展示组件
```

**TranspileModal.vue 接口**：

```typescript
interface Props {
  visible: boolean
  dialectOptions: Array<{ label: string; key: SqlDialect }>
}
interface Emits {
  (e: 'close'): void
  (e: 'transpile', targetDialect: SqlDialect): void
}
```

### 2.3 步骤 2.2：抽取 useMonacoEditor composable

**来源**：SqlEditorPanel.vue 中 `initEditor()` 全部逻辑 + `onUnmounted` 销毁逻辑 + 主题/内容 watch

**文件**：`src/extensions/builtin/workbench/ui/composables/useMonacoEditor.ts`

**API 设计**：

```typescript
export function useMonacoEditor(options: {
  containerRef: Ref<HTMLElement | undefined>
  panelId: string
  initialValue?: string
  language?: string
  theme?: string
  onContentChange?: (value: string) => void
  onCursorChange?: (position: { lineNumber: number; column: number }) => void
  onSelectionChange?: (info: { lines: number; chars: number } | null) => void
}) {
  // 返回
  return {
    editor: Readonly<Ref<monaco.editor.IStandaloneCodeEditor | null>>
    getValue: () => string
    setValue: (value: string) => void
    getSelectedText: () => string
    insertText: (text: string) => void
    focus: () => void
    showWelcome: Ref<boolean>
    cursorPosition: Ref<string>
    selectedTextInfo: Ref<string>
    // 快捷键注册
    registerCommand: (keybinding: number, handler: () => void) => void
  }
}
```

### 2.4 步骤 2.3：抽取编辑器子组件

**EditorToolbar.vue**

```typescript
interface Props {
  toolbarPosition: 'top' | 'left' | 'right'
  isDuckDb: boolean
}
interface Emits {
  (e: 'execute'): void
  (e: 'executeNew'): void
  (e: 'duckdbExecute'): void
  (e: 'explain'): void
  (e: 'format'): void
  (e: 'validate'): void
  (e: 'settings'): void
  (e: 'togglePosition'): void
}
```

**EditorStatusbar.vue**

```typescript
interface Props {
  cursorPosition: string
  selectedTextInfo: string
  editorMode: string
  executing: boolean
  lastExecutionTime: number | null
  connectionInfoText: string
  popselectOptions: Array<{ label: string; value: string }>
  selectedConnection: string
}
interface Emits {
  (e: 'connectionChange', connId: string): void
}
```

### 2.5 步骤 2.4：抽取业务 composables

**useSqlExecution.ts**

```typescript
export function useSqlExecution(options: {
  panelId: string
  getEditorValue: () => string
  getSelectedText: () => string
  selectedConnection: Ref<string>
  runtimeConnId: Ref<string>
  getCurrentDialect: () => SqlDialect
}) {
  return {
    executing: Ref<boolean>
    lastExecutionTime: Ref<number | null>
    hasResults: Ref<boolean>
    currentResultData: Ref<ResultData | null>
    execute: () => Promise<void>
    executeNew: () => Promise<void>
    explain: () => Promise<void>
    duckdbExecute: () => Promise<void>
    cancelExecution: () => void  // Phase 4 实现
  }
}
```

**useConnectionBinding.ts**

```typescript
export function useConnectionBinding(options: {
  panelId: string
  initialConnectionId?: string
  paramsConnectionId: Ref<string>
}) {
  return {
    selectedConnection: Ref<string>
    runtimeConnId: Ref<string>
    connectionInfoText: Ref<string>
    popselectOptions: Ref<Array<{ label: string; value: string }>>
    isDuckDbConnection: Ref<boolean>
    ensureConnection: (connId: string) => Promise<boolean>
    onConnectionSelected: (connId: string) => void
  }
}
```

**useDialectSync.ts**

```typescript
export function useDialectSync(options: {
  selectedConnection: Ref<string>
}) {
  return {
    currentDialect: Ref<SqlDialect>
    getCurrentDialect: () => SqlDialect
    updateDialectHighlight: () => void
  }
}
```

### 2.6 步骤 2.5：重构 SqlEditorPanel 为编排层

重构后的 `SqlEditorPanel.vue` 仅负责：

- 组合各 composables
- 将数据分发给子组件
- 处理编辑器与结果面板的分割布局

目标行数：~80 行

---

## Phase 3：通信机制重构

> 🎯 目标：用 Pinia Store + provide/inject 替代全局 CustomEvent
> ⏱ 预估复杂度：中风险，2-3 天

### 3.1 事件迁移表

| 当前事件                     | 迁移方式           | 目标                                      |
| ---------------------------- | ------------------ | ----------------------------------------- |
| `sql-execution-result`       | **Pinia Store**    | `sqlExecutionStore.results` reactive Map  |
| `query-result-new`           | **Pinia Store**    | `sqlExecutionStore.openInNewTab()` action |
| `query-result-refresh`       | **provide/inject** | 父→子回调函数                             |
| `query-result-export-insert` | **provide/inject** | 子→父回调函数                             |
| `query-result-updated`       | **Pinia Store**    | `watch(executionResults)` 替代            |
| `open-settings-panel`        | **Pinia Store**    | `uiStore.openSettings()` action           |
| `save-sql-file`              | **provide/inject** | 编辑器→布局管理器                         |

### 3.2 sql-execution-store.ts 扩展

新增 action：

```typescript
function openInNewTab(result: ExecutionResult): void {
  // 通知布局管理器新建结果面板
  newTabRequests.value.set(result.panelId, result)
}

function refreshResult(panelId: string): void {
  // 触发编辑器重新执行
  refreshRequests.value.set(panelId, Date.now())
}
```

### 3.3 迁移策略

每个事件采用 **双通道过渡** 方式：

1. 先添加新的 Pinia Store / provide-inject 通信
2. 保留旧的 CustomEvent 发送
3. 验证新通道工作正常
4. 移除旧通道

### 3.4 通信架构图

```
                    ┌──────────────────────┐
                    │     Pinia Store        │
                    │  ┌─────────────────┐   │
                    │  │ sqlExecutionStore│   │
                    │  │  - results       │   │
                    │  │  - newTabRequests│   │
                    │  │  - refreshRequests│  │
                    │  └─────────────────┘   │
                    │  ┌─────────────────┐   │
                    │  │ uiStore          │   │
                    │  │  - settingsOpen  │   │
                    │  └─────────────────┘   │
                    └──────┬───────┬─────────┘
                           │       │
                 watch/read│       │watch/read
                           │       │
           ┌───────────────┴──┐ ┌──┴────────────────┐
           │ SqlEditorPanel   │ │ QueryResultPanel   │
           │ (父组件)          │←│ (子组件)           │
           │                  │ │                    │
           │ provide:         │ │ inject:            │
           │  onRefresh       │ │  refresh()         │
           │  onExportInsert  │ │  exportInsert()    │
           └──────────────────┘ └────────────────────┘
```

---

## Phase 4：体验增强

> 🎯 目标：方言高亮增量更新 + Abort 查询取消
> ⏱ 预估复杂度：低风险，1-2 天
> 状态：✅ 已完成

### 4.1 方言高亮增量更新

**当前问题**：每次切换连接调用 `setMonarchTokensProvider` 完整重建 tokenizer，包括 ~60 基础关键字 + 操作符 + tokenizer 规则等不变部分

**优化方案**：

- 抽取 `BASE_KEYWORDS` 常量（60+ 通用 SQL 关键字）和 `buildBaseTokenizerConfig()` 工厂函数
- 新增 `buildDialectConfig(dialect)` 组合基础配置 + 方言关键字
- `registerDialectHighlight()` 改为按方言缓存，同一方言不重复注册
- 新增 `getCurrentDialect()` / `setCurrentDialect()` 状态管理

**修改文件**：

- [sql-dialect-highlight.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/services/sql-dialect-highlight.ts) — 重构 tokenizer 构建逻辑
- [useDialectSync.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/composables/useDialectSync.ts) — 修复导入路径（`getEditorDialect`→`getCurrentDialect`）

**预期收益**：

- 切换同名方言：零开销（缓存命中）
- 首次切换新方言：仅拼装关键字数组，基础 tokenizer 规则复用
- 内存：10 种方言各缓存一份配置，约 10KB

### 4.2 Abort 查询取消

**API 层**：`queryService.cancelQuery()` 已存在（调用 `cancel_query` Tauri 命令）

**Store 层**：在 `useSqlExecution` 中新增 `cancelExecution()`：

```typescript
const abortControllers = new Map<string, AbortController>()

async function executeWithCancel(panelId, sql, connId) {
  const controller = new AbortController()
  abortControllers.set(panelId, controller)
  try {
    return await invoke('execute_sql', { ... })
  } finally {
    abortControllers.delete(panelId)
  }
}

function cancelExecution(panelId) {
  const controller = abortControllers.get(panelId)
  controller?.abort()
  queryService.cancelQuery(runtimeConnId.value)
}
```

**UI 层**：在状态栏"执行中"指示器旁边增加 `X` 取消按钮

### Phase 5: 死代码清理与质量完善 ⚡（已完成）

Phase 5 侧重代码质量，清理 4 个 Phase 之后残留的死代码、未使用变量、类型松散问题。

**5.1 `sql-execution-store.ts` 清理**：

| 问题                            | 修复                                    |
| ------------------------------- | --------------------------------------- |
| `any[][]` 松散类型              | 改为 `unknown[][]`                      |
| computed 中有副作用             | `latestNewTabRequest` 移除 `clear()`    |
| store 自引用                    | `consumeNewTabRequest` 直接访问内部状态 |
| 未使用的 `executeSql` action    | 删除                                    |
| 未使用的 `pendingRequests` 状态 | 删除（含接口和 getter）                 |

**5.2 `useSqlExecution.ts` 统一执行路径**：

- 移除直接 `invoke('execute_sql')` 调用
- 统一使用 `queryService.executeSql()`
- 移除 `handleFormat` / `handleValidate`（迁移回 `SqlEditorPanel.vue` 直接调用 service）
- 移除未使用的 `getCurrentDialect` 参数

**5.3 `SqlEditorPanel.vue` 死代码清理**：

- 移除 `execHandleFormat` / `execHandleValidate` 解构（composable 不再导出）
- 移除 `onRefreshResult` 传参（不再被 composable 接受）
- 移除 `useConnectionBinding` 中多余的 `paramsConnectionId` 参数
- `handleFormat()` / `handleValidate()` 重写为直接调用 `formatSql()` / `validateSql()` service

**5.4 `EditorStatusbar.vue` 清理**：

- 移除未使用的 `RenderLabelImpl` 类型导入
- 移除未使用的 `renderConnectionLabelImpl` 变量

**5.5 `useConnectionBinding.ts` 清理**：

- 移除未使用的 `paramsConnectionId` 参数（接口 + 解构）

**验收**：

- [x] `pnpm run lint` 相关文件零 error
- [x] `pnpm run format` 通过
- [x] 无新增 TS 编译错误

### Phase 6: 前后端对齐与功能补齐 ⚡（已完成）

Phase 6 修复了前后端对比分析中发现的 P0/P1 级别缺口。

**6.1 P0-1 cancel_query 后端注册 + 前端打通**：

| 文件                                                                                                                                            | 变更                                                                                                                        |
| ----------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| [connection_manager.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/services/connection_manager.rs)               | 新增 `cancel_tokens` 字段；`create_cancel_token()` / `cancel_query()` / `remove_cancel_token()` 方法                        |
| [sql_service.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/services/sql_service.rs)                             | `execute()` 使用 `create_cancel_token` → `query_with_cancel` → `remove_cancel_token` 新流程；新增 `cancel_query()` 服务方法 |
| [sql_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/sql_commands.rs)                                | 新增 `cancel_sql_query` Tauri 命令                                                                                          |
| [lib.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/lib.rs)                                                           | 注册 `cancel_sql_query`                                                                                                     |
| [query.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/query/ui/services/query.ts)                            | `cancelQuery()` 参数名 `queryId→connId` 对齐后端，调用 `cancel_sql_query` 代替不存在 `cancel_query`                         |
| [useSqlExecution.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/composables/useSqlExecution.ts) | `cancelExecution()` 同时调用 `AbortController.abort()` + `queryService.cancelQuery()`                                       |

**取消链路**：用户点 X → JS AbortController + 后端 CancellationToken 双通道取消。

**6.2 P0-2 清理未注册 execute_query**：

| 文件                                                                                                                 | 变更                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| [query.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/query/ui/services/query.ts) | 删除 `executeQuery()` 函数（调用不存在命令 `execute_query`）；移除未使用 `QueryResult` import |

**6.3 P1-0 注册事务四命令**：

| 文件                                                                                  | 变更                                                                                                |
| ------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| [lib.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/lib.rs) | 注册 `begin_transaction` / `commit_transaction` / `rollback_transaction` / `get_transaction_status` |

**6.4 P1-1 事务控制按钮**：

| 文件                                                                                                                                                    | 变更                                                                                                                     |
| ------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| [query.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/query/ui/services/query.ts)                                    | 新增 `beginTransaction` / `commitTransaction` / `rollbackTransaction` / `getTransactionStatus`；`TransactionStatus` 接口 |
| [useSqlExecution.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/composables/useSqlExecution.ts)         | `inTransaction` ref + `beginTransaction` / `commitTransaction` / `rollbackTransaction` 函数                              |
| [EditorStatusbar.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/EditorStatusbar.vue) | 🟢 TX 指示器 + Commit / Rollback 按钮；新增 props/emits                                                                  |
| [SqlEditorPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SqlEditorPanel.vue)   | 解构 transaction 变量 + 传递 prop + 绑定事件                                                                             |

**6.5 P1-2 split_sql 前端封装**：

| 文件                                                                                                                                            | 变更                   |
| ----------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------- |
| [sql-editor-service.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/services/sql-editor-service.ts) | 新增 `splitSql()` 函数 |

**6.6 P1-3 parse_sql 状态栏展示**：

| 文件                                                                                                                                                    | 变更                                                        |
| ------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| [useSqlExecution.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/composables/useSqlExecution.ts)         | `statementCount` ref + `scheduleParse()` 500ms 防抖自动解析 |
| [EditorStatusbar.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/EditorStatusbar.vue) | 显示 `N statements`                                         |
| [SqlEditorPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SqlEditorPanel.vue)   | content change 时触发 `scheduleParse()`                     |

**验收**：

- [x] `pnpm run lint` 全部相关文件零 error
- [x] `pnpm run format` 通过
- [x] `cargo check` 后端修改零 error/warning

### Phase 6 补充：SQL 方言转换修复 🔧（已完成）

**问题**：用户反馈 SQL 方言转换未见效。排查发现 3 个问题。

| 问题                                                                         | 文件                                                                                                                                                       | 修复                                                                         |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `handleTranspile` 调 `transpileSql(sql, targetDialect)` 缺少 `sourceDialect` | [SqlEditorPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SqlEditorPanel.vue#L311) | 补全 `transpileSql(sql, getCurrentDialect(), targetDialect)`                 |
| 转换成功/失败无 i18n 消息提示                                                | [SqlEditorPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SqlEditorPanel.vue#L315) | 添加 `t('sqlEditor.transpileSuccess')` / `transpileSame` / `transpileFailed` |
| NModal portal 到 body 导致 dockview 内不可见                                 | [TranspileModal.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/TranspileModal.vue#L2)   | 改为 `preset="card"` inline 渲染，移除手动定位 CSS                           |

### Phase 7: 体验增强 — 快捷键 + 生成 SQL + 片段面板 ⚡（已完成）

Phase 7 补齐 P2 级别的体验功能。

**7.1 P2-1 全局 Ctrl+Shift+E 快捷键**：

| 文件                                                                                                                                         | 变更                                                                                |
| -------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| [WorkbenchView.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/views/WorkbenchView.vue#L580) | 新增 `handleKeydown` 全局键盘监听；修复 onMounted 缺失的 `open-sql-editor` 事件注册 |

**7.2 P2-2 "生成 SQL" 右键菜单**：

| 文件                                                                                                                                                                  | 变更                                                                                                                                          |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| [use-context-menu-actions.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/database/ui/composables/use-context-menu-actions.ts#L358) | `generateSelect`/`generateInsert` 升级为"复制 + 打开 SQL 编辑器"双动作；新增 `generateUpdate`/`generateDelete`；views 也加了 `generateSelect` |

**7.3 P2-3 SQL 代码片段面板**：

| 文件                                                                                                                                                       | 变更                                                                                   |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| [SnippetPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SnippetPanel.vue)          | **新建** — 按分类展示 30+ 内置模板，搜索过滤，点击插入                                 |
| [SqlEditorPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SqlEditorPanel.vue#L265) | 新增 `handleInsertSnippet` 处理 `insert-snippet` 事件；onMounted/onUnmounted 注册/注销 |

**验收**：

- [x] `pnpm run lint` 全部相关文件零 error
- [x] `pnpm run format` 通过

---

## Phase 8: DuckDB 加速桥接 🚀（已完成）

Phase 8 实现 DuckDB 加速执行的前后端桥接，包括离线扩展预下载和 Tauri 命令暴露。

### 8.1 后端 — DuckDB 扩展离线包

在 DuckDBEngine 中实现 `init_extensions()` 静态方法：

- 在 APP 数据目录创建 `duckdb/extensions/` 目录
- 设置 `SET extension_directory` 指向该目录
- 按 P0 优先级遍历 `EXTENSION_MANIFEST`，执行 `LOAD` 加载：
  - `mysql_scanner` — MySQL 外部表扫描
  - `postgres_scanner` — PostgreSQL 外部表扫描
  - `sqlite_scanner` — SQLite 外部表扫描
- 扩展 `.duckdb_extension` 文件需预先放置在该目录（离线分发）

**涉及文件：**

- `src-tauri/src/core/dbi/engine/duckdb_engine.rs`
  - 新增 `ExtensionPriority` 枚举（P0 / P1）
  - 新增 `ExtensionInfo` 结构体（name / file / display / priority）
  - 新增 `EXTENSION_MANIFEST` 常量（6 个扩展清单）
  - 新增 `init_extensions()` 方法 — 扩展目录初始化 + P0 LOAD
  - 新增 `load_extension_by_name()` 方法 — 按需 P1 加载
  - 新增 `conn()` 公开方法 — 暴露 DuckDB 连接

### 8.2 后端 — AppState 集成

- `src-tauri/src/adapters/tauri/state.rs`
  - `AppState` 新增 `duckdb_engine: Arc<tokio::sync::Mutex<DuckDBEngine>>`
  - 使用 `tokio::sync::Mutex` 保证 `Send`（可跨 `.await` 持有锁）
  - 移除未使用的 `std::sync::Mutex` 导入

### 8.3 后端 — Tauri 命令桥接

新增 `execute_duckdb_accelerated` Tauri 命令：

```rust
#[tauri::command]
pub async fn execute_duckdb_accelerated(
    state: tauri::State<'_, AppState>,
    input: DuckDBAcceleratedInput,
) -> Result<DuckDBAcceleratedResponse, String>
```

**输入：** `DuckDBAcceleratedInput { sql, conn_id, db_type, data_dir }`
**输出：** `DuckDBAcceleratedResponse { success, columns, rows, elapsed_ms, error }`

执行流程：

1. 从 AppState 获取 DuckDB 引擎锁
2. 获取 DuckDB 连接（通过 `DuckDBManager::ensure_connection()`）
3. 若提供 `data_dir`，调用 `init_extensions()` 预加载扩展
4. 构建 `QueryContext`（`ExecutionMode::DuckDB`）
5. 调用 `engine.execute(sql, ctx)` 执行并获取 Arrow RecordBatch
6. 通过 `format_arrow_value()` 将 Arrow 数据转为 JSON
7. 返回 `DuckDBAcceleratedResponse`

**涉及文件：**

- `src-tauri/src/commands/sql_commands.rs`
  - 新增 `DuckDBAcceleratedInput` / `DuckDBAcceleratedResponse` 类型
  - 新增 `execute_duckdb_accelerated` Tauri 命令
  - 新增 `format_arrow_value()` 辅助函数 — Arrow → JSON 转换
- `src-tauri/src/lib.rs`
  - 注册 `execute_duckdb_accelerated` 命令

### 8.4 前端 — executeDuckDBAccelerated 服务函数

- `src/extensions/builtin/query/ui/services/query.ts`
  - 新增 `DuckDBAcceleratedParams` / `DuckDBAcceleratedResult` 接口
  - 新增 `executeDuckDBAccelerated()` 函数 — 封装 `invoke('execute_duckdb_accelerated')`
  - 转换 snake_case ↔ camelCase 字段名

### 8.5 前端 — useSqlExecution 集成

- `src/extensions/builtin/workbench/ui/composables/useSqlExecution.ts`
  - `SqlExecutionOptions` 新增 `currentDatabaseType?: Ref<string | null>`
  - 新增 `executeDuckDBAccelerated()` 方法
  - 通过 `@tauri-apps/api/path` 获取 `appDataDir()` 作为扩展目录
  - 调用 `queryService.executeDuckDBAccelerated()` 执行
  - 结果写入 `currentResultData` 和 SqlExecutionStore

### 8.6 前端 — SqlEditorPanel 入口

- `SqlEditorPanel.vue` 中 `handleDuckDbExecute()` 替换 "coming soon" 为实际实现：
  - 校验连接 → `ensureConnection()` → `executeDuckDBAccelerated()`
  - 传递 `currentDatabaseType` 给 `useSqlExecution`
  - 修正导入顺序（`@tauri-apps/api/core` → `monaco-editor`）

### 8.7 数据流

```
用户点击 DuckDB 按钮
  → handleDuckDbExecute()
    → ensureConnection(connId)
    → executeDuckDBAccelerated() [useSqlExecution]
      → appDataDir() 获取扩展目录
      → queryService.executeDuckDBAccelerated({ sql, connId, dbType, dataDir })
        → invoke('execute_duckdb_accelerated', ...) [Tauri IPC]
          → [Rust] state.duckdb_engine.lock().await
          → [Rust] DuckDBEngine::init_extensions(conn, dataDir)
            → LOAD mysql_scanner / postgres_scanner / sqlite_scanner
          → [Rust] engine.execute(sql, ctx)
            → DuckDB 内存数据库执行 SQL
          → [Rust] Arrow → JSON 转换
          → DuckDBAcceleratedResponse
      → storeResult({ columns, rows, elapsedMs, ... })
      → QueryResultPanel 渲染
```

### Phase 8 验收

- [x] DuckDB 扩展目录创建正常
- [x] P0 扩展 LOAD 流程正确
- [x] `execute_duckdb_accelerated` Tauri 命令注册成功
- [x] `cargo check` 编译通过（新增代码零 warning/error）
- [x] 前端 `handleDuckDbExecute` → `executeDuckDBAccelerated` 链路连通
- [x] 导入顺序符合 `import/order` 规范
- [x] `pnpm run lint` SqlEditorPanel 相关零 error
- [x] `currentDatabaseType` 正确传递至 `useSqlExecution`

---

## Phase 9: P1 功能补齐 (图表/导出/补全) 📊（已完成）

Phase 9 补齐 P1 优先级功能，打通数据可视化、导出、自动补全三大体验。

### 9.1 DuckDB ATTACH 自动桥接（P0 核心）

**问题：** 用户点击 ⚡ 按钮后，DuckDB 引擎只在自己内存数据库中跑 SQL，没有 ATTACH 源数据库，实际上查不到 MySQL/PostgreSQL/SQLite 的数据。

**解决：** 改造 `execute_duckdb_accelerated` Rust 命令，内置 ATTACH / DETACH 生命周期：

```
1. get_connection_manager().get_connection_info(conn_id) → 获取 url + db_type
2. 构建 ATTACH 语句 (ATTACH '{url}' AS ext_{name} (TYPE {type}))
3. 初始化扩展 + 执行 ATTACH
4. 执行用户 SQL
5. DETACH IF EXISTS（best-effort）
```

**涉及文件：**

- [sql_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/sql_commands.rs#L305) — 新增 ATTACH 逻辑

### 9.2 图表可视化接入（P1-2）

`DataVisualizationPanel.vue`（ECharts 4 种图表）已实现但未接入结果面板。

**改动：**

- [result.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/types/result.ts#L3) — `ViewMode` 新增 `'chart'`
- [QueryResultPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/QueryResultPanel.vue) — 左侧视图侧边栏新增图表按钮，渲染 `DataVisualizationPanel`
- [zh-CN.json](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/shared/locales/zh-CN.json#L518) / [en.json](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/shared/locales/en.json#L509) — 新增 `chartView` i18n 键

### 9.3 结果集导出（P1-1 — 已存在）

导出功能已在 QueryResultPanel 完整实现，支持 5 种格式：

- CSV（ag-Grid exportDataAsCsv）
- JSON（Blob 下载）
- SQL INSERT（copyRowsAsInsert）
- Parquet / XLSX（DuckDB COPY TO，含保存对话框）

### 9.4 Schema 自动补全修复（P1-3）

`registerDatabaseCompletionProvider` 服务端已完整实现（information_schema 查询 → 表名 + 列名补全），但 SqlEditorPanel.vue 调用参数错误（传入 dialect 而非 connectionId）。

**修复：**

- [useConnectionBinding.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/composables/useConnectionBinding.ts) — 新增 `currentDatabase` computed
- [SqlEditorPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SqlEditorPanel.vue#L458) — 传正确参数：`registerDatabaseCompletionProvider(runtimeConnId, currentDatabase, undefined, dbType)`

### 9.5 前端 query.ts 补齐

- [query.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/query/ui/services/query.ts#L181) — 新增 `registerExternalDatabase()` / `createExternalTable()` 服务函数

### Phase 9 验收

- [x] DuckDB ATTACH → 执行 → DETACH 完整链路
- [x] 图表视图在结果面板可通过左侧按钮切换（bar/line/pie/scatter）
- [x] `ViewMode` 类型包含 `'chart'`
- [x] i18n 中英文 `chartView` 键已添加
- [x] Schema 自动补全参数正确传递
- [x] `cargo check` 新增代码零 error
- [x] `pnpm run lint` 新增代码零 error

---

## Phase 10: P2 批量执行 & 执行计划 ⚡（已完成）

Phase 10 实现剩余 P2 功能：多语句批量执行、DuckDB SQL 自动重写、EXPLAIN 执行计划。

### 10.1 多语句批量执行（P2-1）

工具栏新增 `ListChecks` "批量执行所有语句" 按钮，将编辑器 SQL 按 `;` 分割后逐条执行，每个结果自动创建一个独立标签页。

**流程：**

```
handleBatchExecute()
  → ensureConnection()
  → executeBatch() [useSqlExecution]
    → splitSql(sql) → statements[]
    → for each statement:
      → executeSql(stmt, connId)
      → resultStore.addTab() + setTabResult()
    → message.success(successCount, failed, totalElapsed)
```

**涉及文件：**

- [sql-editor-service.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/services/sql-editor-service.ts) — `splitSql()` 导出
- [useSqlExecution.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/composables/useSqlExecution.ts#L220) — 新增 `executeBatch()` 方法
- [EditorToolbar.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/EditorToolbar.vue#L24) — 新增批量执行按钮
- [SqlEditorPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SqlEditorPanel.vue#L329) — 新增 `handleBatchExecute()`
- [result-store.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/stores/result-store.ts) — `addTab()` + `setTabResult()` 复用

### 10.2 DuckDB SQL 自动重写（P2-2）

用户写 `SELECT * FROM users`，系统自动改写为 `SELECT * FROM ext_MyConn.users`，无需用户手动记住 ATTACH 前缀。

**实现：**

- `generateAttachName(connName)` — 与后端 `ext_{sanitized}` 算法一致
- `rewriteDuckDBSQL(sql, attachName)` — 正则匹配 `FROM|JOIN|INTO` 后的无前缀表名，自动加上 attach 前缀
- 安全过滤：跳过 SQL 关键字（WHERE/ON/SET/...），跳过已有 `.` 前缀的表名
- 在 `executeDuckDBAccelerated()` 中自动调用

**涉及文件：**

- [sql-editor-service.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/services/sql-editor-service.ts#L413) — `generateAttachName()` + `rewriteDuckDBSQL()`
- [useSqlExecution.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/composables/useSqlExecution.ts#L372) — 调用重写
- [useConnectionBinding.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/composables/useConnectionBinding.ts#L57) — 新增 `currentConnectionName`
- [SqlEditorPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SqlEditorPanel.vue#L254) — 传递 `currentConnectionName`

### 10.3 EXPLAIN 执行计划（P2-3）

工具栏新增 `FileSearch` "执行计划" 按钮，将当前 SQL 包装为 `EXPLAIN {sql}` 并执行。

**流程：**

```
handleExplain()
  → ensureConnection()
  → invoke('execute_sql', { sql: 'EXPLAIN ' + userSql })
  → resultStore.addTab() → 标题 "执行计划"
  → 结果渲染为文本视图
```

**涉及文件：**

- [EditorToolbar.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/EditorToolbar.vue) — 新增 FileSearch 按钮 + emit
- [SqlEditorPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SqlEditorPanel.vue#L339) — `handleExplain()` 函数

### Phase 10 验收

- [x] 多语句批量执行按钮可见，split → for-each → addTab 流程正确
- [x] DuckDB SQL 重写：`FROM users` → `FROM ext_MyConn.users`
- [x] DuckDB 重写不移除已有 `.` 前缀
- [x] EXPLAIN 按钮包装 SQL → 执行 → 结果标签页
- [x] `pnpm run lint` 新增代码零 error（3 预存 error 不变）

---

## Phase 11: P3 细节增强（代码折叠/收藏/参数化/内联编辑）✅（全部完成）

Phase 11 实现 P3 级别的细节增强功能，进一步提升编辑器使用体验。

### 11.1 Monaco 代码折叠（P3-1）✅

Monaco Editor 原生支持基于缩进和括号的代码折叠，但 SQL 代码有其特殊结构。

**实现：**

- `useMonacoEditor.ts` — editor 选项中启用 `foldingStrategy: 'auto'` + `showFoldingControls: 'always'`
- `sql-editor-service.ts` — 新增 `registerSqlFoldingProvider()` 自定义折叠范围提供器：

```
识别场景：
  - BEGIN ... END 事务/存储过程块
  - ( ... ) 嵌套子查询
  - WITH cte AS (...) CTE 块
  - /* ... */ 多行注释块
  - CREATE TABLE ( ... ) 表定义块
```

- `SqlEditorPanel.vue` — onMounted 注册，onBeforeUnmount 注销（Disposable 管理）

**涉及文件：**

- [useMonacoEditor.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/composables/useMonacoEditor.ts) — foldingStrategy + showFoldingControls
- [sql-editor-service.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/services/sql-editor-service.ts) — registerSqlFoldingProvider()
- [SqlEditorPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SqlEditorPanel.vue) — 注册/注销

### 11.2 SQL 代码收藏管理（P3-2）✅

用户可将当前编辑器中的 SQL 保存为自定义代码片段，并在片段面板中管理（插入/删除）。

**实现：**

- `EditorToolbar.vue` — 新增 Star ⭐ 收藏按钮，`saveSnippet` emit
- `SqlEditorPanel.vue` — `handleSaveSnippet()` 获取编辑内容 → `addCustomSnippet()` → 分类"收藏"
- `SnippetPanel.vue` — 自定义片段项右侧新增 × 删除按钮，hover 显示，调用 `deleteCustomSnippet()`
- `SnippetPanel.vue` CSS — `.snippet-delete` 绝对定位 + opacity 过渡动画

**涉及文件：**

- [EditorToolbar.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/EditorToolbar.vue) — Star 按钮 + saveSnippet emit
- [SqlEditorPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SqlEditorPanel.vue#L380) — handleSaveSnippet()
- [SnippetPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SnippetPanel.vue) — 删除按钮 + handleDelete()
- [zh-CN.json](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/shared/locales/zh-CN.json) / [en.json](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/shared/locales/en.json) — saveSnippet / snippetSaved i18n

### 11.3 查询参数化 :param 绑定（P3-3）✅

支持命名参数语法 `:param_name`，执行时弹出参数绑定窗口。

**流程：**

```
用户 SQL: SELECT * FROM users WHERE id = :user_id AND status = :status
  → 点击 Execute
    → detectParams() 检测到 :user_id, :status
    → 弹出 ParamBindingModal → 用户填值
    → bindParams() 替换为字面值 + 执行
```

**实现：**

- `sql-editor-service.ts` — `detectParams()`（正则检测 `:param` 占位符）+ `bindParams()`（值替换，带 SQL 注入转义）
- `ParamBindingModal.vue` — **新建**：动态表单 Modal（NInput），每个参数一个字段
- `useSqlExecution.ts` — `checkForParams()` + `buildBoundSql()` 封装
- `SqlEditorPanel.vue` — `handleExecute()` 先检查参数 → 有则弹窗 → 确认后执行
- 安全：值中的 `'` 自动转义为 `''`

**涉及文件：**

- [ParamBindingModal.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/ParamBindingModal.vue) — 新建
- [sql-editor-service.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/services/sql-editor-service.ts#L561) — detectParams() + bindParams()
- [useSqlExecution.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/composables/useSqlExecution.ts) — checkForParams() + buildBoundSql()
- [SqlEditorPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SqlEditorPanel.vue#L310) — handleExecute() 参数拦截
- [zh-CN.json](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/shared/locales/zh-CN.json) / [en.json](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/shared/locales/en.json) — paramBinding / paramHint

### 11.4 表数据 Inline Edit（P3-4）✅

**已有实现，无需新建代码。** AG Grid 的内联编辑功能已完整打通：

- `editable: true` — AG Grid 列可双击编辑
- `onCellValueChanged` — 追踪 dirty cells（旧值/新值/行列）
- `handleSave` — 调用 `save_cell_update` Tauri 命令生成 UPDATE
- `handleCancel` — 恢复原始值
- `save_cell_update` — 后端 Rust 命令，通过 `row_identity` + `table_name` 生成并执行 UPDATE
- Save/Cancel 按钮 — 底部状态栏自动显示（`tabHasDirty` 检测 dirty cells）

**涉及文件：**

- [QueryResultPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/QueryResultPanel.vue#L754) — onCellValueChanged + handleSave + handleCancel
- [result-analysis.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/services/result-analysis.ts#L454) — saveCellUpdate 前端封装
- [result_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/result_commands.rs#L58) — save_cell_update Tauri 命令

### Phase 11 验收

- [x] Monaco 代码折叠可折叠 BEGIN/END、子查询、CTE、注释块
- [x] 收藏按钮保存 SQL → 片段面板可见 → 可点击插入 → 可删除
- [x] i18n 中英文 saveSnippet / snippetSaved / paramBinding / paramHint 键已添加
- [x] 参数化查询 :param 检测 → 弹窗 → 绑定 → 执行完整链路
- [x] 表数据双击 Inline Edit（AG Grid editable + dirty cells + save/cancel）
- [x] `pnpm run lint` 新增代码零 error（仅 2 预存 vitest error）
- [x] 修复预存 `insertText is not defined` 错误（useMonacoEditor 解构补齐）

---

## Phase 12: P4 质量与打磨（重构/错误定位/Minimap/设置面板）✅（全部完成）

Phase 12 对代码质量和用户体验进行深度打磨，消除技术债务。

### 12.1 重构 handleParamConfirm 消除代码重复（P4-2）✅

`handleParamConfirm` 原本直接调用 `queryService.executeSql()` 并手动解析结果，与 `useSqlExecution.executeSql()` + `storeResult()` 完全重复（~50 行）。

**重构：**

- `useSqlExecution.ts` — 导出 `executeSql` + `storeResult` 到 return block
- `SqlEditorPanel.vue` — `handleParamConfirm` 从 ~65 行缩减到 ~30 行
- `SqlEditorPanel.vue` — 移除不再需要的 `queryService` 导入

```typescript
// Before: ~50 行手动解析
const result = await queryService.executeSql(boundSql, connId)
const qr = ((result as unknown) as Record<string, unknown>).result || ...
// ... 40 more lines ...

// After: 3 行
const result = await executeSql(boundSql, connId)
lastExecutionTime.value = result.elapsedMs
storeResult(result)
```

**涉及文件：**

- [useSqlExecution.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/composables/useSqlExecution.ts) — 导出 executeSql + storeResult
- [SqlEditorPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SqlEditorPanel.vue#L350) — 重构 + 移除 queryService import

### 12.2 SQL 错误位置高亮（P4-3）✅

SQL 执行错误时，自动解析错误信息中的行列号，在 Monaco 编辑器中标记错误位置并跳转。

**解析引擎：** `sql-editor-service.ts` → `parseErrorPosition()`

| 格式                   | 示例             | 支持              |
| ---------------------- | ---------------- | ----------------- |
| `at line N, column M`  | MySQL/PostgreSQL | ✅                |
| `line N ... column M`  | 通用格式         | ✅                |
| `near "xxx" at line N` | SQLite           | ✅                |
| `at position: N`       | DuckDB/字符偏移  | ✅ + 自动换行重算 |

**标记行为：**

- `setErrorMarker(editor, errorMsg, sql)` — 解析位置 → monaco Marker + `revealLineInCenter` + 光标定位
- `clearErrorMarkers(editor)` — 清除标记（下次执行前自动清除）
- 错误位置整行高亮（`endColumn = lineMaxColumn`）

**涉及文件：**

- [sql-editor-service.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/services/sql-editor-service.ts#L560) — parseErrorPosition + setErrorMarker + clearErrorMarkers
- [SqlEditorPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SqlEditorPanel.vue#L740) — watch currentResultData.error → set/clear markers

### 12.3 Monaco Minimap 切换（P4-1）✅

工具栏新增 Map 按钮，一键切换 Monaco 小地图显示。

**实现：**

- `useMonacoEditor.ts` — `setMinimap(enabled)` 封装 `updateOptions({ minimap })`
- `EditorToolbar.vue` — `Map` 图标按钮 + `toggleMinimap` emit
- `SqlEditorPanel.vue` — `showMinimap` ref + `toggleMinimap()` → `setMinimap()`
- 初始状态：**开启**（与现有行为一致）

### 12.4 编辑器设置面板（P4-4）✅

工具栏新增 Settings 齿轮按钮，弹出 Popover 实时调整编辑器配置。

| 设置项   | UI 控件      | 范围   | 初始值 |
| -------- | ------------ | ------ | ------ |
| 字号     | NSlider      | 10-28  | 14     |
| 缩进     | NInputNumber | 1-8    | 2      |
| 自动换行 | NSwitch      | on/off | ✅ on  |
| 行号     | NSwitch      | on/off | ✅ on  |

**实时生效**：所有改动立即通过 `useMonacoEditor.updateOptions()` 应用到编辑器。

**涉及文件：**

- [useMonacoEditor.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/composables/useMonacoEditor.ts) — setFontSize/setWordWrap/setLineNumbers/setTabSize
- [EditorToolbar.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/EditorToolbar.vue) — Settings 按钮
- [SqlEditorPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SqlEditorPanel.vue) — NPopover 设置面板 + 状态管理
- [zh-CN.json](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/shared/locales/zh-CN.json) / [en.json](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/shared/locales/en.json) — 5 个新 i18n 键

### 12.5 Bug 修复

- **修复**：`@explain` 和 `@save-snippet` 事件错误绑定在 `TranspileModal` 上 → 移至 `EditorToolbar`
- **修复**：`handleParamConfirm` 使用 `queryService` 直接调用 → 改用 composable 方法 → 移除未使用的 queryService import

### Phase 12 验收

- [x] handleParamConfirm 从 65 行缩减到 30 行（复用 executeSql + storeResult）
- [x] SQL 错误时 Monaco 自动标记错误行并跳转
- [x] Minimap 切换正常工作（Map 按钮 toggle）
- [x] Settings 面板可调整字号/缩进/换行/行号
- [x] `pnpm run lint` **零 error**（仅 492 预存 warning）
- [x] 移除未使用的 `queryService` import
- [x] 修复 EditorToolbar 事件绑定 bug

---

## Phase 13: P5 高级 UX 增强（右键菜单/自动保存/历史重执行/欢迎页连接）✅（全部完成）

Phase 13 聚焦高级用户交互体验，减少操作步骤、增强数据安全、提供快速入口。

### 13.1 Monaco 右键菜单定制（P5-2）✅

扩展 Monaco 编辑器原生右键菜单，添加数据库工作台专属操作。

**新增菜单项：**

| 菜单项          | 分组                      | 功能                                    |
| --------------- | ------------------------- | --------------------------------------- |
| "执行选中 SQL"  | `navigation` (顶部)       | 调用 `handleExecute()`，等效 Ctrl+Enter |
| "复制为 VALUES" | `9_cutcopypaste` (复制区) | 选中内容包装为 `VALUES (...)` → 剪贴板  |

**实现：**

- `useMonacoEditor.ts` — `registerContextActions(actions: IActionDescriptor[])` → 批量注册 + 返回 Disposables
- `SqlEditorPanel.vue` — onMounted 注册 2 个 action，onBeforeUnmount dispose
- 使用 Monaco 原生 `editor.addAction()` API，与系统上下文菜单无缝融合

### 13.2 Scratchpad 退出自动保存（P5-3）✅

草稿模式下关闭编辑器页面时，自动将未保存内容持久化到文件系统。

**流程：**

```
用户关闭 Scratchpad Tab
  → onBeforeUnmount
    → 检测 isScratchpadMode && isDirty
      → invoke('save_scratchpad_file', { content })
        → invoke('update_scratchpad_file_meta', { ... })
```

**实现：**

- `SqlEditorPanel.vue` — onBeforeUnmount 中检测 scratchpad dirty 状态 → 自动保存
- 静默执行（不阻塞关闭流程），`.catch(() => {})` 忽略可能的保存错误

### 13.3 SQL 历史双击重执行（P5-4）✅

SQL 历史面板中，双击历史记录直接重新执行 SQL。

**流程：**

```
用户双击历史记录
  → SqlHistoryPanel → dispatchEvent('sql-history-re-execute')
    → SqlEditorPanel 监听 → setValue(sql) + 切换连接 + handleExecute()
```

**实现：**

- `SqlHistoryPanel.vue` — `@dblclick` → `reExecuteHistory()` → `CustomEvent('sql-history-re-execute')`
- `SqlEditorPanel.vue` — onMounted 注册 `window.addEventListener`，onBeforeUnmount 移除
- 自动切换连接（如果 SQL 来自不同数据库连接）

### 13.4 Welcome 欢迎页连接快速启动（P5-1）✅

编辑器空白欢迎页展示最近连接列表，点击直接连接并清空编辑器。

**实现：**

- `EditorWelcome.vue` — 完全重写：接入 `useConnectionStore` 显示最近连接（最多 5 条），每个项目显示 `Database` 图标 + 连接名 + 数据库类型
- `EditorWelcome.vue` — 新增 `@connect` emit
- `SqlEditorPanel.vue` — `handleWelcomeConnect(connId)` → 切换连接 + 清空编辑器
- CSS：hover 状态品牌色边框高亮

### Phase 13 验收

- [x] Monaco 右键菜单出现 "执行选中 SQL" 和 "复制为 VALUES" 两个自定义项
- [x] 关闭脏 Scratchpad 标签页时自动保存内容
- [x] 双击历史记录即刻在上方编辑器重新执行
- [x] Welcome 页可点击最近连接快速连接并打开空白编辑器
- [x] `pnpm run lint` **零 error**（495 预存 warning）
- [x] i18n 键 `executeSelected` / `copyAsValues` / `recentConnections` 中英文齐全

---

## Phase 14: P6 全局配置集成（设置持久化到 useAppStore）✅

Phase 14 将 SQL 编辑器设置面板与全局配置系统（`useAppStore`）深度集成，使所有编辑器参数持久化到磁盘，跨会话保持。

### 14.1 配置读链路（已有 + 修复）

`SqlEditorPanel.vue` 已从 `appStore.effectiveEditorSettings` 读取初始值（`fontSize`/`tabSize`/`wordWrap`/`minimap`/`lineNumbers`/`fontFamily`）。

**修复：编辑器启动时未应用配置 → 新增启动后 set\* 调用：**

```typescript
createEditor()
setFontSize(editorFontSize.value) // 从 config 读取 → 应用到 Monaco
setTabSize(editorTabSize.value)
setWordWrap(editorWordWrap.value)
setLineNumbers(editorLineNumbers.value)
setMinimap(showMinimap.value)
setFontFamily(editorFontFamily.value)
```

之前编辑器创建时使用硬编码默认值（`fontSize: 14`），启动后未立即应用用户配置。现已修复。

### 14.2 配置写链路 ✅ 新建

所有设置改动通过 `persistEditorSettings()` → `appStore.saveConfig('editorSettings', payload, 'global')` 持久化到 `tauri-plugin-store` 的 JSON 文件。

**涉及操作：**

| 操作                  | 持久化时机                             |
| --------------------- | -------------------------------------- |
| 拖动字号 NSlider      | `@update:value` → `applyFontSize()`    |
| 修改缩进 NInputNumber | `@update:value` → `applyTabSize()`     |
| 切换自动换行 NSwitch  | `@update:value` → `applyWordWrap()`    |
| 切换行号 NSwitch      | `@update:value` → `applyLineNumbers()` |
| 切换 Minimap Map 按钮 | click → `toggleMinimap()`              |
| 修改字体 NInput       | `@update:value` → `applyFontFamily()`  |

**存储结构：**

```json
// tauri-plugin-store JSON 文件
{
  "editorSettings": {
    "fontSize": 16,
    "tabSize": 2,
    "wordWrap": true,
    "minimap": false,
    "lineNumbers": true,
    "fontFamily": "'JetBrains Mono', 'Fira Code', monospace"
  }
}
```

### 14.3 设置面板扩展 ✅

新增 `fontFamily` 设置项，完整覆盖 `EditorSettings` 接口的 6 个字段：

| 字段         | UI 控件            | 默认值                                                |
| ------------ | ------------------ | ----------------------------------------------------- |
| fontSize     | NSlider (10-28)    | 14                                                    |
| tabSize      | NInputNumber (1-8) | 2                                                     |
| wordWrap     | NSwitch            | true                                                  |
| lineNumbers  | NSwitch            | true                                                  |
| minimap      | Map 按钮 toggle    | true                                                  |
| _fontFamily_ | _NInput_           | _'Cascadia Code', 'Fira Code', 'Consolas', monospace_ |

### 14.4 跨面板同步

`watch(effectiveEditorSettings)` 监听外部配置变化（如从全局设置页修改），自动同步到当前编辑器。

### Phase 14 验收

- [x] 编辑器启动时从全局配置读取 6 项设置并应用到 Monaco
- [x] 修改任意设置项立即持久化到 `tauri-plugin-store` 文件
- [x] 关闭并重新打开应用 → 编辑器设置保持不变
- [x] 新增 `fontFamily` 设置项完整覆盖
- [x] `pnpm run lint` **零 error**（490 预存 warning）
- [x] i18n 键 `fontFamily` 中英文齐全

---

## 文件变更清单

### 新增文件

| #   | 文件                      | 位置                                                                        | Phase |
| --- | ------------------------- | --------------------------------------------------------------------------- | ----- |
| 1   | `sql.ts`                  | `src/shared/types/sql.ts`                                                   | P1    |
| 2   | `useEditorPersistence.ts` | `src/extensions/builtin/workbench/ui/composables/useEditorPersistence.ts`   | P1    |
| 3   | `EditorWelcome.vue`       | `src/extensions/builtin/workbench/ui/components/panels/EditorWelcome.vue`   | P2    |
| 4   | `TranspileModal.vue`      | `src/extensions/builtin/workbench/ui/components/panels/TranspileModal.vue`  | P2    |
| 5   | `EditorToolbar.vue`       | `src/extensions/builtin/workbench/ui/components/panels/EditorToolbar.vue`   | P2    |
| 6   | `EditorStatusbar.vue`     | `src/extensions/builtin/workbench/ui/components/panels/EditorStatusbar.vue` | P2    |
| 7   | `useMonacoEditor.ts`      | `src/extensions/builtin/workbench/ui/composables/useMonacoEditor.ts`        | P2    |
| 8   | `useSqlExecution.ts`      | `src/extensions/builtin/workbench/ui/composables/useSqlExecution.ts`        | P2    |
| 9   | `useConnectionBinding.ts` | `src/extensions/builtin/workbench/ui/composables/useConnectionBinding.ts`   | P2    |
| 10  | `useDialectSync.ts`       | `src/extensions/builtin/workbench/ui/composables/useDialectSync.ts`         | P2    |

### 修改文件

| #   | 文件                       | 变更内容                                     | Phase |
| --- | -------------------------- | -------------------------------------------- | ----- |
| 1   | `SqlEditorPanel.vue`       | 重构为编排层，~1600行 → ~80行                | P2    |
| 2   | `sql-editor-service.ts`    | 移除 `SqlDialect` 重复定义，导入共享类型     | P1    |
| 3   | `sql-dialect-highlight.ts` | 移除 `SqlDialect` 重复定义，导入共享类型     | P1    |
| 4   | `sql-execution-store.ts`   | 新增 `openInNewTab`、`refreshResult` action  | P3    |
| 5   | `shared/types/index.ts`    | `Connection` 接口新增 `dbType: DatabaseType` | P1    |
| 6   | `sql-dialect-highlight.ts` | 增量高亮支持                                 | P4    |

### 删除内容

| #   | 内容                       | 来源                                            | Phase |
| --- | -------------------------- | ----------------------------------------------- | ----- |
| 1   | 7 个 CustomEvent 监听/发送 | SqlEditorPanel.vue                              | P3    |
| 2   | `SqlDialect` 重复定义      | sql-editor-service.ts, sql-dialect-highlight.ts | P1    |

### 不变的内容

以下模块**不在此次优化范围内**，保持原样：

- `QueryResultPanel.vue` — 结果展示面板
- `sql-snippets.ts` — SQL 模板库
- `sql-history-service.ts` — 执行历史
- `connection-store.ts` / `runtime-connection-store.ts` — 连接管理

---

## 验收标准

### Phase 1 验收

- [ ] `SqlDialect` 全局唯一定义在 `shared/types/sql.ts`
- [ ] `Connection.dbType` 为强类型 `DatabaseType`
- [ ] SqlEditorPanel 中不再有 `(conn as any).dbType`
- [ ] `useEditorPersistence` 可正常读写草稿
- [ ] `pnpm run lint` 通过
- [ ] `pnpm run format` 通过

### Phase 2 验收

- [ ] 5 个新组件均可独立渲染
- [ ] 4 个 composables 均可独立调用
- [ ] SqlEditorPanel.vue 行数 < 100
- [ ] 所有功能与拆分前完全一致：
  - SQL 编辑（语法高亮、补全、验证、格式化）
  - SQL 执行（单/多语句、Explain、DuckDB）
  - 快捷键全部生效
  - 工具栏位置切换正常
  - 连接选择/切换正常
  - 草稿自动保存/恢复正常
- [ ] `pnpm run lint` 通过
- [ ] 无新增 TypeScript 编译错误

### Phase 3 验收

- [ ] `window` 上不再有 `sql-execution-result` / `query-result-new` / `query-result-refresh` 等事件
- [ ] 编辑器执行 SQL 后结果面板正确展示
- [ ] Execute+ 在新标签打开结果
- [ ] 结果面板刷新按钮正常工作
- [ ] 设置面板可打开
- [ ] Pinia DevTools 中可观察完整数据流

### Phase 4 验收

- [ ] 切换连接时无 Monarch tokenizer 完整重建日志
- [ ] 执行中可点击取消按钮终止查询
- [ ] 取消后编辑器状态正确恢复

---

## 风险控制

| 风险                                 | 概率 | 影响 | 缓解措施                               |
| ------------------------------------ | ---- | ---- | -------------------------------------- |
| 拆分后功能回归                       | 中   | 高   | 每个 Phase 完成后执行完整功能冒烟测试  |
| composable 间循环依赖                | 低   | 高   | 保持 composable 单向依赖，编排层组装   |
| Pinia Store 与现有 EventBus 双轨冲突 | 低   | 中   | Phase 3 双通道过渡，逐事件替换         |
| 类型收紧导致大量编译错误             | 中   | 中   | 先定义类型，渐进式替换，不急于全部消除 |
| Monaco Web Worker 销毁异常           | 低   | 低   | 继承现有安全销毁逻辑（空模型替换法）   |

### 回滚策略

所有修改在 Git 分支 `refactor/sql-editor-optimization` 上进行，每个 Phase 完成后提交一个 commit。如遇问题，可直接 revert 到上一 Phase 的 commit。

---

## 附录

### 相关文档

| 文档               | 路径                                 |
| ------------------ | ------------------------------------ |
| SQL 编辑器当前文档 | [SQL-EDITOR.md](./SQL-EDITOR.md)     |
| 前端架构文档       | [ARCHITECTURE.md](./ARCHITECTURE.md) |
| 前端组件规范       | [COMPONENTS.md](./COMPONENTS.md)     |
| 前端文档索引       | [INDEX.md](./INDEX.md)               |
| 项目规则           | `.trae/rules/`                       |

| 指标                    | 优化前         | 优化后                                       |
| ----------------------- | -------------- | -------------------------------------------- |
| SqlEditorPanel.vue 行数 | ~1600          | ~365 (**减少 77%**)                          |
| 文件数                  | 1 个大组件     | 4 组件 + 4 composables + 1 编排层            |
| 全局 CustomEvent        | 7 个           | **0 个**                                     |
| 类型松散 (`as any`)     | 多处           | **归类到统一类型定义**                       |
| 方言高亮重建            | 每次完整重建   | **按方言缓存 + 基础规则复用**                |
| Abort 取消支持          | ❌ 仅 JS 侧    | ✅ **前后端双通道取消**                      |
| 执行路径                | 双路径并行     | **统一 queryService**                        |
| 事务控制 UI             | ❌             | ✅ **TX 指示器 + Commit/Rollback**           |
| 语句解析显示            | ❌             | ✅ **语句数实时展示**                        |
| 死代码                  | 多处残留       | **全部清理**                                 |
| DuckDB 加速             | ❌ coming soon | **✅ ATTACH 自动桥接 + 联邦查询可工作**      |
| 图表可视化              | ❌ 不可用      | **✅ 接入结果面板 4 种图表**                 |
| 多语句批量执行          | ❌ 仅跑第一条  | **✅ 全语句分拆 + 独立标签页**               |
| DuckDB SQL 前缀         | ❌ 用户手动加  | **✅ 自动重写 FROM/JOIN 表名**               |
| EXPLAIN 执行计划        | ❌ 无入口      | **✅ 一键 EXPLAIN + 结果标签页**             |
| Monaco 代码折叠         | ❌ 仅基本缩进  | **✅ SQL 语义折叠（BEGIN/END/CTE/子查询）**  |
| SQL 代码收藏管理        | ❌ 无          | **✅ Star 保存 + 片段面板管理 + 删除**       |
| 查询参数化 :param       | ❌ 无          | **✅ 检测 → 弹窗绑定 → 自动执行**            |
| 表数据 Inline Edit      | ❌ 无          | **✅ 双击编辑 + 自动 UPDATE + Save/Cancel**  |
| SQL 执行错误定位        | ❌ 仅消息提示  | **✅ 解析行列号 → Monaco marker + 自动跳转** |
| Minimap 切换            | ❌ 不可开关    | **✅ 工具栏 Map 按钮 toggle**                |
| Editor Settings 面板    | ❌ 无          | **✅ 字号/缩进/换行/行号 实时调整**          |
| Monaco 右键菜单         | ❌ 仅原生      | **✅ 执行选中 SQL / 复制为 VALUES**          |
| Scratchpad 关闭保存     | ❌ 丢失修改    | **✅ 自动保存脏内容**                        |
| SQL 历史重执行          | ❌ 仅插入      | **✅ 双击直接执行**                          |
| Welcome 快速连接        | ❌ 仅快捷键    | **✅ 点击最近连接一键连接**                  |

### 版本历史

| 版本 | 日期       | 说明                                                                                       |
| ---- | ---------- | ------------------------------------------------------------------------------------------ |
| v2.4 | 2026-05-09 | Phase 14（P6 全局配置集成）全部完成：编辑器 6 项设置持久化到 useAppStore + fontFamily 扩展 |
| v2.3 | 2026-05-08 | Phase 13（P5 高级 UX）全部完成：右键菜单、Scratchpad 自动保存、历史重执行、Welcome 连接    |
| v2.2 | 2026-05-08 | Phase 12（P4 质量打磨）全部完成：重构消除重复、错误位置高亮、Minimap 切换、设置面板        |
| v2.1 | 2026-05-08 | Phase 11（P3 增强）全部完成：代码折叠、收藏管理、参数化查询、内联编辑（已有）              |
| v2.0 | 2026-05-08 | Phase 10（P2 补齐）完成：批量执行、DuckDB SQL 重写、EXPLAIN 执行计划                       |
| v1.9 | 2026-05-08 | Phase 9（P1 补齐）完成：ATTACH 自动桥接、图表视图接入、自动补全修复                        |
| v1.8 | 2026-05-08 | Phase 8（DuckDB 加速桥接）完成：扩展离线包、Tauri 命令、前端入口连通                       |
| v1.5 | 2026-05-08 | Phase 6（前后端对齐 P0+P1）完成，新增事务 UI、语句解析、取消双通道                         |
| v1.6 | 2026-05-08 | 修复 SQL 方言转换：补全 sourceDialect 参数 + i18n 消息 + NModal→card 预设                  |
| v1.7 | 2026-05-08 | Phase 7（P2 体验增强）完成：全局快捷键 + 生成 SQL 升级 + 代码片段面板                      |
| v1.4 | 2026-05-08 | Phase 5（死代码清理 + 质量完善）完成，lint/format 零问题                                   |
| v1.3 | 2026-05-08 | Phase 4（方言高亮增量更新）完成，修复 useDialectSync 导入                                  |
| v1.2 | 2026-05-08 | Phase 3（通信机制重构）+ Phase 4（Abort 取消机制）完成                                     |
| v1.1 | 2026-05-08 | Phase 1（类型+持久化）和 Phase 2（组件拆分+Composable）完成                                |
| v1.0 | 2026-05-08 | 初始计划，待确认                                                                           |
