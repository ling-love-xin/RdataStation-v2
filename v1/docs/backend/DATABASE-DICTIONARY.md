# 数据库字典（Database Dictionary）

> 版本：v1.1
> 初稿日期：2026-05-12
> 更新时间：2026-05-18（新增数据源模块表）
> 对应后端版本：R25+

## 概述

RdataStation 使用分层 SQLite + DuckDB 存储架构，分为四个独立数据库文件：

| 数据库        | 引擎   | 作用                               | 归属 |
| ------------- | ------ | ---------------------------------- | ---- |
| `global.db`   | SQLite | 项目索引、全局连接、插件中心、设置 | 系统 |
| `meta.db`     | SQLite | 项目内：连接配置、SQL 历史、设置   | 项目 |
| `<conn>.meta` | SQLite | 每个连接独立的元数据缓存           | 项目 |
| `data.duckdb` | DuckDB | 分析数据、查询结果缓存             | 项目 |

---

## 一、全局数据库（global.db）

### 1.1 项目索引

```sql
CREATE TABLE project_info (
    id              TEXT PRIMARY KEY,     -- 项目 UUID
    name            TEXT NOT NULL,        -- 项目名称
    description     TEXT,                 -- 描述
    path            TEXT NOT NULL,        -- 本地/远程路径
    status          TEXT DEFAULT 'active',-- active | archived
    created_at      TIMESTAMP,
    updated_at      TIMESTAMP,
    last_opened_at  TIMESTAMP
);
-- 索引: last_opened_at DESC, status
```

### 1.2 全局连接

```sql
CREATE TABLE global_connections (
    id                 TEXT PRIMARY KEY,
    name               TEXT NOT NULL,
    driver             TEXT NOT NULL,           -- mysql | postgres | sqlite | duckdb
    host               TEXT,
    port               INTEGER,
    database           TEXT,
    schema_name        TEXT,                    -- 默认 Schema（PG/Oracle）
    username           TEXT,
    password_encrypted TEXT,                    -- AES 加密
    options            TEXT,                    -- JSON 额外配置
    tags               TEXT,                    -- JSON 标签数组
    use_duckdb_fed     BOOLEAN DEFAULT 0,       -- 联邦分析开关
    metadata_path      TEXT,                    -- 元数据缓存文件路径
    is_active          BOOLEAN DEFAULT 1,
    created_at         TIMESTAMP,
    updated_at         TIMESTAMP
);
-- 索引: driver, is_active, updated_at DESC, use_duckdb_fed
```

### 1.3 最近连接

```sql
CREATE TABLE recent_connections (
    id              TEXT PRIMARY KEY,
    connection_id   TEXT NOT NULL,
    last_used       TIMESTAMP,
    usage_count     INTEGER DEFAULT 1
);
-- 索引: last_used DESC
```

### 1.4 应用日志

```sql
CREATE TABLE app_logs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp   TEXT NOT NULL,
    level       TEXT NOT NULL CHECK(level IN ('TRACE','DEBUG','INFO','WARN','ERROR')),
    target      TEXT NOT NULL,          -- 模块名
    message     TEXT NOT NULL,
    fields      TEXT,                   -- JSON 结构化字段
    file        TEXT,
    line        INTEGER,
    session_id  TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
-- 索引: timestamp, level, target, session_id
```

### 1.5 数据源模块（新增，v1.1）

> 详细设计见 [DATA-SOURCE-MODULE.md](./DATA-SOURCE-MODULE.md)

#### 1.5.1 数据源类型目录

```sql
CREATE TABLE data_source_types (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    category    TEXT NOT NULL,          -- relational|file-based|nosql|analytics|cloud|mq|http
    icon        TEXT,
    enabled     BOOLEAN DEFAULT 1,
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

#### 1.5.2 驱动定义目录

```sql
CREATE TABLE drivers (
    id                    TEXT PRIMARY KEY,
    type_id               TEXT NOT NULL REFERENCES data_source_types(id) ON DELETE CASCADE,
    name                  TEXT NOT NULL,
    driver_kind           TEXT NOT NULL DEFAULT 'native',  -- native|jdbc|odbc|wasm|adbc|http|python|js
    is_file               BOOLEAN DEFAULT 0,
    default_port          INTEGER,
    url_template          TEXT,
    download_url          TEXT,              -- 外部驱动下载 URL
    download_checksum     TEXT,              -- SHA256 校验和
    version               TEXT,              -- 驱动版本号
    config_schema         TEXT NOT NULL,     -- JSON Schema：前端动态生成连接表单
    supported_auth_types  TEXT,              -- JSON 数组
    capabilities          TEXT,              -- JSON 数组
    enabled               BOOLEAN DEFAULT 1,
    created_at            TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
-- 索引: type_id, driver_kind, enabled
```

#### 1.5.3 本机驱动文件注册表

```sql
CREATE TABLE driver_files (
    id           TEXT PRIMARY KEY,
    driver_id    TEXT NOT NULL REFERENCES drivers(id),
    file_path    TEXT NOT NULL,              -- 相对路径
    file_name    TEXT NOT NULL,
    file_size    INTEGER,
    checksum     TEXT,                       -- SHA256
    version      TEXT NOT NULL,
    installed_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at   TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
-- 唯一索引: (driver_id, version)
```

#### 1.5.4 环境表

```sql
CREATE TABLE environments (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    description TEXT,
    color       TEXT,
    sort_order  INTEGER DEFAULT 0,
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

#### 1.5.5 环境策略表

```sql
CREATE TABLE environment_policies (
    id              TEXT PRIMARY KEY,
    environment_id  TEXT NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    policy_type     TEXT NOT NULL,           -- read_only|navigation_filter|query_timeout|max_rows|ddl_blocked|dml_blocked|confirm_required
    policy_config   TEXT,                    -- JSON 配置
    enabled         BOOLEAN DEFAULT 1,
    created_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
-- 索引: environment_id
```

#### 1.5.6 认证配置表

```sql
CREATE TABLE auth_configs (
    id          TEXT PRIMARY KEY,
    name        TEXT,
    auth_type   TEXT NOT NULL,               -- password|keyfile|kerberos|oauth2|aws_iam|gcp_sa
    auth_data   TEXT NOT NULL,               -- JSON，密码 AES-256-GCM 加密
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

#### 1.5.7 网络配置表

```sql
CREATE TABLE network_configs (
    id            TEXT PRIMARY KEY,
    name          TEXT,
    network_type  TEXT NOT NULL,             -- ssh|proxy|ssl|none
    config        TEXT NOT NULL,             -- JSON，结构依赖 network_type
    created_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

#### 1.5.8 全局连接扩展字段

```sql
-- ALTER TABLE global_connections ADD COLUMN:
description       TEXT,          -- 连接描述
driver_id         TEXT,          -- → drivers.id
environment_id    TEXT,          -- → environments.id
auth_config_id    TEXT,          -- → auth_configs.id
network_config_id TEXT,          -- → network_configs.id
driver_properties TEXT,          -- JSON
advanced_options  TEXT           -- JSON
```

### 1.6 其他系统表

| 表名                   | 用途                |
| ---------------------- | ------------------- |
| `global_settings`      | key-value 全局配置  |
| `navigator_states`     | 导航器展开/选中状态 |
| `favorite_objects`     | 用户收藏对象        |
| `plugins`              | 插件注册表          |
| `global_drivers`       | 全局驱动模板        |
| `credential_slots`     | 系统凭据存储        |
| `global_saved_queries` | 全局收藏 SQL        |
| `app_info`             | 应用版本/安装信息   |
| `schema_version`       | 迁移版本记录        |

---

## 二、项目元数据库（meta.db）

### 2.1 项目信息

```sql
CREATE TABLE project (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    description     TEXT,
    created_at      TIMESTAMP,
    updated_at      TIMESTAMP
);
```

### 2.2 数据库连接

```sql
CREATE TABLE connections (
    id                 TEXT PRIMARY KEY,
    name               TEXT NOT NULL,
    db_type            TEXT NOT NULL,           -- mysql | postgres | sqlite | duckdb
    host               TEXT,
    port               INTEGER,
    database           TEXT,
    username           TEXT,
    password_encrypted TEXT,
    options            TEXT,                    -- JSON 额外配置
    tags               TEXT,                    -- JSON 标签数组
    is_active          BOOLEAN DEFAULT 1,
    created_at         TIMESTAMP,
    updated_at         TIMESTAMP
);
-- 索引由 002_refactor_connections.sql 在重建时创建
```

### 2.3 SQL 查询历史（企业级）

```sql
CREATE TABLE query_history (
    id              TEXT PRIMARY KEY,
    connection_id   TEXT REFERENCES connections(id),
    database_name   TEXT,
    schema_name     TEXT,
    sql             TEXT NOT NULL,
    sql_hash        TEXT NOT NULL,              -- SHA-256 去重
    exec_mode       TEXT NOT NULL DEFAULT 'native',  -- native | duckdb_fed
    category        TEXT NOT NULL DEFAULT 'query',    -- query | ddl | dml
    success         BOOLEAN DEFAULT 1,
    error_message   TEXT,
    duration_ms     INTEGER,
    rows_returned   INTEGER,
    rows_affected   INTEGER,
    is_pinned       BOOLEAN DEFAULT 0,          -- 固定置顶
    created_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
-- 索引: connection_id, created_at DESC, sql_hash, category, is_pinned
-- 视图: sql_history（兼容旧代码）
```

### 2.4 数据源模块（项目级，新增 v1.1）

> 详细设计见 [DATA-SOURCE-MODULE.md](./DATA-SOURCE-MODULE.md)

#### 2.4.1 项目驱动启用表（⭐ 核心隔离）

```sql
CREATE TABLE project_drivers (
    id           TEXT PRIMARY KEY,
    driver_id    TEXT NOT NULL,              -- 逻辑引用 global.db.drivers.id
    enabled      BOOLEAN DEFAULT 1,
    installed_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
-- 唯一索引: driver_id
-- 注意: driver_id 无 FK 约束（跨DB），由 Rust 层校验
```

#### 2.4.2 环境表

```sql
CREATE TABLE environments (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    description TEXT,
    color       TEXT,
    sort_order  INTEGER DEFAULT 0,
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

#### 2.4.3 环境策略表

```sql
CREATE TABLE environment_policies (
    id              TEXT PRIMARY KEY,
    environment_id  TEXT NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    policy_type     TEXT NOT NULL,
    policy_config   TEXT,
    enabled         BOOLEAN DEFAULT 1,
    created_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
-- 索引: environment_id
```

#### 2.4.4 认证配置表

```sql
CREATE TABLE auth_configs (
    id          TEXT PRIMARY KEY,
    name        TEXT,
    auth_type   TEXT NOT NULL,
    auth_data   TEXT NOT NULL,
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

#### 2.4.5 网络配置表

```sql
CREATE TABLE network_configs (
    id            TEXT PRIMARY KEY,
    name          TEXT,
    network_type  TEXT NOT NULL,
    config        TEXT NOT NULL,
    created_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

#### 2.4.6 项目连接扩展字段

```sql
-- ALTER TABLE connections ADD COLUMN:
description       TEXT,
driver_id         TEXT,          -- → drivers.id (逻辑引用)
environment_id    TEXT,          -- → environments.id
auth_config_id    TEXT,          -- → auth_configs.id
network_config_id TEXT,          -- → network_configs.id
driver_properties TEXT,          -- JSON
advanced_options  TEXT           -- JSON
```

### 2.5 其他项目表

| 表名                   | 用途               |
| ---------------------- | ------------------ |
| `project_settings`     | key-value 项目配置 |
| `workbench_state`      | 工作台布局/面板    |
| `project_used_plugins` | 项目使用的插件     |

---

## 三、连接元数据缓存（`<connection_id>.meta`）

每个数据库连接维护独立的元数据缓存文件，存储从目标数据库采集的 schema 信息。

### 3.1 元数据总表

```sql
CREATE TABLE metadata (
    id              TEXT PRIMARY KEY,     -- 稳定缓存 ID
    obj_type        TEXT NOT NULL,        -- table | view | column | index | function | trigger | procedure
    database_name   TEXT NOT NULL,
    schema_name     TEXT NOT NULL,
    table_name      TEXT NOT NULL,
    name            TEXT,                 -- 对象名称
    data_type       TEXT,                 -- 仅列
    is_nullable     INTEGER,
    is_primary      INTEGER DEFAULT 0,
    is_unique       INTEGER DEFAULT 0,
    comment         TEXT,
    definition      TEXT,                 -- 视图/函数/过程定义
    extra           TEXT,                 -- JSON 扩展
    last_sync       INTEGER NOT NULL      -- Unix 时间戳
);
-- 索引: obj_type, (database_name, schema_name, table_name, name), (database_name, schema_name)
```

### 3.2 同步日志

```sql
CREATE TABLE sync_log (
    id              TEXT PRIMARY KEY,
    start_at        INTEGER NOT NULL,
    end_at          INTEGER NOT NULL,
    success         INTEGER DEFAULT 1,
    message         TEXT,
    objects_fetched INTEGER DEFAULT 0
);
-- 索引: start_at DESC, success
```

---

## 四、项目分析数据库（data.duckdb）

### 4.1 查询结果缓存

```sql
CREATE TABLE query_results (
    id                TEXT PRIMARY KEY,
    query_id          TEXT,
    sql_hash          TEXT,
    connection_id     TEXT,
    result_json       TEXT,           -- JSON 序列化结果
    row_count         INTEGER,
    execution_time_ms INTEGER,
    created_at        TIMESTAMP
);
-- 索引: connection_id, created_at DESC, sql_hash
```

### 4.2 分析数据

```sql
CREATE TABLE analytics (
    id                TEXT PRIMARY KEY,
    analysis_type     TEXT,
    source_connection TEXT,
    source_table      TEXT,
    analysis_json     TEXT,           -- JSON 分析结果
    created_at        TIMESTAMP
);
-- 索引: analysis_type, source_connection
```

---

## 五、前端-后端数据契约

### 5.1 QueryResult（IPC 传输）

后端序列化字段（[models.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/models.rs#L128-L141)）：

```typescript
// 对应 shared/api/index.ts:QueryResult（snake_case IPC 契约）
interface QueryResult {
  columns: string[] // 列名列表
  rows: unknown[][] // 行数据（二维数组）
  affected_rows?: number | null // DML 影响行数
  is_read_only?: boolean | null // 是否只读查询
  total_rows: number // 总行数
}
```

> ⚠️ `columns` 为 `string[]`（后端序列化 `Vec<String>`），不是 `QueryColumn[]`。
> `rows` 为 `unknown[][]`（二维数组），由 Arrow RecordBatch 转换为 `Vec<Vec<Value>>`。

### 5.2 SchemaObject（对象树）

对应 [traits.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/driver/traits.rs#L23-L32)：

```typescript
// 对应 shared/types/databaseMeta.ts:SchemaObject
type SchemaObjectKind =
  | 'Database'
  | 'Schema'
  | 'Table'
  | 'View'
  | 'Column'
  | 'Index'
  | 'PrimaryKey'
  | 'ForeignKey'
  | 'Procedure'
  | 'Function'

interface SchemaObject {
  name: string
  kind: SchemaObjectKind
  children?: SchemaObject[] // undefined = 未加载，[] = 已加载无子项
  comment?: string | null
}
```

### 5.3 命令参数规范

多数 Tauri 命令参数使用 snake_case（与 Rust 字段名一致）：

- `conn_id` — 连接 ID
- `db_type` — 数据库类型
- `timeout_ms` — 超时毫秒
- `elapsed_ms` — 耗时毫秒

前端 API 层（`shared/api/index.ts`）处理 snake_case ↔ camelCase 转换。

---

## 六、数据流概要

```
User Action (Vue)
    ↓ tauri.invoke()
Tauri Command (commands/*.rs)
    ↓ 调用 service
Service Layer (services/*.rs)
    ↓ 调用 dbi
DBI Layer (dbi/*.rs)
    ↓ DriverEngine / DuckDBEngine
Driver Layer (driver/native/*.rs)
    ↓ sqlx / rusqlite / duckdb-rs
Target Database (MySQL/PG/SQLite/DuckDB)
    ↓ QueryResult (Arrow RecordBatch)
    ↓ Serialize to JSON
Tauri IPC
    ↓ 前端接收 snake_case JSON
API Layer (shared/api/index.ts)
    ↓ 类型安全的 invoke 封装
Composable/Hook (useSqlExecution.ts)
    ↓ snake_case → camelCase 转换
Component (Vue SFC)
```

---

## 版本历史

| 版本 | 日期       | 说明                                                                                                                                                                                  |
| ---- | ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| v1.1 | 2026-05-18 | 新增数据源模块表：data_source_types, drivers, driver_files, environments, environment_policies, auth_configs, network_configs, project_drivers；扩展 connections + global_connections |
| v1.0 | 2026-05-12 | 初稿：全局/项目/元数据/分析四库 schema + 数据契约                                                                                                                                     |
