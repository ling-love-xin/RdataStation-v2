# API 接口文档

> 版本：v4.0
> 最后更新：2026-05-10
> 状态：✅ R25 — 九审审计 | 全量 ~175 命令 | 核心模块+外围基础设施 CoreError 迁移

## 概述

本文档描述 RdataStation 后端提供的所有 Tauri 命令（API 接口）。

> ⚠️ 所有命令入口在 `commands/` 目录（非 `adapters/tauri/command.rs`）。实际注册的命令以 [lib.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/lib.rs) 中的 `invoke_handler` 为准（共 ~175 命令）。

## 命令分类

| 分类          | 命令模块                      | 命令数 |
| ------------- | ----------------------------- | ------ |
| 连接管理      | `connection_commands`         | ~15    |
| 驱动管理      | `driver_commands`             | ~4     |
| SQL 执行      | `sql_commands`                | ~12    |
| 元数据导航 🔗 | `metadata_commands`           | 9      |
| 项目管理      | `project_commands`            | ~12    |
| 项目存储      | `project_store_commands`      | ~8     |
| 元数据缓存    | `metadata_cache_commands`     | ~8     |
| 缓存预热      | `cache_warming_commands`      | ~9     |
| 分析资源      | `analytics_resource_commands` | ~25    |
| 洞察分析      | `insight_commands`            | ~10    |
| 联邦查询      | `federation_commands`         | ~2     |
| 草稿箱        | `scratchpad_commands`         | ~20    |
| SQL 解析      | `sql_parse_commands`          | ~6     |
| 数据导出      | `export_commands`             | ~1     |
| 模拟数据      | `mock_commands`               | ~12    |
| DuckDB 连接池 | `duckdb_pool_commands`        | ~3     |

## 连接管理

### connect_database

创建数据库连接。

**参数**：

```typescript
interface ConnectDatabaseInput {
  db_type: string // 数据库类型: "mysql", "postgresql", "sqlite", "duckdb"
  url: string // 连接 URL
  name?: string // 连接名称（可选）
}
```

**返回**：

```typescript
interface ConnectDatabaseResponse {
  conn_id: string // 连接 ID
  name: string // 连接名称
  db_type: string // 数据库类型
  url: string // 连接 URL
  meta: DataSourceMeta // 数据源元数据
}

interface DataSourceMeta {
  supports_transaction: boolean
  supports_streaming: boolean
  supports_arrow: boolean
  supports_federated: boolean
  supports_concurrent_write: boolean
  is_in_memory: boolean
}
```

**示例**：

```typescript
const result = await invoke('connect_database', {
  input: {
    db_type: 'postgresql',
    url: 'postgres://user:pass@localhost:5432/mydb',
    name: 'My PostgreSQL',
  },
})
// result.conn_id: "conn_abc123"
```

### get_connections

获取所有连接列表。

**参数**：无

**返回**：

```typescript
interface ConnectionInfoResponse {
  id: string
  name: string
  db_type: string
  url: string
  is_active: boolean
  connected_at: string // ISO 8601 格式
}

// 返回: ConnectionInfoResponse[]
```

**示例**：

```typescript
const connections = await invoke('get_connections')
// connections: [{ id: "conn_1", name: "MyDB", ... }]
```

### switch_connection

切换当前活动连接。

**参数**：

```typescript
interface SwitchConnectionInput {
  conn_id: string
}
```

**返回**：`void`

**示例**：

```typescript
await invoke('switch_connection', {
  input: { conn_id: 'conn_abc123' },
})
```

### close_connection

关闭指定连接。

**参数**：

```typescript
{
  conn_id: string
}
```

**返回**：`void`

**示例**：

```typescript
await invoke('close_connection', { conn_id: 'conn_abc123' })
```

### close_all_connections

关闭所有连接。

**参数**：无

**返回**：`void`

### get_active_connection

获取当前活动连接。

**参数**：无

**返回**：

```typescript
interface ActiveConnectionResponse {
  conn_id: string
  name: string
  db_type: string
}
// 或 null（如果没有活动连接）
```

### test_connection

测试数据库连接（不保存）。

**参数**：

```typescript
{
  db_type: string
  url: string
}
```

**返回**：

```typescript
interface TestConnectionResponse {
  success: boolean
  message: string
  server_version: string
  response_time_ms: number
}
```

## SQL 执行

### execute_sql

执行 SQL 查询。

**参数**：

```typescript
interface ExecuteSqlInput {
  conn_id?: string // 连接 ID（可选，使用活动连接）
  sql: string // SQL 语句
  timeout_ms?: number // 超时时间（毫秒，可选）
}
```

**返回**：

```typescript
interface ExecuteSqlResponse {
  columns: string[] // 列名
  rows: Value[][] // 数据行
  affected_rows?: number // 影响的行数（INSERT/UPDATE/DELETE）
  execution_time_ms: number // 执行时间
}

type Value =
  | { type: 'null' }
  | { type: 'string'; value: string }
  | { type: 'int64'; value: number }
  | { type: 'float64'; value: number }
  | { type: 'bool'; value: boolean }
  | { type: 'bytes'; value: number[] }
  | { type: 'date'; value: string }
  | { type: 'time'; value: string }
  | { type: 'datetime'; value: string }
```

**示例**：

```typescript
const result = await invoke('execute_sql', {
  input: {
    conn_id: 'conn_abc123',
    sql: 'SELECT * FROM users WHERE id = $1',
    timeout_ms: 30000,
  },
})

// result:
// {
//   columns: ["id", "name", "email"],
//   rows: [
//     [{ type: "int64", value: 1 }, { type: "string", value: "John" }, ...]
//   ],
//   execution_time_ms: 45
// }
```

### execute_transaction

在事务中执行多个 SQL。

**参数**：

```typescript
interface ExecuteTransactionInput {
  conn_id?: string
  sqls: string[] // SQL 语句数组
}
```

**返回**：

```typescript
interface ExecuteTransactionResponse {
  results: ExecuteSqlResponse[]
}
```

**示例**：

```typescript
const result = await invoke('execute_transaction', {
  input: {
    sqls: [
      'BEGIN',
      'UPDATE accounts SET balance = balance - 100 WHERE id = 1',
      'UPDATE accounts SET balance = balance + 100 WHERE id = 2',
      'COMMIT',
    ],
  },
})
```

## 元数据导航 🔗（推荐使用）

> 以下 `load_*` 命令由 [metadata_commands.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/commands/metadata_commands.rs) 实现，调用 `Database` trait 的 `list_*` 方法。
> 不同数据库架构差异由后端驱动层统一处理，前端无需针对 dbType 分支。

**缓存层（R14 更新）**：

所有 8 个 `load_*` 命令采用 **Cache-Aside（旁路缓存）** 策略：

```
请求 → check L1 MetadataCache (LRU, TTL 300s)
  ├── 命中 → 返回（0ms）
  └── 未命中 → 查询数据库 → write L1 → 返回
```

**共享键设计**：

- `load_catalogs` 和 `load_databases` **共享** `Databases` 缓存键
- `load_tables` 和 `load_views` **共享** `Tables` 缓存键

**全覆盖状态（R15）**：

| 命令                  | 缓存键            | Trait 方法               |  状态  |
| :-------------------- | :---------------- | :----------------------- | :----: |
| `load_catalogs`       | Databases         | `list_databases()`       | ✅ R13 |
| `load_databases`      | Databases（共享） | `list_databases()`       | ✅ R13 |
| `load_schemas`        | Schemas           | `list_schemas()`         | ✅ R13 |
| `load_tables`         | Tables            | `list_tables()`          | ✅ R13 |
| `load_views`          | Tables（共享）    | `list_tables() + filter` | ✅ R13 |
| `load_columns`        | Columns           | `list_columns()`         | ✅ R14 |
| `load_procedures`     | Procedures        | `list_procedures()`      | ✅ R14 |
| `load_functions`      | Functions         | `list_functions()`       | ✅ R14 |
| `load_routine_source` | RoutineSource     | `get_routine_source()`   | ✅ R15 |

- 断开连接时自动清除缓存（`ConnectionManager::remove_connection()` → `CacheManager::invalidate_connection()`）
- 手动刷新 F5 前请调用 `invalidate_metadata_cache`（见下方）

### load_catalogs

加载 Catalog 列表。**ANSI SQL 标准：Catalog 是顶层容器，包含多个 Schema。**

> ✅ `load_catalogs` 是推荐的语义化命名。内部委托 `Database trait::list_databases()`。

**参数**：

```typescript
{
  connectionId: string
}
```

**返回**：

```typescript
interface CatalogMeta {
  name: string
}
// 返回: CatalogMeta[]
```

### load_databases

加载数据库列表。**@deprecated** — 请使用 `load_catalogs()`，符合 ANSI SQL Catalog → Schema → Table 层级命名。

**参数**：

```typescript
{
  connId: string
}
```

**返回**：

```typescript
interface DatabaseMeta {
  name: string
}
// 返回: DatabaseMeta[]
```

**数据库差异**：
| 数据库 | 返回值 |
|--------|--------|
| PostgreSQL/MySQL | 实际数据库名列表 |
| SQLite | `[{ name: "main" }]` |
| DuckDB | `[{ name: "main" }]` |

---

### load_schemas

获取 Schema 列表。**推荐替代 `get_schemas`**。

**参数**：

```typescript
{
  connId: string
  dbName: string
}
```

**返回**：

```typescript
interface SchemaMeta {
  name: string
}
// 返回: SchemaMeta[]
```

**数据库差异**：
| 数据库 | 返回值 |
|--------|--------|
| PostgreSQL | `[{ name: "public" }, { name: "pg_catalog" }, ...]` |
| MySQL | `[]`（MySQL 无 Schema 概念） |
| SQLite | `[]`（SQLite 无 Schema 概念） |
| DuckDB | `[{ name: "main" }]` |

> ⚠️ 返回空数组时，前端应跳过 Schema 层级，直接在 Database 下展示表/视图。

---

### load_tables

获取表列表。**推荐替代 `get_tables`**。

**参数**：

```typescript
{
  connId: string
  dbName: string
  schemaName: string
}
```

**返回**：

```typescript
interface TableMeta {
  name: string
  type: string // "table" | "view"
}
// 返回: TableMeta[]
```

---

### load_views

获取视图列表。**推荐替代 `get_views`**。

**参数**：

```typescript
{
  connId: string
  dbName: string
  schemaName: string
}
```

**返回**：

```typescript
interface ViewMeta {
  name: string
  type: 'view'
}
// 返回: ViewMeta[]
```

---

### load_columns

获取表的列详情。**推荐替代 `get_columns` 和 `list_columns`**。

**参数**：

```typescript
{
  connId: string
  dbName: string
  schemaName: string
  tableName: string
}
```

**返回**：

```typescript
interface ColumnMeta {
  name: string
  dataType: string // 列数据类型（如 "varchar", "integer"）
  isNullable: boolean // 是否可为 null
  defaultValue: string | null // 默认值
  isPrimaryKey: boolean // 是否主键
}
// 返回: ColumnMeta[]
```

> ✅ `list_columns` 的返回类型已从 `SchemaObjectResponse[]` 修正为基于 `ColumnDetail` 的完整列信息。

---

### load_procedures

获取存储过程列表。**R14 接入 `Database` trait `list_procedures()`，支持 L1 缓存。**

**参数**：

```typescript
{
  connId: string
  dbName: string
  schemaName: string
}
```

**返回**：

```typescript
interface ProcedureMeta {
  name: string
}
// 返回: ProcedureMeta[]
```

**数据库差异**：
| 数据库 | Trait 方法 | SQL | 备注 |
|--------|-----------|-----|------|
| MySQL | `list_procedures()` | `INFORMATION_SCHEMA.ROUTINES WHERE ROUTINE_TYPE='PROCEDURE'` | - |
| PostgreSQL | `list_procedures()` | `pg_proc WHERE prokind='p'` | - |
| SQLite | `list_procedures()` → 默认空实现 | `[]` | 不支持存储过程 |
| DuckDB | `list_procedures()` → 默认空实现 | `[]` | 不支持存储过程 |

> ✅ R14：`load_procedures` 已从原始 SQL 构建重构为 `Database` trait 方法调用。参数 `dbType` → `dbName`，与 `load_tables`/`load_columns` 语义对齐。

---

### load_functions

获取函数列表。**R14 接入 `Database` trait `list_functions()`，支持 L1 缓存。**

**参数**：

```typescript
{
  connId: string
  dbName: string
  schemaName: string
}
```

**返回**：

```typescript
interface FunctionMeta {
  name: string
}
// 返回: FunctionMeta[]
```

**数据库差异**：
| 数据库 | Trait 方法 | SQL | 备注 |
|--------|-----------|-----|------|
| MySQL | `list_functions()` | `INFORMATION_SCHEMA.ROUTINES WHERE ROUTINE_TYPE='FUNCTION'` | - |
| PostgreSQL | `list_functions()` | `pg_proc WHERE prokind IN ('f','a')` | 包含聚合函数 |
| SQLite | `list_functions()` → 默认空实现 | `[]` | 不支持函数 |
| DuckDB | `list_functions()` → 默认空实现 | `[]` | 函数通过扩展支持 |

> ✅ R14：`load_functions` 已从原始 SQL 构建重构为 `Database` trait 方法调用。参数 `dbType` → `dbName`，与 `load_tables`/`load_columns` 语义对齐。

---

### load_routine_source

**🆕 R15 新增** — 获取过程/函数的 DDL 源码（DBeavor Source Tab 方案）。

**参数**：

```typescript
{
  connId: string
  dbName: string
  schemaName: string
  routineName: string
  routineKind: string // "procedure" | "function"
}
```

**返回**：

```typescript
interface RoutineSourceMeta {
  name: string
  routineKind: string // "procedure" 或 "function"
  sourceCode: string | null // null = 该数据库不支持此 routine
}
```

**数据库差异**：
| 数据库 | Trait 方法 | SQL | 返回 |
|--------|-----------|-----|------|
| PostgreSQL | `get_routine_source()` | `SELECT pg_get_functiondef(oid) FROM pg_proc WHERE proname=? AND prokind=?` | 完整 CREATE OR REPLACE 语句 |
| MySQL | `get_routine_source()` | `SHOW CREATE PROCEDURE/FUNCTION` | 完整 CREATE 语句（含 DEFINER） |
| SQLite | `get_routine_source()` → 默认 | — | `null` |
| DuckDB | `get_routine_source()` → 默认 | — | `null` |

**缓存**：L1 MetadataCache，键 `RoutineSource`，TTL 300s（routine 定义变更频率低）。

**前端集成**：

- 双击导航树中的 procedure/function → `loadRoutineSource()` → Monaco Editor 只读 Tab
- 悬停 → 提取 DDL 第一行作为签名（前端正则解析）
- 右键 → "Copy DDL"

**插件扩展**：新数据库只需实现 `get_routine_source()` trait 方法 → 返回 `Option<String>` → 前端自动生效。

---

## SQL 执行命令

### execute_sql

执行 SQL 查询语句，返回 Arrow RecordBatch 格式的结果集。

**参数**：

```typescript
{
  connId: string
  sql: string
  dbName?: string    // 可选：指定数据库（MySQL/PostgreSQL）
}
```

**返回**：`QueryResult` — 包含 `batches: RecordBatch[]` 和 `row_count: number`

**数据库差异**：
| 数据库 | 事务 | 批量 | 备注 |
|--------|------|------|------|
| MySQL | ✅ | ✅ | sqlx 驱动 |
| PostgreSQL | ✅ | ✅ | sqlx 驱动 |
| SQLite | ✅ | ✅ | rusqlite 驱动 |
| DuckDB | ✅ | ✅ | duckdb-rs 驱动 |

---

### execute_batch

执行多条 SQL 语句（以 `;` 分隔），无返回结果集。

**参数**：

```typescript
{
  connId: string
  sql: string        // 支持多句 SQL，以 ; 分隔
  dbName?: string
}
```

**返回**：`{ rowCount: number }`

**适用场景**：执行 DDL（CREATE TABLE、ALTER TABLE）或批量 DML（INSERT/UPDATE/DELETE）。

---

### get_result_preview

获取查询结果的预览数据（前 N 行）。

**参数**：

```typescript
{
  connId: string
  queryId: string // 由 execute_sql 返回的查询 ID
  limit: number // 预览行数，默认 100
}
```

**返回**：`QueryResult`

---

### cancel_query

取消正在执行的长时间查询。

**参数**：

```typescript
{
  connId: string
  queryId: string
}
```

**返回**：`void`

> ⚠️ 注意：SQL 查询不经过 MetadataCache（只缓存元数据）。查询结果通过 QueryCache 独立缓存（`query_cache.rs`）。

---

## 导航数据层

### invalidate_metadata_cache 🆕

清除指定连接的 L1 元数据缓存，用于手动刷新（F5）。

> R13 新增。断开连接时自动调用，无需手动处理。

**参数**：

```typescript
{
  connId: string
}
```

**返回**：

```typescript
void  // 异步返回 null
```

**调用时机**：

```
用户按 F5 → invalidate_metadata_cache(connId) → load_catalogs(connId) → load_schemas(connId, db) → ...
```

**缓存失效策略**：

| 触发条件         | 方式                                              | 说明       |
| :--------------- | :------------------------------------------------ | :--------- |
| 用户断开连接     | `ConnectionManager::remove_connection()` 自动调用 | ✅ 已实现  |
| 用户手动 F5      | 前端调用 `invalidate_metadata_cache`              | ✅ 已实现  |
| TTL 到期（300s） | LRU 自动淘汰                                      | ✅ 已实现  |
| DDL 执行后       | 前端调用 `invalidate` + 重新加载                  | 需前端集成 |

---

### 数据导航层

```
Connection（连接）
  └── Catalog（目录）                    ← load_catalogs / load_databases
      ├── [Schema（模式）]               ← load_schemas（hasSchemas: true）
      │   ├── Tables（表）               ← load_tables
      │   ├── Views（视图）              ← load_views
      │   ├── Functions（函数）          ← load_functions
      │   └── Procedures（存储过程）     ← load_procedures
      └── [对象文件夹]                   ← hasSchemas: false 时直接显示
```

## 导航模型配置 🗂️（DBeaver/DataGrip 风格）

> 每种数据库的导航树结构由 JSON 配置文件 `schemas/{db}.json` 中的 `navigation` 字段定义。
> 该配置由前端 `use-database-tree-loader.ts` 通过 `getNavConfig()` 异步加载并缓存。

### 配置结构

```typescript
interface NavigationConfig {
  hasCatalogs: boolean // 📍 网络数据库=true，文件数据库=false
  hasSchemas: boolean // Schema 层级有无
  systemSchemas: string[] // 系统 Schema（默认隐藏）
  folders: {
    tables: FolderConfig // Tables 文件夹
    views: FolderConfig // Views 文件夹
    functions: FolderConfig // Functions 文件夹
    procedures: FolderConfig // Procedures 文件夹
    sequences: FolderConfig // Sequences 文件夹
    triggers: FolderConfig // Triggers 文件夹
  }
  tableChildren: {
    columns: boolean // 表下是否显示列
    indexes: boolean // 表下是否显示索引
    constraints: boolean // 表下是否显示约束
    triggers: boolean // 表下是否显示触发器
    foreignKeys: boolean // 表下是否显示外键
    references: boolean // 表下是否显示引用
  }
}

interface FolderConfig {
  enabled: boolean // 是否启用此文件夹
  label: string // 显示标签
  icon: string // 图标名称
  childTypes: string[] // 子节点类型
}
```

### 数据库差异示例

| 配置项                    |     PostgreSQL      |   MySQL   | SQLite |   DuckDB   |
| ------------------------- | :-----------------: | :-------: | :----: | :--------: |
| hasCatalogs               |         ✅          |    ✅     |   ❌   |     ❌     |
| hasSchemas                |         ✅          |    ❌     |   ❌   |     ❌     |
| tables folder             |         ✅          |    ✅     |   ✅   |     ✅     |
| views folder              |         ✅          |    ✅     |   ✅   |     ✅     |
| functions folder          |         ✅          |    ✅     |   ❌   |     ✅     |
| procedures folder         |         ✅          |    ✅     |   ❌   |     ❌     |
| sequences folder          |         ✅          |    ❌     |   ❌   |     ✅     |
| triggers folder           |         ✅          |    ❌     |   ❌   |     ❌     |
| tableChildren.columns     |         ✅          |    ✅     |   ✅   |     ✅     |
| tableChildren.indexes     |         ✅          |    ✅     |   ❌   |     ❌     |
| tableChildren.constraints |         ✅          |    ✅     |   ❌   |     ❌     |
| tableChildren.foreignKeys |         ✅          |    ✅     |   ❌   |     ❌     |
| systemSchemas             | pg_catalog,pg_toast | mysql,sys |   []   | pg_catalog |

---

## 系统命令（R18 新增）

### `get_api_version`

获取 API 版本信息。

**参数**：无

**返回**：

```json
{
  "version": "1.0.0",
  "major": 1,
  "minor": 0,
  "patch": 0,
  "codename": "Foundation"
}
```

### `ping_connection`

连接健康检查，执行 `SELECT 1` 验证连接存活。

**参数**：

| 字段      | 类型  | 必填 | 说明                            |
| --------- | ----- | :--: | ------------------------------- |
| `conn_id` | `str` |  ❌  | 连接 ID，不传则使用当前活动连接 |

**返回**：`bool`（true=连接存活，失败返回 CoreError）

---

## 审计日志（R18 增强）

### `get_sql_history` / `search_sql_history`

R18 增强后，历史记录响应包含完整审计字段：

```json
{
  "id": "abc123",
  "sql": "SELECT * FROM users",
  "conn_id": "conn-uuid",
  "db_type": "postgres",
  "executed_at": "2026-05-10T08:30:00Z",
  "duration_ms": 42,
  "success": true,
  "error_message": null,
  "rows_affected": null,
  "rows_returned": 1500
}
```

| 字段            | 类型             | 说明                     |
| --------------- | ---------------- | ------------------------ |
| `db_type`       | `Option<String>` | 数据库类型（R18 新增）   |
| `duration_ms`   | `Option<u64>`    | 执行耗时毫秒（R18 新增） |
| `success`       | `Option<bool>`   | 是否成功（R18 新增）     |
| `error_message` | `Option<String>` | 错误信息（R18 新增）     |
| `rows_affected` | `Option<u64>`    | 影响行数（R18 新增）     |
| `rows_returned` | `Option<u64>`    | 返回行数（R18 新增）     |

---

## 安全（R18 新增）

### 密码加密

连接密码使用 AES-256-GCM 加密存储，密钥由机器标识派生（主机名+用户名+主目录+SHA-256）。

- 模块：[`core/crypto.rs`](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/crypto.rs)
- 加密格式：`base64(nonce[12B] + ciphertext)`
- 兼容性：解密失败时回退到明文（兼容 R17 及之前存储的数据）

### 新增数据库示例

添加 ClickHouse 支持只需在 `schemas/clickhouse.json` 中定义：

```json
{
  "driver_id": "clickhouse",
  "navigation": {
    "hasCatalogs": true,
    "hasSchemas": false,
    "systemSchemas": ["system"],
    "folders": {
      "tables":     { "enabled": true,  "label": "Tables",          "icon": "table" },
      "views":      { "enabled": true,  "label": "Views",           "icon": "eye" },
      "functions":  { "enabled": false, ... },
      "procedures": { "enabled": false, ... },
      "sequences":  { "enabled": false, ... },
      "triggers":   { "enabled": false, ... }
    },
    "tableChildren": {
      "columns": true,
      "indexes": false,
      "constraints": false,
      "triggers": false,
      "foreignKeys": false,
      "references": false
    }
  }
}
```

> 🏗️ 前端 `VirtualTree` 零改动，新增数据库自动适配导航结构。

---

## 元数据查询（旧版，逐步废弃）

> ⚠️ 以下 `get_*` / `list_*` 命令已被新的 `load_*` 命令取代。保留用于向后兼容。

### get_databases

**已废弃** → 请使用 `load_databases`。

**参数**：`{ conn_id: string }`
**返回**：`DatabaseInfoResponse[]`

### get_schemas

**已废弃** → 请使用 `load_schemas`。

**参数**：`{ conn_id: string; database: string }`
**返回**：`SchemaInfoResponse[]`

### get_tables

**已废弃** → 请使用 `load_tables`。

**参数**：`{ conn_id: string; database: string; schema: string }`
**返回**：`TableInfoResponse[]`

### get_views

**已废弃** → 请使用 `load_views`。

**参数**：`{ conn_id: string; database: string; schema: string }`
**返回**：`TableInfoResponse[]`

### get_columns

**已废弃** → 请使用 `load_columns`。

**参数**：`{ conn_id: string; database: string; schema: string; table: string }`
**返回**：`ColumnInfoResponse[]`

### list_columns（已变更）

**返回类型变更**：`SchemaObjectResponse[]` → `ColumnDetail[]`。

新返回类型包含完整列元数据：

```typescript
interface ColumnDetail {
  name: string
  data_type: string
  nullable: boolean
  is_primary_key: boolean
  is_foreign_key: boolean
  default_value: Option<String>
  comment: Option<String>
}
```

参见 `load_columns` 获取对应的前端类型。

## 历史记录

### get_sql_history

获取 SQL 执行历史。

**参数**：

```typescript
{
  limit?: number;  // 默认 100
}
```

**返回**：

```typescript
interface SqlHistoryResponse {
  id: string
  sql: string
  conn_id?: string
  executed_at: string // ISO 8601
}
// 返回: SqlHistoryResponse[]
```

### search_sql_history

搜索 SQL 历史。

**参数**：

```typescript
{
  keyword: string;
  limit?: number;
}
```

**返回**：`SqlHistoryResponse[]`

### clear_sql_history

清空 SQL 历史。

**参数**：无

**返回**：`void`

### remove_sql_history

删除单条 SQL 历史。

**参数**：

```typescript
{
  id: string
}
```

**返回**：`void`

## 最近连接

### get_recent_connections

获取最近连接列表。

**参数**：无

**返回**：

```typescript
interface RecentConnectionResponse {
  name: string
  db_type: string
  url: string
  last_used_at: string // ISO 8601
}
// 返回: RecentConnectionResponse[]
```

### remove_recent_connection

删除最近连接记录。

**参数**：

```typescript
{
  name: string
}
```

**返回**：`void`

## 驱动管理

### get_drivers

获取所有支持的驱动列表。

**参数**：无

**返回**：

```typescript
interface DriverDescriptor {
  id: string
  name: string
  description: string
  version: string
  icon?: string
  driver_kind: string // "native" | "jdbc" | "odbc" | "wasm" | "adbc" | "http" | "python" | "js"
  default_port: number | null
  category: string // "relational" | "file-based" | "nosql" | "analytics" | "cloud"
  require_database: boolean
  require_file: boolean
  supports_ssl: boolean
  supports_ssh_tunnel: boolean
  supports_http_proxy: boolean
  supports_socks_proxy: boolean
  url_template?: string // "postgres://{username}:{password}@{host}:{port}/{database}"
  connection_fields: DriverField[]
  features: string[]
  navigation?: {
    hasCatalogs: boolean
    hasSchemas: boolean
    systemSchemas: string[]
    folders: Record<string, { enabled: boolean; label: string; icon: string; childTypes: string[] }>
    tableChildren: Record<string, boolean>
  }
}

interface DriverField {
  name: string
  label: string
  field_type: 'string' | 'number' | 'password' | 'boolean' | 'select'
  required: boolean
  default_value?: string
  options?: string[] // 用于 select 类型
}

interface DriverFeatures {
  supports_transactions: boolean
  supports_ssl: boolean
  supports_ssh_tunnel: boolean
  supports_multiple_databases: boolean
  supports_schemas: boolean
  supports_views: boolean
  supports_stored_procedures: boolean
  supports_functions: boolean
  supports_triggers: boolean
}

// 返回: DriverDescriptor[]
```

### get_driver_info

获取指定驱动的详细信息。

**参数**：

```typescript
{
  driver_id: string
}
```

**返回**：`DriverDescriptor | null`

## DBI 统一数据访问 🔥

### dbi_query

通过 DBI 执行查询（支持智能路由）。

**参数**：

```typescript
interface DBIQueryInput {
  sql: string
  conn_id?: string
  mode?: 'native' | 'duckdb' | 'stream' | 'auto' // 默认 'auto'
  timeout_ms?: number
}
```

**返回**：

```typescript
interface DBIQueryResponse {
  columns: string[]
  rows: Value[][]
  affected_rows?: number
  execution_time_ms: number
  execution_mode: 'native' | 'duckdb' | 'stream' // 实际使用的执行模式
  is_read_only: boolean
}
```

**示例**：

```typescript
// 自动模式（智能推荐）
const result = await invoke('dbi_query', {
  input: {
    sql: 'SELECT u.*, o.total FROM users u JOIN orders o ON u.id = o.user_id GROUP BY u.id',
    mode: 'auto',
  },
})
// result.execution_mode: "duckdb" (复杂查询自动路由到 DuckDB)

// 强制使用原生驱动
const result2 = await invoke('dbi_query', {
  input: {
    sql: 'SELECT * FROM users WHERE id = 1',
    mode: 'native',
  },
})
```

### dbi_execute

通过 DBI 执行写操作（INSERT/UPDATE/DELETE）。

**参数**：

```typescript
interface DBIExecuteInput {
  sql: string
  conn_id?: string
}
```

**返回**：

```typescript
interface DBIExecuteResponse {
  affected_rows: number
  execution_time_ms: number
}
```

**示例**：

```typescript
const result = await invoke('dbi_execute', {
  input: {
    sql: "UPDATE users SET name = 'John' WHERE id = 1",
  },
})
// result.affected_rows: 1
```

### register_external_database

注册外部数据库到 DuckDB（用于联邦查询）。

**参数**：

```typescript
interface RegisterExternalDBInput {
  name: string // 数据库别名
  driver: string // 驱动类型: "mysql", "postgresql"
  connection_string: string // 连接字符串
}
```

**返回**：`void`

**示例**：

```typescript
await invoke('register_external_database', {
  input: {
    name: 'mysql_prod',
    driver: 'mysql',
    connection_string: 'mysql://user:pass@prod-host:3306/mydb',
  },
})
```

### detach_external_database

卸载外部数据库。

**参数**：

```typescript
{
  name: string
}
```

**返回**：`void`

### load_file_source

加载文件数据源到 DuckDB（CSV/Excel/Parquet）。

**参数**：

```typescript
interface LoadFileSourceInput {
  path: string // 文件绝对路径
  table_name: string // 临时表名
}
```

**返回**：`void`

**示例**：

```typescript
await invoke('load_file_source', {
  input: {
    path: '/path/to/data.csv',
    table_name: 'temp_csv_data',
  },
})

// 现在可以查询
const result = await invoke('dbi_query', {
  input: {
    sql: 'SELECT * FROM temp_csv_data WHERE column1 > 100',
    mode: 'duckdb',
  },
})
```

### persist_result_set

持久化结果集到 DuckDB。

**参数**：

```typescript
interface PersistResultSetInput {
  result_name: string // 结果集名称
  sql: string // 查询 SQL
}
```

**返回**：`void`

**示例**：

```typescript
await invoke('persist_result_set', {
  input: {
    result_name: 'user_orders_2024',
    sql: `
      SELECT u.*, o.total 
      FROM mysql_prod.users u 
      JOIN pg_prod.orders o ON u.id = o.user_id 
      WHERE o.created_at > '2024-01-01'
    `,
  },
})

// 后续可以查询持久化的结果集
const result = await invoke('dbi_query', {
  input: {
    sql: 'SELECT * FROM user_orders_2024 WHERE total > 1000',
    mode: 'duckdb',
  },
})
```

### list_external_databases

列出已注册的外部数据库。

**参数**：无

**返回**：

```typescript
interface ExternalDatabaseInfo {
  name: string
  driver: string
  connection_string: string
  read_only: boolean
  is_attached: boolean
}
// 返回: ExternalDatabaseInfo[]
```

### list_result_sets

列出已持久化的结果集。

**参数**：无

**返回**：

```typescript
interface ResultSetInfo {
  name: string
  created_at: string // ISO 8601
  row_count: number
  source_sql: string
}
// 返回: ResultSetInfo[]
```

### drop_result_set

删除持久化的结果集。

**参数**：

```typescript
{
  result_name: string
}
```

**返回**：`void`

### recommend_execution_mode

智能推荐执行模式（基于 SQL 分析）。

**参数**：

```typescript
{
  sql: string
}
```

**返回**：

```typescript
interface RecommendModeResponse {
  mode: 'native' | 'duckdb' | 'stream'
  reason: string // 推荐理由
}
```

**示例**：

```typescript
const recommendation = await invoke('recommend_execution_mode', {
  input: {
    sql: 'SELECT u.*, COUNT(o.id) as order_count FROM users u LEFT JOIN orders o ON u.id = o.user_id GROUP BY u.id ORDER BY order_count DESC',
  },
})
// recommendation.mode: "duckdb"
// recommendation.reason: "Complex query with JOIN, GROUP BY, and ORDER BY - DuckDB acceleration recommended"
```

## 项目管理

### create_project

创建新项目。

**参数**：

```typescript
{
  name: string;
  path: string;
  description?: string;
}
```

**返回**：

```typescript
interface ProjectInfo {
  id: string
  name: string
  path: string
  status: 'active' | 'archived'
  created_at: string
  updated_at: string
}
```

### open_project

打开项目。

**参数**：

```typescript
{
  path: string
}
```

**返回**：`ProjectInfo`

### get_project_config

获取项目配置。

**参数**：

```typescript
{
  project_id: string
}
```

**返回**：

```typescript
interface ProjectConfig {
  theme: 'light' | 'dark' | 'system'
  editor: EditorConfig
  connections: ConnectionConfig[]
}
```

### update_project_config

更新项目配置。

**参数**：

```typescript
{
  project_id: string
  config: Partial<ProjectConfig>
}
```

---

## 版本历史

| 版本 | 日期       | 变更                                                                                      |
| ---- | ---------- | ----------------------------------------------------------------------------------------- | ------------ |
| v3.8 | 2026-05-10 | R22: connection_commands → CoreError、From<String><think> for CoreError、核心三件套全覆盖 |
| v3.9 | 2026-05-10 | R23: 评分校准到 8.6、result_commands CoreError、TODO 清零                                 |
| v3.8 | 2026-05-10 | R22: connection_commands → CoreError、From<String> for CoreError、核心三件套全覆盖        |
| v3.7 | 2026-05-10 | R21: P0+P1+P2 全修复、connect_database 脱敏、pool_status 真实数据、eprintln→tracing       | check 0 错误 |
| v3.6 | 2026-05-10 | R20: MySQL 集成测试、旧数据迁移。新增 P0 明文 URL 响应问题标记                            |
| v3.5 | 2026-05-10 | R19: URL 密码脱敏、前端 API 完整（getApiVersion/pingConnection/SqlAuditRecord）           |
| v3.4 | 2026-05-10 | R18: metadata_commands → CoreError、API 版本化、AES-256-GCM 加密                          |
| v3.3 | 2026-05-10 | R17: 二次审计修复、编译警告清零                                                           |
| v3.2 | 2026-05-10 | R16: SQL 注入防护、安全加固                                                               |
| v3.1 | 2026-05-10 | R15: RoutineSource DDL 查看                                                               |
| v3.0 | 2026-05-10 | R14: 字段/函数/存储过程 L1 缓存全覆盖                                                     |

**返回**：`void`

### get_recent_projects

获取最近项目列表。

**参数**：无

**返回**：`ProjectInfo[]`

### add_recent_project

添加项目到最近列表。

**参数**：

```typescript
{
  path: string
}
```

**返回**：`void`

## 端口协商

### negotiate_port

协商可用端口。

**参数**：

```typescript
{
  preferred_port?: number;
  port_range?: [number, number];
}
```

**返回**：

```typescript
{
  port: number
  is_preferred: boolean
}
```

### is_port_available

检查端口是否可用。

**参数**：

```typescript
{
  port: number
}
```

**返回**：`boolean`

### get_common_db_ports

获取常用数据库端口。

**参数**：无

**返回**：

```typescript
{
  mysql: 3306
  postgresql: 5432
  mongodb: 27017
  redis: 6379
  // ...
}
```

## 洞察引擎 🔍

洞察系统提供 12 个 Tauri 命令，覆盖列洞察、多列分析、规则引擎、持久化和存储管理。

### get_column_insight_full

计算并返回完整的列洞察报告。

**参数**：

```typescript
{
  tempTable: string
  column: string
}
```

**返回**：`ColumnInsightFull` (包含 stats / histogram / sample)

### save_column_insight_snapshot

将当前列洞察保存为持久化版本快照。

**参数**：

```typescript
{
  temp_table: string
  column_name: string
}
```

**返回**：`string` (version_id)

### get_column_insight_history

查询某列的所有洞察版本历史。

**参数**：

```typescript
{
  column_name: string
}
```

**返回**：`InsightVersionEntry[]`

### get_insight_storage_stats

获取洞察存储统计（快照数、列数、体积）。

**参数**：无

**返回**：`InsightStorageStats`

### cleanup_insight_snapshots

清理超过指定天数的旧洞察快照。

**参数**：

```typescript
{
  older_than_days: number // 默认 90
}
```

**返回**：`CleanupResult { removed_count, freed_bytes }`

### get_insight_version_detail

获取指定版本的完整洞察数据。

**参数**：

```typescript
{
  version_id: string
}
```

**返回**：`ColumnInsightFull | null`

### get_table_profile

获取表的元数据探查（列名/类型/可空/主键/行数）。

**参数**：

```typescript
{
  conn_id: string
  db_type: string
  database: string
  schema: string
  table: string
}
```

**返回**：`TableProfile { table_name, db_type, columns: TableColumnMeta[], row_count, schema_name }`

### profile_column_from_table

从表探查结果中点击列名，触发端到端列洞察（无需预先建立 DuckDB temp 表）。

> 后端自动：`SqlService 取样 → DuckDB temp 表 → 列洞察全量分析` 一步完成。

**参数**：

```typescript
{
  conn_id: string
  database: string
  schema: string
  table: string
  column_name: string
}
```

**返回**：`ColumnInsightFull { table_name, column_name, column_type, stats_detail, histogram, sample_values }`

### get_column_quality

计算列数据质量评分（0-100），基于完整性、唯一性、类型一致性、分布均匀性四维度加权。

**参数**：

```typescript
{
  column_name: string
  temp_table: string
}
```

**返回**：`QualityScore { column_name, overall_score, level, dimensions: [{ name, score, weight, detail }], summary }`

**等级划分**：
| 分数 | 等级 | 颜色 |
|:--:|:--:|:--:|
| ≥85 | 优秀 | 绿色 |
| ≥70 | 良好 | 蓝色 |
| ≥50 | 一般 | 黄色 |
| ≥30 | 较差 | 橙色 |
| <30 | 差 | 红色 |

### batch_evaluate_columns

一次调用完成全表所有列的质量评估（取样 → DuckDB 临时表 → 逐列洞察 → 聚合评分）。

> 后端自动：SELECT LIMIT 500 → JSON 解析 → DuckDB temp 表 → 全列 insight → 聚合 TableQuality

**参数**：

```typescript
{
  conn_id: string
  database: string
  schema: string
  table: string
}
```

**返回**：`TableQuality { table_name, overall_score, level, column_scores: [{ column_name, quality_score, level, null_rate }], summary, scored_count, total_columns }`

**使用场景**：TableProfileView "质量评估" 按钮，导航树右键表探查后一键评估

### get_schema_insight

Schema 级洞察分析：外键推断、跨表类型一致性检查、孤立表检测、冗余列检测、Schema 健康评分。

> 后端自动：`information_schema.tables + columns` → 模式匹配外键 → 类型对比 → 评分

**参数**：

```typescript
{
  conn_id: string
  database: string
  schema: string
}
```

**返回**：`SchemaInsightReport { schema_name, table_count, total_columns, fk_candidates: [{ source_table, source_column, target_table, confidence }], type_mismatches: [{ column_name, tables, severity }], orphan_tables: [{ table_name, column_count, reason }], redundant_columns: [{ column_name, table_count, suggestion }], summary, health_score, health_level }`

**检测维度**：
| 维度 | 方法 | 说明 |
|:--|:--|:--|
| 外键候选 | `infer_foreign_keys()` | 4 种命名模式（\_id/\_key/\_ref/\_uuid）+ 置信度（high/medium/low） |
| 类型一致 | `detect_type_mismatches()` | 同名列在不同表类型对比（critical/warning/info） |
| 孤立表 | `detect_orphan_tables()` | 未被任何表引用的表（含原因说明） |
| 冗余列 | `detect_redundant_columns()` | 时间戳/审计列扩散检测 + 规范化建议 |
| 健康评分 | `compute_health()` | 加权评分算法（0-100） |

> **架构优化（v12.0+）**：DuckDB 实例已统一为全局单例 `DuckDBManager`。`ResultService` 和 `DuckDBEngine`（DBI）共用同一个 DuckDB 连接，避免重复创建 in-memory 实例。`duckdb_engine.rs` 的 `tokio::sync::Mutex` 已替换为 `std::sync::Mutex`（DuckDB 操作均为同步无 I/O 等待）。

### execute_insight_rule

执行一条声明式规则（支持列洞察和多列分析）。

**参数**：

```typescript
{
  rule_id: string // 规则 ID (如 "correlation")
  params: Record<string, string> // SQL 模板参数
  temp_table: string
}
```

**返回**：动态 `Value` (JSON)

### list_insight_rules

列出所有可用的洞察规则。

**参数**：`category?: string` (可选过滤 "column" / "multi")

**返回**：`RuleMeta[]`

### list_rules_for_column

根据列类型过滤适用规则。

**参数**：

```typescript
{
  column_type: string // "Numeric" | "Text" | "DateTime" | "Boolean"
}
```

**返回**：`RuleMeta[]`

**示例**：

```typescript
// 执行多列相关性分析
const result = await invoke('execute_insight_rule', {
  input: {
    rule_id: 'correlation',
    params: { table: 'tmp_abc', col1: 'age', col2: 'salary' },
    temp_table: 'tmp_abc',
  },
})
// result: { correlation: 0.87, p_value: 0.001 }

// 清理 90 天前的旧快照
const cleaned = await invoke('cleanup_insight_snapshots', {
  input: { older_than_days: 90 },
})
// cleaned: { removed_count: 15, freed_bytes: 204800 }
```

## 错误处理

### 错误响应格式

```typescript
// 成功响应
{
  // 返回数据
}

// 错误响应
{
  error: string // 错误消息
}
```

### 错误代码

| 错误                    | 说明         |
| ----------------------- | ------------ |
| `Connection not found`  | 连接不存在   |
| `Connection timeout`    | 连接超时     |
| `Authentication failed` | 认证失败     |
| `Database not found`    | 数据库不存在 |
| `Query syntax error`    | SQL 语法错误 |
| `Constraint violation`  | 约束冲突     |
| `Pool exhausted`        | 连接池耗尽   |

### 前端错误处理示例

```typescript
import { invoke } from '@tauri-apps/api/core'

try {
  const result = await invoke('execute_sql', {
    input: { sql: 'SELECT * FROM users' },
  })
  console.log(result)
} catch (error) {
  // 错误处理
  if (error.includes('Connection not found')) {
    // 提示用户连接已断开
    showReconnectDialog()
  } else if (error.includes('syntax error')) {
    // 高亮 SQL 错误位置
    highlightSqlError(error)
  } else {
    // 通用错误提示
    showErrorNotification(error)
  }
}
```

## TypeScript 类型定义

完整的 TypeScript 类型定义：

```typescript
// types/api.ts

export type DatabaseType = 'mysql' | 'postgresql' | 'sqlite' | 'duckdb' | 'mongodb'

export interface ConnectionConfig {
  host: string
  port: number
  database: string
  username: string
  password: string
  ssl?: boolean
  ssh?: SshConfig
}

export interface SshConfig {
  host: string
  port: number
  username: string
  private_key?: string
  password?: string
}

export type Value =
  | { type: 'null' }
  | { type: 'string'; value: string }
  | { type: 'int64'; value: number }
  | { type: 'float64'; value: number }
  | { type: 'bool'; value: boolean }
  | { type: 'bytes'; value: Uint8Array }
  | { type: 'date'; value: string }
  | { type: 'time'; value: string }
  | { type: 'datetime'; value: string }

export interface QueryResult {
  columns: string[]
  rows: Value[][]
  affected_rows?: number
  execution_time_ms: number
}

// ... 其他类型定义
```

## API 调用工具函数

```typescript
// utils/api.ts

import { invoke } from '@tauri-apps/api/core'

export async function executeSql(sql: string, connId?: string): Promise<QueryResult> {
  const response = await invoke<ExecuteSqlResponse>('execute_sql', {
    input: { sql, conn_id: connId },
  })

  return {
    columns: response.columns,
    rows: response.rows,
    affected_rows: response.affected_rows,
    execution_time_ms: response.execution_time_ms,
  }
}

export async function getTables(
  connId: string,
  database: string,
  schema: string
): Promise<string[]> {
  const response = await invoke<TableInfoResponse[]>('get_tables', {
    input: { conn_id: connId, database, schema },
  })

  return response.map(t => t.name)
}

// ... 其他工具函数
```

---

## DBeaver/DataGrip 设计对标分析 🎯

> 本节从企业级数据库管理工具的视角，分析 RdataStation 的架构设计与 DBeaver/DataGrip 的对标程度。

### 一、核心设计理念对比

| 设计理念       | DBeaver                    | DataGrip                    | RdataStation                                |
| :------------- | :------------------------- | :-------------------------- | :------------------------------------------ |
| **插件模型**   | Eclipse 插件（plugin.xml） | IntelliJ 插件（plugin.xml） | JSON 配置 + Rust trait（schemas/{db}.json） |
| **驱动发现**   | 插件注册表                 | 插件注册表                  | DriverRegistry（OnceLock + auto_register）  |
| **连接管理**   | 连接池（DBCP）             | 连接池（内置）              | ConnectionManager（sqlx Pool）              |
| **元数据浏览** | JDBC DatabaseMetaData      | JDBC DatabaseMetaData       | Database trait `list_*` 方法                |
| **SQL 编辑器** | 自研编辑器                 | IntelliJ 编辑器             | Monaco Editor                               |
| **数据表格**   | 自研表格                   | IntelliJ 表格               | AG Grid（虚拟滚动）                         |
| **布局系统**   | Eclipse RCP                | IntelliJ Platform           | dockview-vue（VSCode 风格）                 |
| **沙箱隔离**   | ❌ 无                      | ❌ 无                       | ✅ Wasm 插件（wasmtime + WASI）             |

### 二、配置驱动对比

#### DBeaver：plugin.xml

```xml
<extension point="org.jkiss.dbeaver.dataSourceProvider">
    <datasource class="..." id="postgresql" label="PostgreSQL" ...>
        <treeInjection .../>
    </datasource>
</extension>
```

#### RdataStation：schemas/{db}.json

```json
{
  "driver_id": "postgres",
  "metadata": { "category": "relational", ... },
  "fields": [ ... ],
  "navigation": {
    "hasCatalogs": true,
    "hasSchemas": true,
    "folders": { "tables": {...}, "views": {...}, ... },
    "tableChildren": { "columns": true, "indexes": true, ... }
  }
}
```

**关键对比**：

- DBeaver 使用 XML + Java 类组合定义驱动
- DataGrip 使用 Java 类 + 注解
- RdataStation 使用 JSON + Rust trait，**零代码扩展新数据库类型**

### 三、数据导航流对比

```
DBeaver:
  DB Navigator → JDBC DatabaseMetaData.getTables() → 树节点

DataGrip:
  Database Explorer → Introspector → 树节点

RdataStation:
  database-navigator.vue → use-database-tree-loader.ts
    → getNavConfig(dbType)                  ← JSON 配置
    → databaseApi.loadTables(connId, ...)   ← Tauri IPC
    → metadata_commands::load_tables        ← Rust 命令
    → Database trait::list_tables()         ← 驱动实现
    → VirtualTree 渲染                       ← 前端组件
```

### 四、新数据库扩展成本对比

> 更新于 R10（v10.0 / v2.5）：URL 模板引擎消除了后端硬编码 match。

| 新增数据库（如 ClickHouse） | DBeaver            | DataGrip          | RdataStation（R9）         | RdataStation（R10）        |
| :-------------------------- | :----------------- | :---------------- | :------------------------- | :------------------------- |
| 创建连接表单                | XML 配置 + Java 类 | Java 类           | 1 个 JSON 文件             | 1 个 JSON 文件             |
| 导航树结构                  | XML treeInjection  | Java 实现         | JSON `navigation` 字段     | JSON `navigation` 字段     |
| URL 构建                    | `<urlTemplate>`    | Java 硬编码       | 3 处 Rust 硬编码           | JSON `urlTemplate` ✅      |
| 驱动种类声明                | `<driverType>`     | Java 类           | Rust `DriverKind` 枚举     | JSON `driverKind` ✅       |
| 元数据查询                  | Java JDBC 实现     | Java 实现         | Rust `impl Database` trait | Rust `impl Database` trait |
| SQL 执行                    | Java JDBC 实现     | Java 实现         | Rust `impl Database` trait | Rust `impl Database` trait |
| 前端改动                    | Java UI 类         | Java UI 类        | **零改动** ✅              | **零改动** ✅              |
| 后端核心改动                | —                  | —                 | 3 处代码                   | **零改动** ✅              |
| **总计代码量**              | ~500-1000 行 Java  | ~500-1000 行 Java | ~1 JSON + ~250 行 Rust     | ~1 JSON + ~200 行 Rust     |

### 五、RdataStation 的核心优势

1. **Wasm 沙箱隔离**：插件崩溃不影响主程序，DBeaver/DataGrip 无此特性
2. **Apache Arrow 零拷贝**：Rust ↔ 插件数据传输效率远超 JDBC
3. **JSON 配置即插即用**：新增数据库 = 1 个 JSON + 1 个 Rust 驱动实现，前端零改动
4. **性能**：Rust 原生性能 + sqlx 异步连接池，启动 < 1.5 秒
5. **内存可控**：核心 < 150MB，远低于 DBeaver（~500MB+）和 DataGrip（~1GB+）

### 六、当前差距（经架构适配性重新评估）

> 详见 [08-optimization-plan.md §29 — 架构适配性分析](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/docs/08-optimization-plan.md)

#### 核心发现：基础设施完备，缓存已集成（R13 ✅）

```
MetadataCache (L1 内存 LRU + TTL) ← ✅ 已实现
MetadataCacheManager (L3 磁盘 SQLite) ← ✅ 已实现
SmartPool (动态缩放 + 健康检查) ← ✅ 已实现
metadata_commands.rs 读取缓存？ ← ✅ R13 已集成
```

|  优先级   | 差距项                          | 说明                                 | 工作量 |
| :-------: | :------------------------------ | :----------------------------------- | :----: |
| ~~🔴 P0~~ | ~~metadata_commands 启用缓存~~  | ✅ **已完成**（R13）                 |   —    |
|   🟡 P1   | Schema 选择器                   | 后端 3 行 + 前端 2 天                |  2天   |
|   🟡 P1   | 自省截断（大 Schema > 1000 表） | Trait 不改，`list_tables` 加截断逻辑 |  1天   |
|   🟡 P1   | 元数据分离连接                  | 大 Schema 场景                       |  2天   |
|   🟡 P1   | 启动延迟连接                    | 多连接恢复                           |  2天   |
|   🟢 P2   | 连接类型 (Dev/Test/Prod)        | ConnectionInfo 加字段                |  1天   |
|   🟢 P2   | Bootstrap Queries               | ConnectionConfig 加字段              | 0.5天  |
|   🟢 P2   | 增量刷新                        | 仅 PostgreSQL 有效                   |  3天   |
|   🟢 P2   | 智能刷新                        | 成本/收益不对等                      |  3天   |
|    P3     | Session Manager                 | Trait 扩展                           |  5天   |
|    P3     | 后台任务系统                    | Tauri events                         |  3天   |
|   ❌ P4   | 离线迷你目录                    | **不适合桌面工具场景**               |   —    |

### 七、总结

RdataStation 的核心架构（配置驱动 + 动态渲染 + trait 抽象 + Arrow 传输 + URL 模板引擎）**完全对标并且在扩展性方面超越** DBeaver/DataGrip 的插件模型。新增数据库 = 1 个 JSON + 1 个 DriverFactory，无需修改前端组件和后端核心代码，这是 RdataStation 的核心差异化优势。

**最新对标发现**（2026-05-09）：DBeaver 的 "Separate Connections"（元数据分离连接）和 DataGrip 的 "Introspection Levels"（三级自省）是两个**最值得立即借鉴**的架构决策。前者解决大 Schema 场景下 metadata queries 阻塞用户查询的根本性问题，后者将首次加载大数据库的时间从数十秒优化到秒级。具体分析和实施路线见 [08-optimization-plan.md §28](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/docs/08-optimization-plan.md)。
