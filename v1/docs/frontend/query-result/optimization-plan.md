# 结果集模块架构优化计划

> 版本：v1.0
> 创建日期：2026-05-08
> 状态：⏳ 待确认
> 依赖：无外部依赖，纯架构优化

---

## 📖 目录

- [优化目标](#优化目标)
- [现状诊断](#现状诊断)
- [优化分组总览](#优化分组总览)
- [A 组：状态管理层统一](#a-组状态管理层统一-p0)
- [B 组：前端巨型组件拆分](#b-组前端巨型组件拆分-p0p1)
- [C 组：Rust 后端巨型 Service 拆分](#c-组rust-后端巨型-service-拆分-p1)
- [D 组：DuckDB 引擎优化](#d-组duckdb-引擎优化-p1)
- [E 组：性能优化](#e-组性能优化-p1p2)
- [F 组：类型安全治理](#f-组类型安全治理-p2p3)
- [G 组：规范合规清理](#g-组规范合规清理-p2p3)
- [H 组：缺失功能补齐](#h-组缺失功能补齐-p3)
- [文件变更清单](#文件变更清单)
- [验收标准](#验收标准)
- [风险控制](#风险控制)
- [相关文档](#相关文档)

---

## 优化目标

1. **状态统一**：用 Pinia Store 替代 3 份数据副本 + 7 个 CustomEvent 事件总线
2. **组件轻量化**：将 1000 行的 `QueryResultPanel.vue` 拆分为 composables + 子组件，单文件不超过 250 行
3. **后端解耦**：将 1546 行的 `result_service.rs` 按职责拆分为 7 个独立 service 模块
4. **类型安全强化**：消除全部 `any` 类型，统一接口定义，单一来源
5. **性能可扩展**：支持后端分页、DuckDB 连接池化、临时表生命周期管理

---

## 现状诊断

### 核心问题：状态管理"三头马车"

同一份查询结果被存储在 3 个互相不知对方的响应式对象中：

| 存储位置                           | 数据结构    | 行数据字段               | 备注                            |
| ---------------------------------- | ----------- | ------------------------ | ------------------------------- |
| `MainContentArea.resultSets`       | `ResultSet` | `data: unknown[]`        | 驱动 `v-if` 面板显示            |
| `QueryResultPanel.resultTabs`      | `ResultTab` | `rows: unknown[][]`      | 标签页 + 过滤状态（实际生产用） |
| `useResultStore` (result-store.ts) | Pinia       | `rowData` / `columnDefs` | **僵尸代码，无任何组件使用**    |

加上 DuckDB 内存临时表中的数据，**同一结果集占用 4 份内存**。

### 通信机制问题：7 个 CustomEvent 满天飞

```
sql-execution-result → query-result-updated / query-result-new → query-result-refresh
                                                                → open-column-insight
                                                                → insight-filter-by-value
                                                                → table-column-click
```

### 巨型文件

| 文件                   | 行数  | 承担职责数                                  |
| ---------------------- | ----- | ------------------------------------------- |
| `QueryResultPanel.vue` | ~1000 | 7（标签/Grid/过滤/菜单/导出/快捷键/视图）   |
| `result_service.rs`    | ~1546 | 7（过滤/DuckDB/洞察/质量/探查/持久化/规则） |

---

## 优化分组总览

| 分组          | 编号  | 项数   | 优先级   | 预估改动量              |
| ------------- | ----- | ------ | -------- | ----------------------- |
| A 状态管理    | A1~A4 | 4      | 🔴 P0    | ~500 行改动             |
| B 前端拆分    | B1~B7 | 7      | 🔴 P0~P1 | ~1200 行改动 + 8 新文件 |
| C 后端拆分    | C1~C7 | 7      | 🟡 P1    | ~800 行改动 + 6 新文件  |
| D DuckDB 优化 | D1~D3 | 3      | 🟡 P1    | ~300 行改动             |
| E 性能优化    | E1~E4 | 4      | 🟡 P1~P2 | ~400 行改动             |
| F 类型安全    | F1~F3 | 3      | 🟢 P2~P3 | ~200 行改动             |
| G 规范合规    | G1~G6 | 6      | 🟢 P2~P3 | ~100 行改动             |
| H 缺失功能    | H1~H5 | 5      | 🟢 P3    | ~800 行新增             |
| **合计**      |       | **39** |          |                         |

---

## A 组：状态管理层统一 (P0)

### A1：统一结果数据源到 Pinia

**问题**：同一查询结果在 `MainContentArea.resultSets` → props → `QueryResultPanel.resultTabs` → `rowData` computed，共 3 份拷贝。

**方案**：将 `ResultTab[]` 多标签管理移入 `useResultStore`，所有组件共享同一个 store 引用。

```typescript
// 新 useResultStore — 唯一真相来源
export const useResultStore = defineStore('result', () => {
  const tabs = ref<ResultTab[]>([])
  const activeTabId = ref<string | null>(null)

  const activeTab = computed(() => tabs.value.find(t => t.id === activeTabId.value))

  function addTab(sql: string, connId: string): ResultTab { ... }
  function closeTab(id: string): void { ... }
  function setTabResult(id: string, data: QueryResult): void { ... }
  function reExecuteTab(id: string): Promise<void> { ... }
  function applyQuickFilter(id: string, expr: string): void { ... }
  function executeSqlFilter(id: string, clause: string): Promise<void> { ... }
  function executeDuckdbAnalysis(id: string, sql: string): Promise<void> { ... }
  function exportTab(id: string, format: ExportFormat): Promise<void> { ... }

  return { tabs, activeTabId, activeTab, addTab, closeTab, setTabResult, ... }
})
```

**涉及文件**：`result-store.ts`（重写）, `MainContentArea.vue`, `QueryResultPanel.vue`

### A2：删除僵尸 store 代码

**问题**：`useResultStore` 现有实现定义了 16 个 ref + 9 个 actions，但无任何组件引用。

**方案**：按 A1 方案完全重写 `result-store.ts`。旧实现的类型定义（`ResultData`, `ExecutionMeta`, `ResultState`）将被新的统一类型体系替代。

**涉及文件**：`result-store.ts`

### A3：统一类型定义

**问题**：3 个文件中各自定义了不兼容的 Result 接口。

| 接口                         | 行数据字段          | 来源           |
| ---------------------------- | ------------------- | -------------- |
| `MainContentArea.ResultSet`  | `data: unknown[]`   | 本地 interface |
| `QueryResultPanel.ResultTab` | `rows: unknown[][]` | 本地 interface |
| `result-store.ResultData`    | `rows: unknown[][]` | Pinia store    |

**方案**：新建 `src/extensions/builtin/workbench/ui/types/result.ts`，定义唯一的 `ResultTab`、`QueryResult`、`FilterMode` 等类型。

```typescript
// types/result.ts — 唯一类型定义文件
export type FilterMode = 'quick' | 'sql' | 'duckdb'
export type ViewMode = 'grid' | 'text' | 'record'

export interface QueryResult {
  columns: string[]
  rows: unknown[][]
  rowCount: number
  elapsedMs: number
  tempTable?: string
  affectedRows?: number
}

export interface ResultTab {
  id: string
  title: string
  originalSql: string
  connectionId: string
  duckdbTempTable: string
  columns: string[]
  rows: unknown[][]
  originalRowCount: number
  displayedRowCount: number
  filterMode: FilterMode
  quickFilterExpression: string
  filteredRowCount: number
  sqlFilterExpression: string
  isSqlFilterLoading: boolean
  duckdbSql: string
  isDuckdbLoading: boolean
  isAnalysisActive: boolean
  executionTime: number
  timestamp: string
  dirtyRows: Set<number>
}
```

**涉及文件**：`types/result.ts`（新建）, `MainContentArea.vue`, `QueryResultPanel.vue`, `result-store.ts`, `FilterModeSwitcher.vue`

### A4：消除 CustomEvent 事件总线

**当前**：7 个 `window.dispatchEvent` 形成隐式依赖链。

**方案**：全部替换为 Pinia actions 调用。

| 原事件                    | 替换为                                                      | 状态                   |
| ------------------------- | ----------------------------------------------------------- | ---------------------- |
| `sql-execution-result`    | `resultStore.setTabResult(tabId, result)`                   | ⏳ 待实施              |
| `query-result-updated`    | 不再需要（共享 store）                                      | ⏳ 待实施              |
| `query-result-new`        | 不再需要（共享 store）                                      | ⏳ 待实施              |
| `query-result-refresh`    | `resultStore.reExecuteTab(tabId)`                           | ⏳ 待实施              |
| `open-column-insight`     | `insightStore.loadColumnInsight(tempTable, column)`         | ⏳ 待实施              |
| `table-column-click`      | `insightStore.loadColumnFromTable(...)`                     | ✅ 已实施 (2026-05-08) |
| `insight-filter-by-value` | `resultStore.applyQuickFilter(tabId, expr)`                 | ⏳ 尚未存在于代码中    |
| `open-schema-insight`     | `insightStore.requestSchemaInsight(...)` → Dockview watcher | ✅ 已实施 (2026-05-08) |
| `open-table-profile`      | `insightStore.requestTableProfile(...)` → Dockview watcher  | ✅ 已实施 (2026-05-08) |

**涉及文件**：`QueryResultPanel.vue`, `MainContentArea.vue`, `ColumnInsightPanel.vue`, `SqlEditorPanel.vue`

---

## B 组：前端巨型组件拆分 (P0~P1)

### B1：拆分 QueryResultPanel.vue

**当前**：~1000 行，承担 7 种职责。

**目标**：拆为外壳组件（~80 行）+ composables + 子组件。

```
QueryResultPanel.vue (外壳，~80 行)
├── useResultTabs()           — 标签页 CRUD
├── useFilterModes()          — 三种过滤模式执行逻辑
├── useGridKeyboard()         — 键盘快捷键
├── ResultTabsBar.vue         — 标签切换栏
├── ToolbarStrip.vue          — 模式切换 + 过滤输入
├── ResultGridView.vue        — AG Grid + 空状态
├── ResultTextView.vue        — 文本视图
├── ResultRecordView.vue      — 记录视图
├── ResultValueViewer.vue     — 值查看器
├── ResultStatusBar.vue       — 底部状态栏
└── ResultContextMenu.vue     — 右键菜单（已存在）
```

**涉及文件**：`QueryResultPanel.vue` + 新建 composables 和子组件

### B2~B7：子项明细

| 编号 | 子项                                                  | 行数预估 | 类型    |
| ---- | ----------------------------------------------------- | -------- | ------- |
| B2   | `useResultTabs()` composable                          | ~80      | Comp    |
| B3   | `useGridConfig()` composable                          | ~120     | Comp    |
| B4   | `useFilterModes()` composable                         | ~150     | Comp    |
| B5   | `useResultExport()` composable                        | ~100     | Comp    |
| B6   | 视图子组件 (GridView/TextView/RecordView/ValueViewer) | ~400     | 4 × Vue |
| B7   | `menuActions` 菜单逻辑提取为 handler map              | ~100     | Util    |

---

## C 组：Rust 后端巨型 Service 拆分 (P1)

### 当前结构

```
result_service.rs (~1546 行)
├── SQL 过滤（re_execute_with_filter）
├── DuckDB 管理（连接/临时表/查询/类型推断/Value 转换）
├── 洞察统计（数值/文本/时间/布尔 - get_column_stats_internal ~ compute_*）
├── 直方图 + 样本
├── 质量评分（compute_column_quality / compute_table_quality）
├── 表探查（fetch_table_columns / fetch_row_count）
├── 规则引擎公开 API
├── 持久化（save/load/cleanup/version）
└── 工具函数（extract_rows_from_serialized、infer_type、json_to_duckdb_value、sha256_hex）
```

### 目标结构

```
services/
├── filter_service.rs          — re_execute_with_filter
├── duckdb_service.rs          — DuckDB 连接池、临时表 CRUD、查询、类型转换
├── insight_engine.rs          — 统计计算、直方图、样本
├── quality_service.rs         — 质量评分、批量评估
├── table_profile.rs           — 表探查、information_schema 查询
├── insight_persistence.rs     — 持久化/版本化/清理/checksum
├── result_service.rs (缩减)   — 门面层，协调各子 service
└── insight/ (不变)            — 规则引擎、schema analyzer 保持现有
```

### C1~C7：子项明细

| 编号 | 新模块                 | 预估行数 | 从 result_service.rs 迁出内容                                                                                |
| ---- | ---------------------- | -------- | ------------------------------------------------------------------------------------------------------------ |
| C1   | 重构 result_service.rs | ~100     | 仅保留门面方法和 ResultSet/ColumnInsightFull 等数据结构定义                                                  |
| C2   | filter_service.rs      | ~80      | re_execute_with_filter                                                                                       |
| C3   | duckdb_service.rs      | ~250     | get_or_create_duckdb、create_temp_table、query_duckdb、类型转换函数                                          |
| C4   | insight_engine.rs      | ~300     | get*column_stats_internal、4 种 compute*\*\_stats、get_column_sample_internal、get_column_histogram_internal |
| C5   | quality_service.rs     | ~120     | compute_column_quality、compute_table_quality                                                                |
| C6   | table_profile.rs       | ~150     | get_table_profile、fetch_table_columns、fetch_row_count                                                      |
| C7   | insight_persistence.rs | ~180     | save/load/cleanup/version API                                                                                |

---

## D 组：DuckDB 引擎优化 (P1)

### D1：DuckDB 连接池化

**当前**：全局 `OnceLock<Arc<Mutex<Connection>>>`，所有请求串行化。

**方案**：改为连接池（N 个 `:memory:` 连接 + `Semaphore` 控制并发）。

```rust
pub struct DuckDbPool {
    connections: Vec<Arc<Mutex<Connection>>>,
    semaphore: Arc<Semaphore>,
    max_connections: usize,
}

impl DuckDbPool {
    pub fn new(max: usize) -> Self { ... }
    pub async fn acquire(&self) -> Result<DuckDbGuard, CoreError> { ... }
}
```

### D2：临时表生命周期管理

**方案**：

- 每个临时表关联 `created_at` 和 `owner_tab_id`
- Tab 关闭时（`closeTab` action）自动调用 `DROP TABLE`
- 全局 LRU 淘汰：超过 50 个临时表时，淘汰最久未使用的
- 程序退出时清理所有临时表

### D3：会话隔离（后续预留）

为每个 `connectionId` 创建独立 DuckDB 实例，互不干扰。Phase 1 暂不实施，先做架构预留。

---

## E 组：性能优化 (P1~P2)

| 编号 | 优化项                 | 当前问题                                     | 方案                                                                         |
| ---- | ---------------------- | -------------------------------------------- | ---------------------------------------------------------------------------- |
| E1   | 后端分页               | 全量推到前端，≥5 万行直接渲染 → 卡死         | Tauri command 增加 `page`/`pageSize`；AG Grid 可选切换 Server-Side Row Model |
| E2   | rowData 预转换         | 每次 activeTab 变化遍历 rows 做 Array→Object | 在 addTab 时一次性转换，存储为 `Record<string, unknown>[]`                   |
| E3   | textViewContent 懒计算 | computed 每次拼接全部行列                    | 仅切换到 text 视图时才计算；大数据截断前 10000 行                            |
| E4   | columnDefs 缓存        | 每次渲染重新创建 columns.map()               | 在 tab.columns 变化时才重新计算                                              |

---

## F 组：类型安全治理 (P2~P3)

| 编号 | 优化项                        | 影响文件                                                                                  |
| ---- | ----------------------------- | ----------------------------------------------------------------------------------------- |
| F1   | 消除全部 `any` 类型           | `MainContentArea.vue`, `QueryResultPanel.vue`, `result-analysis.ts`                       |
| F2   | 统一 `FilterMode` 类型        | 收敛到 `types/result.ts`，删除 `result-store.ts` 和 `FilterModeSwitcher.vue` 中的重复定义 |
| F3   | gridApi 使用 AG Grid 官方类型 | `import type { GridApi } from '@ag-grid-community/core'`                                  |

---

## G 组：规范合规清理 (P2~P3)

| 编号 | 违规代码                                      | 文件:行号                |
| ---- | --------------------------------------------- | ------------------------ |
| G1   | `unwrap_or_default()`                         | `result_service.rs:L229` |
| G2   | `expect("Failed to create in-memory DuckDB")` | `result_service.rs:L689` |
| G3   | `unwrap_or_else(\|_\| "VARCHAR".to_string())` | `result_service.rs:L344` |
| G4   | 多处 `.unwrap_or(0)`, `.unwrap_or(false)`     | 散布各函数               |
| G5   | 前端 `any` 类型                               | 同 F1                    |
| G6   | ESLint/Prettier 确认通过                      | 全量前端文件             |

---

## H 组：缺失功能补齐 (P3)

| 编号 | 功能                | 说明                                                               |
| ---- | ------------------- | ------------------------------------------------------------------ |
| H1   | 单元格编辑持久化    | `handleSave` 当前只 `clear()` dirty set → 应通过 UPDATE 写回数据库 |
| H2   | 导出格式扩展        | 补充 Excel (xlsx)、Parquet、SQL Dump                               |
| H3   | 列配置持久化        | 刷新后 AG Grid 列宽/顺序/冻结配置丢失 → 保存到 localStorage        |
| H4   | 过滤预设            | 保存常用过滤条件为 preset，一键应用                                |
| H5   | 多结果集对比 (diff) | 选择两个 tab → 渲染差异视图（列差异 + 值差异高亮）                 |

---

## 文件变更清单

### 新建文件

```
src/extensions/builtin/workbench/ui/
├── types/
│   └── result.ts                           # 统一类型定义
├── composables/
│   ├── useResultTabs.ts                    # 标签管理
│   ├── useGridConfig.ts                    # AG Grid 配置
│   ├── useFilterModes.ts                   # 三模式过滤逻辑
│   ├── useResultExport.ts                  # 导出逻辑
│   └── useGridKeyboard.ts                  # 键盘快捷键
└── components/panels/result-panel/
    ├── ResultTabsBar.vue                   # 标签切换栏
    ├── ToolbarStrip.vue                    # 模式 + 过滤输入顶条
    ├── ResultGridView.vue                  # Grid 视图
    ├── ResultTextView.vue                  # Text 视图
    ├── ResultRecordView.vue                # Record 视图
    └── ResultValueViewer.vue               # 值查看器

src-tauri/src/core/services/
├── filter_service.rs                       # SQL 过滤
├── duckdb_service.rs                       # DuckDB 管理
├── insight_engine.rs                       # 洞察计算
├── quality_service.rs                      # 质量评分
├── table_profile.rs                        # 表探查
└── insight_persistence.rs                  # 持久化

docs/frontend/
├── QUERY-RESULT-OPTIMIZATION-PLAN.md      # 本文档
├── QUERY-RESULT-OPTIMIZATION-PROGRESS.md   # 进度追踪
├── QUERY-RESULT-API-V2.md                  # 接口契约 V2
└── QUERY-RESULT-ARCHITECTURE-V2.md         # 架构设计 V2
```

### 修改文件

| 文件                            | 改动类型                                     |
| ------------------------------- | -------------------------------------------- |
| `result-store.ts`               | **重写** — 实现真正的 Single Source of Truth |
| `MainContentArea.vue`           | 删除本地 `resultSets`，改为从 store 读取     |
| `QueryResultPanel.vue`          | **重写** — 拆为外壳 + composables + 子组件   |
| `FilterModeSwitcher.vue`        | 删除本地 `FilterMode` 导出，改为 import      |
| `ColumnInsightPanel.vue`        | 删除事件监听，改为 watch store               |
| `SqlEditorPanel.vue`            | 删除事件 dispatch，改为 store action         |
| `result_service.rs`             | 缩减为门面层，迁出 6 个子模块                |
| `result_commands.rs`            | 更新 import 路径                             |
| `result-analysis.ts`            | 统一类型引用                                 |
| `lib.rs` (Rust)                 | 注册新 service 模块                          |
| `lib.rs` (Tauri commands)       | 更新 command 注册                            |
| `docs/frontend/INDEX.md`        | 增加新文档索引                               |
| `docs/README.md`                | 增加新文档索引                               |
| `docs/frontend/QUERY-RESULT.md` | 添加重构声明                                 |

### 删除文件（归档）

| 文件                    | 原因             |
| ----------------------- | ---------------- |
| `result-store.ts`（旧） | 被新实现完全替代 |

---

## 验收标准

### A 组验收

- [ ] `MainContentArea.vue` 中无本地 `resultSets` ref
- [ ] `QueryResultPanel.vue` 中无本地 `resultTabs` ref
- [ ] `window.dispatchEvent` 调用归零（result 相关事件）
- [ ] `useResultStore` 是唯一的 result 数据读写入口

### B 组验收

- [ ] `QueryResultPanel.vue` 不超过 250 行
- [ ] 每个 composable 可独立 import 使用
- [ ] 每个子组件 props 类型完整，无 `any`

### C 组验收

- [ ] `result_service.rs` 不超过 200 行
- [ ] 每个子 service 模块独立编译通过
- [ ] 所有原有 Tauri command 功能不变

### D 组验收

- [ ] DuckDB 并发分析不互相阻塞
- [ ] Tab 关闭时对应临时表自动 DROP
- [ ] 临时表数量不超过配置上限

### E 组验收

- [ ] 10 万行结果渲染不卡顿（后端分页模式）
- [ ] Text 视图不再卡顿（惰性计算）

### F 组验收

- [ ] `pnpm run lint` 无错误
- [ ] 前端 `any` 类型归零（严格执行）

### G 组验收

- [ ] `cargo clippy -- -D warnings` 通过
- [ ] `cargo fmt` 通过
- [ ] Rust 代码中 `unwrap()` / `expect()` 归零（除测试代码外）

---

## 风险控制

| 风险                                 | 等级  | 缓解措施                                                |
| ------------------------------------ | ----- | ------------------------------------------------------- |
| A 组改动影响面大（跨 4 个组件）      | 🟡 中 | A1~A4 在一个 PR 中完成，确保原子性；先写测试再改代码    |
| B 组拆分可能引入新 bug               | 🟡 中 | 保持原有 AG Grid 配置不变，只做组织结构调整             |
| C 组 Rust 模块重组可能影响编译       | 🟢 低 | 纯文件拆分，逻辑不变；`cargo check` 逐模块验证          |
| D1 连接池可能引入并发问题            | 🟡 中 | DuckDB `:memory:` 模式本身线程安全；先单连接验证再扩展  |
| E1 后端分页需改动 Tauri command 签名 | 🔴 高 | 新增 `page`/`pageSize` 可选参数，向后兼容；前端渐进切换 |

---

## 相关文档

| 文档               | 路径                                                                             | 说明                            |
| ------------------ | -------------------------------------------------------------------------------- | ------------------------------- |
| 优化进度追踪       | [QUERY-RESULT-OPTIMIZATION-PROGRESS.md](./QUERY-RESULT-OPTIMIZATION-PROGRESS.md) | 39 项任务状态                   |
| V2 接口契约        | [QUERY-RESULT-API-V2.md](./QUERY-RESULT-API-V2.md)                               | Store/Composable/Component 接口 |
| V2 架构设计        | [QUERY-RESULT-ARCHITECTURE-V2.md](./QUERY-RESULT-ARCHITECTURE-V2.md)             | 组件树、数据流、目录结构        |
| 现有文档（待废弃） | [QUERY-RESULT.md](./QUERY-RESULT.md)                                             | V3.1 旧架构文档                 |
| 现有需求文档       | [QUERY-RESULT-DESIGN.md](./QUERY-RESULT-DESIGN.md)                               | V1.0 需求文档（功能规格仍有效） |
| 前端架构索引       | [INDEX.md](./INDEX.md)                                                           | 前端文档总索引                  |
| 项目文档中心       | [../README.md](../README.md)                                                     | 项目级文档中心                  |

---

## 版本历史

| 版本 | 日期       | 说明                              |
| ---- | ---------- | --------------------------------- |
| v1.0 | 2026-05-08 | 初始版本，制定 8 组 39 项优化计划 |
