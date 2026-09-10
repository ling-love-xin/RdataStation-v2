# Checklist

## 阻塞性缺陷修复

- [ ] 点击"执行计划"按钮 → 后端拼接 EXPLAIN → 新标签展示计划结果
- [ ] 非 DML 语句执行计划返回友好错误提示
- [ ] 方言选择器显示 MySQL/PostgreSQL/SQLite/DuckDB/Generic 五种方言
- [ ] 方言转译成功时编辑器内容正确替换
- [ ] 方言转译失败时 toast warning + 原始 SQL 不变

## 查询取消完整链路

- [ ] 前端取消按钮点击后 `abortController.abort()` 触发
- [ ] 后端 `cancel_token` 收到取消信号后终止数据库查询
- [ ] 连接释放后立即可执行新 SQL（< 1 秒恢复）
- [ ] 批量执行取消：已执行语句结果保留，未执行语句标记 "已取消"
- [ ] 批量执行取消后 `executing` 状态立即恢复 false

## Schema 感知自动补全

- [ ] `FROM` / `JOIN` 后触发 → 展示表名列表
- [ ] `SELECT` 后 / `WHERE` 后触发 → 展示列名列表
- [ ] 无连接时降级为 SQL 关键字静态补全
- [ ] 补全缓存 TTL 30s 内不重复请求后端
- [ ] `get_schema_completions` Tauri 命令已注册 specta

## 状态栏

- [ ] 光标移动时 StatusBar 实时显示 `Ln {line}, Col {col}`
- [ ] 选中文本时 StatusBar 显示选中范围 `(n selected)`

## 历史系统统一

- [ ] `getHistory()` 从 `invoke('get_sql_history')` 获取
- [ ] `addHistory()` 不再调用（后端自动记录）
- [ ] `deleteHistory()` / `clearHistory()` / `searchHistory()` 调用后端 API
- [ ] 前端 `localStorage key 'sql-execution-history'` 不再读写
- [ ] 历史面板加载正常展示最新记录

## 后端修复

- [ ] `get_transaction_status` 从 session 真实获取事务状态
- [ ] `sql_service.rs` L598 无 `panic!()`
- [ ] `format_arrow_value` 不包含 14 个重复的 `as_array!` 宏调用

## 性能优化

- [ ] DuckDB 加速执行响应使用 Arrow IPC 而非 JSON `Vec<Vec<Value>>`
- [ ] `execute_sql_paginated` 通过 LIMIT/OFFSET SQL 包装实现原生分页
- [ ] `QueryResult` 序列化包含 `column_types` 字段
- [ ] `ExecuteSqlResponse` 包含 `column_types: Vec<String>`
- [ ] `ResultTable.vue` 数字列右对齐、布尔列 checkbox 渲染

## 代码质量

- [ ] `pnpm run lint` 零错误
- [ ] `pnpm run format` 通过
- [ ] `cargo clippy -- -D warnings` 通过
- [ ] `cargo fmt` 通过
- [ ] 无新增 `any` 类型
- [ ] 无新增 `unwrap()` / `expect()` 在生产代码
