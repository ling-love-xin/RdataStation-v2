# 多模式编辑器面板 Spec

## Why

当前编辑器仅支持单一的 SQL 查询模式，无法根据使用场景（即席查询 vs 数据分析 vs 代码开发）提供差异化的工具栏和交互。需要重构为类似 VSCode 的可插拔面板架构，支持三种编辑面板类型，并优化查询执行的数据链路。

## What Changes

- 重构 EditorPanel 为面板工厂架构，按文件语言/类型动态加载不同的 Toolbar + StatusBar 组合
- 新增三种编辑器模式：查询编辑器、分析编辑器、代码编辑器
- 查询编辑器新增三种执行模式：普通模式（Arrow→JSON）、分析模式（零拷贝 DuckDB）、智能模式（动态选择）
- 分析编辑器新增联邦查询选择器，纯 DuckDB 引擎处理
- 代码编辑器预留 LSP（Language Server Protocol）扩展点
- 优化 IPC 数据链路：按需分页传输，附带列类型元数据
- **BREAKING**: EditorManager.ts 拆分为 editor-state / file-manager / result-set-manager / instance-service / cross-window-service 五个模块，EditorManager 转为门面
- **DONE** (2026-05-31): 删除死代码 EditorInstanceRegistry.ts
- **DONE** (2026-05-31): EditorPanelParams 统一类型接口替换所有 `Record<string, unknown>`
- **DONE** (2026-05-31): EditorPanelFactory 改用 `computed` 响应式解析编辑器类型
- **DONE** (2026-05-31): openAnalysisPanel 统一走 scratchpad API 持久化
- **DONE** (2026-06-09): 文件 CRUD 完善 — 脏关闭拦截 + 新建未保存文件 + 外部变更检测 + 多窗口冲突解决
  - OpenFileInfo 新增 `exists: boolean`, `lastModifiedAt: number | null` 字段
  - `file-manager.ts` 新增 `newFile()`, `closeFileChecked()`, `closeFilesChecked()`, `saveCurrentFileToDisk()`, `saveFileAs()`, `checkExternalFileChanges()`, `reconcileCrossWindowConflict()` 7 个函数
  - 新增 `useFileDialogs.ts` — 提供 `confirmUnsavedClose()` / `confirmExternalChange()` / `confirmFileConflict()` 三种 naive-ui 确认对话框
  - `cross-window-service.ts` `listenStateSync` 增加冲突检测逻辑，脏状态时调用 `reconcileCrossWindowConflict`
  - `EditorPanel.vue` 所有关闭路径改用 `closeFileChecked()` 而非直接 `closeFile()`
  - 安装 `@tauri-apps/plugin-fs@2.5.1` npm 依赖用于文件读写
  - 安装 `@tauri-apps/plugin-dialog` (已有) 用于另存为对话框

## Impact

- Affected specs: plugin-system-design（面板贡献点复用）
- Affected code:
  - `src/extensions/builtin/workbench/ui/components/panels/EditorPanel.vue` — 重构为工厂组件
  - ~~`src/extensions/builtin/workbench/manager/EditorManager.ts`~~ — **已拆分为**：
    - `editor-state.ts` — 共享响应式状态（openFiles, activeFilePath, 运行时等）
    - `file-manager.ts` — 文件 CRUD + 脏关闭拦截 + 新建未保存文件 + saveFileAs + 外部变更检测 + 跨窗口冲突解决
    - `result-set-manager.ts` — 结果集管理（createResultSet, detachResultPanel）
    - `instance-service.ts` — 编辑器实例注册（registerFileEditor, getEditorView）
    - `cross-window-service.ts` — 跨窗口同步（popout/merge, state sync）
    - `EditorManager.ts` — 门面聚合 + SQL 执行（499行，was 973行）
  - ~~`src/extensions/builtin/workbench/manager/EditorInstanceRegistry.ts`~~ — **已删除**（死代码，零引用）
  - `src/extensions/builtin/workbench/types/editor-types.ts` — 新增 `EditorPanelParams` 统一接口
  - `src/extensions/builtin/workbench/ui/components/panels/*.vue` — 所有面板 `Record<string,unknown>` → `EditorPanelParams`
  - `src/extensions/builtin/workbench/ui/composables/useSqlExecution.ts` — 新增三种执行模式分支
  - `src/extensions/builtin/query/ui/services/query.ts` — 消除 `as unknown as`，对齐 specta 类型
  - `src-tauri/src/core/models.rs` — QueryResult 序列化新增 `column_types` 字段
  - `src-tauri/src/commands/sql_commands.rs` — 注册 `execute_duckdb_accelerated` 到 specta，新增分页查询命令

---

## ADDED Requirements

### Requirement: 面板工厂架构

系统 SHALL 提供基于文件语言/类型的面板工厂，根据 `EditorType` 枚举动态组装 Toolbar、EditorBody、StatusBar 三区域。

#### Scenario: 打开 .sql 文件默认进入查询编辑器

- **GIVEN** 用户打开一个 `.sql` 文件
- **WHEN** 系统创建编辑器面板
- **THEN** 面板类型为 `query`，工具栏显示执行/格式化/方言转换等按钮，状态栏显示连接名/游标位置/SQL 语句数

#### Scenario: 通过面板类型选择器切换编辑器模式

- **GIVEN** 用户在一个已打开的编辑器中
- **WHEN** 用户点击面板类型切换下拉框（查询编辑器 → 分析编辑器 → 代码编辑器）
- **THEN** 工具栏和状态栏即时切换，编辑器内容保留不变

---

### Requirement: 查询编辑器 — 三种执行模式

查询编辑器（`EditorType::Query`）SHALL 支持普通模式、分析模式、智能模式三种 SQL 执行方式。

#### Scenario: 普通模式执行 SQL

- **GIVEN** 用户编写了一条 SELECT 语句，执行模式为"普通模式"
- **WHEN** 用户点击"执行"按钮
- **THEN** SQL 发送到后端数据库执行，结果通过 Arrow→JSON 序列化后返回，在结果面板中以分页表格展示

#### Scenario: 分析模式执行 SQL（零拷贝 DuckDB）

- **GIVEN** 用户编写了一条聚合分析 SQL，执行模式为"分析模式"
- **WHEN** 用户点击"本地加速执行"按钮
- **THEN** 后端将查询结果零拷贝 ATTACH 到 DuckDB 引擎，后续分析操作直接在 DuckDB 上完成，无需重复网络传输

#### Scenario: 智能模式自动选择执行策略

- **GIVEN** 用户编写了一条 SQL 语句，执行模式为"智能模式"
- **WHEN** 后端预估返回行数 ≤ 阈值（可配置，默认 1000）
- **THEN** 使用普通模式执行
- **WHEN** 后端预估返回行数 > 阈值
- **THEN** 自动切换为分析模式，将数据零拷贝到 DuckDB

---

### Requirement: 查询编辑器工具栏

查询编辑器工具栏 SHALL 包含以下按钮组：

| 分组 | 按钮                            | 行为                        |
| ---- | ------------------------------- | --------------------------- |
| 执行 | 执行 (Ctrl+Enter)               | 执行当前 SQL 或选中 SQL     |
| 执行 | 新标签页执行 (Ctrl+Shift+Enter) | 在新结果标签页中执行        |
| 执行 | 本地加速执行                    | DuckDB 零拷贝分析模式       |
| 执行 | 执行计划 (Explain)              | 发送 EXPLAIN 获取查询计划   |
| 编辑 | 格式化 SQL                      | 通过 sqlglot 格式化         |
| 编辑 | 方言转换                        | 弹出方言选择器，转换 SQL    |
| 模式 | 执行模式切换                    | 普通/分析/智能 三选一单选组 |
| 事务 | 开始/提交/回滚事务              | 事务控制按钮组              |
| 历史 | SQL 历史                        | 打开历史面板                |

#### Scenario: 点击执行计划按钮

- **GIVEN** 编辑器中有 SQL 语句
- **WHEN** 用户点击"执行计划"按钮
- **THEN** 发送 `EXPLAIN <sql>` 到后端，结果以树形或文本形式展示在新结果标签页中

#### Scenario: 点击方言转换按钮

- **GIVEN** 编辑器中有 MySQL 方言 SQL
- **WHEN** 用户选择目标方言为 PostgreSQL
- **THEN** 通过 sqlglot 转译 SQL，替换编辑器内容

---

### Requirement: 分析编辑器

分析编辑器（`EditorType::Analysis`）SHALL 以 DuckDB 为唯一执行引擎，支持联邦查询。

#### Scenario: 联邦查询选择

- **GIVEN** 用户在分析编辑器中
- **WHEN** 用户点击工具栏中的"联邦查询"下拉框
- **THEN** 展示已注册的外部数据库连接列表（ATTACH 的数据源），用户可多选以确定查询范围

#### Scenario: 分析编辑器执行

- **GIVEN** 用户在分析编辑器中编写了 DuckDB SQL（可包含 ATTACH 的外部表引用）
- **WHEN** 用户点击执行
- **THEN** SQL 直接发送到本地 DuckDB 引擎执行，不经过远程数据库

---

### Requirement: 代码编辑器

代码编辑器（`EditorType::Code`）SHALL 为后续语言服务器协议（LSP）提供扩展点。

#### Scenario: 代码编辑器打开 .rs 文件

- **GIVEN** 用户打开 `.rs` 文件
- **WHEN** 系统创建编辑器面板
- **THEN** 面板类型为 `code`，工具栏无执行按钮，仅保留保存/格式化等通用操作，预留 LSP 诊断/补全/悬停提示扩展点

#### Scenario: LSP 诊断显示

- **GIVEN** 代码编辑器已绑定语言服务器
- **WHEN** 语言服务器返回诊断信息（错误/警告）
- **THEN** 编辑器在对应行显示波浪线标记，状态栏显示诊断计数

---

### Requirement: IPC 分页传输与列类型元数据

后端 QueryResult 序列化 SHALL 支持分页参数并附带列类型信息。

#### Scenario: 分页查询

- **GIVEN** 查询返回 5000 行数据
- **WHEN** 前端请求第 2 页（每页 100 行，offset=100）
- **THEN** 后端仅序列化第 101-200 行，`total_rows` 仍返回 5000

#### Scenario: 列类型元数据

- **GIVEN** 查询返回列 `id INT, name VARCHAR, score FLOAT`
- **WHEN** 结果序列化
- **THEN** 响应中包含 `column_types: ["INTEGER", "TEXT", "FLOAT"]`，前端据此决定排序策略和输入控件类型

---

### Requirement: EditorManager 拆分

EditorManager.ts SHALL 拆分为以下独立模块：

| 模块                     | 职责                                        |
| ------------------------ | ------------------------------------------- |
| `FileStateStore` (Pinia) | 文件打开/关闭/脏状态/激活路径               |
| `EditorInstanceRegistry` | EditorView 实例注册/查询/主实例判定         |
| `ResultSetManager`       | 结果集生命周期/面板绑定/数量限制            |
| `DockviewBridge`         | dockview API 封装/面板移动/浮动             |
| `EditorModeResolver`     | 根据文件扩展名/语言/用户选择解析 EditorType |

#### Scenario: 结果集上限提示

- **GIVEN** 某文件已有 5 个结果集（MAX_RESULT_SETS）
- **WHEN** 新结果集产生
- **THEN** 最旧的结果集被移除，同时显示 toast 提示"已移除最早的结果集"

---

## MODIFIED Requirements

### Requirement: EditorPanel.vue 重构为面板工厂

当前 EditorPanel.vue SHALL 重构为 `EditorPanelFactory.vue`，根据 `EditorType` 动态渲染对应的子面板组件：

```
EditorPanelFactory.vue
├─ QueryEditorPanel.vue    → 查询编辑器（含查询工具栏 + 状态栏）
├─ AnalysisEditorPanel.vue → 分析编辑器（含 DuckDB 工具栏 + 联邦查询选择器）
└─ CodeEditorPanel.vue     → 代码编辑器（含 LSP 工具栏 + 诊断状态栏）
```

三者共享 `EditorBody`（CodeMirror 容器 + 标签栏 + 结果面板），仅 Toolbar 和 StatusBar 不同。

### Requirement: QueryResult 序列化变更

`QueryResult::serialize()` SHALL 新增 `column_types` 字段，并支持可选的 `offset`/`limit` 分页参数：

```rust
// 新增
state.serialize_field("column_types", &self.column_types())?;
state.serialize_field("total_rows", &self.total_rows())?;
```

`column_types()` 方法从 Arrow Schema 中提取每列的 Rust 类型名称（`INTEGER`, `FLOAT`, `TEXT`, `BOOLEAN`, `NULL`, `BYTES`）。

---

## REMOVED Requirements

### Requirement: 旧 EditorPanel 中的硬编码状态栏假值

**Reason**: 当前 `statusbarProps` 中 `lastExecutionTime: null`, `inTransaction: false`, `statementCount: 0` 为硬编码假值，从未与 `useSqlExecution` 同步。
**Migration**: 重构后的各编辑器面板从对应的 composable 实时获取状态栏数据。
