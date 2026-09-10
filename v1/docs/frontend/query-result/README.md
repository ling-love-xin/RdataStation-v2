# 查询结果面板文档

> 版本：v3.1
> 最后更新：2026-05-04
> 状态：⏳ 架构重构中 (参见 V2 设计文档)

> ⚠️ **重要声明**：本文档描述的 V3.1 架构已进入重构阶段。
> 新的 V2 架构设计请查阅以下文档：
>
> - [QUERY-RESULT-OPTIMIZATION-PLAN.md](./QUERY-RESULT-OPTIMIZATION-PLAN.md) — 8 组 39 项优化计划
> - [QUERY-RESULT-ARCHITECTURE-V2.md](./QUERY-RESULT-ARCHITECTURE-V2.md) — V2 组件树/数据流
> - [QUERY-RESULT-API-V2.md](./QUERY-RESULT-API-V2.md) — V2 Store/Composable/Component 接口
> - [QUERY-RESULT-OPTIMIZATION-PROGRESS.md](./QUERY-RESULT-OPTIMIZATION-PROGRESS.md) — 实施进度追踪

> 本文档描述 RdataStation 查询结果面板的完整设计与实现
> 版本: v3.0 — **1:n 架构** (DBeaver 风格)

---

## 目录

- [1. 架构变更：n:n → 1:n](#1-架构变更nn--1n)
- [2. 布局架构（DBeaver 风格）](#2-布局架构dbeaver-风格)
- [3. 组件树](#3-组件树)
- [4. 多标签管理系统](#4-多标签管理系统)
- [5. AG Grid 配置与优化](#5-ag-grid-配置与优化)
- [6. 数据流](#6-数据流)
- [7. 技术栈](#7-技术栈)

---

## 1. 架构变更：n:n → 1:n

### 旧架构 (n:n)

```
SqlEditorPanel #1 ──→ panel_queryResult (独立的 Dockview 面板)
SqlEditorPanel #2 ──→ panel_queryResult (所有编辑器共享一个面板)
                    ↓
  多个编辑器竞争同一个结果面板 → 混乱
```

### 新架构 (1:n) — DBeaver 风格

```
SqlEditorPanel #1 ┌─ 编辑器 ────────────────────────┐
                  │ SQL 编辑器 (Monaco)              │
                  ├─ 分割线 (可拖拽) ────────────────┤
                  │ 结果 #1 │ 结果 #2 │ (Execute+)   │
                  │ AG Grid 内嵌                     │
                  └─ 状态栏 ─────────────────────────┘
                  │
                  └── 每个编辑器拥有自己的结果区域
                      Execute+ 在同一个编辑器内开新标签

SqlEditorPanel #2 ┌─ 编辑器 ────────────────────────┐
                  │ (独立的编辑器，独立的结果区域)     │
                  └──────────────────────────────────┘
```

### 关键变更

| 变更                 | 旧                              | 新                     |
| -------------------- | ------------------------------- | ---------------------- |
| 结果面板位置         | 独立的 Dockview 面板            | 嵌入 SQL 编辑器内部    |
| 编辑器↔结果关系      | n:n 共享                        | 1:n 一对一             |
| 全局事件             | `query-result-updated` 全局广播 | 结果直接写入本地 ref   |
| 面板创建             | `ensureResultPanel()` 动态创建  | 编辑器自带结果区域     |
| 多结果               | 面板内标签                      | 编辑器内结果标签       |
| `WorkbenchView` 职责 | 创建/管理结果面板               | 仅透传事件，不创建面板 |

```
┌─ QueryResultPanel ───────────────────────────────────────────────────┐
│ ┌─ 结果标签栏 ───────────────────────────────────────────────────┐   │
│ │ [结果 #1 x] [结果 #2 x] [结果 #3 x]                            │   │
│ └────────────────────────────────────────────────────────────────-┘   │
│ SQL: SELECT * FROM users  [复制]                                      │
├───────────────────────────────────────────────────────────────────────┤
│ [🔍 即时过滤] [🗄️ SQL过滤] [🧠 DuckDB分析]                              │
├───────────────────────────────────────────────────────────────────────┤
│ ▸ 当前模式输入区...                                                    │
├───────────────────────────────────────────────────────────────────────┤
│ ┌─ AG Grid ───────────────────────────────────────────────────────┐   │
│ │ # │ id │ name   │ email            │ age │ ...                  │   │
│ │ 1 │ 1  │ "ACDC" │ "acdc@mail.com"  │ 42  │                      │   │
│ │ 2 │ 2  │ "Rock" │ "rock@mail.com"  │ 35  │                      │   │
│ │   │    │        │                  │     │                      │   │
│ │   │    │        │                  │     │ (斑马条纹交替行)      │   │
│ └──────────────────────────────────────────────────────────────────┘   │
├───────────────────────────────────────────────────────────────────────┤
│ [🔄][💾][✕][📥]        │ 即时过滤 │ 347 行 (3/4 页)   │ 0.003s │     │
└───────────────────────────────────────────────────────────────────────┘
```

### 多结果标签

| 功能               | 说明                                                  |
| ------------------ | ----------------------------------------------------- |
| 每次执行打开新标签 | 点击执行按钮覆盖当前，点击 ➕ **Execute+** 打开新标签 |
| 标签切换           | 点击标签栏切换结果集                                  |
| 标签关闭           | 点击标签栏的 × 关闭，自动切换到相邻标签               |
| 标签标题           | `结果 #1`, `结果 #2` 递增编号                         |

---

## 2. 组件树

```
QueryResultPanel.vue (主面板 — DBeaver风格多标签)
├── result-tabs                    — 结果标签切换栏
├── toolbar-strip                  — 顶条
│   ├── FilterModeSwitcher.vue    — 三模式切换按钮组
│   ├── QuickFilterInput.vue      — 模式1: 即时过滤 (前端300ms防抖)
│   ├── SqlFilterInput.vue        — 模式2: SQL WHERE 条件输入
│   └── DuckDBAnalysisInput.vue   — 模式3: DuckDB 分析 + 快捷按钮
├── result-body                   — DBeaver风格主体布局
│   ├── view-sidebar              — 左侧视图切换栏 (网格/文本/记录)
│   ├── grid-area                 — 中间表格区
│   │   ├── AG Grid               — 高性能数据表格（ag-grid-community）
│   │   ├── text-view             — 文本视图 (textarea)
│   │   └── record-view           — 记录视图 (单行详情)
│   └── value-viewer              — 右侧值查看器 (可折叠)
├── result-statusbar              — 底部状态栏
│   ├── sbar-left                 — 操作按钮 (刷新/保存/取消/导出)
│   ├── sbar-center               — 模式标签 + 行数 + 执行时间
│   └── sbar-right                — 分页控制 (首页/上页/页码/下页/末页)
└── ResultContextMenu.vue         — 右键菜单 (cell/header)

MultiTabResults.vue (多语句结果包装器)
├── tab-bar                       — 多结果 Tab 栏 (NTabs)
├── tab-content                   — 结果内容区
│   └── QueryResultPanel          — 委托渲染
└── status-bar                    — 底部状态栏

ColumnInsightsPanel.vue (独立 dockview 右侧面板)
```

### 2.1 文件路径

```
src/extensions/builtin/workbench/ui/
├── components/panels/
│   ├── QueryResultPanel.vue           # 主结果面板
│   ├── MultiTabResults.vue            # 多语句结果包装器
│   └── result-panel/
│       ├── FilterModeSwitcher.vue     # 模式切换器
│       ├── QuickFilterInput.vue       # 即时过滤输入
│       ├── SqlFilterInput.vue         # SQL过滤输入
│       ├── DuckDBAnalysisInput.vue    # DuckDB分析输入
│       ├── ResultStatusBar.vue        # 状态栏
│       ├── ResultContextMenu.vue      # 右键菜单
│       ├── SqlPreviewBar.vue          # SQL预览条
│       └── QuickFilterInput.vue       # 快速过滤输入
├── stores/
│   └── result-store.ts                # 结果状态管理 (Pinia)
└── services/
    └── result-analysis.ts             # Tauri API 调用层
```

---

## 3. 多标签管理系统

### 3.1 ResultTab 数据结构

```typescript
interface ResultTab {
  id: string // "result_时间戳_序号"
  title: string // "结果 #1"
  originalSql: string // 原始 SQL
  connectionId: string // 连接 ID
  duckdbTempTable: string // DuckDB 临时表名
  columns: string[] // 列名数组
  rows: unknown[][] // 行数据
  originalRowCount: number // 原始行数
  displayedRowCount: number // 显示行数
  filteredRowCount: number // 过滤后行数
  filterMode: FilterMode // 当前过滤模式
  quickFilterExpression: string // 即时过滤表达式
  sqlFilterExpression: string // SQL WHERE 条件
  isSqlFilterLoading: boolean // SQL过滤加载中
  duckdbSql: string // DuckDB 分析 SQL
  isDuckdbLoading: boolean // DuckDB分析加载中
  isAnalysisActive: boolean // 是否为 DuckDB 分析模式
  executionTime: number // 执行耗时 ms
  timestamp: string // 执行时间
  dirtyRows: Set<number> // 脏行集合
}
```

### 3.2 标签生命周期

```
execute（覆盖）→ query-result-updated 事件 → 复用或新建标签
execute+（新增）→ query-result-new 事件 → 始终新建标签
关闭标签 → splice 移除 → 自动切换到最近标签
```

### 3.3 三视图模式

| 视图     | 组件     | 说明                         |
| -------- | -------- | ---------------------------- |
| 网格视图 | AG Grid  | 默认视图，高性能虚拟滚动表格 |
| 文本视图 | textarea | TSV 格式显示所有数据         |
| 记录视图 | div      | 单行详细视图，支持上下翻页   |

### 3.4 值查看器

右侧可折叠面板，显示选中单元格的完整内容：

```typescript
interface SelectedCell {
  column: string // 列名
  row: number // 行索引 (0-based)
  value: any // 原始值
}
```

---

## 4. 三模式详解

### 4.1 模式1: 即时过滤 (前端)

| 属性   | 值                             |
| ------ | ------------------------------ |
| 输入   | 自由文本表达式                 |
| 防抖   | 300ms                          |
| 实现   | `gridApi.setQuickFilter(expr)` |
| 耗时   | 0（纯前端）                    |
| 状态栏 | `原始 N 行 → 过滤后 M 行`      |

### 4.2 模式2: SQL 过滤 (后端重新查询)

| 属性   | 值                                              |
| ------ | ----------------------------------------------- |
| 输入   | SQL WHERE 条件（可含子查询）                    |
| 快捷键 | `Ctrl+Enter`                                    |
| 拼接   | `SELECT * FROM (原始SQL) AS _result WHERE 条件` |
| 状态栏 | 行数 + 执行时间                                 |

### 4.3 模式3: DuckDB 分析

| 属性     | 值                                            |
| -------- | --------------------------------------------- |
| 输入     | 完整 DuckDB SQL（`{table}` 引用临时表）       |
| 快捷键   | `Ctrl+Enter`                                  |
| 快捷按钮 | 计数 / 去重 / 分组 / **基于前端过滤结果分析** |
| 状态栏   | 行数 + DuckDB 执行时间                        |

---

## 5. AG Grid 配置与优化

### 5.1 紧凑型布局（默认）

所有结果集默认为紧凑模式：

| 参数                     | 值                             | 说明                         |
| ------------------------ | ------------------------------ | ---------------------------- |
| `headerHeight`           | `24`                           | 表头高度                     |
| `rowHeight`              | `22`                           | 行高                         |
| `rowBuffer`              | `20`                           | 预渲染行数                   |
| `font-size`              | `11px` (grid), `10px` (header) | 全局字号                     |
| `pagination`             | `自动`                         | <50000行时分页，否则虚拟滚动 |
| `paginationPageSize`     | `100`                          | 默认每页100行                |
| `paginationPageSelector` | `[50, 100, 200, 500]`          | 页大小选择器                 |

### 5.2 Grid ↔ Record ↔ Text 联动

| 操作                      | 行为                                 |
| ------------------------- | ------------------------------------ |
| 点击左侧栏 Database 图标  | 切换到网格视图（默认）               |
| 点击左侧栏 AlignLeft 图标 | 切换到文本视图                       |
| 点击左侧栏 List 图标      | 切换到记录视图                       |
| **点击任意一行**          | **自动切换到记录视图并显示该行数据** |
| 记录模式下◀▶按钮          | 上下翻页切换记录                     |

### 5.3 Record 视图布局

```
┌─ Record View ────────────────────────────────────┐
│ ◀  3 / 347  ▶                                    │
├───────────────────────────────────────────────────┤
│ ID          │ 1                                   │
│ NAME        │ AC/DC                               │
│ EMAIL       │ acdc@mail.com                       │
│ CREATED_AT  │ 2024-01-15 10:30:00                │
│ DESCRIPTION │ Full content text...                │
├───────────────────────────────────────────────────┤
│ (斑马纹交替行)                                      │
└───────────────────────────────────────────────────┘
```

### 5.4 Text 视图布局

TSV 格式文本显示，便于复制：

```
[1]    1    AC/DC    acdc@mail.com    42
[2]    2    Rock     rock@mail.com    35
[3]    3    Metal    metal@mail.com   28
```

### 5.5 列类型检测

```typescript
// 列类型检测
isLikelyNumeric(col) → width: 110, flex: 0, text-align: right
isLikelyDate(col)    → width: 140, flex: 1
isLikelyLongText(col) → width: 200, flex: 2
其他                   → width: 130, flex: 1
```

### 5.6 全局行号（#2）

支持分页的全局行号：`page * pageSize + rowIndex + 1`

### 5.7 性能优化参数（#18-#22）

| 参数                            | 值         | 说明                     |
| ------------------------------- | ---------- | ------------------------ |
| `columnVirtualisation`          | `true`     | 列虚拟化，大宽表优化     |
| `rowBuffer`                     | `20`       | 预渲染行数，减少滚动白屏 |
| `blockLoadDebounceMs`           | `50`       | 滚动节流                 |
| `domLayout`                     | `'normal'` | 标准 DOM 布局            |
| `singleClickEdit`               | `false`    | 单击编辑关闭             |
| `stopEditingWhenCellsLoseFocus` | `true`     | 失焦自动停止编辑         |

### 5.8 视觉优化（#11-#16）

| 优化               | 实现                                              |
| ------------------ | ------------------------------------------------- |
| NULL 值视觉        | 灰色 italic + 小字号                              |
| 数字列右对齐       | CSS `.text-right` + monospace                     |
| 斑马条纹           | `:deep(.ag-row-even)` 交替底色                    |
| 冻结列分割线       | `.ag-pinned-left-cols-container` 右边框           |
| 分页选择器         | `[50, 100, 200, 500]` 下拉跳页                    |
| 数据量大时分页关闭 | `< 50000 行时分页，≥ 50000 行虚拟滚动             |
| **移除浮动筛选行** | `floatingFilter: false`，列头过滤通过右键菜单操作 |

> **布局原则**：结果集表格不设单独的筛选过滤行，保持 DBeaver 风格的干净表格。筛选通过列头右键菜单或顶部的三模式过滤面板完成。

### 5.9 导出菜单

| 格式   | 实现                        |
| ------ | --------------------------- |
| CSV    | `gridApi.exportDataAsCsv()` |
| JSON   | `JSON.stringify` + Blob URL |
| INSERT | `clipboard.writeText`       |

---

## 6. 右键菜单

### 6.1 单元格/行右键菜单

| 菜单项                | 功能                  |
| --------------------- | --------------------- |
| 复制单元格值          | 复制当前单元格        |
| 复制行(TSV)           | 选中行/全部行 TSV     |
| 复制为 JSON           | JSON 格式复制         |
| 复制为 INSERT         | 生成 INSERT 语句      |
| 以此值过滤 (即时过滤) | 切换模式1，填入表达式 |
| 以此值过滤 (SQL过滤)  | 切换模式2，填入 WHERE |
| 列洞察                | 发送到列洞察面板      |

### 6.2 列头右键菜单

| 菜单项              | 功能           |
| ------------------- | -------------- |
| 升序/降序排序       | 前端排序       |
| 发送排序到 SQL 过滤 | 模式2 ORDER BY |
| 发送排序到 DuckDB   | 模式3 分析 SQL |
| 隐藏列              | 隐藏当前列     |
| 自适应列宽          | 自动调整宽度   |
| 自适应所有列宽      | 自动调整所有列 |
| 列汇总 (DuckDB)     | 模式3 聚合 SQL |
| 列洞察面板          | 列统计面板     |

---

## 7. 键盘快捷键

| 快捷键                     | 作用域     | 功能                               |
| -------------------------- | ---------- | ---------------------------------- |
| `Ctrl/Cmd + Enter`         | SQL 编辑器 | 执行 SQL                           |
| `Ctrl/Cmd + Shift + Enter` | SQL 编辑器 | **Execute+：执行并打开新结果标签** |
| `Ctrl/Cmd + Enter`         | 模式2/3    | 执行过滤/分析 SQL                  |
| `Ctrl/Cmd + S`             | 结果面板   | 保存编辑                           |
| `Ctrl/Cmd + R`             | 结果面板   | 刷新（重新执行原始查询）           |
| `Ctrl/Cmd + C`             | AG Grid 内 | 增强复制 (TSV 格式)                |

---

## 8. 列洞察面板

参见 [8. 列洞察面板](#8-列洞察面板)

---

## 9. 单元格编辑

参见 [9. 单元格编辑](#9-单元格编辑)

---

## 10. 数据流

### 10.0 完整数据流

```
执行 SQL（正常）→ SqlEditorPanel.handleExecute()
  → event('sql-execution-result', { result, originalSql, connectionId, elapsedMs })
  → WorkbenchView (透传)
  → event('query-result-updated', { ... })
  → QueryResultPanel.handleResultUpdate() → 复用或新建标签

执行 SQL（Execute+）→ SqlEditorPanel.handleExecuteNew()
  → event('query-result-new', { ... })  ← 始终新建标签
  → QueryResultPanel.handleResultNew() → 始终新建标签
```

---

## 11. 后端命令

### 11.1 Tauri 命令

| 命令                       | 参数                                                      | 返回                           |
| -------------------------- | --------------------------------------------------------- | ------------------------------ |
| `execute_sql`              | `conn_id`, `sql`, `timeout_ms`                            | `ExecuteSqlResponse`           |
| `re_execute_with_filter`   | `conn_id`, `original_sql`, `where_clause`, `order_clause` | `ResultSet`（含 `temp_table`） |
| `execute_duckdb_analysis`  | `temp_table`, `sql`, `columns`, `rows`                    | `ResultSet`                    |
| `get_column_insights`      | `temp_table`, `column_name`                               | `ColumnStats`                  |
| `create_duckdb_temp_table` | `columns`, `rows`                                         | `String`（表名）               |

---

## 12. 技术栈

### 前端优化清单

| 优先级 | 优化项                           | 状态 |
| ------ | -------------------------------- | ---- |
| P0     | 数字/日期/长文本列智能宽度       | ✅   |
| P0     | 全局分页行号                     | ✅   |
| P0     | NULL 值视觉增强                  | ✅   |
| P0     | floatingFilter 优化              | ✅   |
| P0     | 导出菜单 (CSV/JSON/INSERT)       | ✅   |
| P0     | AG Grid 空列定义保护             | ✅   |
| P0     | `hasDirty` 运行时错误修复        | ✅   |
| P1     | 多结果标签系统                   | ✅   |
| P1     | Execute+ 按钮 (Ctrl+Shift+Enter) | ✅   |
| P1     | DBeaver 风格底部状态栏           | ✅   |
| P1     | ColumnInsightsPanel 事件驱动解耦 | ✅   |
| P2     | 斑马条纹行                       | ✅   |
| P2     | 数字列右对齐                     | ✅   |
| P2     | 冻结列分割线                     | ✅   |
| P2     | 分页跳转选择器                   | ✅   |
| P3     | 列虚拟化                         | ✅   |
| P3     | rowBuffer=30                     | ✅   |
| P3     | 大数据量自动关闭分页             | ✅   |

### 架构变更记录

| 日期    | 变更                                                   | 影响                               |
| ------- | ------------------------------------------------------ | ---------------------------------- |
| 2026-04 | 移除 `ResultStatusBar.vue` 使用，状态栏内联            | `ResultStatusBar.vue` 废弃         |
| 2026-04 | `FilterModeSwitcher` 自包含 `FilterMode` 类型          | 不再依赖 `result-store.ts`         |
| 2026-04 | `ColumnInsightsPanel` 从 `useResultStore` 改为事件驱动 | 不再依赖 `result-store.ts`         |
| 2026-04 | `QueryResultPanel` 完全自包含多标签架构                | `result-store.ts` 仅被废弃组件引用 |
