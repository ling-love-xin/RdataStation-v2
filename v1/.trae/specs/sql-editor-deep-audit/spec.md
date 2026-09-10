# SQL 编辑器深度审计与优化 Spec

## Why
SQL 编辑器是 RdataStation 的核心交互界面。当前实现覆盖了基础执行和三种执行模式，但存在执行计划/方言转译功能缺失、前后端历史系统双轨、查询取消机制不完善、缺失 schema 感知自动补全、批量执行取消不可靠等关键缺陷，需要系统性优化。

## What Changes
- **修复阻塞性缺陷**: 执行计划 (Explain) 和方言转译 (Transpile) 两个 TODO stub 功能实现
- **统一历史系统**: 前端 localStorage 历史与后端 SQLite 历史合并为单一后端驱动
- **完善查询取消**: 前端 AbortController 信号传递到后端 `query_with_cancel`
- **新增 schema 感知补全**: CodeMirror 6 自定义 CompletionSource，基于 MetadataBrowser 获取表/列信息
- **优化批量执行**: 批量执行支持逐语句取消，错误边界不中断后续
- **补齐游标位置同步**: QueryStatusBar 从 CodeMirror 实时获取 cursor/行号/列号
- **修复数据完整性**: 执行计划按钮/方言转译/列类型元数据贯通
- **性能优化**: DuckDB 加速响应保持 Arrow batch、智能模式避免 COUNT(*) 全表扫描、分页查询支持后端原生分页
- **BREAKING**: 无（所有改动为增量增强）

## Impact
- Affected specs: `multi-mode-editor`（共享 EditorBody/Toolbar 重构，本 spec 专注功能实现而非架构拆分）
- Affected code:
  - **前端 `QueryEditorPanel.vue`**: 修补 handleExplain / handleTranspile / cursorPosition 同步
  - **前端 `useSqlExecution.ts`**: AbortController 连通后端、批量执行取消增强
  - **前端 `sql-editor-service.ts`**: 新增 schema 补全 provider、修复 DuckDB 调用签名
  - **前端 `sql-history-service.ts`**: localStorage → 后端 Tauri invoke 替换
  - **前端 `services/cm-sql-extensions.ts`**: 扩展 lintSource/completionSource
  - **前端 `ResultTable.vue`**: columnTypes 驱动列渲染优化
  - **后端 `sql_commands.rs`**: execute_duckdb_accelerated 响应保留 Arrow batch、新增 explain 命令
  - **后端 `sql_service.rs`**: 修复 get_transaction_status 假实现、cancel 信号通道
  - **后端 `result_service.rs`**: 新增 column_types 字段序列化

---

## ADDED Requirements

### Requirement: 执行计划 (Explain) 功能
系统 SHALL 支持向数据库发送 EXPLAIN 查询并展示查询计划结果。

#### Scenario: 点击执行计划按钮
- **GIVEN** 编辑器中有 SELECT 语句 `SELECT * FROM users WHERE id = 1`
- **WHEN** 用户点击工具栏"执行计划"按钮
- **THEN** 后端自动拼接 `EXPLAIN SELECT * FROM users WHERE id = 1`，执行后在新结果标签页展示（标题含 "[Explain]" 前缀）

#### Scenario: 非 SELECT 语句的执行计划
- **GIVEN** 编辑器中有 `UPDATE users SET name = 'test'`
- **WHEN** 用户点击执行计划
- **THEN** 后端对支持的数据库发送 `EXPLAIN UPDATE ...`，不支持时返回错误提示 "执行计划仅支持 SELECT/INSERT/UPDATE/DELETE"

---

### Requirement: SQL 方言转译
系统 SHALL 通过 sqlglot-rust 后端支持 SQL 方言互转。

#### Scenario: MySQL → PostgreSQL 转译
- **GIVEN** 编辑器中有 MySQL SQL: `SELECT IFNULL(name, 'N/A') FROM users`
- **WHEN** 用户在方言选择器中选择目标方言 "PostgreSQL" 并点击转译
- **THEN** 编辑器内容替换为 `SELECT COALESCE(name, 'N/A') FROM users`

#### Scenario: 转译失败降级
- **GIVEN** SQL 包含目标方言不支持的特性
- **WHEN** 转译失败
- **THEN** 弹出 warning toast "部分语法无法转译，已保留原始 SQL"，编辑器内容不变

---

### Requirement: 前后端统一 SQL 历史系统 ✅ DONE (v0.5.10)
SQL 历史记录的增删查改 SHALL 统一由后端 Rust API 驱动，前端不再维护独立的 localStorage 副本。

#### Scenario: 执行后历史自动记录
- **GIVEN** 用户执行了一条 SQL
- **WHEN** 后端 `sql_service.execute()` 返回成功
- **THEN** 后端自动调用 `history_store::save_sql_history()` 写入 SQLite，前端刷新历史列表时从后端拉取

#### Scenario: 前端查询历史列表
- **GIVEN** 用户打开历史面板
- **WHEN** 面板挂载时
- **THEN** 调用 `get_sql_history` Tauri 命令获取最近 100 条记录，按时间倒序展示

#### Scenario: 收藏/标签/备注持久化
- **GIVEN** 用户为某条历史记录添加标签 "daily-report"
- **WHEN** 用户保存标签
- **THEN** 标签持久化到后端，下次加载历史时恢复

---

### Requirement: Schema 感知 SQL 自动补全
CodeMirror 6 SHALL 注册自定义 CompletionSource，根据当前连接的活动 schema 提供表名/列名/视图名/函数名补全。

#### Scenario: 表名补全 (FROM 后)
- **GIVEN** 用户连接了 MySQL 数据库，schema 为 `public`，有表 `users`, `orders`
- **WHEN** 用户输入 `SELECT * FROM ` 后触发自动补全
- **THEN** 下拉列表显示 `users`, `orders` 及该 schema 下所有表

#### Scenario: 列名补全 (SELECT 后 / WHERE 条件中)
- **GIVEN** 用户输入 `SELECT ` 或 `WHERE ` 或在已引用表别名后输入 `.`
- **WHEN** 触发自动补全
- **THEN** 下列列表包含当前上下文可用的列名（从 Arrow Schema 或 MetadataBrowser 获取）

#### Scenario: 关键字补全（无连接时降级）
- **GIVEN** 未连接任何数据库
- **WHEN** 触发自动补全
- **THEN** 降级为 SQL 关键字补全（SELECT/FROM/WHERE/JOIN/GROUP BY 等）和方言通用函数

#### Scenario: 缓存与 TTL
- **GIVEN** schema 已加载到前端缓存
- **WHEN** 用户在同一会话中再次触发补全
- **THEN** 直接使用缓存（TTL 30s），不重复请求后端

---

### Requirement: 查询取消（完整链路）
前端的 AbortController SHALL 传递取消信号到后端数据库驱动的 `query_with_cancel`。

#### Scenario: 用户主动取消
- **GIVEN** 用户执行了一个长时间运行的查询
- **WHEN** 用户点击工具栏"取消"按钮
- **THEN** 前端 `abortController.abort()` → 后端 `cancel_token.cancel()` → 驱动层 `query_with_cancel` 中断查询 → 连接释放

#### Scenario: 超时取消
- **GIVEN** 查询配置了 30 秒超时
- **WHEN** 30 秒后查询仍在执行
- **THEN** 后端 `tokio::time::timeout` 触发 → cancel_token 标记取消 → 返回 `QueryTimeout` 错误 → 前端显示 "查询超时"

---

### Requirement: 游标位置与状态栏同步
QueryStatusBar SHALL 实时展示 CodeMirror 游标的行号/列号/选中字符数。

#### Scenario: 游标移动
- **GIVEN** 用户在编辑器中移动光标到第 42 行第 10 列
- **WHEN** CodeMirror 触发 `cursorActivity` 事件
- **THEN** StatusBar 更新为 `Ln 42, Col 10`

#### Scenario: 文本选中
- **GIVEN** 用户选中 3 行共 200 个字符
- **WHEN** 选中事件触发
- **THEN** StatusBar 更新为 `Ln 10-12, Col 1-40 (200 selected)`

---

### Requirement: 批量执行取消增强
`executeBatch()` SHALL 支持在中途取消时立即停止后续语句。

#### Scenario: 批量执行中取消
- **GIVEN** 用户执行了含有 10 条语句的批量 SQL
- **WHEN** 第 3 条语句执行完成后用户点击取消
- **THEN** 第 4-10 条语句被跳过，已执行的 3 条结果保留在结果面板中

---

### Requirement: 分页查询后端原生支持
分页查询 SHALL 由数据库层面支持 LIMIT/OFFSET 而非后端全量查询后切片。

#### Scenario: 用户翻页
- **GIVEN** 查询返回 5000 行
- **WHEN** 用户点击"下一页"
- **THEN** 后端重新执行 `SELECT * FROM (...) LIMIT 100 OFFSET 100`（利用数据库引擎分页），而非全量查询后内存切片

---

## MODIFIED Requirements

### Requirement: QueryResult 新增 column_types 字段
`QueryResult::serialize()` SHALL 附带回传列类型元数据。  
（已在 `multi-mode-editor` spec 定义，此处仅为实现验证点）

### Requirement: DuckDB 加速执行响应保持 Arrow batch
`execute_duckdb_accelerated` 命令 SHALL 在响应中直接返回 Arrow RecordBatch 序列化数据，而非逐行转为 JSON 的 `Vec<Vec<serde_json::Value>>`（性能优化）。

### Requirement: 修复 get_transaction_status 假实现
`SqlService::get_transaction_status()` SHALL 从连接会话真实查询事务状态，而非硬编码返回 `is_in_transaction: false`。

### Requirement: 前端历史 localStorage → 后端 Tauri invoke ✅ DONE (v0.5.10)
`sql-history-service.ts` 的所有 `localStorage` 读写 SHALL 替换为 `invoke('get_sql_history')` / `invoke('remove_sql_history')` / `invoke('clear_sql_history')` / `invoke('search_sql_history')`。

### Requirement: 消除 `L598 panic!` 
[sql_service.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/services/sql_service.rs#L598) 中的 `panic!("expected Ok")` SHALL 改为 `assert!` 或 `unreachable!` 带上下文。

---

## REMOVED Requirements

### Requirement: 前端 localStorage 历史存储 ✅ REMOVED (v0.5.10)
**Reason**: 与后端 SQLite 历史双轨运行，数据不一致  
**Migration**: 所有历史读写改为 Tauri invoke 调用后端 API，localStorage key `sql-execution-history` 废弃，首次迁移时可读取旧数据导入后端（收藏/标签/备注等 UI 元数据迁移至独立 key `sql-history-meta`）