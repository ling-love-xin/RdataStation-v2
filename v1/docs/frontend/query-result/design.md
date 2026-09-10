# 数据库工具结果集面板（Result Set Panel）实现需求文档

> 版本：v1.0
> 最后更新：2026-05-03
> 状态：✅ 持续更新

---

## 1. 项目背景

我们正在构建一款**本地优先、不上云**的桌面数据库管理工具，技术栈为 **Rust (Tauri) + Vue3 + dockview + ag-grid (社区版)**。产品采用"应用级/项目级"双层架构，支持多种数据库连接，并深度集成 **DuckDB** 作为本地加速与分析引擎。

当前任务是实现"结果集面板"，它需在保留 **DBeaver 风格的服务端重新查询过滤**的同时，集成 **DuckDB 二次分析**与**前端即时过滤**，形成独特的三模式过滤分析体系。

## 2. 技术要求

- **前端**：Vue 3（组合式 API + `<script setup>`）+ TypeScript
- **布局**：dockview（结果集面板作为可拖拽、分屏、标签化的面板）
- **表格**：ag-grid 社区版（Community Edition），必须启用虚拟滚动
- **后端交互**：通过 Tauri `invoke` 调用 Rust 后端命令，所有数据库及 DuckDB 操作由后端执行
- **样式**：支持暗色模式，可使用 Tailwind CSS 或 CSS 变量，风格简洁专业

## 3. 页面整体布局（dockview 面板内部）

```
┌──────────────────────────────────────────────────────────┐
│ 标签页头部：结果名称（如"结果1: Album"）                  │
├──────────────────────────────────────────────────────────┤
│ SQL 预览栏：SELECT a.* FROM Album AS a [复制SQL]         │
├──────────────────────────────────────────────────────────┤
│ 过滤分析模式切换：                                       │
│ [🔍 即时过滤] [🗄️ SQL过滤] [🧠 DuckDB分析]                │
├──────────────────────────────────────────────────────────┤
│ 对应模式的输入区（仅显示当前激活的模式）                   │
│                                                            │
│ 模式1：🔍 即时过滤                                         │
│ ┌──────────────────────────────────────────────────────┐   │
│ │ 输入表达式即时过滤已加载数据： [区分大小写]           │   │
│ │ Title LIKE '%Rock%'                                  │   │
│ │ (当前内存中 5000 行 → 过滤后 234 行)                 │   │
│ └──────────────────────────────────────────────────────┘   │
│                                                            │
│ 模式2：🗄️ SQL过滤（DBeaver 风格，拼接 WHERE 重新查询）    │
│ ┌──────────────────────────────────────────────────────┐   │
│ │ 输入 SQL WHERE 条件（可包含子查询）： [执行▶]         │   │
│ │ ArtistId IN (SELECT id FROM artists WHERE ...)       │   │
│ │ (将拼接为 SELECT ... FROM (原始SQL) WHERE 条件)       │   │
│ └──────────────────────────────────────────────────────┘   │
│                                                            │
│ 模式3：🧠 DuckDB 分析（本地 DuckDB 引擎）                   │
│ ┌──────────────────────────────────────────────────────┐   │
│ │ 针对本地 DuckDB 临时表（全量原始结果）执行完整 SQL：   │   │
│ │ SELECT ArtistId, COUNT(*) AS cnt FROM result_temp     │   │
│ │ GROUP BY ArtistId ORDER BY cnt DESC                   │   │
│ │ [执行▶] [清除分析] [快捷分析：去重|分组|计数|...]      │   │
│ │ [⚡ 基于当前前端过滤结果分析]（衔接模式1）              │   │
│ └──────────────────────────────────────────────────────┘   │
├──────────────────────────────────────────────────────────┤
│ 表格区域（ag-grid）                                       │
│ - 显示对应模式产出的结果集                                 │
│ - 列头支持单击排序，列宽拖拽，虚拟滚动                     │
│ - 第一列为行序号                                           │
│ - 支持单元格编辑、行多选                                   │
│ - 列头右键菜单（发送排序/过滤到不同模式）                  │
├──────────────────────────────────────────────────────────┤
│ 底部状态栏（自定义组件）                                   │
│ 左侧按钮：[刷新] [保存] [取消] [导出数据...]               │
│ 中间信息：当前过滤模式标签、可见行数/总行数                 │
│ 右侧性能时间：                                             │
│ - 模式1：无额外耗时                                        │
│ - 模式2：数据库 0.003s（获取 0.001s）                      │
│ - 模式3：DuckDB 0.001s                                     │
│ 时间戳：2026-04-30 23:35:27                                │
└──────────────────────────────────────────────────────────┘
```

**可选侧边面板（通过 dockview 注册）**：

- **列洞察面板**：点击表格某一列或通过右键菜单触发，显示该列基于 DuckDB 的统计信息（数值列：AVG、MIN、MAX、MEDIAN；文本列：频率分布 Top 10）。面板可自由拖拽。

## 4. 详细功能清单（TODO 项，不可遗漏）

### 4.1 基础表格功能（ag-grid 社区版）

- [ ] **虚拟滚动**：支持 50 万行以上数据不卡顿，使用客户端行模型 + `suppressColumnVirtualisation: false` 或视口模型。
- [ ] **行序号列**：固定在最左侧，宽度 60px，不可排序，通过 `valueGetter: (params) => params.node.rowIndex + 1` 实现。
- [ ] **列头排序**：单击切换升序/降序/不排序，按住 Shift 可多列排序。**映射行为**：默认执行前端排序，但右键菜单提供"发送为 SQL ORDER BY"或"发送为 DuckDB 查询"。
- [ ] **列宽调整**：拖拽调整，双击边缘自动适应内容宽度。
- [ ] **单元格选择与行多选**：支持单击选择单元格，Ctrl/Shift 选择多行。由于社区版无范围选择，主要使用行级复制。
- [ ] **复制与粘贴**：
  - `Ctrl+C` 复制所有选中行，TSV 格式写入剪贴板。
  - 可尝试实现 Shift 点击模拟区域选择并复制，但作为 P2。
- [ ] **单元格编辑**：
  - 双击进入编辑，Enter 确认，Esc 取消。
  - 修改行用脏标记（左侧指示器变色）。
  - 底部"保存"按钮变为可用，点击后通过 Tauri 命令将修改写入数据库。
  - "取消"或"刷新"可放弃所有修改。

### 4.2 顶部 SQL 预览与过滤分析模式切换

- [ ] **SQL 预览栏**：只读显示当前原始查询 SQL，右侧提供"复制 SQL"按钮。
- [ ] **三模式切换按钮组**：醒目的按钮或 Tab 切换当前过滤/分析模式，默认为"🔍 即时过滤"。

### 4.3 模式 1：即时过滤（纯前端）

- [ ] **输入框**：输入表达式，300ms 防抖后自动过滤表格（调用 ag-grid 的 `setQuickFilter` 或自定义 `filterInstance`）。
- [ ] **支持语法**：类似 DBeaver，支持 `column = value`，`LIKE`，`AND/OR`，基本比较运算符。
- [ ] **状态反馈**：状态栏显示 `已加载 5000 行 → 过滤后 234 行`。
- [ ] **清除按钮**：一键恢复原始数据展示。

### 4.4 模式 2：SQL 过滤（DBeaver 风格，拼接 WHERE 重新查询数据库）

- [ ] **输入框**：输入 SQL WHERE 条件（可包含子查询），右侧有"执行"按钮（Enter 或点击触发）。
- [ ] **后端处理**：前端调用 Tauri 命令 `re_execute_with_filter(connection_id, original_sql, where_clause, order_clause)`，由 Rust 后端将 WHERE 条件安全拼接后重新向数据库执行。
- [ ] **结果处理**：返回的新结果集替换当前表格，状态栏显示"数据库 XX 行，耗时 X.XXXs"，同时将新结果写入 DuckDB 临时表（用于模式 3）。
- [ ] **列头交互支持**：列头右键提供"以此列排序/过滤 -> 发送到 SQL 过滤模式"，自动填充 WHERE/ORDER BY 并执行。
- [ ] **清除**：重新执行原始 SQL 获取未过滤结果。

### 4.5 模式 3：DuckDB 深度分析

- [ ] **输入框**：输入完整 SQL 语句，针对当前结果集对应的 DuckDB 临时表（如 `result_temp`）。
- [ ] **执行**：调用 Tauri 命令 `execute_duckdb_analysis(temp_table, sql)`，返回新结果集并显示在表格。
- [ ] **状态指示**：表格上方显示淡黄色提示条"当前为 DuckDB 分析结果，原始总行数 XXXXX"。
- [ ] **快捷分析按钮组**：
  - 去重：`SELECT DISTINCT * FROM result_temp`
  - 分组计数：`SELECT <选中列>, COUNT(*) FROM result_temp GROUP BY <选中列>`（需选中列）
  - 计数：`SELECT COUNT(*) FROM result_temp`
  - 求和/平均值等数值函数。
- [ ] **衔接模式 1**："基于当前前端过滤结果分析"按钮：将前端过滤后的数据（或过滤表达式）传递给后端，创建一个新的 DuckDB 临时表，然后切换至该表进行分析。此功能可先作为 P2，初始实现可分析全量原始结果。
- [ ] **清除分析**：退出分析，表格重新加载原始结果（从数据库或缓存）。

### 4.6 底部状态栏

- [ ] **操作按钮**：[刷新]（重新执行原始 SQL）、[保存]（提交编辑）、[取消]（回滚编辑）、[导出数据...]。
- [ ] **过滤模式标签**：显示当前处于"即时过滤"、"SQL过滤"或"DuckDB分析"。
- [ ] **行数显示**：根据模式动态显示，如"原始 12345 行 | 过滤后 234 行"或"分析结果 12 行"。
- [ ] **性能指标**：格式化为 `数据库 0.003s（获取 0.001s）` 或 `DuckDB 0.001s`，总耗时自动累加。
- [ ] **查询时间戳**：精确执行时刻。

### 4.7 数据导出

- [ ] **导出 CSV**：可选使用 ag-grid 前端导出或更优方案——调用 Tauri 命令由 DuckDB 的 `COPY ... TO` 执行，效率更高。
- [ ] **导出 Excel (XLSX)**：由 Rust 后端生成文件。
- [ ] **导出 SQL INSERT 语句**：Rust 后端生成。
- [ ] **导出 JSON**：Rust 后端生成。
- [ ] 所有导出需通过 Tauri 对话框选择保存路径。

### 4.8 右键菜单与列头菜单

- [ ] **单元格/行右键菜单**：
  - 复制（所选行，TSV）
  - 复制为 INSERT 语句
  - 复制为 JSON
  - 将值用于即时过滤（如 `= 'Rock'`）
  - 将值用于 SQL 过滤
  - 将值用于 DuckDB 分析（生成 WHERE 条件）
  - 发送整列到"列洞察面板"
- [ ] **列头右键菜单**：
  - 前端升序/降序排序（客户端）
  - 发送排序为 SQL ORDER BY（切换至模式2）
  - 发送排序为 DuckDB 查询（切换至模式3）
  - 隐藏列
  - 自动调整此列 / 所有列宽度
  - 列汇总（发送至 DuckDB 快速计算 SUM/AVG 等，并显示在状态栏）
  - 发送列到"列洞察面板"

### 4.9 侧边"列洞察面板"（dockview 独立面板）

- [ ] 监听表格列点击或右键菜单触发，显示选中列的详细统计。
- [ ] 统计通过 DuckDB 对临时表执行：`SELECT col, COUNT(*) FROM temp GROUP BY col ORDER BY COUNT(*) DESC LIMIT 10`（文本列）或 `SELECT COUNT, AVG, MIN, MAX, MEDIAN, SUM FROM temp`（数值列）。
- [ ] 面板内容动态刷新，设计简洁，支持暗色模式。

### 4.10 键盘快捷键

- [ ] `Ctrl/Cmd + Enter`：在模式2或模式3中执行过滤/分析 SQL。
- [ ] `Ctrl/Cmd + R` 或 `F5`：刷新（重新执行原始查询）。
- [ ] `Ctrl/Cmd + C`：增强复制（选中行 TSV）。
- [ ] `Ctrl/Cmd + S`：保存编辑。
- [ ] `Tab` / `Shift+Tab`：在可编辑单元格间移动（ag-grid 默认）。

### 4.11 与后端的 Tauri 命令设计

- [ ] `execute_sql(connection_id, sql_text, options) -> ResultSet`：执行原始 SQL，返回列信息、行数据、元信息（数据库耗时，行数，时间戳）。后端需自动将结果写入项目级 DuckDB 临时表，并返回临时表名。
- [ ] `re_execute_with_filter(connection_id, original_sql, where_clause, order_clause) -> ResultSet`：安全拼接 WHERE/ORDER BY 并重新执行，返回新结果集，同时更新 DuckDB 临时表。
- [ ] `execute_duckdb(temp_table, sql_text) -> ResultSet`：在指定临时表上执行分析 SQL。
- [ ] `get_column_insights(temp_table, column_name) -> ColumnStats`：返回列统计信息。
- [ ] `export_data(format, temp_table_or_query, save_path)`：执行导出。
- [ ] `save_cell_changes(connection_id, table_name, changes)`：提交编辑后的数据更改。
- [ ] `transfer_frontend_filter_to_duckdb(connection_id, temp_table, filter_expression)`：将前端过滤结果写入新 DuckDB 临时表（模式1→模式3衔接用）。

## 5. 前端组件结构与状态管理

### 组件树

```
ResultPanel.vue (dockview 面板容器)
├── SqlPreviewBar.vue
├── FilterModeSwitcher.vue
├── QuickFilterInput.vue (模式1)
├── SqlFilterInput.vue (模式2)
├── DuckDBAnalysisInput.vue (模式3)
├── ResultGrid.vue (封装 ag-grid)
├── ResultStatusBar.vue
├── ResultContextMenu.vue (右键菜单)
└── ColumnInsightsPanel.vue (独立 dockview 面板，可选)
```

### 核心状态 (composables/useResultStore.ts)

```typescript
interface ResultState {
  // 数据
  rowData: any[]
  columnDefs: ColDef[]

  // 原始查询信息
  originalSql: string
  connectionId: string

  // DuckDB 临时表名
  duckdbTempTable: string
  originalRowCount: number

  // 当前过滤模式
  filterMode: 'quick' | 'sql' | 'duckdb'

  // 模式1（即时过滤）
  quickFilterExpression: string

  // 模式2（SQL过滤）
  sqlFilterExpression: string
  isSqlFilterLoading: boolean

  // 模式3（DuckDB分析）
  duckdbSql: string
  isDuckdbLoading: boolean
  isAnalysisActive: boolean // 表格是否展示分析结果

  // 编辑状态
  dirtyRows: Set<number>

  // 性能元数据
  executionTime: {
    dbDuration: number
    duckdbDuration: number
    timestamp: string
  }

  // 行数信息
  displayedRowCount: number
}
```

## 6. 开发顺序与优先级

| 阶段                  | 功能                                                                                         | 预估范围  |
| --------------------- | -------------------------------------------------------------------------------------------- | --------- |
| P0 基础骨架           | 创建 ResultPanel.vue，集成 dockview；实现 ResultGrid.vue 展示静态数据；打通 execute_sql 命令 | 前端+后端 |
| P0 模式 1 即时过滤    | 实现 QuickFilterInput 及 ag-grid 客户端过滤                                                  | 纯前端    |
| P0 模式 2 SQL 过滤    | 实现 SqlFilterInput，对接 re_execute_with_filter 命令，拼接 WHERE 重新查询                   | 前端+后端 |
| P0 模式 3 DuckDB 分析 | 实现 DuckDBAnalysisInput，对接 execute_duckdb 命令，结果回填表格                             | 前端+后端 |
| P1 状态栏完善         | 动态显示行数、性能指标、时间戳                                                               | 纯前端    |
| P1 编辑功能           | 单元格编辑、脏标记、保存/取消                                                                | 前端+后端 |
| P1 右键菜单与列头菜单 | 集成三模式交互发送                                                                           | 纯前端    |
| P2 导出功能           | 全部四种格式与文件保存对话框                                                                 | 前端+后端 |
| P2 列洞察面板         | 独立 dockview 面板，DuckDB 实时统计                                                          | 前端+后端 |
| P2 模式衔接           | 模式1→模式3 的过滤子集转移                                                                   | 前端+后端 |
| P2 快捷键与细节打磨   | 全局快捷键增强                                                                               | 纯前端    |

---

## 5. 当前实现评估

### 5.1 已实现

| 功能             | 状态 | 说明                        |
| ---------------- | ---- | --------------------------- |
| AG Grid 基础表格 | ✅   | 社区版，ClientSideRowModel  |
| 行序号列         | ✅   | 固定左侧 # 列               |
| 列头排序         | ✅   | 单列排序，状态栏显示        |
| 列宽调整         | ✅   | 拖拽 + 自适应按钮           |
| 行多选           | ✅   | multiple 模式               |
| 复制 TSV/CSV     | ✅   | toolbar 按钮                |
| 复制 INSERT      | ✅   | toolbar 按钮                |
| 导出 CSV/JSON    | ✅   | 文件下载                    |
| 空状态           | ✅   | 友好提示                    |
| 暗色模式         | ✅   | 跟随系统                    |
| 分页             | ✅   | 200条/页，可切换            |
| 搜索             | ✅   | quick filter + 高亮         |
| NULL 显示        | ✅   | `<span class="null-value">` |
| 事件通信         | ✅   | 通过 window event           |
| Tab 导航         | ✅   | 结果 / 输出                 |

### 5.2 待实现（按设计文档）

| 功能                     | 优先级 | 说明                                                       |
| ------------------------ | ------ | ---------------------------------------------------------- |
| SQL 预览栏               | P0     | 当前结果所属 SQL 的只读显示 + 复制按钮                     |
| 三模式切换               | P0     | 即时过滤 / SQL过滤 / DuckDB分析                            |
| 模式1：即时过滤表达式    | P0     | 类似 DBeaver 的表达式语法，300ms 防抖                      |
| 模式2：SQL 过滤          | P0     | 拼接 WHERE 重新查询数据库 + 后端 Tauri 命令                |
| 模式3：DuckDB 分析       | P0     | 针对临时表执行完整 SQL + 快捷分析                          |
| 底部状态栏（完整版）     | P0     | 刷新/保存/取消/导出按钮 + 模式标签 + 性能时间              |
| 列头右键菜单             | P0     | 排序/发送到模式/隐藏列/自适应                              |
| 单元格右键菜单           | P0     | 复制/INSERT/JSON/发送到模式                                |
| 单元格编辑               | P1     | 双击编辑 + 保存/取消                                       |
| 数据导出增强             | P1     | 后端生成 Excel/SQL INSERT/JSON                             |
| 列洞察面板               | P2     | 侧边独立 dockview 面板                                     |
| DuckDB 临时表管理        | P0     | 结果集自动写入 DuckDB 临时表                               |
| 后端 filter/analyze 命令 | P0     | Tauri `re_execute_with_filter` / `execute_duckdb_analysis` |
