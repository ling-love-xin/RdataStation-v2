# ⛔ 已废弃 — 编辑面板模块设计（原 SQL 编辑器模块）

> **此文档已废弃，不再维护。**
> 原因：编辑器引擎已从 Monaco Editor 迁移至 CodeMirror 6（2026-05-17 完成）。
> 替代文档：[编辑器架构设计文档 v3.0](../editor-architecture-v3.md)
>
> 以下为历史存档内容，仅供参考。

---

# 编辑面板模块完整设计（原 SQL 编辑器模块）（已废弃）

> 版本：v1.13
> 创建日期：2026-05-09
> 最后更新：2026-05-16
> 状态：📐 设计文档
> 关联：[README.md](./README.md) · [optimization-plan.md](./optimization-plan.md)

---

## 📖 目录

- [0. 模块概念：从"SQL 编辑器"到"编辑面板"](#0-模块概念从sql-编辑器到编辑面板)
- [1. 模块定位与目标](#1-模块定位与目标)
- [2. 总体架构](#2-总体架构)
- [3. 组件树与职责](#3-组件树与职责)
- [4. 数据流架构](#4-数据流架构)
- [5. 状态管理矩阵](#5-状态管理矩阵)
- [6. 配置系统设计](#6-配置系统设计)
- [7. 接口契约](#7-接口契约)
- [8. 功能模块设计](#8-功能模块设计)
- [9. 未实现功能技术方案](#9-未实现功能技术方案)
- [10. 文件结构清单](#10-文件结构清单)

---

## 0. 模块概念：从"SQL 编辑器"到"编辑面板"

### 0.1 名称演进

本模块早期命名为**"SQL 编辑器模块"**，因为最初只有 `SqlEditorPanel` 一个编辑器面板。随着功能扩展，模块内出现了第二种编辑器面板 `CodeEditorPanel`，二者共享 Monaco Editor 引擎和 dockview 生命周期，但服务不同场景。因此本模块的准确名称应为**"编辑面板模块（Editor Panel Module）"**。

### 0.2 两种面板的关系

```
编辑面板模块 (Editor Panel Module)
├── 共享基础设施
│   ├── useMonacoEditor         ← 统一的 Monaco 实例管理
│   ├── Monaco Editor 引擎      ← 语法高亮、补全、主题
│   └── dockview-vue 生命周期   ← 面板注册、拖拽、Tab 管理
│
├── SqlEditorPanel              ← SQL 执行变体
│   ├── EditorToolbar           ← 执行/格式化/方言转换工具栏
│   ├── QueryResultPanel        ← 查询结果展示
│   ├── useSqlExecution         ← SQL 执行逻辑
│   ├── useConnectionBinding    ← 数据库连接绑定
│   └── useDialectSync          ← 方言同步
│
└── CodeEditorPanel             ← 文件编辑变体
    ├── EditorStatusbar         ← 状态栏（语言/编码/EOL/缩进）
    ├── EditorSettingsPopup     ← 编辑器设置弹窗
    ├── SaveStatusIndicator     ← 保存状态指示器
    ├── useFileSave             ← 文件保存/自动保存/重试
    └── useEditorSettings       ← 编辑器显示设置管理
```

### 0.3 使用场景对比

| 维度         | SqlEditorPanel                                                                                                                              | CodeEditorPanel                                                                                                                              |
| ------------ | ------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| **定位**     | 数据库 SQL 交互                                                                                                                             | 通用代码/文件编辑                                                                                                                            |
| **核心能力** | SQL 执行、结果展示、方言处理                                                                                                                | 文件保存、多语言编辑、草稿箱联动                                                                                                             |
| **工具栏**   | 执行/取消/格式化/方言转换/收藏                                                                                                              | 无（通过状态栏操作）                                                                                                                         |
| **状态栏**   | 连接选择 + 事务指示 + 光标                                                                                                                  | 语言/编码/EOL/缩进/保存状态                                                                                                                  |
| **注册入口** | [query/extension.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/query/extension.ts) (panel `sql_editor`) | [query/extension.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/query/extension.ts) (panel `code_editor`) |

### 0.4 本次重构要点（2026-05-16）

CodeEditorPanel 经历了一次解耦重构，从 ~1113 行单文件拆分为 5 个职责清晰的子模块：

| 文件                                                                                                                                                            | 行数 | 职责                                                  |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------- | :--: | ----------------------------------------------------- |
| [CodeEditorPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/CodeEditorPanel.vue)         | 325  | 编排层：Monaco 生命周期 + 文件保存 + 子组件拼装       |
| [CodeEditorStatusbar.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/CodeEditorStatusbar.vue) | 479  | 状态栏：下拉菜单 + click-outside + 编码/语言/EOL 切换 |
| [EditorSettingsPopup.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/EditorSettingsPopup.vue) | 314  | 设置弹窗：16 项编辑器配置 UI                          |
| [useEditorSettings.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/composables/useEditorSettings.ts)             | 170  | 设置逻辑：状态管理 + 类型化 handlers                  |
| [useFileSave.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/composables/useFileSave.ts)                         | ~130 | 文件保存：自动保存/手动保存/重试/状态管理             |
| [SaveStatusIndicator.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SaveStatusIndicator.vue) | ~80  | 保存状态图标组件                                      |

单元测试覆盖率：**51 测试用例**（33 useFileSave + 18 SaveStatusIndicator），全部通过。

---

## 1. 模块定位与目标

### 1.1 定位

编辑面板模块是 RdataStation 的核心交互模块，对标 DBeaver / DataGrip 的编辑器体验。模块包含两种编辑器面板，共享 Monaco Editor 引擎和 dockview 生命周期：

- **SqlEditorPanel**：数据库 SQL 交互 — 编写、执行、结果分析闭环
- **CodeEditorPanel**：通用代码/文件编辑 — 多语言支持、文件保存、草稿箱联动

### 1.2 设计目标

```
用户体验目标:
  ├── 零等待感知 ─── 编辑器 < 100ms 启动、SQL 执行实时反馈
  ├── 上下文智能 ─── 连接感知的补全、方言、高亮、错误定位
  ├── 无损操作   ─── 自动保存草稿、历史追溯、参数化复用
  └── 视觉一致   ─── 全局主题 + 编辑器 6 项设置跨会话保持

架构目标:
  ├── 组件轻量 ─── 单文件 < 400 行、职责单一
  ├── 通信规范 ─── Pinia Store + provide/inject、零全局 CustomEvent
  ├── 配置统一 ─── 单一 CONFIG_REGISTRY → useAppStore → tauri-plugin-store
  └── 类型安全 ─── 消除 as any、统一接口契约
```

### 1.3 数据库支持矩阵

| 数据库     | 语法高亮 | 代码补全 | 执行 | 方言转换 | DuckDB 加速 | EXPLAIN |
| ---------- | -------- | -------- | ---- | -------- | ----------- | ------- |
| MySQL      | ✅       | ✅       | ✅   | ✅       | ✅          | ✅      |
| PostgreSQL | ✅       | ✅       | ✅   | ✅       | ✅          | ✅      |
| SQLite     | ✅       | ✅       | ✅   | ✅       | ✅          | ✅      |
| DuckDB     | ✅       | ✅       | ✅   | ✅       | N/A         | ✅      |

---

## 2. 总体架构

### 2.1 分层架构

```
┌──────────────────────────────────────────────────────────────────┐
│                        UI Layer (Vue 3)                          │
│  ┌───────────────┐  ┌──────────────┐  ┌──────────────────────┐  │
│  │ SqlEditorPanel│  │CodeEditor    │  │QueryResult           │  │
│  │ (编排层 ~947L)│  │Panel(~325L)  │  │Panel                 │  │
│  └───────┬───────┘  └──────┬───────┘  └──────────┬───────────┘  │
│          │                 │                      │              │
│  ┌───────┴─────────────────┴──────────────────────┴───────────┐  │
│  │                    Composables Layer                        │  │
│  │  useSqlExecution / useMonacoEditor / useConnectionBinding   │  │
│  │  useDialectSync / useEditorPersistence / useResultTabs     │  │
│  │  useFileSave / useEditorSettings                           │  │
│  └──────────────────────────┬─────────────────────────────────┘  │
├─────────────────────────────┼────────────────────────────────────┤
│                     Pinia Store Layer                             │
│  ┌──────────────────────────┼─────────────────────────────────┐  │
│  │  sql-execution-store  │  result-store  │  useAppStore      │  │
│  │  layout-store         │  workbench-store                   │  │
│  └──────────────────────────┼─────────────────────────────────┘  │
├─────────────────────────────┼────────────────────────────────────┤
│                     Service Layer                                 │
│  ┌──────────────────────────┼─────────────────────────────────┐  │
│  │  sql-editor-service  │  sql-dialect-highlight               │  │
│  │  sql-snippets        │  sql-history-service                 │  │
│  │  result-analysis     │  query.ts (Tauri IPC)                │  │
│  └──────────────────────────┼─────────────────────────────────┘  │
├─────────────────────────────┼────────────────────────────────────┤
│                   Tauri IPC Bridge                                │
│  invoke('execute_sql') / invoke('cancel_sql_query') / ...        │
├─────────────────────────────┼────────────────────────────────────┤
│                    Rust Backend                                   │
│  ConnectionManager → Database trait → MySQL/PostgreSQL/SQLite/   │
│  DuckDB implementations                                           │
└──────────────────────────────────────────────────────────────────┘
```

### 2.2 关键设计决策

| 决策                               | 理由                                                               |
| ---------------------------------- | ------------------------------------------------------------------ |
| 1:n 编辑器↔结果面板（非 n:n）      | 每个编辑器独立持有结果区域，避免多编辑器的结果混乱（DBeaver 风格） |
| Pinia Store 通信（非 CustomEvent） | 显式数据流、DevTools 可调试、无内存泄漏风险                        |
| composable 封装（非 Mixin）        | 类型安全、按需导入、可独立测试                                     |
| tauri-plugin-store 持久化          | 跨会话 JSON 文件持久化，未来可迁移至 Rust SQLite                   |
| 三层配置优先级                     | global-settings.json → project-settings.json → 系统硬编码          |

### 2.3 Rust 后端分层

```
src-tauri/src/
├── commands/           ← Tauri 命令薄层（只调服务层）
│   ├── sql_commands.rs        14 命令: execute_sql / transaction / history / DuckDB
│   └── sql_parser_commands.rs  5 命令: validate / format / transpile / parse / split
│
├── core/
│   ├── services/
│   │   └── sql_service.rs     SQL 执行编排（缓存/超时/取消/历史/截断）
│   ├── models.rs              QueryResult / ArrowBatch（Arrow → JSON）
│   ├── error.rs               CoreError + 6 子错误类型（均实现 Serialize）
│   └── driver/
│       ├── traits.rs          Database trait 抽象（可插拔引擎）
│       └── native/
│           ├── mysql.rs       MySQL（sqlx）
│           ├── postgres.rs    PostgreSQL（sqlx）
│           ├── sqlite.rs      SQLite（rusqlite）
│           └── duckdb.rs      DuckDB（duckdb-rs）
│
└── adapters/
    └── tauri/
        └── command.rs         适配器层（类型定义，待接入命令）
```

> ℹ️ v1.12 已移除 `adapters/tauri/command.rs`（220 行死代码）。所有 DTO 类型已完整迁移至 `commands/` 目录，Tauri Command 直接使用 `commands/` 中定义的类型。

### 2.4 测试覆盖状态（v1.12）

| 层级                 | 测试数 | 覆盖范围                                                                                                                           |
| -------------------- | ------ | ---------------------------------------------------------------------------------------------------------------------------------- |
| Rust 单元测试        | **22** | 空SQL、空白SQL、无连接、指定连接不存在、历史操作、事务 begin/commit/rollback、取消查询、外部数据库注册、结构体构造、Options 默认值 |
| Rust 集成测试        | 0      | ⚠️ 需要真实数据库连接（计划使用 SQLite 内存数据库）                                                                                |
| 前端单元测试         | **7**  | `bindParams` 参数替换、多参数、引号转义、重复名称、空参数、部分匹配安全、正则特殊字符                                              |
| 前端 Composable 测试 | 0      | ⚠️ 需 mock `tauri.invoke`，覆盖 `useSqlExecution.executeQuery/executeBatch/cancelQuery`                                            |
| 前端组件测试         | 0      | ⚠️ 需 `@vue/test-utils`，覆盖 `SqlEditorPanel.vue` 渲染与交互                                                                      |
| 前端 Store 测试      | 0      | ⚠️ 需测试 `result-store.ts` 的 tab 管理、过滤、导出逻辑                                                                            |

> 📋 **测试技术栈**：vitest + happy-dom（前端），`#[tokio::test]`（Rust）。基础设施已就绪。

---

## 3. 组件树与职责

### 3.1 完整组件树

```
WorkbenchView.vue (dockview-vue 容器)
│
├── WorkbenchTitleBar.vue ────────────── 工作台标题栏（Tab 标题 + 菜单）
├── NavigatorPanel.vue ───────────────── 数据库导航树
│
├── SqlEditorPanel.vue ───────────────── 编排层 (SQL 执行变体)
│   ├── EditorToolbar.vue ────────────── 工具栏
│   │   ├── 执行组: Execute / Execute+ / DuckDB / EXPLAIN
│   │   ├── 编辑组: Format / Validate / Transpile
│   │   └── 功能组: History / Star(收藏) / Map(Minimap) / Settings
│   ├── EditorWelcome.vue ────────────── 欢迎页 (空编辑器时)
│   ├── Monaco Editor ────────────────── SQL 编辑区 (通过 useMonacoEditor)
│   ├── TranspileModal.vue ───────────── 方言转换弹窗
│   ├── ParamBindingModal.vue ────────── 参数绑定弹窗
│   ├── Settings Popover ─────────────── 编辑器设置 (NPopover)
│   ├── QueryResultPanel.vue ─────────── 结果展示面板
│   │   ├── MultiTabResults.vue ─────── 多 Tab 结果管理
│   │   ├── OutputPanel.vue ─────────── 输出/消息视图
│   │   ├── DataVisualizationPanel.vue ─ 图表视图 (ECharts)
│   │   ├── ColumnInsightPanel.vue ──── 列洞察
│   │   └── (AG Grid) ───────────────── 数据表格
│   └── EditorStatusbar.vue ──────────── 状态栏 (SqlEditorPanel 的)
│       ├── 光标位置 + 选中信息
│       ├── 执行状态 + 耗时
│       ├── 连接的 NPopselect
│       └── 事务指示器 (TX)
│
└── CodeEditorPanel.vue ──────────────── 编排层 (文件编辑变体, 325行)
    ├── Monaco Editor ────────────────── 通用编辑区 (通过 useMonacoEditor)
    ├── EditorWelcome ────────────────── 欢迎覆盖层 (无文件时)
    └── CodeEditorStatusbar.vue ──────────── 状态栏 (CodeEditorPanel 的)
        ├── SaveStatusIndicator ──────── 保存状态 ✓ ⟳ ● ✗
        ├── 语言下拉选择器 ──────────── 20+ 编程语言
        ├── 编码选择器 ──────────────── UTF-8 / GBK 等 8 种
        ├── EOL 选择器 ──────────────── LF / CRLF
        ├── 编辑模式 ────────────────── INS / OVR 切换
        ├── 缩进选择器 ──────────────── Spaces/Tabs + 大小
        └── EditorSettingsPopup ──────── 编辑器设置弹窗 (16 项)
```

### 3.2 组件职责矩阵

| 组件                            | 行数  | 职责                                                            | 依赖 Composable                                                                                  |
| ------------------------------- | ----- | --------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| **SqlEditorPanel**              | ~947  | 编排所有子组件 + 协调 composables                               | useMonacoEditor, useSqlExecution, useConnectionBinding, useDialectSync, useEditorPersistence     |
| **CodeEditorPanel**             | 325   | Monaco 生命周期 + 文件保存编排 + 子组件拼装                     | useMonacoEditor, useFileSave, useEditorSettings                                                  |
| **EditorToolbar**               | ~200  | 工具栏按钮 + 分组折叠 + 位置切换                                | (纯展示 + emit)                                                                                  |
| **EditorStatusbar** (SqlEditor) | ~180  | 状态信息展示 + 连接选择器 + 事务指示                            | (纯展示 + emit)                                                                                  |
| **CodeEditorStatusbar**         | 479   | 状态栏 + 下拉菜单 + 语言/编码/EOL 切换 + 设置弹窗挂载           | (自管理 click-outside + refs)                                                                    |
| **EditorSettingsPopup**         | 314   | 16 项编辑器显示设置 UI（字号/字体/换行/Minimap 等）             | (纯展示，props: settings + handlers)                                                             |
| **SaveStatusIndicator**         | ~80   | 保存状态图标：idle ✓ / saving ⟳ / saved ✓ / unsaved ● / error ✗ | (纯展示)                                                                                         |
| **EditorWelcome**               | ~80   | 空编辑器欢迎页 + 最近连接                                       | (纯展示 + emit)                                                                                  |
| **QueryResultPanel**            | >2000 | 结果展示 + 三模式过滤 + Inline Edit + 导出                      | useResultTabs, useGridConfig, useGridKeyboard, useFilterModes, useFilterPresets, useResultExport |
| **TranspileModal**              | ~60   | 方言转换弹窗                                                    | (纯展示 + emit)                                                                                  |
| **ParamBindingModal**           | ~100  | 参数绑定表单                                                    | (纯展示 + emit)                                                                                  |
| **SqlHistoryPanel**             | ~300  | 历史列表 + 搜索 + 收藏 + 双击重执行                             | (事件驱动)                                                                                       |
| **SnippetPanel**                | ~200  | 代码片段列表 + 搜索 + 插入/删除                                 | (事件驱动)                                                                                       |
| **SettingsPanel**               | ~500  | 工作台全局设置 (需重构)                                         | 无 (当前用 localStorage)                                                                         |

---

## 4. 数据流架构

### 4.1 执行链路

```
┌──────────┐
│ 用户点击  │ Execute / Ctrl+Enter
│  Execute   │
└─────┬─────┘
      │
      ▼
┌─────────────────────────────────────────────────────────────────┐
│ SqlEditorPanel.handleExecute()                                  │
│   1. getSelectedText() || getEditorValue()                      │
│   2. detectParams(sql) ──→ 有 :param? ──→ ParamBindingModal    │
│   3. ensureConnection(connId)                                   │
│   4. useSqlExecution.executeSql(sql, runtimeConnId)              │
└─────────────────────────┬───────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────────┐
│ useSqlExecution.executeSql()                                    │
│   1. scheduleParse() ──→ 更新状态栏语句数                        │
│   2. queryService.executeSql(sql, connId)                       │
│      └─→ invoke('execute_sql', { sql, conn_id })                │
│          └─→ [Rust] ConnectionManager → Database::query()       │
│              └─→ QueryResult { batches: Vec<RecordBatch> }      │
│   3. storeResult() ──→ resultStore.addTab() + setTabResult()    │
│   4. clearErrorMarkers() ──→ 清除 Monaco 错误标记                │
│   5. (on error) setErrorMarker() ──→ 解析错误位置 → Monaco 标记  │
└─────────────────────────────────────────────────────────────────┘
```

### 4.2 DuckDB 加速链路

```
handleDuckDbExecute()
  → ensureConnection(connId)
  → executeDuckDBAccelerated()
    → rewriteDuckDBSQL(sql, attachName)  // FROM users → FROM ext_MyConn.users
    → appDataDir() → 获取扩展目录
    → queryService.executeDuckDBAccelerated({ sql, connId, dbType, dataDir })
      → invoke('execute_duckdb_accelerated', ...)
        → [Rust] DuckDBEngine
          → init_extensions()
          → ATTACH 'url' AS ext_name (TYPE mysql/postgres/sqlite)
          → execute(user SQL)
          → DETACH IF EXISTS (best-effort)
          → Arrow RecordBatch → JSON
    → storeResult()
```

### 4.3 配置读写链路

```
┌─ 读链路 ─────────────────────────────────────────────────────────┐
│                                                                  │
│  appStore.initialize()                                           │
│    → Store.load('global-settings.json')                          │
│    → zod 逐字段校验 → globalConfig.value                         │
│    → Store.load('project-{id}-settings.json')                    │
│    → zod 逐字段校验 → projectConfig.value                        │
│                                                                  │
│  effectiveEditorSettings (computed)                              │
│    = DEFAULT_EDITOR_SETTINGS                                     │
│    ⋈ globalConfig.editorSettings                                 │
│    ⋈ projectConfig.editorSettings (projectOverridable: true)     │
│                                                                  │
│  SqlEditorPanel.onMounted                                       │
│    → read effectiveEditorSettings → init refs                    │
│    → createEditor() → setFontSize/setTabSize/... (立即应用)       │
│                                                                  │
│  watch(effectiveEditorSettings)  ← 外部配置变更自动同步           │
└──────────────────────────────────────────────────────────────────┘

┌─ 写链路 ─────────────────────────────────────────────────────────┐
│                                                                  │
│  用户调整设置 → applyXxx()                                       │
│    → Monaco updateOptions()  (实时生效)                           │
│    → persistEditorSettings()                                     │
│      → appStore.saveConfig('editorSettings', payload, 'global')  │
│        → Store.set('editorSettings', payload)                    │
│        → Store.save() → global-settings.json                     │
│          { "editorSettings": { "fontSize": 16, ... } }            │
│        → globalConfig.value.editorSettings = payload  (内存同步)  │
│                                                                  │
│  SettingsPanel (全局设置页)                                       │
│    → appStore.saveBatch([...])                                   │
│      → Theme + Language + EditorSettings + DefaultEngine         │
│    → appStore.applyTheme()                                       │
└──────────────────────────────────────────────────────────────────┘
```

---

## 5. 状态管理矩阵

### 5.1 Store 职责分工

| Store                 | 文件                                   | 作用域 | 职责                                             |
| --------------------- | -------------------------------------- | ------ | ------------------------------------------------ |
| **useAppStore**       | `@/stores/useAppStore`                 | 全局   | 主题、语言、编辑器设置、默认引擎、最近项目、布局 |
| **resultStore**       | `workbench/stores/result-store`        | 工作台 | 结果 Tab 管理、分页、三模式过滤                  |
| **sqlExecutionStore** | `workbench/stores/sql-execution-store` | 工作台 | 执行结果分发、新 Tab 请求、刷新请求              |
| **layoutStore**       | `workbench/stores/layout-store`        | 工作台 | 面板布局、分割位置                               |
| **insightStore**      | `workbench/stores/insight-store`       | 工作台 | 列洞察数据                                       |
| **workbenchStore**    | `workbench/stores/workbench-store`     | 工作台 | 面板管理、编辑器状态                             |

### 5.2 通信机制选择

| 场景                  | 机制                                       | 原因                                             |
| --------------------- | ------------------------------------------ | ------------------------------------------------ |
| 执行结果 → 结果面板   | **Pinia Store** (resultStore)              | 生产者/消费者解耦，支持多编辑器                  |
| 编辑器 → 工具栏       | **Props + Emits**                          | 父子组件直接通信                                 |
| 编辑器 → 状态栏       | **Composable 共享 ref**                    | 同一编排层内共享                                 |
| 历史面板 → 编辑器     | **CustomEvent** (`sql-history-re-execute`) | 跨 dockview 面板通信（临时方案，待迁移至 Pinia） |
| 片段面板 → 编辑器     | **CustomEvent** (`insert-snippet`)         | 同上                                             |
| 全局设置 → 所有编辑器 | **Pinia Store** (useAppStore) + watch      | 一对多广播                                       |

### 5.3 待迁移的 CustomEvent

> 当前 2 个残留 CustomEvent 由 Phase 3 双通道过渡时期遗留，建议迁移：

| 事件                     | 当前方式    | 建议迁移至                                                 |
| ------------------------ | ----------- | ---------------------------------------------------------- |
| `sql-history-re-execute` | CustomEvent | `sqlExecutionStore.reExecuteRequest`                       |
| `insert-snippet`         | CustomEvent | provide/inject 或 `sqlExecutionStore.insertSnippetRequest` |

---

## 6. 配置系统设计

### 6.1 CONFIG_REGISTRY 现状与扩展

当前 `config.ts` 定义了 7 个配置键。Workbench SettingsPanel 有 6 个 Tab 的设置全部游离在外。

#### 6.1.1 已有键 (7 个)

```typescript
CONFIG_REGISTRY = {
  theme, // 'light' | 'dark' | 'system'
  language, // 'zh-CN' | 'en'
  editorSettings, // { fontSize, tabSize, wordWrap, minimap, lineNumbers, fontFamily }
  defaultEngine, // 'native' | 'duckdb'
  recentProjects, // string[]
  dockviewLayout, // SerializedDockviewLayout   (projectOnly)
  sidebarState, // SerializedSidebarState     (projectOnly)
}
```

#### 6.1.2 待新增键 (6 个 — 覆盖 Workbench SettingsPanel)

```typescript
// 连接池设置
connectionPool: {
  key: 'connectionPool',
  default: {
    maxConnections: 10,
    minIdleConnections: 2,
    connectionTimeout: 30,        // 秒
    idleTimeout: 300,             // 秒
    autoReconnect: true,
    healthCheck: true,
    healthCheckInterval: 60,      // 秒
  },
  rule: { globalDefault: true, projectOverridable: false, projectOnly: false },
}

// 历史设置
historySettings: {
  key: 'historySettings',
  default: {
    maxHistoryItems: 100,
    retentionDays: 30,
    enableHistory: true,
    includeSQL: true,
    enableUndo: true,
  },
  rule: { globalDefault: true, projectOverridable: false, projectOnly: false },
}

// 监控设置
monitoringSettings: {
  key: 'monitoringSettings',
  default: {
    enableMonitoring: true,
    updateInterval: 5,           // 秒
    enableAlerts: true,
    alertOnDisconnect: true,
    alertOnSlowQuery: true,
    slowQueryThreshold: 1000,    // 毫秒
  },
  rule: { globalDefault: true, projectOverridable: false, projectOnly: false },
}

// 性能设置
performanceSettings: {
  key: 'performanceSettings',
  default: {
    virtualScrollBuffer: 5,
    maxCacheSize: 100,           // MB
    cacheExpireMinutes: 60,
    enableLazyLoad: true,
    enablePreload: true,
  },
  rule: { globalDefault: true, projectOverridable: false, projectOnly: false },
}

// 外观设置 (UI 字体/紧凑模式，不包含编辑器字体和主题)
appearanceSettings: {
  key: 'appearanceSettings',
  default: {
    uiFontSize: 13,              // UI 字体大小 (非编辑器)
    compactMode: false,
  },
  rule: { globalDefault: true, projectOverridable: false, projectOnly: false },
}

// 结果面板设置
resultSettings: {
  key: 'resultSettings',
  default: {
    pageSize: 200,               // 默认分页大小
    defaultViewMode: 'grid',     // 'grid' | 'text' | 'chart'
    nullDisplay: 'NULL',
    dateFormat: 'YYYY-MM-DD HH:mm:ss',
  },
  rule: { globalDefault: true, projectOverridable: true, projectOnly: false },
}
```

#### 6.1.3 GlobalConfig / ProjectConfig 接口扩展

```typescript
interface GlobalConfig {
  theme: Theme
  language: Language
  editorSettings: EditorSettings
  defaultEngine: DefaultEngine
  recentProjects: string[]
  // 新增
  connectionPool: ConnectionPoolSettings
  historySettings: HistorySettings
  monitoringSettings: MonitoringSettings
  performanceSettings: PerformanceSettings
  appearanceSettings: AppearanceSettings
  resultSettings: ResultSettings
}

interface ProjectConfig {
  theme?: Theme
  editorSettings?: Partial<EditorSettings>
  defaultEngine?: DefaultEngine
  dockviewLayout?: SerializedDockviewLayout
  sidebarState?: SerializedSidebarState
  // 新增
  resultSettings?: Partial<ResultSettings>
}
```

### 6.2 两个 SettingsPanel 统一方案

```
现状:
  settings/.../SettingsPanel.vue  ──→ useAppStore.saveConfig()  ✅ 正确
  workbench/.../SettingsPanel.vue ──→ localStorage.setItem()     ❌ 游离

目标:
  ┌─────────────────────────────────────────────────┐
  │  全局设置页 (settings/SettingsPanel.vue)        │
  │  ├── 外观 (主题/语言/编辑器/UI字体/紧凑模式)    │
  │  ├── 编辑器 (字号/缩进/换行/行号/Minimap/字体)  │
  │  ├── 结果面板 (分页大小/默认视图/NULL显示/日期) │
  │  └── 默认引擎                                    │
  └─────────────────────────────────────────────────┘
           ↓ 所有保存通过 useAppStore.saveConfig()

  ┌─────────────────────────────────────────────────┐
  │  工作台设置页 (workbench/SettingsPanel.vue)     │
  │  ├── 连接池 (转入 config.ts CONFIG_REGISTRY)    │
  │  ├── 历史 (转入 config.ts CONFIG_REGISTRY)      │
  │  ├── 监控 (转入 config.ts CONFIG_REGISTRY)      │
  │  ├── 性能 (转入 config.ts CONFIG_REGISTRY)      │
  │  ├── 快捷键 (从 CONFIG_REGISTRY 读取)           │
  │  └── 外观 → 改为只读展示 + "在全局设置中修改"   │
  └─────────────────────────────────────────────────┘
           ↓ 所有保存通过 useAppStore.saveConfig()
```

---

## 7. 接口契约

### 7.1 Tauri 命令接口（19 条后端注册 + 15 条 query.ts 封装）

| 命令                         | 输入类型                   | 输出类型                    | 前端入口                                     |
| ---------------------------- | -------------------------- | --------------------------- | -------------------------------------------- |
| `execute_sql`                | `{ sql, conn_id }`         | `QueryResult`               | ✅ `queryService.executeSql()`               |
| `execute_transaction`        | `{ sqls, conn_id }`        | `QueryResult[]`             | ✅ `queryService.executeTransaction()`       |
| `begin_transaction`          | `{ conn_id }`              | `void`                      | ✅ `queryService.beginTransaction()`         |
| `commit_transaction`         | `{ conn_id }`              | `void`                      | ✅ `queryService.commitTransaction()`        |
| `rollback_transaction`       | `{ conn_id }`              | `void`                      | ✅ `queryService.rollbackTransaction()`      |
| `get_transaction_status`     | `{ conn_id }`              | `TransactionStatus`         | ✅ `queryService.getTransactionStatus()`     |
| `cancel_sql_query`           | `{ conn_id }`              | `void`                      | ✅ `queryService.cancelQuery()`              |
| `execute_duckdb_accelerated` | `DuckDBAcceleratedInput`   | `DuckDBAcceleratedResponse` | ✅ `queryService.executeDuckDBAccelerated()` |
| `get_sql_history`            | `{ conn_id? }`             | `SqlHistoryItem[]`          | ✅ `queryService.getSqlHistory()`            |
| `search_sql_history`         | `{ query }`                | `SqlHistoryItem[]`          | ✅ `queryService.searchSqlHistory()`         |
| `clear_sql_history`          | `{ conn_id? }`             | `void`                      | ✅ `queryService.clearSqlHistory()`          |
| `remove_sql_history`         | `{ id }`                   | `void`                      | ✅ `queryService.removeSqlHistory()`         |
| `register_external_database` | `RegisterExternalDbInput`  | `void`                      | ⚠️ **缺 UI**                                 |
| `create_external_table`      | `CreateExternalTableInput` | `void`                      | ⚠️ **缺 UI**                                 |
| `parse_sql`                  | `{ sql }`                  | `ParseResult`               | ✅ `parseSql()`                              |
| `format_sql`                 | `{ sql, dialect }`         | `FormatResult`              | ✅ `formatSql()`                             |
| `transpile_sql`              | `{ sql, from, to }`        | `TranspileResult`           | ✅ `transpileSql()`                          |
| `validate_sql`               | `{ sql, dialect }`         | `ValidateResult`            | ✅ `validateSql()`                           |
| `split_sql`                  | `{ sql }`                  | `SplitResult`               | ✅ `splitSql()`                              |

> ⚠️ **已知问题**：历史命令（get_sql_history 等 4 条）在 query.ts 有封装但 **SqlHistoryPanel 实际使用 localStorage 实现**（sql-history-service.ts），后端历史命令目前未被 UI 消费。详见附录"前端审计修复历史"。

### 7.2 Composable 接口

```typescript
// useMonacoEditor — Monaco 编辑器生命周期
function useMonacoEditor(options: {
  containerRef: Ref<HTMLElement | undefined>
  panelId: string
  initialValue?: string
  onContentChange?: (value: string) => void
  onCursorChange?: (pos: { lineNumber: number; column: number }) => void
  onSelectionChange?: (info: { lines: number; chars: number } | null) => void
}): {
  editor: Readonly<Ref<monaco.editor.IStandaloneCodeEditor | null>>
  getValue: () => string
  setValue: (value: string) => void
  getSelectedText: () => string
  insertText: (text: string) => void
  focus: () => void
  showWelcome: Ref<boolean>
  cursorPosition: Ref<string>
  selectedTextInfo: Ref<string>
  setFontSize: (size: number) => void
  setTabSize: (size: number) => void
  setWordWrap: (enabled: boolean) => void
  setLineNumbers: (enabled: boolean) => void
  setMinimap: (enabled: boolean) => void
  setFontFamily: (family: string) => void
  registerContextActions: (actions: IActionDescriptor[]) => IDisposable[]
}

// useSqlExecution — SQL 执行逻辑
function useSqlExecution(options: {
  panelId: string
  getEditorValue: () => string
  getSelectedText: () => string
  runtimeConnId: Ref<string>
  currentDatabaseType?: Ref<string | null>
  currentConnectionName?: Ref<string>
}): {
  executing: Ref<boolean>
  lastExecutionTime: Ref<number | null>
  hasResults: Ref<boolean>
  currentResultData: Ref<ResultData | null>
  inTransaction: Ref<boolean>
  statementCount: Ref<number>
  executeSql: (sql: string, connId: string) => Promise<ExecuteResult>
  execute: () => Promise<void>
  executeNew: () => Promise<void>
  executeBatch: () => Promise<void>
  explain: () => Promise<void>
  duckdbExecute: () => Promise<void>
  cancelExecution: () => void
  beginTransaction: () => Promise<void>
  commitTransaction: () => Promise<void>
  rollbackTransaction: () => Promise<void>
  storeResult: (result: ExecuteResult) => void
  scheduleParse: () => void
  checkForParams: (sql: string) => string[] | null
  buildBoundSql: (sql: string, params: Record<string, string>) => string
}

// useConnectionBinding — 连接管理
function useConnectionBinding(options: { panelId: string; initialConnectionId?: string }): {
  selectedConnection: Ref<string>
  runtimeConnId: Ref<string>
  connectionInfoText: Ref<string>
  popselectOptions: Ref<Array<{ label: string; value: string }>>
  isDuckDbConnection: Ref<boolean>
  currentDatabase: Ref<string>
  currentConnectionName: Ref<string>
  databaseType: Ref<string | null>
  ensureConnection: (connId: string) => Promise<boolean>
  onConnectionSelected: (connId: string) => void
}
```

### 7.3 结果面板 Composables

```typescript
// useResultTabs — 结果 Tab 管理
function useResultTabs(panelId: string): {
  tabs: Ref<ResultTab[]>
  activeTabId: Ref<string | null>
  activeTab: ComputedRef<ResultTab | null>
  addTab: (sql: string, connectionId: string) => void
  removeTab: (tabId: string) => void
  setActiveTab: (tabId: string) => void
}

// useResultExport — 结果导出
function useResultExport(activeTab: Ref<ResultTab | null>): {
  exportCSV: () => void
  exportJSON: () => void
  exportInsert: () => void
  exportParquet: () => Promise<void>
  exportXLSX: () => Promise<void>
  copyRowsAsInsert: () => void
}

// useFilterModes — 三模式过滤
function useFilterModes(activeTab: Ref<ResultTab | null>): {
  filterMode: Ref<FilterMode>
  quickFilter: Ref<string>
  sqlFilter: Ref<string>
  duckdbSql: Ref<string>
  applyQuickFilter: () => void
  applySqlFilter: () => Promise<void>
  applyDuckdbFilter: () => Promise<void>
  resetFilter: () => void
}
```

---

## 8. 功能模块设计

### 8.1 SQL 编辑模块

```
Monaco Editor 配置:
  language: 'sql'
  theme: 跟随 isDark (vs-dark / vs)
  fontSize:      14 (可配置, 10-28)
  tabSize:       2  (可配置, 1-8)
  wordWrap:      true (可配置)
  lineNumbers:   true (可配置)
  minimap:       true (可配置)
  fontFamily:    'Cascadia Code', 'Fira Code', 'Consolas', monospace (可配置)
  foldingStrategy: 'auto'
  showFoldingControls: 'always'
  renderWhitespace: 'selection'
  bracketPairColorization: { enabled: true }
  autoClosingBrackets: 'always'
  autoClosingQuotes: 'always'
  suggest: { showKeywords: true, showSnippets: true }

注册提供器 (Disposable 管理):
  ├── CompletionItemProvider  → 数据库 schema 补全 (TTL 5min)
  ├── FoldingRangeProvider    → SQL 语义折叠
  └── ContextActions × 2      → "执行选中 SQL" / "复制为 VALUES"
```

### 8.2 SQL 执行模块

```
执行模式:
  ├── 单语句执行 ──── Ctrl+Enter
  │   └── 检测 :param? → ParamBindingModal → bindParams() → execute
  ├── 新标签执行 ──── Ctrl+Shift+Enter
  │   └── 执行 → resultStore.addTab() → 新建 Tab (不覆盖现有)
  ├── 批量执行 ────── 工具栏 ListChecks 按钮
  │   └── splitSql() → forEach → addTab() → 汇总成功/失败
  ├── EXPLAIN ─────── 工具栏 FileSearch 按钮
  │   └── "EXPLAIN " + sql → execute → 新 Tab "执行计划"
  └── DuckDB 加速 ──── 工具栏 ⚡ 按钮
      └── rewriteDuckDBSQL() → executeDuckDBAccelerated()

性能策略:
  ├── 500ms 防抖解析 → statementCount 展示
  ├── 1000ms 防抖保存 → localStorage 草稿
  └── Abort 双通道取消 → JS AbortController + Rust CancellationToken
```

### 8.3 结果展示模块

```
QueryResultPanel:
  ├── 视图模式切换 ── grid (表格) / chart (图表) / text (输出)
  │   └── grid: AG Grid (虚拟滚动, 排序, 筛选, 分页 200条/页)
  │   └── chart: ECharts (bar/line/pie/scatter)
  │   └── text: 原始文本输出
  ├── 三模式过滤
  │   ├── quick: AG Grid 客户端快速过滤
  │   ├── sql:   WHERE 子句服务器端过滤
  │   └── duckdb: DuckDB 临时表分析
  ├── 导出 (5 种格式)
  │   ├── CSV    (ag-Grid exportDataAsCsv)
  │   ├── JSON   (Blob 下载)
  │   ├── INSERT (手动生成)
  │   ├── Parquet (DuckDB COPY TO)
  │   └── XLSX   (DuckDB COPY TO)
  ├── Inline Edit
  │   └── 双击 → 编辑 → dirty cells → save_cell_update → UPDATE
  └── Column Insight
      └── 列统计 (nullRatio, distinctRatio, min/max/mean/stddev, histogram)
```

---

## 9. 未实现功能技术方案

### 9.1 P0: 统一配置体系

**目标**：Workbench SettingsPanel 接入 `useAppStore`。

**步骤**：

```
1. config.ts 新增 6 个 CONFIG_REGISTRY 条目 (见 6.1.2)
2. GlobalConfig 接口新增 6 个字段
3. zod schema 新增 6 个子 schema
4. useAppStore 新增 6 个 effectiveXxx computed
5. workbench/SettingsPanel.vue:
   - 替换 localStorage 为 useAppStore.saveConfig()
   - 外观 Tab 删除 theme/fontSize (与全局设置重复)
   - 快捷键 Tab: 配置键 shortcutBindings 实现真正可编辑
6. 删除 workbench SettingsPanel 中 100+ 行硬编码 resetSettings
```

### 9.2 P1: DuckDB 外部数据源 UI

**目标**：为 `register_external_database` 和 `create_external_table` 提供前端入口。

**技术方案**：

```
触发方式:
  NavigatorPanel.vue → DuckDB 连接右键菜单
    ├── "注册外部数据库..." → ExternalDbDialog.vue
    └── "创建外部表..."     → CreateExternalTableDialog.vue

ExternalDbDialog.vue:
  ┌────────────────────────────────────┐
  │ 注册外部数据库                      │
  │                                    │
  │ 数据库类型: [MySQL ▼]              │
  │ 连接 URL:   [mysql://...      ]    │
  │ 数据库名:   [mydb              ]    │
  │                                    │
  │         [取消]  [注册]             │
  └────────────────────────────────────┘
    → invoke('register_external_database', {
        db_type: 'mysql',
        url: 'mysql://user:pass@host:3306/db',
        database: 'mydb'
      })

注册成功后:
  → ATTACH 自动执行
  → 外部表出现在导航树 DuckDB 连接下
  → 用户可直接 SELECT * FROM external_db.table
```

### 9.3 P1: 执行计划可视化

**目标**：将 EXPLAIN 文本转为交互式树形图。

**技术方案**：

```
解析层 (sql-editor-service.ts):
  new function: parseExplainResult(dbType, rawText) → ExplainTreeNode[]

  ExplainTreeNode {
    operation: string      // 'Seq Scan' | 'Index Scan' | 'Hash Join' | 'Sort' | ...
    cost: number           // 预估耗时
    rows: number           // 预估行数
    actualTime?: number    // 实际耗时 (EXPLAIN ANALYZE)
    children: ExplainTreeNode[]
  }

渲染层 (ExplainVisualization.vue):
  使用 ECharts tree series:
  - 节点大小 = 相对 cost
  - 节点颜色 = 性能分级 (绿→黄→红)
  - SCAN 节点标记扫描类型 (seq → 红色警告)
  - 悬浮 Tooltip 显示详细信息

触发入口:
  QueryResultPanel → 当 Tab 标题为 "执行计划" 时
    → viewMode 新增 'explain-tree'
    → 点击左侧树状图图标切换
```

### 9.4 P1: 批量导入 SQL 文件

**目标**：支持拖放/选择 `.sql` 文件到编辑器。

**技术方案**：

```
触发方式 (二选一或都支持):
  A. EditorToolbar 新增 "打开 SQL 文件" 按钮
     → Tauri dialog.open({ filters: [{ name: 'SQL', extensions: ['sql'] }] })
  B. 编辑器区域 @drop / @dragover 事件
     → 接受 .sql 文件

流程:
  选择文件 → Tauri fs.readTextFile(path)
    → editor.setValue(content)
    → 可选: 弹出确认框 "是否立即执行?"
      ├── 是 → splitSql() → 逐条 executeBatch()
      └── 否 → 仅加载到编辑器
```

### 9.5 P2: SQL 对比视图

**技术方案**：

```
使用 Monaco Diff Editor:
  monaco.editor.createDiffEditor(container, {
    readOnly: true,
    renderSideBySide: true,      // side-by-side 模式
    originalEditable: false,
  })

触发场景:
  SqlHistoryPanel → 选中两条历史记录 → 右键 "对比 SQL"
    → 打开 DiffEditorPanel (新 dockview Tab)
    → setModel({ original: sql1, modified: sql2 })

组件:
  DiffEditorPanel.vue:
    - 接收 originalSql + modifiedSql props
    - 使用 useMonacoEditor 的 createDiffEditor 变体
    - 左上角标题: "原 SQL (时间戳)" ← → "新 SQL (时间戳)"
```

### 9.6 P2: 编辑器主题微调

**技术方案**：

```
配置扩展 (EditorSettings):
  interface EditorSettings {
    // 现有
    fontSize: number
    tabSize: number
    wordWrap: boolean
    minimap: boolean
    lineNumbers: boolean
    fontFamily: string
    // 新增
    colorTheme: 'vs-dark' | 'vs' | 'hc-black' | 'custom'
    cursorStyle: 'line' | 'block' | 'underline'
    cursorBlinking: 'blink' | 'smooth' | 'phase' | 'expand' | 'solid'
    renderIndentGuides: boolean
    matchBrackets: 'always' | 'near' | 'never'
  }

预设主题 (settings/.../SettingsPanel.vue 新增):
  - Monokai (默认 dark)
  - Solarized Light
  - One Dark Pro
  - GitHub Light
  - 自定义 (颜色拾取器逐 token 配置)

实现:
  预设主题通过 monaco.editor.defineTheme() 注册
  自定义颜色存储到 editorSettings.customColors: Record<string, string>
  编辑器启动时根据 colorTheme 选择对应主题名
```

---

## 10. 文件结构清单

### 10.1 完整文件树

```
src/
├── stores/
│   ├── config.ts                      # CONFIG_REGISTRY (13/13 ✅)
│   └── useAppStore.ts                 # 全局配置 Store (effectiveXxx computed)
│
├── extensions/builtin/workbench/
│   ├── ui/
│   │   ├── components/panels/
│   │   │   ├── SqlEditorPanel.vue     # 编排层 - SQL 执行变体 (~947L)
│   │   │   ├── CodeEditorPanel.vue    # 编排层 - 文件编辑变体 (325L)
│   │   │   ├── EditorStatusbar.vue    # 状态栏 (SqlEditorPanel 的)
│   │   │   ├── CodeEditorStatusbar.vue    # 状态栏 (CodeEditorPanel 的, 479L)
│   │   │   ├── EditorSettingsPopup.vue # 编辑器设置弹窗 (314L)
│   │   │   ├── SaveStatusIndicator.vue # 保存状态图标 (~80L)
│   │   │   ├── EditorToolbar.vue      # 工具栏
│   │   │   ├── EditorWelcome.vue      # 欢迎页
│   │   │   ├── QueryResultPanel.vue   # 结果面板
│   │   │   ├── MultiTabResults.vue    # 多 Tab 结果
│   │   │   ├── OutputPanel.vue        # 输出视图
│   │   │   ├── DataVisualizationPanel.vue  # ECharts 图表
│   │   │   ├── ColumnInsightPanel.vue # 列洞察
│   │   │   ├── TranspileModal.vue     # 方言转换弹窗
│   │   │   ├── ParamBindingModal.vue  # 参数绑定弹窗
│   │   │   ├── SqlHistoryPanel.vue    # 执行历史
│   │   │   ├── SnippetPanel.vue       # 代码片段
│   │   │   ├── SettingsPanel.vue      # 工作台设置 (需重构)
│   │   │   └── WorkbenchTitleBar.vue  # 工作台标题栏
│   │   │
│   │   ├── composables/
│   │   │   ├── useMonacoEditor.ts     # Monaco 生命周期
│   │   │   ├── useSqlExecution.ts     # SQL 执行逻辑
│   │   │   ├── useConnectionBinding.ts # 连接管理
│   │   │   ├── useDialectSync.ts      # 方言同步
│   │   │   ├── useEditorPersistence.ts # 草稿持久化
│   │   │   ├── useResultTabs.ts       # 结果 Tab
│   │   │   ├── useResultExport.ts     # 结果导出
│   │   │   ├── useResultDiff.ts       # 结果对比
│   │   │   ├── useGridConfig.ts       # AG Grid 配置
│   │   │   ├── useGridKeyboard.ts     # Grid 键盘
│   │   │   ├── useFilterModes.ts      # 三模式过滤
│   │   │   ├── useFilterPresets.ts    # 过滤预设
│   │   │   ├── useTitleBar.ts         # 标题栏操作
│   │   │   └── menuActionHandlers.ts  # 右键菜单（工具模块，非 composable）
│   │   │
│   │   ├── stores/
│   │   │   ├── result-store.ts        # 结果 Tab + 过滤
│   │   │   ├── sql-execution-store.ts # 执行结果分发
│   │   │   ├── layout-store.ts        # 布局
│   │   │   ├── insight-store.ts       # 列洞察
│   │   │   └── workbench-store.ts     # 工作台状态
│   │   │
│   │   ├── types/
│   │   │   └── result.ts              # QueryResult / ResultTab / FilterMode / ...
│   │   │
│   │   └── views/
│   │       └── WorkbenchView.vue      # dockview 容器 + 全局快捷键
│   │
│   └── services/
│       ├── sql-editor-service.ts      # 补全/验证/格式化/解析/错误标记/折叠/参数
│       ├── sql-dialect-highlight.ts   # 方言 Monarch tokenizer
│       ├── sql-snippets.ts            # 代码模板库
│       └── sql-history-service.ts     # 执行历史
│
├── extensions/builtin/Query/ui/services/
│   └── query.ts                       # Tauri IPC 封装 (15 命令)
│
├── extensions/builtin/settings/ui/components/
│   └── SettingsPanel.vue              # 全局设置页 (已接入 useAppStore)
│
└── shared/
    ├── types/
    │   ├── index.ts                   # Connection 等共享类型
    │   └── sql.ts                     # SqlDialect / DatabaseType
    └── locales/
        ├── zh-CN.json                 # 中文本地化
        └── en.json                    # 英文本地化
```

### 10.2 待新增文件

| 文件                            | 用途                      | 优先级 |
| ------------------------------- | ------------------------- | ------ |
| `ExternalDbDialog.vue`          | DuckDB 外部数据库注册弹窗 | P1     |
| `CreateExternalTableDialog.vue` | DuckDB 外部表创建弹窗     | P1     |
| `ExplainVisualization.vue`      | 执行计划树形图组件        | P1     |
| `DiffEditorPanel.vue`           | SQL 对比视图              | P2     |

---

## 附录

### 版本历史

| 版本  | 日期       | 说明                                                                                                                                                                                                                                                                                      |
| ----- | ---------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| v1.12 | 2026-05-10 | 死代码清理（adapters/tauri/command.rs 220行移除）、DRY 重构（new_sql_service 工厂函数、From impl for TransactionStatusResponse + SqlHistoryResponse 消除 42 行重复映射）、前端测试 0→7（bindParams 单元测试）、SqlEditorPanel 移除未使用 import clearErrorMarkers、审计评分 87.3（B+/A-） |
| v1.11 | 2026-05-10 | 吞错修复（result-store console.warn + executeBatch firstError）、Rust 测试 2→22（11x）、adapters 架构文档化、前端测试计划标注                                                                                                                                                             |
| v1.10 | 2026-05-10 | P1+P2 清理：移除 dead_code execute_update/value_to_sql（47行）、删除 shared/types/sql.ts 冗余 ExecuteSqlResponse、统一 shared/api ExecuteSqlResponse 字段（truncated/total_rows/error）、修复 row_count → total_rows                                                                      |
| v1.9  | 2026-05-10 | P0+P1 深度修复：Tauri 命令统一返回 CoreError × 19、消除 sql-editor-service any × 8、sql_service 魔法字符串常量化 × 8、useSqlExecution as unknown as 清理 × 4、SqlExecuteResult 冗余字段移除、abortController 遮蔽 Bug 修复                                                                |
| v1.8  | 2026-05-10 | P0 全面修复：消除驱动层 unwrap × 16、查询超时机制、结果行数硬限制、console.log 清理                                                                                                                                                                                                       |
| v1.7  | 2026-05-09 | 前端死代码清理：query.ts 4 函数 + query-store.ts 历史代码                                                                                                                                                                                                                                 |
| v1.6  | 2026-05-09 | CONFIG_REGISTRY 完成 13/13 + SQL 历史双轨制标记 dead functions                                                                                                                                                                                                                            |
| v1.5  | 2026-05-09 | CONFIG_REGISTRY 扩展 4 键 + Workbench SettingsPanel 迁移 useAppStore                                                                                                                                                                                                                      |
| v1.4  | 2026-05-09 | 前端审计修复：删除幽灵命令 get_query_history、文档一致性修正 × 6、记录历史存储双轨制                                                                                                                                                                                                      |
| v1.3  | 2026-05-09 | 遗漏修复：SQLite rows_to_arrow NULL 类型误判、useSqlExecution parseTimer 清理、WorkbenchTitleBar 桩动作                                                                                                                                                                                   |
| v1.2  | 2026-05-09 | 驱动层审计修复：rows_to_arrow NULL 类型误判 × 2、PostgreSQL query_with_params、MySQL affected_rows、\_db 命名                                                                                                                                                                             |
| v1.1  | 2026-05-09 | 审计修复：Rust unwrap × 2、runtimeConnId 类型、handleExplain 重构、QueryResultPanel any × 10                                                                                                                                                                                              |
| v1.0  | 2026-05-09 | 初始设计文档，覆盖完整架构 + 未实现功能方案                                                                                                                                                                                                                                               |

### 审计修复历史 (2026-05-09)

| #        | 修复项                                            | 文件                                                                                                                                                             | 变更                                                            |
| -------- | ------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------- |
| 1        | `task.progress.lock().unwrap()`                   | [cache_warming_commands.rs:499](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/cache_warming_commands.rs#L499)                    | → `lock().map_err(\|e\| ...)?`                                  |
| 2        | `results.into_iter().next().unwrap()`             | [stream_engine.rs:45](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/dbi/engine/stream_engine.rs#L45)                                 | → `.next().ok_or_else(\|\| ...)?`                               |
| 3        | `runtimeConnId: Ref<string \| null>`              | [useConnectionBinding.ts:19](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/composables/useConnectionBinding.ts#L19) | → `Ref<string>('')`                                             |
| 4        | `binding.runtimeConnId as unknown as Ref<string>` | [SqlEditorPanel.vue:377](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SqlEditorPanel.vue#L377)   | → 移除不安全双重转换                                            |
| 5        | `handleExplain()` 手动 invoke                     | [SqlEditorPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SqlEditorPanel.vue)            | → 委托 `executeExplain(t('...'))`                               |
| 6        | 新增 `executeExplain()`                           | [useSqlExecution.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/composables/useSqlExecution.ts)                  | 统一执行链路 + 新建 Tab + 自定义标题                            |
| 7        | 新增 `title?: string` 到 ExecutionResult          | [sql-execution-store.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/stores/sql-execution-store.ts)               | 支持 EXPLAIN Tab 自定义标题                                     |
| 8        | QueryResultPanel `any` × 10                       | [QueryResultPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/QueryResultPanel.vue)        | → AG Grid 事件类型 + PresetSelectEvent + Record<string,unknown> |
| **Lint** | 审计前：**5 errors, 484 warnings**                | 审计后：**0 errors, 469 warnings**                                                                                                                               | -15 warnings                                                    |

### 驱动层修复历史 (2026-05-09)

| #         | 修复项                                                     |             严重度             | 文件                                                                                                                              | 变更                                                         |
| --------- | ---------------------------------------------------------- | :----------------------------: | --------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------ |
| 1         | `postgres_rows_to_arrow()` 首行 NULL → 类型误判 → 数据丢失 |             🔴 P0              | [postgres.rs:L275-L363](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/native/postgres.rs#L275) | 两阶段重构：Phase 1 扫描非 NULL 值检测类型，Phase 2 统一收集 |
| 2         | `mysql_rows_to_arrow()` 首行 NULL → 类型误判 → 数据丢失    |             🔴 P0              | [mysql.rs:L355-L410](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/native/mysql.rs#L355)       | 同上两阶段重构                                               |
| 3         | PostgreSQL 缺少 `query_with_params()`                      |             🟠 P1              | [postgres.rs:L86](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/native/postgres.rs#L86)        | 新增 36 行实现，遵循 MySQL 参数绑定模式                      |
| 4         | MySQL `affected_rows` 空写结果不一致                       |             🟡 P2              | [mysql.rs:L53](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/native/mysql.rs#L53)              | `None` → `if is_read_only { None } else { Some(0) }`         |
| 5         | PostgreSQL `list_tables` `_db` 命名误导                    |             🟡 P2              | [postgres.rs:L198](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/native/postgres.rs#L198)      | `_db: &str` → `db: &str`                                     |
| **Cargo** | 审计前：28 errors (预存)                                   | 审计后：1 error (预存，非驱动) | 驱动层 0 新错误                                                                                                                   |

### 遗漏修复历史 (Round 8, 2026-05-09)

| #         | 修复项                                                                               |                  严重度                  | 文件                                                                                                                                                            | 变更                                                                                       |
| --------- | ------------------------------------------------------------------------------------ | :--------------------------------------: | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| 1         | `sqlite_rows_to_arrow()` 首行 NULL → 类型误判 → 数据丢失（与 postgres/mysql 同根因） |                  🔴 P0                   | [sqlite.rs:L401-L407](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/native/sqlite.rs#L401)                                   | 移除 `Null` 分支中的 `detected_type = Some(DataType::Utf8)`，NULL 值不再错误地覆盖类型检测 |
| 2         | `useSqlExecution` parseTimer 未清理 → 组件卸载后定时器回调访问已销毁 ref             |                  🟡 P2                   | [useSqlExecution.ts:L437-L442](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/composables/useSqlExecution.ts#L437)  | 新增 `cleanup()` 函数，清除 parseTimer                                                     |
| 3         | `SqlEditorPanel` onBeforeUnmount 未调用 composable 清理                              |                  🟡 P2                   | [SqlEditorPanel.vue:L862](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SqlEditorPanel.vue#L862) | onBeforeUnmount 中添加 `cleanupExecution()` 调用                                           |
| 4         | `WorkbenchTitleBar` 6 个桩菜单动作 `console.log(...)` → 生产环境无意义输出           |                  🟡 P2                   | [WorkbenchTitleBar.vue:L274](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/WorkbenchTitleBar.vue#L274)  | 统一替换为 `const notImplemented = (): void => {}`                                         |
| **Lint**  | 审计前：**0 errors, 469 warnings**                                                   |    修复后：**0 errors, 449 warnings**    | -20 warnings                                                                                                                                                    |
| **Cargo** | 审计前：19 errors (预存)                                                             | 修复后：19 errors (全部预存，非本次变更) | 0 新错误                                                                                                                                                        |

### 前端审计修复历史 (Round 9, 2026-05-09)

| #   | 修复项                                                                   | 严重度 | 文件                                                                                                                           | 变更                                                                    |
| --- | ------------------------------------------------------------------------ | :----: | ------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------- |
| 1   | `getQueryHistory()` 调用不存在的 `get_query_history` 命令 → **幽灵命令** | 🔴 P0  | [query.ts:L57](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/query/ui/services/query.ts#L57)   | 删除 `getQueryHistory()` 函数（仅定义、从未导入、调用的后端命令不存在） |
| 2   | `QueryHistoryItem` 接口仅被已删除的 getQueryHistory 使用                 | 🟡 P2  | [query.ts:L225](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/query/ui/services/query.ts#L225) | 删除 `QueryHistoryItem` 接口（其他文件有独立定义）                      |
| 3   | 文档 §10.1 SnippetPanel.vue 重复出现                                     | 🟡 P2  | [DESIGN.md:L865](file:///e:/myapps/tauirapps/RdataStation/rdata-station/docs/frontend/SQL-EDITOR-DESIGN.md#L865)               | 删除重复条目，替换为遗漏的 `WorkbenchTitleBar.vue`                      |
| 4   | 文档路径 `query/ui/services/` 应为 `Query/ui/services/`（大小写）        | 🟡 P2  | [DESIGN.md:L902](file:///e:/myapps/tauirapps/RdataStation/rdata-station/docs/frontend/SQL-EDITOR-DESIGN.md#L902)               | 修正路径大小写                                                          |
| 5   | 文档 §3.1 组件树遗漏 `WorkbenchTitleBar.vue`                             | 🟡 P2  | [DESIGN.md:L117](file:///e:/myapps/tauirapps/RdataStation/rdata-station/docs/frontend/SQL-EDITOR-DESIGN.md#L117)               | 添加 WorkbenchTitleBar.vue 到组件树                                     |
| 6   | `menuActionHandlers.ts` 被归类为 composable 但不遵循 `useXxx` 模式       | 🟡 P2  | [DESIGN.md:L881](file:///e:/myapps/tauirapps/RdataStation/rdata-station/docs/frontend/SQL-EDITOR-DESIGN.md#L881)               | 标注为"工具模块，非 composable"，补充 `useTitleBar.ts`                  |
| 7   | `QueryResultPanel.vue` 行数 ~900 → 实际 >2000                            | 🟡 P2  | [DESIGN.md:L155](file:///e:/myapps/tauirapps/RdataStation/rdata-station/docs/frontend/SQL-EDITOR-DESIGN.md#L155)               | 更新为实际行数 >2000                                                    |
| 8   | §7.1 命令表标注历史存储双轨制问题                                        | 🟠 P1  | [DESIGN.md:L481](file:///e:/myapps/tauirapps/RdataStation/rdata-station/docs/frontend/SQL-EDITOR-DESIGN.md#L481)               | 添加已知问题注释                                                        |

> ⚠️ **未修复的已知问题**:
>
> - **SQL 历史存储分化**: SqlHistoryPanel 使用 localStorage（sql-history-service.ts），后端 Tauri 命令（get_sql_history 等）被 shared/api → query/extension 消费但不含 tags/notes/favorites。query.ts 死代码已清理（Round 12）。
> - **QueryResultPanel.vue**: >2000 行，需拆分重构

### 配置体系修复历史 (Round 10, 2026-05-09)

| #   | 修复项                                                   | 严重度 | 文件                                                                                                                                                | 变更                                                                                                                                                    |
| --- | -------------------------------------------------------- | :----: | --------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | CONFIG_REGISTRY 扩展：`connectionPool` 键                | 🟠 P1  | [config.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/stores/config.ts)                                                            | 新增 `ConnectionPoolSettings` 类型 + zod schema + 注册表条目（7 字段，全局默认）                                                                        |
| 2   | CONFIG_REGISTRY 扩展：`historySettings` 键               | 🟠 P1  | [config.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/stores/config.ts)                                                            | 新增 `HistorySettings` 类型 + zod schema + 注册表条目（5 字段）                                                                                         |
| 3   | CONFIG_REGISTRY 扩展：`monitoringSettings` 键            | 🟠 P1  | [config.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/stores/config.ts)                                                            | 新增 `MonitoringSettings` 类型 + zod schema + 注册表条目（6 字段）                                                                                      |
| 4   | CONFIG_REGISTRY 扩展：`performanceSettings` 键           | 🟠 P1  | [config.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/stores/config.ts)                                                            | 新增 `PerformanceSettings` 类型 + zod schema + 注册表条目（5 字段）                                                                                     |
| 5   | `GlobalConfig` 接口扩展到 9 字段                         | 🟠 P1  | [config.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/stores/config.ts)                                                            | 添加 connectionPool / historySettings / monitoringSettings / performanceSettings                                                                        |
| 6   | `useAppStore` 新增 4 个 effective computed + 4 个 setter | 🟠 P1  | [useAppStore.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/stores/useAppStore.ts)                                                  | `effectiveConnectionPool` / `effectiveHistorySettings` / `effectiveMonitoringSettings` / `effectivePerformanceSettings` + 对应 `setConnectionPool()` 等 |
| 7   | Workbench SettingsPanel localStorage → useAppStore       | 🟠 P1  | [SettingsPanel.vue](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/components/panels/SettingsPanel.vue) | 删除 `loadSettings()` / `localStorage.setItem`，改用 `appStore.setConnectionPool/setHistorySettings/setMonitoringSettings/setPerformanceSettings`       |

| 指标                | 修复前                       | 修复后                     |
| ------------------- | ---------------------------- | -------------------------- |
| **Lint**            | 1 error, 425 warnings (预存) | **0 errors, 421 warnings** |
| **CONFIG_REGISTRY** | 7/13 键                      | **11/13 键**               |

### 配置体系完成历史 (Round 11, 2026-05-09)

| #   | 修复项                                                   | 严重度 | 文件                                                                                                                 | 变更                                                                                               |
| --- | -------------------------------------------------------- | :----: | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| 1   | CONFIG_REGISTRY 扩展：`appearanceSettings` 键            | 🟠 P1  | [config.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/stores/config.ts)                             | 新增 `AppearanceSettings`（uiFontSize + compactMode）+ zod schema + 注册表                         |
| 2   | CONFIG_REGISTRY 扩展：`resultSettings` 键                | 🟠 P1  | [config.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/stores/config.ts)                             | 新增 `ResultSettings`（pageSize + defaultViewMode + nullDisplay + dateFormat）+ projectOverridable |
| 3   | `useAppStore` 新增 2 个 effective computed + 2 个 setter | 🟠 P1  | [useAppStore.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/stores/useAppStore.ts)                   | `effectiveAppearanceSettings` / `effectiveResultSettings`（含 project merge）+ setter              |
| 4   | SQL 历史双轨制标记                                       | 🟠 P1  | [query.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/query/ui/services/query.ts) | 4 个 history 函数添加 `@deprecated` JSDoc，标注未被 UI 消费及后续统一方向                          |

| 指标                | 修复前                 | 修复后                                           |
| ------------------- | ---------------------- | ------------------------------------------------ |
| **Lint**            | 0 errors, 421 warnings | **2 errors, 423 warnings**（均预存，非本次变更） |
| **CONFIG_REGISTRY** | 11/13 键               | **13/13 键 ✅ 完成**                             |

### 前端死代码清理历史 (Round 12, 2026-05-09)

| #   | 修复项                                    | 严重度 | 文件                                                                                                                           | 变更                                                                                                    |
| --- | ----------------------------------------- | :----: | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------- |
| 1   | 删除 `query.ts` 4 个死 history 函数       | 🟡 P2  | [query.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/query/ui/services/query.ts)           | 移除 `getSqlHistory/searchSqlHistory/clearSqlHistory/removeSqlHistory`（仅被 dead query-store.ts 调用） |
| 2   | 删除 `query.ts` `SqlHistoryItem` 接口     | 🟡 P2  | [query.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/query/ui/services/query.ts)           | 移除死接口定义                                                                                          |
| 3   | 删除 `query.ts` `SqlHistoryResponse` 导入 | 🟡 P2  | [query.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/query/ui/services/query.ts)           | 移除无用类型导入                                                                                        |
| 4   | 清理 `query-store.ts` 历史相关代码        | 🟡 P2  | [query-store.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/query/ui/stores/query-store.ts) | 移除 `sqlHistory/historyLoading` refs + 4 历史 actions + SqlHistory 导入                                |

| 指标              | 修复前 | 修复后  |
| ----------------- | ------ | ------- |
| **Lint errors**   | 4      | **0**   |
| **Lint warnings** | 410    | **410** |

> 📝 **SQL 历史现状澄清**:
>
> - `shared/api/index.ts` → `sqlApi.getSqlHistory()` → `tauriInvoke('get_sql_history')` → **存活**（被 `query/extension.ts` 使用）
> - `query.ts` → `queryService.getSqlHistory()` → `invoke('get_sql_history')` → **已删除**（仅被 dead query-store.ts 使用）
> - `sql-history-service.ts` → localStorage → **存活**（被 `SqlHistoryPanel.vue` 使用，含 tags/notes/favorites 丰富字段）
>
> 后端 Tauri 命令（get_sql_history 等 4 条）被 **shared/api 路径**消费，未被 SQL 编辑器 UI 消费。

### 相关文档

| 文档               | 路径                                                                 |
| ------------------ | -------------------------------------------------------------------- |
| SQL 编辑器当前文档 | [SQL-EDITOR.md](./SQL-EDITOR.md)                                     |
| 架构优化计划       | [SQL-EDITOR-OPTIMIZATION-PLAN.md](./SQL-EDITOR-OPTIMIZATION-PLAN.md) |
| 前端架构文档       | [ARCHITECTURE.md](./ARCHITECTURE.md)                                 |
| 前端组件规范       | [COMPONENTS.md](./COMPONENTS.md)                                     |
| 项目规则           | `.trae/rules/`                                                       |

### 综合审计修复历史 (Round 15, 2026-05-10)

**审计评分**：69.2/100 (C+) → 修复后的预估评分见各指标

| #                                | 修复项                                                                                         | 严重度 | 文件                                                                                                                                                                                                                                                             | 变更                                                                                 |
| -------------------------------- | ---------------------------------------------------------------------------------------------- | :----: | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| **P0: 消除驱动层 unwrap() × 16** |
| 1                                | postgres.rs get_table_detail 6x `.map(\|a\| a.value(row_idx)).unwrap_or(...)` → `.map_or(...)` | 🔴 P0  | [postgres.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/native/postgres.rs)                                                                                                                                               | 消除所有 unwrap 字样，使用标准 Option 组合子                                         |
| 2                                | postgres.rs `detected_type.unwrap_or(DataType::Utf8)` 冗余 → `effective_type` 复用             | 🟡 P2  | [postgres.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/native/postgres.rs)                                                                                                                                               | 修复 arrow 转换中重复变量                                                            |
| 3                                | mysql.rs `detected_type.clone().unwrap_or()` → `match` 模式                                    | 🟡 P2  | [mysql.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/native/mysql.rs)                                                                                                                                                     | 消除 unwrap 字样                                                                     |
| 4                                | sqlite.rs 3x `row.get(i).unwrap_or(Value::Null)` → for 循环 + `?` err propagation              | 🔴 P0  | [sqlite.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/native/sqlite.rs)                                                                                                                                                   | query_with_params / query_with_cancel / transaction query 三处行迭代改为严格错误传播 |
| 5                                | sqlite.rs `detected_type.unwrap_or()` → `match` + `effective_type`                             | 🟡 P2  | [sqlite.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/native/sqlite.rs)                                                                                                                                                   | 消除 unwrap 字样                                                                     |
| 6                                | duckdb.rs 3x `row.get(i).unwrap_or(Value::Null)` → for 循环 + `?` err propagation              | 🔴 P0  | [duckdb.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/native/duckdb.rs)                                                                                                                                                   | query / query_with_cancel / transaction query 三处行迭代改为严格错误传播             |
| 7                                | duckdb.rs `detected_type.unwrap_or()` → `match` + `effective_type`                             | 🟡 P2  | [duckdb.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/native/duckdb.rs)                                                                                                                                                   | 消除 unwrap 字样                                                                     |
| **P0: 查询超时机制**             |
| 8                                | sql_service.rs 超时逻辑 `tokio::spawn` 泄漏 → `tokio::time::timeout`                           | 🔴 P0  | [sql_service.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/services/sql_service.rs)                                                                                                                                              | 消除 task 泄漏 + 超时错误信息                                                        |
| 9                                | query.ts `timeout_ms: null` → 透传参数                                                         | 🔴 P0  | [query.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/query/ui/services/query.ts)                                                                                                                                             | `_timeoutMs` 参数从不使用 → 透传到后端                                               |
| 10                               | useSqlExecution.ts 默认超时 30s + 透传到 queryService                                          | 🔴 P0  | [useSqlExecution.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/composables/useSqlExecution.ts)                                                                                                                  | 新增 `DEFAULT_QUERY_TIMEOUT_MS = 30_000`，本地 wrapper 透传                          |
| **P0: 结果行数硬限制**           |
| 11                               | QueryResult 新增 `truncate(max_rows)` 方法                                                     | 🔴 P0  | [models.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/models.rs)                                                                                                                                                                 | RecordBatch 级别精确保留前 N 行                                                      |
| 12                               | sql_service.rs `MAX_QUERY_ROWS = 10_000` + 执行后截断                                          | 🔴 P0  | [sql_service.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/services/sql_service.rs)                                                                                                                                              | 新增 `truncated` 字段 + 常量 + 截断调用                                              |
| 13                               | ExecuteSqlResponse / SqlExecuteResult 新增 `truncated: bool`                                   | 🔴 P0  | [sql_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/sql_commands.rs) / [query-service.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/query/infrastructure/types/query-service.ts) | 跨层传递截断标志                                                                     |
| 14                               | useSqlExecution.ts 截断时 `message.warning` 提示                                               | 🟠 P1  | [useSqlExecution.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/composables/useSqlExecution.ts)                                                                                                                  | 结果被截断时显示黄色警告                                                             |
| **P1: console.log 清理**         |
| 15                               | sql-editor-service.ts 6x `console.warn/error` → 删除                                           | 🟠 P1  | [sql-editor-service.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/services/sql-editor-service.ts)                                                                                                                  | 所有 console 调用删除 + 空白 catch 块添加注释                                        |
| 16                               | useSqlExecution.ts 1x 未使用的 catch(error) → catch                                            | 🟡 P2  | [useSqlExecution.ts](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src/extensions/builtin/workbench/ui/composables/useSqlExecution.ts)                                                                                                                  | 消除 lint warning                                                                    |

| 指标                           | 修复前           | 修复后                   |
| ------------------------------ | ---------------- | ------------------------ |
| **驱动层 unwrap**              | 18 (audit count) | **0**                    |
| **查询超时**                   | 无（forever）    | **30s 默认**             |
| **行数限制**                   | 无限制           | **10,000 行硬限制**      |
| **sql-editor-service console** | 6                | **0**                    |
| **Cargo check**                | 1 error → 0      | **0 errors**             |
| **Lint (新增)**                | -                | **0 errors, 0 warnings** |

> ⚠️ **Deferred（本次未修复的精确定义）**:
>
> - **Tauri commands `Result<T, String>` → `Result<T, CoreError>`**: 涉及 19 个命令签名变更，需一次大规模重构
> - **QueryResultPanel.vue >2000 lines**: 需拆分重构，风险高，单独进行
> - **P1: 剩余 149 any/console.log across 27 workbench files**: 分批处理
