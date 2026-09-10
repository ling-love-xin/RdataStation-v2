# Tasks

## Phase 1: 基础设施 — 类型定义与核心拆分

- [ ] Task 1: 定义 EditorType 枚举和面板工厂接口
  - 在 `workbench/types/editor-types.ts` 中定义 `EditorType` 枚举（`query` | `analysis` | `code`）
  - 定义 `EditorToolbarConfig` 接口（按钮组/分隔符/可见性条件）
  - 定义 `EditorStatusBarConfig` 接口（字段列表/格式化函数）
  - 定义 `EditorModeResolver`：根据文件扩展名返回默认 `EditorType`（.sql→query, .rs→code 等）

- [ ] Task 2: 拆分 EditorManager.ts 为独立模块
  - 抽离 `FileStateStore` (Pinia)：管理 `openFiles` / `activeFilePath` / `isDirty` / 文件开关闭
  - 抽离 `EditorInstanceRegistry`：管理 `EditorView` 实例注册/查询/主实例判定/状态保存恢复
  - 抽离 `ResultSetManager`：管理 `resultSets` 生命周期/面板绑定/MAX_RESULT_SETS 上限+toast 提示
  - 抽离 `DockviewBridge`：封装 dockview API（addPanel/movePanelOrGroup/getPanel）
  - 删除原 `EditorManager.ts`，逐模块替换引用

- [ ] Task 3: 修复 frontend ↔ backend IPC 类型安全
  - 消除 `query.ts` 中所有 `as unknown as` 双重强转
  - 确保 specta 生成的 `ExecuteSqlResponse` / `TransactionStatusResponse` 类型与前端实际使用一致
  - 注册 `execute_duckdb_accelerated` 到 specta `collect_commands!` 宏
  - 为 `DuckDBAcceleratedResult` 使用 specta 生成的类型而非手写 interface

## Phase 2: 后端 — QueryResult 与分页

- [ ] Task 4: QueryResult 新增 `column_types` 字段
  - 在 `models.rs` 中为 `QueryResult` 实现 `column_types()` 方法，从 Arrow Schema 字段提取类型名称
  - 修改 `Serialize` 实现，追加 `column_types` 字段序列化
  - 前端 `result.ts` 中 `QueryResult` / `ResultTab` 新增 `columnTypes: string[]` 字段

- [ ] Task 5: 后端新增分页查询命令
  - 新增 `execute_sql_paginated` Tauri 命令，参数包含 `offset: u32` / `limit: u32`
  - 复用 `sql_service.execute()`，返回结果前执行 `result.slice(offset, limit)` 行级切片
  - 响应中 `total_rows` 返回原始总行数而非分页后行数
  - 前端 `query.ts` 新增 `executeSqlPaginated()` 函数

- [ ] Task 6: 修复 `unwrap_or` / `unwrap` 违规
  - `sql_commands.rs:L443` 的 `unwrap_or(Value::Null)` 改为 `.map_or(Value::Null, Value::Number)`
  - `sql_commands.rs:L458` 的 `unwrap_or_default()` 改为 `unwrap_or_else(|| "?".to_string())` 或类似安全处理

## Phase 3: 前端 — 面板工厂与三种编辑器

- [ ] Task 7: 创建 EditorPanelFactory.vue（面板路由组件）
  - 从 `props.params` 或 EditorModeResolver 获取当前 `EditorType`
  - 根据 `EditorType` 动态渲染 `QueryEditorPanel` / `AnalysisEditorPanel` / `CodeEditorPanel`
  - 共享 `EditorBody`（CodeMirror 容器 + 标签栏 + 结果面板宿主元素）
  - 替换原 `EditorPanel.vue` 在 dockview 中的注册

- [ ] Task 8: 创建 QueryEditorPanel.vue（查询编辑器）
  - 组装 QueryToolbar + EditorBody + QueryStatusBar
  - QueryToolbar 包含：执行/新标签执行/本地加速/执行计划/格式化/方言转换/执行模式切换/事务按钮/历史
  - QueryStatusBar 从 `useSqlExecution` 实时读取 `lastExecutionTime` / `inTransaction` / `statementCount` / `columnTypes`
  - 保留现有 `ResultSubTab` + `QueryResultPanel` 结果面板集成
  - 注册快捷键 Ctrl+Enter（执行）/ Ctrl+Shift+Enter（新标签执行）

- [ ] Task 9: 实现三种执行模式（普通/分析/智能）
  - `useSqlExecution` 新增 `executionMode` ref：`normal` | `analysis` | `smart`
  - 普通模式：调用 `executeSql()`（现有逻辑）
  - 分析模式：调用 `executeDuckDBAccelerated()`，执行前自动 ATTACH 源数据库
  - 智能模式：先调用 `estimateRowCount()`（发送 `EXPLAIN` 或 `COUNT(*)`），根据阈值选择普通/分析模式
  - QueryToolbar 中执行模式切换为三选一 `NButtonGroup` 单选组

- [ ] Task 10: 创建 AnalysisEditorPanel.vue（分析编辑器）
  - 组装 AnalysisToolbar + EditorBody + AnalysisStatusBar
  - AnalysisToolbar 新增"联邦查询选择器"：从 `useFederationQuery` composable 获取已 ATTACH 的外部数据源列表，多选下拉
  - 执行按钮仅调用 DuckDB 引擎（`executeDuckDBAccelerated`）
  - 联邦查询选择后自动生成 `ATTACH '...' AS db_name` 语句前缀提示

- [ ] Task 11: 创建 CodeEditorPanel.vue（代码编辑器）
  - 组装 CodeToolbar + EditorBody + CodeStatusBar
  - CodeToolbar 仅保留保存/格式化/查找替换等通用编辑按钮，无执行/SQL 相关按钮
  - CodeStatusBar 预留 LSP 诊断计数、语言模式、编码、缩进显示
  - 定义 `LspExtensionPoint` 接口（diagnostics / completions / hover 回调），不做实现

## Phase 4: 收尾与验证

- [ ] Task 12: 统一消除空 catch 吞没
  - 遍历所有 Editor 相关文件，将 `try { ... } catch { /* */ }` 替换为 `try { ... } catch { console.warn('[Module]', e) }`
  - 至少覆盖 `EditorPanel.vue` / `EditorManager.ts` 拆分后的模块

- [ ] Task 13: 端到端验证
  - 打开 `.sql` 文件 → 确认进入 QueryEditorPanel → 执行 SELECT → 结果面板正常显示
  - 切换到分析编辑器 → 确认工具栏和状态栏切换 → DuckDB 加速执行正常
  - 切换到代码编辑器 → 确认无 SQL 按钮 → 保存正常
  - 打开 `.rs` 文件 → 确认自动进入 CodeEditorPanel
  - 验证 `column_types` 在结果面板中可用
  - 验证分页查询返回正确行数

# Task Dependencies

- Task 2 依赖 Task 1（EditorType 定义后才能拆分）
- Task 7 依赖 Task 2（面板工厂需要拆分后的模块）
- Task 8/10/11 依赖 Task 7（子面板依赖面板工厂）
- Task 9 依赖 Task 5/6（智能模式依赖分页命令）
- Task 13 依赖 Task 1-12（全链路）
- Task 3/4/5/6 可并行（后端与前端 IPC 独立）
- Task 8/10/11 可并行（三种子面板独立）
