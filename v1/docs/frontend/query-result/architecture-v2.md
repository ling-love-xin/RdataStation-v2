# 结果集模块 — V2 架构设计

> 版本：v2.0
> 创建日期：2026-05-08
> 状态：✅ 已实施 (Phase 10 重构完成)
> 替代：`QUERY-RESULT.md` V3.1 旧架构文档

---

## 📖 目录

- [1. 设计原则](#1-设计原则)
- [2. 组件树](#2-组件树)
- [3. 数据流](#3-数据流)
- [4. 目录结构](#4-目录结构)
- [5. 前后端通信层](#5-前后端通信层)
- [6. DuckDB 池化架构](#6-duckdb-池化架构)
- [7. 状态生命周期](#7-状态生命周期)
- [8. 错误处理策略](#8-错误处理策略)
- [9. 测试分层](#9-测试分层)
- [10. 新旧架构对照](#10-新旧架构对照)

---

## 1. 设计原则

| 原则                               | 说明                                                                      |
| ---------------------------------- | ------------------------------------------------------------------------- |
| **Single Source of Truth**         | `useResultStore` 是结果的唯一数据源，任何组件不得拥有本地 result 状态副本 |
| **Composition over Configuration** | 业务逻辑提取为 composables，组件只负责编排                                |
| **Props down, Actions up**         | 子组件通过 props 接收数据，通过 emits → store actions 触发状态变更        |
| **Facade Pattern**                 | Rust `ResultService` 作为门面，协调各子 service；外部只和门面交互         |
| **Resource RAII**                  | DuckDB 临时表生命周期绑定到 `ResultTab`，tab 关闭 → 表 DROP               |

---

## 2. 组件树

```
App.vue
└── WorkbenchLayout (dockview-vue)
    ├── NavigatorPanel
    │   └── table-column-click → insightStore.loadColumnFromTable()
    │
    └── MainContentArea
        ├── SqlEditorPanel (Monaco Editor)
        │   ├── executeSql()
        │   │   └── await tauri.invoke('execute_sql', { sql })
        │   │   └── resultStore.addTab(sql, connId)
        │   │   └── resultStore.setTabResult(tabId, result)
        │   │
        │   └── Ctrl+R → resultStore.reExecuteTab(tabId)
        │
        └── QueryResultPanel (外壳，~80 行)
            │
            ├── [Composable] useResultTabs()
            ├── [Composable] useFilterModes()
            ├── [Composable] useGridKeyboard()
            │
            ├── ResultTabsBar.vue
            │     Props: tabs, activeTabId
            │     Emits: select, close, rename
            │
            ├── ToolbarStrip.vue
            │     Props: filterMode, expressions, counts, times
            │     Emits: applyQuickFilter, executeSqlFilter,
            │            executeDuckdbAnalysis, bridgeFilter, clearFilter
            │
            ├── ResultGridView.vue         ← v-if viewMode === 'grid'
            │     ├── [Composable] useGridConfig()
            │     ├── [Composable] useResultExport()
            │     ├── AG Grid
            │     └── ResultContextMenu.vue
            │         Emits: action → dispatch by category
            │
            ├── ResultTextView.vue         ← v-if viewMode === 'text'
            │     Props: tab, maxRows?
            │
            ├── ResultRecordView.vue       ← v-if viewMode === 'record'
            │     Props: tab, selectedRowIndex
            │
            ├── ResultValueViewer.vue      ← v-if valueViewer.show
            │     Props: rowIndex, column, value, dataType
            │
            └── ResultStatusBar.vue
                  Props: rowCount, displayedCount, filteredCount, elapsed
```

---

## 3. 数据流

### 3.1 主流程：SQL 执行 → 结果显示

```
用户输入 SQL → Ctrl+Enter
  │
  ▼
SqlEditorPanel.executeSql()
  │
  ├── 0. resultStore.addTab(sql, connId)
  │      └── 创建 ResultTab (id, title, originalSql, ...)
  │      └── tabs.push(tab)
  │      └── activeTabId = tab.id
  │
  ├── 1. result = await tauri.invoke('execute_sql', { connectionId, sql })  ← 现有 command
  │
  ├── 2. resultStore.setTabResult(tab.id, {
  │      columns: result.columns,
  │      rows: result.rows,
  │      rowCount: result.rowCount,
  │      elapsedMs: result.elapsedMs,
  │      tempTable: result.tempTable,
  │    })
  │      └── 更新 tab.columns, tab.rows, tab.originalRowCount, ...
  │
  ├── 3. resultStore.ensureDuckdbTable(tab.id)
  │      └── await tauri.invoke('result_create_temp_table', { ... })
  │      └── 设置 tab.duckdbTempTable
  │
  └── 4. 所有订阅 tabs / activeTabId 的组件自动响应式更新
```

### 3.2 Quick Filter 流程

```
ToolbarStrip → emit('applyQuickFilter', expression)
  │
  ▼
resultStore.applyQuickFilter(tabId, expression)
  │
  ├── 调用 AG Grid API (gridApi.value?.setQuickFilter(expression))
  │
  ├── 更新 tab.quickFilterExpression
  ├── 更新 tab.filteredRowCount ← gridApi.getDisplayedRowCount()
  │
  └── ToolbarStrip 通过 activeTab 响应式自动更新显示
```

### 3.3 SQL Filter 流程

```
ToolbarStrip → emit('executeSqlFilter', clause)
  │
  ▼
resultStore.executeSqlFilter(tabId, clause)
  │
  ├── tab.isSqlFilterLoading = true
  │
  ├── result = await resultAnalysis.executeFilteredQuery({
  │     connectionId: tab.connectionId,
  │     originalSql: tab.originalSql,
  │     whereClause: clause,
  │   })
  │
  ├── tab.columns = result.columns
  ├── tab.rows = result.rows
  ├── tab.displayedRowCount = result.rowCount
  ├── tab.sqlFilterExpression = clause
  ├── tab.isSqlFilterLoading = false
  │
  └── 重新 ensureDuckdbTable(tabId)    ← 可选, 如果分析需要
```

### 3.4 DuckDB Analysis 流程

```
ToolbarStrip → emit('executeDuckdbAnalysis', sql)
  │
  ▼
resultStore.executeDuckdbAnalysis(tabId, sql)
  │
  ├── tab.isDuckdbLoading = true
  │
  ├── result = await resultAnalysis.executeDuckDbAnalysis({
  │     connectionId: tab.connectionId,
  │     tempTable: tab.duckdbTempTable,
  │     analysisSql: sql,
  │   })
  │
  ├── tab.rows = result.rows
  ├── tab.columns = result.columns
  ├── tab.displayedRowCount = result.rowCount
  ├── tab.duckdbSql = sql
  ├── tab.isAnalysisActive = true
  ├── tab.isDuckdbLoading = false
  │
  └── 可选：bridge filter → 将结果写回 temp table
```

### 3.5 列洞察流程

```
ResultGridView → @contextMenu → 右键列头
  → ResultContextMenu emit → dispatch('openInsight', ctx)
  │
  ▼
insightStore.loadColumnInsight(tab.duckdbTempTable, column)
  │
  ├── insight = await resultAnalysis.getColumnInsight({
  │     connectionId: tab.connectionId,
  │     tempTable: tab.duckdbTempTable,
  │     column,
  │   })
  │
  ├── insightStore.currentInsight = insight
  ├── insightStore.isOpen = true
  │
  └── ColumnInsightPanel 通过 watch(insightStore.isOpen) 自动显示
```

### 3.6 数据流向总图

```
┌─────────────────────────────────────────────────────────────┐
│                      Frontend (Vue 3)                        │
│                                                              │
│  SqlEditorPanel                                              │
│    │                                                         │
│    ├── executeSql() ──→ resultStore.addTab()                 │
│    │                    resultStore.setTabResult()            │
│    │                                                         │
│  QueryResultPanel ──→ resultStore (read activeTab)           │
│    │                    ├── tabs[]                            │
│    ├── ToolbarStrip ──→ resultStore.apply*.filter()          │
│    ├── ResultGridView ──→ resultStore (read rowData)         │
│    └── ResultContextMenu ──→ insightStore.open*()            │
│                                resultStore.export*()         │
│                                                              │
│  ColumnInsightPanel ──→ insightStore (read currentInsight)   │
│                                                              │
├─────────────────────────────────────────────────────────────┤
│                    Tauri IPC (invoke)                         │
│                                                              │
│  result-analysis.ts                                          │
│    ├── executeFilteredQuery()   ──→ result_apply_sql_filter  │
│    ├── executeDuckDbAnalysis()  ──→ result_run_duckdb        │
│    ├── getColumnInsight()       ──→ result_column_insight    │
│    └── getTableProfile()        ──→ result_table_profile     │
│                                                              │
├─────────────────────────────────────────────────────────────┤
│                      Backend (Rust)                           │
│                                                              │
│  Tauri Commands (result_commands.rs)                         │
│    │                                                         │
│  ResultService (门面)                                        │
│    ├── FilterService                                         │
│    ├── DuckDbService  ←── DuckDbPool                         │
│    ├── InsightEngine                                         │
│    ├── QualityService                                        │
│    ├── TableProfile                                          │
│    └── InsightPersistence                                    │
│                                                              │
│  DuckDbPool                                                  │
│    ├── :memory: conn #1 ←─ Semaphore acquire/release         │
│    ├── :memory: conn #2                                      │
│    └── :memory: conn #N                                      │
│                                                              │
│  Data Sources (MySQL / PG / SQLite / ...)                    │
└─────────────────────────────────────────────────────────────┘
```

---

## 4. 目录结构

### 目标结构（→ 表示新建或移动）

```
src/extensions/builtin/workbench/ui/
│
├── types/
│   └── result.ts                    → 新建  统一类型定义
│
├── composables/
│   ├── useResultTabs.ts             → 新建  标签页管理
│   ├── useGridConfig.ts             → 新建  AG Grid 配置
│   ├── useFilterModes.ts            → 新建  三模式过滤
│   ├── useResultExport.ts           → 新建  导出逻辑
│   └── useGridKeyboard.ts           → 新建  键盘快捷键
│
├── services/
│   └── result-analysis.ts           保留  调用 Tauri invoke 的封装层
│
├── stores/
│   ├── result-store.ts              重写  Single Source of Truth
│   └── insight-store.ts             保留+扩展
│
└── components/
    ├── MainContentArea.vue           修改  删除本地 resultSets
    │
    └── panels/
        ├── QueryResultPanel.vue      重写  外壳 (~80行)
        ├── ColumnInsightPanel.vue    修改  删除事件监听
        ├── MultiTabResults.vue       可选  (多语句结果)
        │
        └── result-panel/
            ├── ResultTabsBar.vue      → 新建
            ├── ToolbarStrip.vue       → 新建
            ├── ResultGridView.vue     → 新建
            ├── ResultTextView.vue     → 新建
            ├── ResultRecordView.vue   → 新建
            ├── ResultValueViewer.vue  → 新建
            ├── ResultStatusBar.vue    → 新建
            ├── FilterModeSwitcher.vue 修改  删除本地 FilterMode 导出
            ├── QuickFilterInput.vue   保留
            ├── SqlFilterInput.vue     保留
            ├── DuckDBAnalysisInput.vue 保留
            └── ResultContextMenu.vue  修改  使用 MenuAction[] 驱动
```

### 后端目录结构

```
src-tauri/src/
├── commands/
│   └── result_commands.rs           修改  更新 import 路径
│
├── core/
│   ├── command.rs                    修改  注册新 service 在 state 中
│   │
│   └── services/
│       ├── result_service.rs         重写  门面层 (~100行)
│       ├── filter_service.rs          → 新建
│       ├── duckdb_service.rs          → 新建
│       ├── insight_engine.rs          → 新建
│       ├── quality_service.rs         → 新建
│       ├── table_profile.rs           → 新建
│       ├── insight_persistence.rs     → 新建
│       │
│       └── insight/ (不变)
│           ├── mod.rs
│           ├── schema_analyzer.rs
│           ├── rule_engine.rs
│           └── rules/
```

---

## 5. 前后端通信层

`result-analysis.ts` 是**唯一的前端→后端调用入口**，任何组件不直接调用 `tauri.invoke`。

```typescript
// result-analysis.ts — 接口 + 实现

import type { QueryResult, ColumnInsight, TableProfile } from '../types/result'

export interface IResultAnalysis {
  executeFilteredQuery(opts: SqlFilterOptions): Promise<QueryResult>
  executeDuckDbAnalysis(opts: DuckDbOptions): Promise<QueryResult>
  createTempTable(opts: TempTableOptions): Promise<string>
  getColumnInsight(opts: ColumnInsightOptions): Promise<ColumnInsight>
  getTableProfile(opts: TableProfileOptions): Promise<TableProfile>
  saveInsightSnapshot(opts: SnapshotOptions): Promise<string>
}

class ResultAnalysis implements IResultAnalysis {
  // 每个方法 = tauri.invoke 的一次薄封装，无业务逻辑
  async executeFilteredQuery(opts: SqlFilterOptions): Promise<QueryResult> {
    return await invoke('result_apply_sql_filter', { ...opts })
  }
}

export const resultAnalysis: IResultAnalysis = new ResultAnalysis()
```

---

## 6. DuckDB 池化架构

### 当前问题

```rust
// ❌ 全局单例 — 瓶颈
static DUCKDB: OnceLock<Arc<Mutex<Connection>>> = OnceLock::new();
// 所有请求共享一把 Mutex → 串行化
```

### 目标设计

```rust
pub struct DuckDbPool {
    connections: Vec<Arc<Mutex<Connection>>>,
    semaphore: Arc<Semaphore>,
    max_connections: usize,
}

pub struct DuckDbGuard<'a> {
    conn: &'a Arc<Mutex<Connection>>,
    _permit: SemaphorePermit<'a>,
}

impl DuckDbPool {
    pub fn new(max_connections: usize) -> Result<Self, CoreError> {
        let mut connections = Vec::with_capacity(max_connections);
        for _ in 0..max_connections {
            connections.push(Arc::new(Mutex::new(
                Connection::open_in_memory()?
            )));
        }
        Ok(Self {
            connections,
            semaphore: Arc::new(Semaphore::new(max_connections)),
            max_connections,
        })
    }

    pub async fn acquire(&self) -> Result<DuckDbGuard<'_>, CoreError> {
        let permit = self.semaphore.acquire().await?;
        let idx = /* 轮询选择一个 connection */;
        Ok(DuckDbGuard {
            conn: &self.connections[idx],
            _permit: permit,
        })
    }
}
```

### 临时表生命周期

```
ResultTab 创建
  │
  ├── ensureDuckdbTable(tabId)
  │     └── INSERT rows INTO duckdb_temp_{uuid}
  │     └── 注册到 TempTableRegistry:
  │          { name: "duckdb_temp_{uuid}", ownerTabId, createdAt }
  │
  ├── ... 用户分析/洞察 ...
  │
  ├── resultStore.closeTab(tabId)
  │     └── TempTableRegistry.drop("duckdb_temp_{uuid}")
  │
  ├── 后台 LRU: 每 60s 扫描
  │     └── 总数 > 50 → DROP 最早创建的 x 个
  │
  └── 程序退出: tauri::async_runtime::spawn cleanup_all
```

---

## 7. 状态生命周期

### ResultTab 生命周期

```
状态流转：

┌─────────┐    setTabResult()    ┌──────────┐   closeTab()    ┌──────────┐
│ PENDING │ ──────────────────→  │  LOADED  │ ─────────────→  │  CLOSED  │
│ (无数据) │                     │  (有数据) │                 │  (已销毁) │
└─────────┘                     └──────────┘                 └──────────┘
                                     │
                                     ├── quickFilter → tab.rows 不变 (Grid 层过滤)
                                     ├── sqlFilter   → tab.rows 被替换
                                     └── duckdbAnalysis → tab.rows 被替换
```

### ColumnInsight 生命周期

```
┌──────────┐   loadColumnInsight()  ┌──────────┐   closeInsight()  ┌──────────┐
│  CLOSED  │ ────────────────────→  │  OPEN    │ ───────────────→  │  CLOSED  │
│          │                        │ has data │                    │          │
└──────────┘                        └──────────┘                    └──────────┘
```

---

## 8. 错误处理策略

### 前端

```typescript
// Store action 级别统一 try-catch
async function executeSqlFilter(tabId: string, clause: string) {
  const tab = tabs.value.find(t => t.id === tabId)
  if (!tab) return

  tab.isSqlFilterLoading = true
  try {
    const result = await resultAnalysis.executeFilteredQuery({ ... })
    tab.rows = result.rows
    // ...
  } catch (err) {
    const message = parseError(err)  // util/extract useful message
    // Toast 提示 (不阻塞用户)
    // 可选：回退到 quick filter 模式
  } finally {
    tab.isSqlFilterLoading = false
  }
}
```

### 后端

```rust
// ❌ 禁止
let conn = duckdb::Connection::open_in_memory()
    .expect("Failed to create in-memory DuckDB");

// ✅ 必须
let conn = duckdb::Connection::open_in_memory()
    .map_err(|e| CoreError::common(CommonError::General(
        format!("DuckDB initialization failed: {}", e)
    )))?;
```

---

## 9. 测试分层

| 层级        | 测试类型                       | 覆盖范围                    |
| ----------- | ------------------------------ | --------------------------- |
| Unit        | `useResultStore` actions       | 标签CRUD、过滤状态变更      |
| Unit        | `useGridConfig()`              | 列定义生成、分页逻辑        |
| Unit        | `ResultGridView.vue`           | Props 渲染、事件触发        |
| Integration | `QueryResultPanel.vue` + store | 完整用户操作流程            |
| Integration | Rust `DuckDbService`           | 临时表创建/查询/DROP        |
| E2E         | Playwright                     | 真实 SQL 执行 → 过滤 → 洞察 |

---

## 10. 新旧架构对照

| 维度                 | 旧 (V3.1)                      | 新 (V2)                               |
| -------------------- | ------------------------------ | ------------------------------------- |
| **数据源**           | 3 份 Result 副本, 字段名不一致 | 1 份 `useResultStore.tabs[]`          |
| **通信**             | 7 个 CustomEvent, `any` 类型   | Pinia actions, 完整类型推导           |
| **QueryResultPanel** | ~1000 行单文件                 | ~80 行外壳 + 5 composables + 7 子组件 |
| **result_service**   | ~1546 行单文件                 | 1 门面 + 6 子 service                 |
| **DuckDB**           | 全局 `OnceLock<Mutex>`         | `DuckDbPool` + Semaphore              |
| **临时表**           | 创建后永不清理                 | Tab 关闭自动 DROP + LRU 淘汰          |
| **分页**             | Client-side 全量               | 可选 Server-Side Row Model            |
| **类型**             | 大量 `any` + 分散定义          | 0 any + `types/result.ts` 唯一定义    |
| **Rust unwrap**      | 4+ 处                          | 0 处（除测试外）                      |

---

## 版本历史

| 版本 | 日期       | 说明                                                              |
| ---- | ---------- | ----------------------------------------------------------------- |
| v2.0 | 2026-05-08 | 新架构设计 V2，涵盖组件树、数据流、目录结构、DuckDB池化、测试分层 |
