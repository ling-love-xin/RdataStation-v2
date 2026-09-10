# 多模式编辑器面板 实施计划

> Spec: `.trae/specs/multi-mode-editor/spec.md`
> Tasks: `.trae/specs/multi-mode-editor/tasks.md`
> Checklist: `.trae/specs/multi-mode-editor/checklist.md`

---

## 一、目标总览

将当前单一 SQL 查询编辑器重构为 **VSCode 风格的面板工厂架构**，支持三种编辑器模式：

```
EditorPanelFactory.vue  (面板路由)
├─ QueryEditorPanel.vue    查询编辑器 — DBeaver/DataGrip 风格
├─ AnalysisEditorPanel.vue 分析编辑器 — DuckDB 纯分析引擎
└─ CodeEditorPanel.vue     代码编辑器 — 预留 LSP 扩展
```

---

## 二、实施步骤

### Step 1: 定义 EditorType 与编辑器类型系统

**文件**: `src/extensions/builtin/workbench/types/editor-types.ts`

新增以下类型定义：

```typescript
// 编辑器模式枚举
export type EditorType = 'query' | 'analysis' | 'code'

// 工具栏按钮配置
interface ToolbarButton {
  id: string // 唯一标识
  group: 'execute' | 'edit' | 'mode' | 'transaction' | 'federation' | 'general'
  label: string // 按钮文本
  icon?: string // lucide 图标名
  shortcut?: string // 快捷键 (如 'Ctrl+Enter')
  visible: (ctx: EditorContext) => boolean // 可见性条件
  action: (ctx: EditorContext) => void // 点击行为
}

// 工具栏分隔符
interface ToolbarSeparator {
  type: 'separator'
  group: string
}

type ToolbarItem = ToolbarButton | ToolbarSeparator

// 工具栏配置
interface EditorToolbarConfig {
  items: ToolbarItem[]
}

// 状态栏字段配置
interface StatusBarField {
  id: string
  label: string
  value: () => string
  visible: (type: EditorType) => boolean
}

// 编辑器面板上下文
interface EditorContext {
  editorType: EditorType
  filePath: string
  language: string
  editorView: EditorView | null
  connectionId: string | null
  isExecuting: boolean
}
```

**新增 `EditorModeResolver`**：

```typescript
// 根据文件扩展名返回默认 EditorType
function resolveEditorType(
  filePath: string,
  language: string,
  userPreference?: EditorType
): EditorType
// 规则：.sql → query, .duckdb.sql → analysis, .rs/.ts/.py/.js/.go → code
// 用户手动切换后优先使用 userPreference
```

---

### Step 2: 拆分 EditorManager.ts（5 个独立模块）

当前 `EditorManager.ts` (~400+ 行) 包含 7 种职责，需拆分为：

| 新模块                   | 文件路径                                      | 职责                                                                     |
| ------------------------ | --------------------------------------------- | ------------------------------------------------------------------------ |
| `FileStateStore`         | `workbench/ui/stores/file-state-store.ts`     | Pinia Store，管理 openFiles / activeFilePath / isDirty / file open/close |
| `EditorInstanceRegistry` | `workbench/manager/EditorInstanceRegistry.ts` | EditorView 实例注册/注销/查询/主实例判定/状态保存恢复                    |
| `ResultSetManager`       | `workbench/manager/ResultSetManager.ts`       | 结果集 CRUD / MAX_RESULT_SETS 限制 & toast 提示 / 面板绑定               |
| `DockviewBridge`         | `workbench/manager/DockviewBridge.ts`         | dockviewApi 封装 / addPanel / movePanelOrGroup / getPanel / 浮动弹窗     |
| `EditorModeResolver`     | `workbench/manager/EditorModeResolver.ts`     | 文件扩展名 → EditorType 映射                                             |

**拆分策略**：

1. 先创建 5 个新模块文件，逐个抽出原 `EditorManager.ts` 的方法
2. 维持 `EditorManager` 作为兼容性 re-export 层（逐步废弃）
3. 全量搜索替换引用后，删除原文件

---

### Step 3: 修复 IPC 类型安全（前端 ↔ 后端）

**文件**: `src/extensions/builtin/query/ui/services/query.ts`

当前问题：所有 specta 调用都使用 `as unknown as` 双重强转。

**修复方案**：

1. 确认 specta `ExecuteSqlResponse` 结构体字段与前端 `ExecuteSqlResponse` 一致
2. 若不一致，修改前端 interface 以对齐 specta 类型，而非强转
3. 消除 `query.ts` 中全部 8 处 `as unknown as`
4. 为 `execute_duckdb_accelerated` 添加 `#[specta::specta]` 并在 specta 绑定中注册
5. 移除 `DuckDBAcceleratedResult` 手写 interface，改为从 specta 导入

**关键文件变更**：

```
src-tauri/src/commands/sql_commands.rs  — 添加 #[specta::specta] 到 execute_duckdb_accelerated
src-tauri/src/main.rs                   — collect_commands! 注册新命令
src/extensions/builtin/query/ui/services/query.ts — 替换 tauriInvoke 为 typed(commands.xxx)
```

---

### Step 4: 后端 — QueryResult 新增 column_types

**文件**: `src-tauri/src/core/models.rs`

```rust
impl QueryResult {
    /// 从 Arrow Schema 提取列类型名称
    pub fn column_types(&self) -> Vec<String> {
        self.batches
            .first()
            .map(|batch| {
                batch.schema()
                    .fields()
                    .iter()
                    .map(|f| {
                        // Arrow DataType → 简化的类型名
                        match f.data_type() {
                            DataType::Int8 | DataType::Int16 | DataType::Int32 | DataType::Int64
                            | DataType::UInt8 | DataType::UInt16 | DataType::UInt32 | DataType::UInt64
                                => "INTEGER",
                            DataType::Float16 | DataType::Float32 | DataType::Float64
                                => "FLOAT",
                            DataType::Utf8 | DataType::LargeUtf8
                                => "TEXT",
                            DataType::Boolean => "BOOLEAN",
                            DataType::Null => "NULL",
                            DataType::Binary | DataType::LargeBinary | DataType::FixedSizeBinary(_)
                                => "BYTES",
                            _ => "TEXT", // fallback
                        }.to_string()
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}
```

修改 `Serialize` 实现：

```rust
impl Serialize for QueryResult {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("QueryResult", 6)?;
        state.serialize_field("columns", &self.columns)?;
        state.serialize_field("column_types", &self.column_types())?;  // ← 新增
        state.serialize_field("rows", &self.to_rows())?;
        state.serialize_field("affected_rows", &self.affected_rows)?;
        state.serialize_field("is_read_only", &self.is_read_only)?;
        state.serialize_field("total_rows", &self.total_rows())?;
        state.end()
    }
}
```

前端同步新增：

```typescript
// src/extensions/builtin/workbench/ui/types/result.ts
interface QueryResult {
  columns: string[]
  columnTypes: string[]  // ← 新增
  rows: unknown[][]
  ...
}
```

---

### Step 5: 后端 — 分页查询命令

**文件**: `src-tauri/src/commands/sql_commands.rs`

新增命令：

```rust
#[derive(serde::Deserialize, specta::Type)]
pub struct ExecuteSqlPaginatedInput {
    pub conn_id: Option<String>,
    pub sql: String,
    pub offset: u32,
    pub limit: u32,
    pub timeout_ms: Option<u32>,
}

#[tauri::command]
#[specta::specta]
pub async fn execute_sql_paginated(
    input: ExecuteSqlPaginatedInput,
) -> Result<ExecuteSqlResponse, CoreError> {
    // 1. 执行完整查询获取 QueryResult
    // 2. 调用 result.slice(offset, limit) 截取行
    // 3. total_rows 返回原始总行数
    // 4. 序列化时仅包含截取后的行
}
```

前端新增：

```typescript
// src/extensions/builtin/query/ui/services/query.ts
export async function executeSqlPaginated(
  sql: string,
  connectionId: string,
  offset: number,
  limit: number,
  timeoutMs?: number
): Promise<ExecuteSqlResponse> {
  // 使用 specta 类型，不走 as unknown as
}
```

---

### Step 6: 修复 Rust unwrap 违规

两处需要修复：

**`sql_commands.rs:L443`**：

```rust
// ❌ 当前
.unwrap_or(Value::Null)
// ✅ 修复
.map_or(Value::Null, Value::Number)
```

**`sql_commands.rs:L458`**：

```rust
// ❌ 当前
.unwrap_or_default()
// ✅ 修复
.unwrap_or_else(|| "?".to_string())
```

---

### Step 7: 创建 EditorPanelFactory.vue

**文件**: `src/extensions/builtin/workbench/ui/components/panels/EditorPanelFactory.vue`

这是面板路由组件，替代当前 `EditorPanel.vue` 在 dockview 中的注册：

```vue
<template>
  <div class="editor-panel">
    <!-- 面板类型切换选择器（悬浮在右上角） -->
    <div v-if="showTypeSwitcher" class="type-switcher">
      <NSelect v-model:value="currentEditorType" :options="editorTypeOptions" size="tiny" />
    </div>

    <!-- 根据 EditorType 动态渲染子面板 -->
    <QueryEditorPanel v-if="currentEditorType === 'query'" :params="props.params" />
    <AnalysisEditorPanel v-if="currentEditorType === 'analysis'" :params="props.params" />
    <CodeEditorPanel v-if="currentEditorType === 'code'" :params="props.params" />
  </div>
</template>
```

**EditorBody 共享组件**（提取自当前 EditorPanel.vue）：

```vue
<!-- EditorBody.vue — 三个子面板共用 -->
<template>
  <div class="editor-body">
    <div v-if="isReadonly" class="readonly-warning">...</div>
    <div class="tab-bar"><!-- 标签栏（保持不变） --></div>
    <div class="editor-split">
      <div class="editor-area">
        <div ref="editorContainerRef" class="cm-container" />
        <EditorWelcome v-if="showWelcome" ... />
      </div>
      <div v-if="hasResults" class="split-handle" @mousedown="startSplitDrag" />
      <div v-if="hasResults" class="result-area">
        <ResultSubTab />
        <div class="result-panel-host" />
      </div>
    </div>
  </div>
</template>
```

---

### Step 8: 创建 QueryEditorPanel.vue（查询编辑器）

**文件**: `src/extensions/builtin/workbench/ui/components/panels/QueryEditorPanel.vue`

```
QueryEditorPanel.vue
├─ QueryToolbar.vue       ← 新建工具栏组件
├─ EditorBody.vue         ← 共享编辑器主体
├─ QueryStatusBar.vue     ← 新建状态栏组件
└─ ResultSubTab + QueryResultPanel  ← 复用现有
```

**QueryToolbar.vue 按钮布局**：

```
[▶ 执行] [▶+ 新标签执行] [⚡ 本地加速] [📊 执行计划] [|] [格式化] [方言▾] [|] [普通|分析|智能] [|] [事务▾] [📋 历史]
```

每个按钮映射到现有的 `EditorManager` / `useSqlExecution` 方法：

- 执行 → `executeSingleStatement()`
- 新标签执行 → `executeNewTab()`
- 本地加速 → `executeDuckDBAccelerated()`
- 执行计划 → `executeExplainPlan()`（新方法：发送 `EXPLAIN <sql>`）
- 格式化 → `EditorManager.formatSQL()`
- 方言转换 → `transpileSql()`（弹出 NModal 选择目标方言）
- 模式切换 → `executionMode` ref（三选一 NButtonGroup）
- 事务 → 下拉菜单：开始事务 / 提交 / 回滚
- 历史 → 打开 SQL 历史面板

**QueryStatusBar.vue**：

```
连接: postgres@localhost  |  LN 12, COL 5  |  3条SQL  |  事务中  |  125ms  |  SQL
```

---

### Step 9: 实现三种执行模式（查询编辑器核心）

**文件**: `src/extensions/builtin/workbench/ui/composables/useSqlExecution.ts`

```typescript
// 新增执行模式
const executionMode = ref<'normal' | 'analysis' | 'smart'>('normal')
const SMART_MODE_THRESHOLD = 1000 // 可配置阈值

async function executeWithMode(): Promise<void> {
  switch (executionMode.value) {
    case 'normal':
      return executeSingleStatement() // 现有逻辑
    case 'analysis':
      return executeDuckDBAccelerated() // 现有逻辑
    case 'smart':
      return executeSmart()
  }
}

async function executeSmart(): Promise<void> {
  // 1. 先查询行数预估（EXPLAIN 或 SELECT COUNT(*)）
  const estimatedRows = await estimateRowCount(sql, connId)

  // 2. 根据阈值选择策略
  if (estimatedRows <= SMART_MODE_THRESHOLD) {
    executionMode.value = 'normal' // 小数据量走普通模式
    return executeSingleStatement()
  } else {
    executionMode.value = 'analysis' // 大数据量走分析模式
    return executeDuckDBAccelerated()
  }
}

async function estimateRowCount(sql: string, connId: string): Promise<number> {
  // 发送 EXPLAIN 获取预估行数，或执行 SELECT COUNT(*) FROM (<sql>) AS _sub
  // 后端新命令：estimate_query_rows
}
```

**后端新增行数预估命令**（可选）：

```rust
#[tauri::command]
pub async fn estimate_query_rows(conn_id: Option<String>, sql: String) -> Result<u64, CoreError> {
    // 包装 SQL: SELECT COUNT(*) FROM ({sql}) AS _sub
    // 返回行数
}
```

---

### Step 10: 创建 AnalysisEditorPanel.vue（分析编辑器）

**文件**: `src/extensions/builtin/workbench/ui/components/panels/AnalysisEditorPanel.vue`

```
AnalysisEditorPanel.vue
├─ AnalysisToolbar.vue    ← 分析专用工具栏
├─ EditorBody.vue         ← 共享编辑器主体
└─ AnalysisStatusBar.vue  ← 分析专用状态栏
```

**AnalysisToolbar.vue 独有按钮**：

```
[▶ 执行] [▶+ 新标签执行] [|] [联邦查询▾] [|] [格式化] [|] [📋 历史]
```

**联邦查询选择器**：

```vue
<!-- 联邦查询：多选已 ATTACH 的外部数据源 -->
<NSelect
  v-model:value="selectedFederationSources"
  :options="federationSourceOptions"
  multiple
  placeholder="选择联邦数据源..."
  @update:value="onFederationChange"
/>
```

选择数据源后，自动在编辑器顶部插入：

```sql
-- ATTACH 'postgres:dbname=mydb' AS pg_source (TYPE postgres)
-- ATTACH 'mysql:host=localhost:3306/db' AS mysql_source (TYPE mysql)
```

**Composable**: `useFederationQuery.ts`

- 从后端获取已 ATTACH 的外部数据库列表
- 管理当前选中的数据源
- 提供 ATTACH / DETACH 操作

---

### Step 11: 创建 CodeEditorPanel.vue（代码编辑器）

**文件**: `src/extensions/builtin/workbench/ui/components/panels/CodeEditorPanel.vue`

```
CodeEditorPanel.vue
├─ CodeToolbar.vue        ← 通用编辑工具栏
├─ EditorBody.vue         ← 共享编辑器主体
└─ CodeStatusBar.vue      ← LSP 诊断状态栏
```

**CodeToolbar.vue**：

```
[💾 保存] [格式化] [🔍 查找] [↩ 撤销] [↪ 重做]
```

**无** SQL 执行相关按钮。

**LSP 扩展点接口**：

```typescript
// src/extensions/builtin/workbench/types/lsp-types.ts
export interface LspExtensionPoint {
  // 诊断提供者
  getDiagnostics?: (filePath: string, content: string) => Promise<LspDiagnostic[]>
  // 补全提供者
  getCompletions?: (filePath: string, position: LspPosition) => Promise<LspCompletion[]>
  // 悬停提示提供者
  getHover?: (filePath: string, position: LspPosition) => Promise<LspHover | null>
  // 跳转定义提供者
  getDefinition?: (filePath: string, position: LspPosition) => Promise<LspLocation | null>
}

export interface LspDiagnostic {
  severity: 'error' | 'warning' | 'info'
  message: string
  range: { start: LspPosition; end: LspPosition }
}

export interface LspPosition {
  line: number
  character: number
}
export interface LspCompletion {
  label: string
  kind: string
  detail?: string
  insertText?: string
}
export interface LspHover {
  contents: string
  range?: { start: LspPosition; end: LspPosition }
}
export interface LspLocation {
  uri: string
  range: { start: LspPosition; end: LspPosition }
}
```

**CodeStatusBar.vue**：

```
语言: Rust  |  编码: UTF-8  |  缩进: 4空格  |  LN 5, COL 12  |  ⚠ 2W  ❌ 1E
```

---

### Step 12: 统一消除空 catch

遍历所有编辑器相关文件，将空 catch 替换为有意义的 warn：

```typescript
// ❌ 之前
try {
  dockviewApi?.getPanel(id)?.api.setVisible(true)
} catch {
  /* */
}

// ✅ 之后
try {
  dockviewApi?.getPanel(id)?.api.setVisible(true)
} catch {
  console.warn('[DockviewBridge] getPanel failed for', id)
}
```

---

### Step 13: 端到端验证

| 验证场景                    | 预期结果                                     |
| --------------------------- | -------------------------------------------- |
| 打开 `test.sql`             | 自动进入 QueryEditorPanel                    |
| 查询编辑器执行 SELECT       | 结果面板正常展示（含 column_types）          |
| 切换面板类型 查询→分析→代码 | 工具栏/状态栏即时切换，编辑器内容不变        |
| 分析编辑器 DuckDB 加速      | 正常执行，结果正确                           |
| 联邦查询选择器              | 展示已 ATTACH 数据源，选择后插入 ATTACH 提示 |
| 智能模式执行                | ≤1000 行走普通模式，>1000 行走分析模式       |
| 打开 `main.rs`              | 自动进入 CodeEditorPanel，无 SQL 按钮        |
| 分页查询                    | offset=100 / limit=50 返回正确行数           |
| `pnpm run lint`             | 零错误                                       |
| `pnpm run format`           | 通过                                         |

---

## 三、关键文件变更清单

| 文件                                                     | 操作 | 说明                                                         |
| -------------------------------------------------------- | ---- | ------------------------------------------------------------ |
| `workbench/types/editor-types.ts`                        | 修改 | 新增 EditorType / EditorToolbarConfig / EditorContext 等类型 |
| `workbench/types/lsp-types.ts`                           | 新增 | LSP 扩展点接口定义                                           |
| `workbench/ui/stores/file-state-store.ts`                | 新增 | 文件状态 Pinia Store                                         |
| `workbench/manager/EditorInstanceRegistry.ts`            | 新增 | 编辑器实例注册表                                             |
| `workbench/manager/ResultSetManager.ts`                  | 新增 | 结果集管理器                                                 |
| `workbench/manager/DockviewBridge.ts`                    | 新增 | Dockview API 桥接                                            |
| `workbench/manager/EditorModeResolver.ts`                | 新增 | 编辑器类型解析器                                             |
| `workbench/manager/EditorManager.ts`                     | 删除 | 原 God Object，拆分后移除                                    |
| `workbench/ui/components/panels/EditorPanelFactory.vue`  | 新增 | 面板工厂路由组件                                             |
| `workbench/ui/components/panels/EditorBody.vue`          | 新增 | 共享编辑器主体（从 EditorPanel 抽出）                        |
| `workbench/ui/components/panels/EditorPanel.vue`         | 删除 | 重构后移除                                                   |
| `workbench/ui/components/panels/QueryEditorPanel.vue`    | 新增 | 查询编辑器面板                                               |
| `workbench/ui/components/panels/QueryToolbar.vue`        | 新增 | 查询工具栏                                                   |
| `workbench/ui/components/panels/QueryStatusBar.vue`      | 新增 | 查询状态栏                                                   |
| `workbench/ui/components/panels/AnalysisEditorPanel.vue` | 新增 | 分析编辑器面板                                               |
| `workbench/ui/components/panels/AnalysisToolbar.vue`     | 新增 | 分析工具栏（含联邦查询选择器）                               |
| `workbench/ui/components/panels/AnalysisStatusBar.vue`   | 新增 | 分析状态栏                                                   |
| `workbench/ui/components/panels/CodeEditorPanel.vue`     | 新增 | 代码编辑器面板                                               |
| `workbench/ui/components/panels/CodeToolbar.vue`         | 新增 | 代码工具栏                                                   |
| `workbench/ui/components/panels/CodeStatusBar.vue`       | 新增 | 代码状态栏（LSP 诊断）                                       |
| `workbench/ui/composables/useSqlExecution.ts`            | 修改 | 新增 executionMode / estimateRowCount / executeSmart         |
| `workbench/ui/composables/useFederationQuery.ts`         | 新增 | 联邦查询管理                                                 |
| `workbench/ui/types/result.ts`                           | 修改 | 新增 columnTypes                                             |
| `query/ui/services/query.ts`                             | 修改 | 消除 as unknown as，新增 executeSqlPaginated                 |
| `src-tauri/src/core/models.rs`                           | 修改 | 新增 column_types() + Serialize 修改                         |
| `src-tauri/src/commands/sql_commands.rs`                 | 修改 | 注册 specta，新增 execute_sql_paginated，修复 unwrap         |

---

## 四、依赖关系

```
Step 1 (类型定义)
  ├── Step 2 (EditorManager 拆分)
  ├── Step 5 (分页查询 ── 后端)
  └── Step 9 (执行模式)
         └── Step 3 (IPC 类型安全)
Step 2 ──→ Step 7 (面板工厂)
              ├── Step 8 (查询编辑器)
              ├── Step 10 (分析编辑器)
              └── Step 11 (代码编辑器)
Step 4 (column_types) ── 独立可并行
Step 6 (unwrap 修复) ── 独立可并行
Step 12 (空 catch) ── 独立可并行
所有 Step ──→ Step 13 (端到端验证)
```

---

## 五、风险与注意事项

1. **拆分 EditorManager 时需渐进式迁移**：先创建新模块 → 保持旧 `EditorManager` 作为 re-export 代理 → 全量替换引用 → 删除旧文件。避免一次性删除导致大面积编译错误。
2. **DuckDB ATTACH 零拷贝依赖后端支持**：`execute_duckdb_accelerated` 当前已实现 ATTACH/DETACH，确认无回归。
3. **LSP 仅预留扩展点**：不做实际 LSP 客户端实现，只定义接口契约。
4. **分页查询性能**：当前 `to_rows()` 全量转换后再 slice，后续可优化为 Arrow slice 在序列化前执行。
5. **不破坏现有 dockview 面板注册**：`EditorPanelFactory.vue` 作为 dockview 组件注册，替代原 `EditorPanel`。
