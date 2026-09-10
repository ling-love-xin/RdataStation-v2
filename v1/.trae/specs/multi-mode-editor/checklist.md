# Checklist

## 类型定义与接口

- [ ] `EditorType` 枚举定义在 `editor-types.ts`，包含 `query` / `analysis` / `code` 三个值
- [ ] `EditorToolbarConfig` 接口定义完整（按钮组/分隔符/可见性条件）
- [ ] `EditorStatusBarConfig` 接口定义完整（字段列表/格式化函数）
- [ ] `EditorModeResolver` 能正确将 `.sql` → `query`、`.rs` / `.ts` / `.py` → `code`

## EditorManager 拆分

- [ ] `FileStateStore` (Pinia) 管理文件打开/关闭/脏状态/激活路径
- [ ] `EditorInstanceRegistry` 管理 EditorView 实例注册/主实例判定
- [ ] `ResultSetManager` 管理结果集生命周期，超过 MAX_RESULT_SETS 时 toast 提示
- [ ] `DockviewBridge` 封装 dockview API 调用
- [ ] 原 `EditorManager.ts` 已删除，所有引用已迁移到新模块
- [ ] 无新增 `any` 类型，无 `as unknown as` 强转

## IPC 类型安全

- [ ] `query.ts` 中所有 `as unknown as` 已消除，改用 specta 生成的直接类型
- [ ] `execute_duckdb_accelerated` 已在 `collect_commands!` 中注册 `#[specta::specta]`
- [ ] `DuckDBAcceleratedResult` 使用 specta 生成类型

## 后端 QueryResult

- [ ] `QueryResult::column_types()` 方法已实现，从 Arrow Schema 提取类型名称
- [ ] `Serialize` 实现包含 `column_types` 字段
- [ ] 前端 `ResultTab` 包含 `columnTypes: string[]`
- [ ] `execute_sql_paginated` Tauri 命令已实现（offset/limit 参数）
- [ ] 分页响应中 `total_rows` 为原始总数而非切片后数量

## Rust 代码规范

- [ ] `sql_commands.rs` 中无 `unwrap()` / `unwrap_or()` / `unwrap_or_else()` / `unwrap_or_default()`
- [ ] `sql_service.rs` 中 `unwrap_or(DEFAULT_CONN_KEY)` 已替换为安全替代

## 面板工厂

- [ ] `EditorPanelFactory.vue` 根据 `EditorType` 动态渲染三种子面板
- [ ] EditorBody（CodeMirror + 标签栏 + 结果面板宿主）为三者共享
- [ ] 原 `EditorPanel.vue` 不再作为 dockview 组件注册

## 查询编辑器

- [ ] `QueryEditorPanel.vue` 渲染 QueryToolbar + EditorBody + QueryStatusBar
- [ ] QueryToolbar 包含完整的按钮组（执行/新标签/加速/计划/格式化/方言/模式切换/事务/历史）
- [ ] 快捷键 Ctrl+Enter（执行）和 Ctrl+Shift+Enter（新标签执行）注册正常
- [ ] `useSqlExecution` 支持 `executionMode`：normal / analysis / smart
- [ ] 智能模式能根据行数阈值自动切换普通/分析模式

## 分析编辑器

- [ ] `AnalysisEditorPanel.vue` 渲染 AnalysisToolbar + EditorBody + AnalysisStatusBar
- [ ] 联邦查询选择器展示已 ATTACH 的外部数据源列表
- [ ] 执行按钮仅走 DuckDB（不经过远程数据库）
- [ ] 联邦查询选择后能生成 ATTACH 语句提示

## 代码编辑器

- [ ] `CodeEditorPanel.vue` 渲染 CodeToolbar + EditorBody + CodeStatusBar
- [ ] 工具栏无 SQL 执行相关按钮
- [ ] 状态栏预留 LSP 诊断计数等扩展点
- [ ] `LspExtensionPoint` 接口定义完整（diagnostics / completions / hover）

## 端到端验证

- [ ] 打开 `.sql` 文件 → 默认 QueryEditorPanel
- [ ] 查询编辑器执行 SELECT → 结果面板正确展示
- [ ] 面板类型切换 → 工具栏/状态栏即时切换，内容保留
- [ ] 分析编辑器 DuckDB 加速执行正常
- [ ] 打开 `.rs` 文件 → 自动进入 CodeEditorPanel
- [ ] `column_types` 在结果中正确显示
- [ ] 分页查询返回行数正确
- [ ] `pnpm run lint` 零错误
- [ ] `pnpm run format` 通过
