# Tasks

## Phase 1: 阻塞性缺陷修复 — 执行计划 + 方言转译

- [ ] Task 1: 实现执行计划 (Explain) 功能
  - [ ] 1.1: 前端 `QueryEditorPanel.vue` 的 `handleExplain()` 从 stub 改为实际调用
  - [ ] 1.2: 前端 `useSqlExecution` 新增 `executeExplain(sql, connId)` 方法，Tauri invoke `execute_sql` 时 sql 加 `EXPLAIN` 前缀
  - [ ] 1.3: 结果以新标签页展示，标题 `[Explain] <原始SQL前50字符>`
  - [ ] 1.4: 后端 sql 命令层对非 SELECT/DML 语句的 EXPLAIN 返回明确错误提示

- [ ] Task 2: 实现方言转译 (Transpile) 功能
  - [ ] 2.1: 前端 `QueryToolbar` 新增方言选择下拉框（MySQL/PostgreSQL/SQLite/DuckDB/Generic）
  - [ ] 2.2: `handleTranspile()` 调用 `sql-editor-service.ts` 的 `transpileSql()` ，替换编辑器内容
  - [ ] 2.3: 转译失败时 toast warning + 保持原始 SQL 不变
  - [ ] 2.4: 后端确认 `transpile_sql` specta 命令已注册

## Phase 2: 查询取消完整链路

- [ ] Task 3: 前端 AbortController 连通后端
  - [ ] 3.1: `useSqlExecution.executeSingleStatement()` 中 `abortController` 的 signal 传递给后端（当前仅前端中断，后端继续跑）
  - [ ] 3.2: 后端 `execute_sql` 命令接收 `cancel_signal` Channel 或响应 cancel_token 状态
  - [ ] 3.3: 验证 cancel 后连接立即释放可执行下一条 SQL

- [ ] Task 4: 批量执行取消增强
  - [ ] 4.1: `executeBatch()` 循环中每条语句执行前检查 `abortController.signal.aborted`
  - [ ] 4.2: 已执行的语句结果保留在 ResultStore 中，被跳过的语句标记为 "已取消"
  - [ ] 4.3: 取消后 `executing.value = false` 立即恢复

## Phase 3: Schema 感知自动补全

- [ ] Task 5: 后端新增 schema 元数据查询命令
  - [ ] 5.1: 新增 `get_schema_completions` Tauri 命令，参数 `conn_id`, `schema_name`（可选）
  - [ ] 5.2: 后端调用 `MetadataBrowser::list_tables()` / `list_columns()` 获取补全项列表
  - [ ] 5.3: 命令注册到 specta，生成前端类型

- [ ] Task 6: 前端 CodeMirror CompletionSource
  - [ ] 6.1: `cm-sql-extensions.ts` 新增 `sqlCompletionSource` 函数
  - [ ] 6.2: 在 `FROM` / `JOIN` 关键字后触发表名补全
  - [ ] 6.3: 在 `SELECT` 后 / `WHERE` 条件中触发列名补全
  - [ ] 6.4: 无连接时降级为 SQL 关键字 (100+) 静态补全
  - [ ] 6.5: 缓存补全结果 30s TTL，避免重复请求
  - [ ] 6.6: 注册到 EditorBody 的 CodeMirror extensions 数组中

## Phase 4: 状态栏 + 历史系统 + 数据修复

- [ ] Task 7: 游标位置与状态栏同步
  - [ ] 7.1: `EditorBody` 暴露 `onCursorActivity` 回调，传递 `{line, col, selectedLength}`
  - [ ] 7.2: `QueryEditorPanel` 接收后更新 `cursorPosition` ref
  - [ ] 7.3: `QueryStatusBar` 显示格式 `Ln {line}, Col {col}`，选中时追加 `({n} selected)`

- [ ] Task 8: 统一 SQL 历史系统（前端 localStorage → 后端）
  - [ ] 8.1: `sql-history-service.ts` 所有 `localStorage` 操作替换为 Tauri invoke
  - [ ] 8.2: `addHistory` → invoke `execute_sql` 后端自动记录，前端不用调用
  - [ ] 8.3: `getHistory` → invoke `get_sql_history`
  - [ ] 8.4: `deleteHistory` → invoke `remove_sql_history`
  - [ ] 8.5: `clearHistory` → invoke `clear_sql_history`
  - [ ] 8.6: `searchHistory` → invoke `search_sql_history`
  - [ ] 8.7: 收藏/标签/备注功能确认后端 `SqlHistoryRecord` 支持对应字段，否则追加迁移

## Phase 5: 后端数据修复与性能

- [ ] Task 9: 修复后端缺陷
  - [ ] 9.1: `sql_service.rs` L498-502 `get_transaction_status` 从硬编码 false 改为真实 session 查询
  - [ ] 9.2: `sql_service.rs` L598 `panic!("expected Ok")` 改为 `unreachable!` 或 `assert!`
  - [ ] 9.3: `sql_commands.rs` `format_arrow_value` 14 个 `as_array!` 优化为 `dyn Array` 的 value 方法聚合

- [ ] Task 10: DuckDB 加速执行 Arrow batch 传输
  - [ ] 10.1: `execute_duckdb_accelerated` 响应改为返回 Arrow IPC 序列化 bytes 而非 JSON
  - [ ] 10.2: 前端接收后反序列化 Arrow batch，保持零拷贝

- [ ] Task 11: 分页查询后端原生支持
  - [ ] 11.1: `execute_sql_paginated` 改为重新发送带 LIMIT/OFFSET 的 SQL 而非全量查询后切片
  - [ ] 11.2: 包装原始 SQL: `SELECT * FROM ({original_sql}) LIMIT {limit} OFFSET {offset}`

- [ ] Task 12: QueryResult column_types 字段贯通
  - [ ] 12.1: `QueryResult::column_types()` 方法实现，从 Arrow Schema 提取类型名
  - [ ] 12.2: `ExecuteSqlResponse` 新增 `column_types: Vec<String>` 字段
  - [ ] 12.3: 前端 `ExecutionResult` 和 `ResultTab` 新增 `columnTypes`
  - [ ] 12.4: `ResultTable.vue` 利用 columnTypes 优化数字列右对齐、布尔列 checkbox 渲染

## Task Dependencies

- Task 1/2 可并行（独立的两个功能）
- Task 5 依赖无（后端独立）
- Task 6 依赖 Task 5（schema 补全需要后端命令）
- Task 8 依赖无（后端已有对应 API）
- Task 3/4 可并行（取消链路独立于补全）
- Task 7 依赖无（纯前端 CodeMirror 事件）
- Task 9 可并行（后端独立修复）
- Task 10/11/12 可并行（后端独立优化）
