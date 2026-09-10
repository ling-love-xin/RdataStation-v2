# -*- coding: utf-8 -*-
"""追加 Round 26 节到迁移文档（锚点替换）。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\docs\migration\v1-to-v2-mapping.md')
t = p.read_text(encoding='utf-8')

marker = '''### ⏳ 下一轮候选

- GPUI 视图层：workbench 主界面（数据库导航/连接管理/资源分析）首个可交互视图；
- mock 命令装配（mock_commands → GPUI 或 CLI 入口）；
- v1 源文件清理决策（v1/ 暂存区保留 vs 按 Feature 删除）。
'''

round26 = '''### ✅ Round 26（已完成，M5 查询工作台第一步：SQL 执行 + 结果集渲染）

**目标**：启动 M5 查询工作台核心——选中连接（DuckDB 联邦开启）时，「SQL 查询」区可输入 SQL 并对分析引擎库真实执行，结果集以列头 + 行表格渲染。至此工作台形成完整闭环：**连接（M3）→ 导航（M4）→ 查询（M5）**。

**探查结论**：
- engine `DuckDbDatabase::query`（Database trait）返回的 `QueryResult` 中 **`rows` 字段从不填充**（数据只在 Arrow `batches`，`..Default::default()` 留空 rows）——**不能直接用 result.rows**。
- `duckdb::types::Value` **无 Display**（20+ 变体）；engine `duckdb_service::duckdb_value_to_json`（pub fn）把 duckdb Value → `serde_json::Value`，可复用。
- **实现改用 duckdb-rs 直连**（workbench 已依赖 `duckdb = 1.10502.0`）：`Connection::open → prepare → query → column_name`，值经 `duckdb_value_to_json` 转字符串——**同步执行，无需 Runtime**。

**新增/改动**：

| 文件 | 内容 |
| --- | --- |
| `crates/workbench/src/services/query_runner.rs` | 新增 `execute_sql(duckdb_path, sql) -> Result<QueryOutput, String>`：SQL 非空校验 → duckdb::Connection::open → prepare → query → 收集行（`row.get::<usize, Value>` 循环至 Err 断）→ **query 后**取列名 → 值转字符串（String 去引号、NULL → "NULL"） |
| `crates/workbench/src/services/mod.rs` | 注册 `query_runner` 模块 |
| `crates/workbench/src/panels.rs` | EditorPanel 加 `sql_input`（受控 Input）+ `query_result`（Rc<RefCell<Option<QueryOutput>>>）；「SQL 查询（DuckDB 分析库）」区：Input + 「执行」按钮 → execute_sql(global.duckdb) → 结果表格（列头 + 行，定宽截断）；DDL/无返回显示"无结果" |
| `crates/workbench/tests/query_runner.rs` | 新增 3 集成测试：SELECT 读回列/行断言、空 SQL 报错、非法 SQL 报错 |

**本轮踩坑修复（已记录，后续直接复用）**：
- **duckdb-rs 列元数据时序**：`column_count()/column_name()` 必须在 `stmt.query([])` **之后**调用（"The statement was not executed yet"）；且 `Rows` 持有 stmt 可变借用，需**先收集完数据再读列名**（块作用域 drop rows）。
- **column_name 返回 `Result<&String, Error>`**（引用）——`map(|v| v.to_string())` 再 `unwrap_or_else`，不能直接 `unwrap_or_else(|_| String)`。
- **serde_json 字符串带引号**：`duckdb_value_to_json` 的 Text → JSON `"paid"`——`match String(s) => s` 去引号；Null → "NULL"。
- **Rc 非 Copy**：执行按钮闭包 move `query_result` 后渲染又借用 → 闭包用独立 `qr_closure = query_result.clone()`。

**验证**：workbench 18/18 通过（8 lib + 2 navigator + 3 query_runner + 5 real_connections）；**全仓 401 测试 0 失败**（398 → 401）；`cargo build -p rds-app` + 运行验证进程稳定（SQL 区渲染无 panic）。

'''

assert marker in t, 'marker not found'
t = t.replace(marker, round26 + marker)
p.write_text(t, encoding='utf-8')
print('Round 26 appended, total lines:', len(t.splitlines()))
