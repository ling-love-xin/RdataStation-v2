# 结果集模块 — V2 接口契约

> 版本：v2.0
> 创建日期：2026-05-08
> 状态：⏳ 设计稿 (待实施)
> 替代：原 `result-store.ts` 分散类型定义

---

## 📖 目录

- [1. 统一类型系统](#1-统一类型系统)
- [2. Pinia Store: useResultStore](#2-pinia-store-useresultstore)
- [3. Pinia Store: useInsightStore](#3-pinia-store-useinsightstore)
- [4. Composables 接口](#4-composables-接口)
- [5. 子组件 Props/Events](#5-子组件-props--events)
- [6. Tauri Command 接口](#6-tauri-command-接口)
- [7. Rust Service 接口](#7-rust-service-接口)
- [8. 迁移对照](#8-迁移对照)

---

## 1. 统一类型系统

> 定义文件：`src/extensions/builtin/workbench/ui/types/result.ts`
> 规则：本文件是**所有 Result 类型的唯一来源**，其他任何文件不得重复定义。

```typescript
// ==================== 基础枚举 ====================

export type FilterMode = 'quick' | 'sql' | 'duckdb'
export type ViewMode = 'grid' | 'text' | 'record'
export type ExportFormat = 'csv' | 'json' | 'insert' | 'xlsx' | 'parquet' | 'sql'

// ==================== 查询结果 ====================

export interface QueryResult {
  columns: string[]
  rows: unknown[][]
  rowCount: number
  elapsedMs: number
  tempTable?: string
  affectedRows?: number
}

// ==================== 结果标签页 ====================

export interface ResultTab {
  readonly id: string
  title: string
  originalSql: string
  connectionId: string
  duckdbTempTable: string
  columns: string[]
  rows: unknown[][]
  originalRowCount: number
  displayedRowCount: number
  filterMode: FilterMode

  // Quick Filter
  quickFilterExpression: string
  filteredRowCount: number

  // SQL Filter
  sqlFilterExpression: string
  isSqlFilterLoading: boolean

  // DuckDB Analysis
  duckdbSql: string
  isDuckdbLoading: boolean
  isAnalysisActive: boolean

  // Meta
  executionTime: number
  timestamp: string

  // Editing
  dirtyRows: Set<number>
}

// ==================== 列洞察 ====================

export interface ColumnInsight {
  columnName: string
  dataType: string
  stats: ColumnStats
  histogram?: HistogramBucket[]
  sample: unknown[]
  quality: QualityScore
}

export interface ColumnStats {
  count: number
  nullCount: number
  distinctCount: number
  // 数值型
  min?: number
  max?: number
  mean?: number
  median?: number
  stddev?: number
  skewness?: number
  kurtosis?: number
  // 文本型
  minLength?: number
  maxLength?: number
  avgLength?: number
  mostFrequent?: Array<{ value: string; count: number }>
  // 时间型
  minDate?: string
  maxDate?: string
  // 通用
  nullRatio: number
  distinctRatio: number
  emptyStringCount?: number
  zeroCount?: number
}

export interface HistogramBucket {
  min: number
  max: number
  count: number
  label: string
}

export interface QualityScore {
  overall: number
  grade: 'excellent' | 'good' | 'fair' | 'poor'
  dimensions: {
    completeness: number
    uniqueness: number
    typeConsistency: number
    distribution: number
  }
  summary: string
}

// ==================== 表探查 ====================

export interface TableProfile {
  tableName: string
  rowCount: number
  columns: TableColumnMeta[]
}

export interface TableColumnMeta {
  name: string
  dataType: string
  nullable: boolean
  isPrimaryKey: boolean
  defaultValue: string | null
  comment: string
}

// ==================== 右键菜单 Action ====================

export type MenuActionCategory = 'edit' | 'filter' | 'sort' | 'view' | 'export' | 'meta'

export interface MenuAction {
  label: string
  key: string
  category: MenuActionCategory
  icon: string
  divider?: boolean
  shortcut?: string
  visible?: (ctx: MenuContext) => boolean
  handler?: (ctx: MenuContext) => void
}

export interface MenuContext {
  value: unknown
  column: string
  columnIndex: number
  rowIndex: number
  tab: ResultTab
}

// ==================== 导出选项 ====================

export interface ExportOptions {
  format: ExportFormat
  includeHeaders: boolean
  nullAs: string
  delimiter?: string // CSV only
  quoteFields?: boolean // CSV only
  prettyPrint?: boolean // JSON only
  includeSchema?: boolean // SQL only
}

// ==================== 过滤预设 ====================

export interface FilterPreset {
  id: string
  name: string
  filterMode: FilterMode
  expression: string
  createdAt: string
}
```

---

## 2. Pinia Store: useResultStore

> 文件：`src/extensions/builtin/workbench/ui/stores/result-store.ts` (重写)
> 职责：结果标签页的**唯一数据源**，替代 MainContentArea/QueryResultPanel 的本地状态

```typescript
// ==================== Store 接口 ====================

export const useResultStore = defineStore('result', () => {
  // ——— 状态 ———
  const tabs: Ref<ResultTab[]>
  const activeTabId: Ref<string | null>
  const activeTab: ComputedRef<ResultTab | undefined>
  const isAnyLoading: ComputedRef<boolean>

  // ——— 标签管理 ———
  function addTab(sql: string, connectionId: string): ResultTab
  function closeTab(id: string): void
  function switchTab(id: string): void
  function closeAllTabs(): void

  // ——— 结果设置 ———
  function setTabResult(id: string, result: QueryResult): void
  function removeTabResult(id: string): void // 清除结果但保留标签

  // ——— 过滤操作 ———
  function setFilterMode(id: string, mode: FilterMode): void
  function applyQuickFilter(id: string, expression: string): void
  function clearFilter(id: string): void
  function executeSqlFilter(id: string, whereClause: string): Promise<void>
  function executeDuckdbAnalysis(id: string, duckSql: string): Promise<void>
  function bridgeFilterFromDuckdb(id: string): Promise<void>

  // ——— 导出 ———
  function exportTab(id: string, format: ExportFormat, options?: ExportOptions): Promise<void>
  function copyAsInsert(tabId: string): Promise<void>

  // ——— 编辑 ———
  function markCellDirty(tabId: string, rowIndex: number): void
  function resetDirtyCells(tabId: string): void
  function saveEdits(tabId: string): Promise<void>
  function cancelEdits(tabId: string): void

  // ——— 刷新 ———
  function reExecuteTab(id: string): Promise<void>

  // ——— 内部 ———
  function ensureDuckdbTable(id: string): Promise<void>

  return {
    tabs,
    activeTabId,
    activeTab,
    isAnyLoading,
    addTab,
    closeTab,
    switchTab,
    closeAllTabs,
    setTabResult,
    removeTabResult,
    setFilterMode,
    applyQuickFilter,
    clearFilter,
    executeSqlFilter,
    executeDuckdbAnalysis,
    bridgeFilterFromDuckdb,
    exportTab,
    copyAsInsert,
    markCellDirty,
    resetDirtyCells,
    saveEdits,
    cancelEdits,
    reExecuteTab,
  }
})
```

### Store 使用模式

```typescript
// ❌ 旧模式 (分散状态 + 事件总线)
const resultSets = ref<ResultSet[]>([])
window.dispatchEvent(new CustomEvent('query-result-refresh', { detail: { ... } }))

// ✅ 新模式 (单一 store)
const resultStore = useResultStore()
resultStore.addTab(sql, connId)
resultStore.reExecuteTab(tabId)
```

---

## 3. Pinia Store: useInsightStore

> 文件：`src/extensions/builtin/workbench/ui/stores/insight-store.ts` (保持现有 + 扩展)
> 职责：列洞察与表探查的状态管理

```typescript
export const useInsightStore = defineStore('insight', () => {
  // ——— 状态 ———
  const isOpen: Ref<boolean>
  const activeColumn: Ref<string | null>
  const currentInsight: Ref<ColumnInsight | null>
  const isLoading: Ref<boolean>
  const tableProfile: Ref<TableProfile | null>
  const diffColumns: Ref<string[]>
  const diffSummary: Ref<Record<string, string>>

  // ——— 操作 ———
  function loadColumnInsight(tempTable: string, column: string): Promise<void>
  function loadTableInsight(tempTable: string): Promise<TableProfile>
  function loadColumnFromTable(
    tempTable: string,
    columnName: string,
    tableName: string,
    schema: string
  ): Promise<void>
  function loadSnapshotInsight(snapshotId: string): Promise<void>
  function compareColumnVersions(
    tempTable: string,
    column: string,
    olderSnapshotId: string
  ): Promise<void>
  function closeInsight(): void

  // ——— 数据转换 ———
  function histogramForChart(histogram?: HistogramBucket[]): { labels: string[]; data: number[] }

  return {
    isOpen,
    activeColumn,
    currentInsight,
    isLoading,
    tableProfile,
    diffColumns,
    diffSummary,
    loadColumnInsight,
    loadTableInsight,
    loadColumnFromTable,
    loadSnapshotInsight,
    compareColumnVersions,
    closeInsight,
    histogramForChart,
  }
})
```

---

## 4. Composables 接口

### 4.1 useResultTabs

> 文件：`src/extensions/builtin/workbench/ui/composables/useResultTabs.ts`

```typescript
export function useResultTabs() {
  const store: ReturnType<typeof useResultStore>

  const tabs: ComputedRef<ResultTab[]>
  const activeTab: ComputedRef<ResultTab | undefined>
  const activeTabId: ComputedRef<string | null>

  function createTab(sql: string, connectionId: string): ResultTab
  function removeTab(id: string): void
  function activateTab(id: string): void
  function renameTab(id: string, title: string): void

  return { tabs, activeTab, activeTabId, createTab, removeTab, activateTab, renameTab }
}
```

### 4.2 useGridConfig

> 文件：`src/extensions/builtin/workbench/ui/composables/useGridConfig.ts`

```typescript
export function useGridConfig(tab: ComputedRef<ResultTab | undefined>) {
  const columnDefs: ComputedRef<ColDef[]>
  const defaultColDef: ColDef
  const pagination: ComputedRef<boolean>
  const rowData: ComputedRef<Record<string, unknown>[]>

  function resetColumnState(gridApi: GridApi): void
  function saveColumnState(gridApi: GridApi): void
  function restoreColumnState(): Record<string, unknown> | null

  return {
    columnDefs,
    defaultColDef,
    pagination,
    rowData,
    resetColumnState,
    saveColumnState,
    restoreColumnState,
  }
}
```

### 4.3 useFilterModes

> 文件：`src/extensions/builtin/workbench/ui/composables/useFilterModes.ts`

```typescript
export function useFilterModes(tab: ComputedRef<ResultTab | undefined>) {
  const currentMode: ComputedRef<FilterMode>
  const isQuickActive: ComputedRef<boolean>
  const isSqlActive: ComputedRef<boolean>
  const isDuckdbActive: ComputedRef<boolean>

  function setMode(mode: FilterMode): void
  function applyQuickFilter(expression: string): void
  function applySqlFilter(clause: string): Promise<void>
  function applyDuckdbAnalysis(sql: string): Promise<void>
  function clearCurrentFilter(): void

  return {
    currentMode,
    isQuickActive,
    isSqlActive,
    isDuckdbActive,
    setMode,
    applyQuickFilter,
    applySqlFilter,
    applyDuckdbAnalysis,
    clearCurrentFilter,
  }
}
```

### 4.4 useResultExport

> 文件：`src/extensions/builtin/workbench/ui/composables/useResultExport.ts`

```typescript
export function useResultExport(tab: ComputedRef<ResultTab | undefined>) {
  const isExporting: Ref<boolean>

  function exportAs(format: ExportFormat, options?: ExportOptions): Promise<void>
  function copyAsInsert(): Promise<void>
  function copyCellValue(value: unknown): void
  function copyRow(rowIndex: number): void

  return { isExporting, exportAs, copyAsInsert, copyCellValue, copyRow }
}
```

### 4.5 useGridKeyboard

> 文件：`src/extensions/builtin/workbench/ui/composables/useGridKeyboard.ts`

```typescript
export function useGridKeyboard(tab: ComputedRef<ResultTab | undefined>) {
  type KeyboardHandler = (event: KeyboardEvent) => void
  type KeyboardBinding = { key: string; ctrl?: boolean; shift?: boolean; handler: KeyboardHandler }

  const bindings: KeyboardBinding[]

  function registerBinding(binding: KeyboardBinding): void
  function unregisterBinding(key: string): void

  return { bindings, registerBinding, unregisterBinding }
}
```

---

## 5. 子组件 Props / Events

### 5.1 ResultTabsBar.vue

```typescript
// Props
interface Props {
  tabs: ResultTab[]
  activeTabId: string | null
}

// Emits
interface Emits {
  (e: 'select', tabId: string): void
  (e: 'close', tabId: string): void
  (e: 'rename', tabId: string, title: string): void
}
```

### 5.2 ToolbarStrip.vue

```typescript
// Props
interface Props {
  filterMode: FilterMode
  quickFilterExpression: string
  sqlFilterExpression: string
  duckdbSql: string
  isSqlFilterLoading: boolean
  isDuckdbLoading: boolean
  isAnalysisActive: boolean
  rowCount: number
  displayedRowCount: number
  filteredRowCount: number
  executionTime: number
}

// Emits
interface Emits {
  (e: 'update:filterMode', mode: FilterMode): void
  (e: 'applyQuickFilter', expression: string): void
  (e: 'executeSqlFilter', clause: string): void
  (e: 'executeDuckdbAnalysis', sql: string): void
  (e: 'bridgeFilter'): void
  (e: 'clearFilter'): void
}
```

### 5.3 ResultGridView.vue

```typescript
// Props
interface Props {
  columnDefs: ColDef[]
  defaultColDef: ColDef
  rowData: Record<string, unknown>[]
  pagination: boolean
  activeTab: ResultTab
}

// Emits
interface Emits {
  (e: 'contextMenu', ctx: MenuContext): void
  (e: 'cellEdit', tabId: string, rowIndex: number): void
  (e: 'columnHeaderClick', column: string): void
}
```

### 5.4 ResultValueViewer.vue

```typescript
// Props
interface Props {
  rowIndex: number
  column: string
  value: unknown
  dataType: string
}

// Emits
interface Emits {
  (e: 'close'): void
  (e: 'copy'): void
}
```

---

## 6. Tauri Command 接口

> 前端调用方式：通过 `result-analysis.ts` 封装层间接调用，组件不直接 invoke

### 6.1 执行 SQL 过滤

```typescript
// 前端调用
const result = await ResultAnalysis.executeFilteredQuery(opts: SqlFilterOptions): Promise<QueryResult>

interface SqlFilterOptions {
  connectionId: string
  originalSql: string
  whereClause: string    // 不含 WHERE 关键字
  limit?: number
}

// 后端 Tauri Command
#[tauri::command]
async fn result_apply_sql_filter(
    state: State<'_, AppState>,
    connection_id: String,
    original_sql: String,
    where_clause: String,
    limit: Option<u32>,
) -> Result<QueryResult, CoreError>
```

### 6.2 DuckDB 分析

```typescript
// 前端调用
const result = await ResultAnalysis.executeDuckDbAnalysis(opts: DuckDbOptions): Promise<QueryResult>

interface DuckDbOptions {
  connectionId: string
  tempTable: string
  analysisSql: string   // 完整 DuckDB SQL 语句
}
```

### 6.3 DuckDB 桥接过滤

```typescript
// 前端调用
const result = await ResultAnalysis.bridgeDuckDbFilter(opts: DuckDbOptions): Promise<QueryResult>
// 将 DuckDB 再处理结果回写到临时表，再 1:1 映射到前端 rows
```

### 6.4 列洞察

```typescript
// 前端调用
const insight = await ResultAnalysis.getColumnInsight(opts: ColumnInsightOptions): Promise<ColumnInsight>

interface ColumnInsightOptions {
  connectionId: string
  tempTable: string
  column: string
}
```

### 6.5 表探查

```typescript
const profile = await ResultAnalysis.getTableProfile(opts: TableProfileOptions): Promise<TableProfile>

interface TableProfileOptions {
  connectionId: string
  tempTable: string
}
```

---

## 7. Rust Service 接口

> 后端调用链：`Tauri Command` → `ResultService` (门面) → 子 Service

### 7.1 ResultService (门面)

```rust
impl ResultService {
    pub fn new(state: AppState) -> Self;

    pub async fn execute_filtered_query(&self, opts: SqlFilterOptions) -> Result<QueryResult, CoreError>;
    pub async fn execute_duckdb_analysis(&self, opts: DuckDbOptions) -> Result<QueryResult, CoreError>;
    pub async fn create_temp_table(&self, conn_id: &str, rows: Vec<Vec<Value>>, columns: Vec<String>) -> Result<String, CoreError>;
    pub async fn get_column_insight(&self, temp_table: &str, column: &str) -> Result<ColumnInsightFull, CoreError>;
    pub async fn get_table_profile(&self, conn_id: &str, temp_table: &str) -> Result<TableProfile, CoreError>;
    pub async fn save_insight_snapshot(&self, temp_table: &str, column: &str) -> Result<String, CoreError>;
}
```

### 7.2 DuckDbService

```rust
impl DuckDbService {
    pub fn new(max_connections: usize) -> Self;

    pub async fn create_temp_table(
        &self, conn_id: &str, rows: Vec<Vec<Value>>, columns: Vec<String>
    ) -> Result<String, CoreError>;

    pub async fn query(&self, conn_id: &str, sql: &str) -> Result<Vec<RecordBatch>, CoreError>;
    pub async fn drop_temp_table(&self, table_name: &str) -> Result<(), CoreError>;
    pub async fn cleanup_old_tables(&self, max_age_secs: u64) -> usize;
    pub async fn active_table_count(&self) -> usize;
}
```

### 7.3 InsightEngine

```rust
impl InsightEngine {
    pub fn new(duckdb_svc: Arc<DuckDbService>) -> Self;

    pub async fn compute_column_stats(&self, temp_table: &str, column: &str) -> Result<ColumnStats, CoreError>;
    pub async fn compute_histogram(&self, temp_table: &str, column: &str, bins: u32) -> Result<Vec<HistogramBucket>, CoreError>;
    pub async fn get_column_sample(&self, temp_table: &str, column: &str, size: u32) -> Result<Vec<Value>, CoreError>;
}
```

### 7.4 QualityService

```rust
impl QualityService {
    pub fn new(insight_engine: Arc<InsightEngine>) -> Self;

    pub async fn compute_column_quality(
        &self, temp_table: &str, column: &str, data_type: &str
    ) -> Result<QualityScore, CoreError>;

    pub async fn compute_table_quality(
        &self, temp_table: &str, columns: Vec<(String, String)>
    ) -> Result<Vec<QualityScore>, CoreError>;
}
```

---

## 8. 迁移对照

| 旧模式 (当前)                                              | 新模式 (V2)                               |
| ---------------------------------------------------------- | ----------------------------------------- |
| `MainContentArea.resultSets: Ref<ResultSet[]>`             | `useResultStore().tabs`                   |
| `QueryResultPanel.resultTabs: ResultTab[]` (本地 reactive) | `useResultStore().tabs` (共享 store)      |
| `window.dispatchEvent('sql-execution-result', ...)`        | `resultStore.setTabResult(tabId, result)` |
| `window.addEventListener('query-result-refresh', ...)`     | `resultStore.reExecuteTab(tabId)`         |
| `window.dispatchEvent('open-column-insight', ...)`         | `insightStore.loadColumnInsight(...)`     |
| `activeTab.value.rows` → `rowData` computed (每次 map)     | `tab.rows` 在 `addTab` 时一次性转换       |
| `result-store.ts` 中 `FilterMode` 导出                     | `types/result.ts` 中唯一定义              |
| Rust `OnceLock<Arc<Mutex<Connection>>>`                    | `DuckDbPool` with Semaphore               |
| Rust `result_service.rs` 1546 行 mono-service              | 6 个独立 service + 1 个门面               |

---

## 版本历史

| 版本 | 日期       | 说明                           |
| ---- | ---------- | ------------------------------ |
| v2.0 | 2026-05-08 | 基于优化计划的完整接口契约设计 |
