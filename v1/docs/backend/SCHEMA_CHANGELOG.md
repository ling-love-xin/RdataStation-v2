# 数据库表变更历史

> 版本：v1.0
> 最后更新：2026-05-03
> 状态：✅ 持续更新

> 本文档记录了 RdataStation 项目从初始版本到当前版本的所有数据库表结构变更。

---

## 一、变更概览

| 数据库      | 表名               | 变更次数 | 当前版本 | 主要变更                                                            |
| ----------- | ------------------ | -------- | -------- | ------------------------------------------------------------------- |
| 项目 SQLite | connections        | 7        | v1.7     | 恢复 schema_name/use_duckdb_fed/metadata_path，支持 DuckDB 联邦分析 |
| 全局 SQLite | global_connections | 6        | v1.6     | 恢复 schema_name/use_duckdb_fed/metadata_path，支持 DuckDB 联邦分析 |
| 项目 SQLite | query_history      | 3        | v2.0     | 重构为完整查询历史表，支持 exec_mode/category/sql_hash/is_pinned    |
| 项目 SQLite | sql_history        | 2        | v1.2     | 改为 query_history 的兼容视图                                       |
| 全局 SQLite | project_info       | 2        | v1.2     | 新增 last_opened_at 字段                                            |
| 项目 SQLite | project_settings   | 1        | v1.0     | 无变更                                                              |
| 项目 SQLite | workbench_state    | 1        | v1.0     | 无变更                                                              |
| 全局 SQLite | navigator_states   | 1        | v1.0     | 无变更                                                              |
| 全局 SQLite | favorite_objects   | 1        | v1.0     | 无变更                                                              |
| 全局 SQLite | plugins            | 1        | v1.0     | 无变更                                                              |
| 项目 DuckDB | query_results      | 1        | v1.0     | 无变更                                                              |
| 项目 DuckDB | analytics          | 1        | v1.0     | 无变更                                                              |
| 连接元数据  | metadata           | 1        | v1.0     | 无变更                                                              |
| 连接元数据  | sync_log           | 1        | v1.0     | 无变更                                                              |

---

## 二、详细变更历史

### 2.1 connections 表（项目级连接配置）

**数据库**: 项目 SQLite (`{project_path}/.RSMETA/project.db`)

**变更次数**: 4 次

#### 版本 v1.0 - 初始版本

```sql
CREATE TABLE connections (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    db_type     TEXT NOT NULL,          -- 数据库类型
    host        TEXT,
    port        INTEGER,
    database    TEXT,
    username    TEXT,
    password    TEXT,                   -- 明文密码
    options     TEXT,                   -- JSON 配置
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

**变更说明**:

- 使用 `db_type` 字段表示数据库类型
- 密码字段为明文 `password`
- 无标签、无激活状态控制

#### 版本 v1.1 - 密码加密支持

```sql
ALTER TABLE connections RENAME COLUMN password TO password_encrypted;
```

**变更说明**:

- 将 `password` 重命名为 `password_encrypted`
- 为后续加密存储做准备

#### 版本 v1.2 - 字段名统一

```sql
ALTER TABLE connections RENAME COLUMN db_type TO driver;
```

**变更说明**:

- 将 `db_type` 重命名为 `driver`
- 与全局 `global_connections` 表保持一致
- 原因：`driver` 更符合驱动选型的语义

#### 版本 v1.3 - 标签支持

```sql
ALTER TABLE connections ADD COLUMN tags TEXT;
```

**变更说明**:

- 新增 `tags` 字段（JSON 数组字符串）
- 用于标识连接的分类（如 "全局"、"项目"、"生产" 等）

#### 版本 v1.4 - 激活状态控制

```sql
ALTER TABLE connections ADD COLUMN is_active BOOLEAN DEFAULT 1;
```

**变更说明**:

- 新增 `is_active` 字段
- 用于软删除和连接启用/禁用控制

#### 当前版本（v1.4 - 最终）

```sql
CREATE TABLE IF NOT EXISTS connections (
    id                 TEXT PRIMARY KEY,
    name               TEXT NOT NULL,
    driver             TEXT NOT NULL,           -- 数据库驱动类型 (mysql, postgres, sqlite, duckdb)
    host               TEXT,                    -- 主机地址
    port               INTEGER,                 -- 端口号
    database           TEXT,                    -- 数据库名
    username           TEXT,                    -- 用户名
    password_encrypted TEXT,                    -- 加密后的密码
    options            TEXT,                    -- JSON 格式的额外配置
    tags               TEXT,                    -- JSON 格式的标签数组
    is_active          BOOLEAN DEFAULT 1,       -- 是否激活
    created_at         TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at         TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_connections_driver ON connections(driver);
CREATE INDEX IF NOT EXISTS idx_connections_active ON connections(is_active);
CREATE INDEX IF NOT EXISTS idx_connections_updated ON connections(updated_at DESC);
```

#### 版本 v1.5 - 恢复 Schema 名

```sql
ALTER TABLE connections ADD COLUMN schema_name TEXT;
```

**变更说明**:

- 恢复 `schema_name` 字段
- 用于记录 PostgreSQL/Oracle 等多 Schema 数据库的默认 Schema
- 原因：用户在切换项目时需要记住最后使用的 Schema

#### 版本 v1.6 - 恢复 DuckDB 联邦分析开关

```sql
ALTER TABLE connections ADD COLUMN use_duckdb_fed BOOLEAN DEFAULT 0;
```

**变更说明**:

- 恢复 `use_duckdb_fed` 字段
- **核心功能**：标记该连接是否启用 DuckDB 联邦查询
- 用于加速分析查询，将外部数据源导入 DuckDB 进行高性能分析

#### 版本 v1.7 - 恢复元数据缓存路径

```sql
ALTER TABLE connections ADD COLUMN metadata_path TEXT;
```

**变更说明**:

- 恢复 `metadata_path` 字段
- 记录每个连接的元数据缓存文件路径
- 元数据包括：表/列/索引信息，存储在独立的 SQLite 文件中

#### 当前版本（v1.7 - 最终）

```sql
CREATE TABLE IF NOT EXISTS connections (
    id                 TEXT PRIMARY KEY,
    name               TEXT NOT NULL,
    driver             TEXT NOT NULL,           -- 数据库驱动类型 (mysql, postgres, sqlite, duckdb)
    host               TEXT,                    -- 主机地址
    port               INTEGER,                 -- 端口号
    database           TEXT,                    -- 数据库名
    schema_name        TEXT,                    -- 默认 Schema 名（PostgreSQL/Oracle 等）
    username           TEXT,                    -- 用户名
    password_encrypted TEXT,                    -- 加密后的密码
    options            TEXT,                    -- JSON 格式的额外配置
    tags               TEXT,                    -- JSON 格式的标签数组
    use_duckdb_fed     BOOLEAN DEFAULT 0,       -- 是否启用 DuckDB 联邦分析
    metadata_path      TEXT,                    -- 元数据缓存文件路径
    is_active          BOOLEAN DEFAULT 1,       -- 是否激活
    created_at         TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at         TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_connections_driver ON connections(driver);
CREATE INDEX IF NOT EXISTS idx_connections_active ON connections(is_active);
CREATE INDEX IF NOT EXISTS idx_connections_updated ON connections(updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_connections_duckdb_fed ON connections(use_duckdb_fed);
```

---

### 2.2 global_connections 表（全局连接配置）

**数据库**: 全局 SQLite (`{data_dir}/RdataStation/system/global.db`)

**变更次数**: 3 次

#### 版本 v1.0 - 初始版本

```sql
CREATE TABLE global_connections (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    driver      TEXT NOT NULL,
    host        TEXT,
    port        INTEGER,
    database    TEXT,
    username    TEXT,
    password    TEXT,                   -- 明文密码
    options     TEXT,
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

#### 版本 v1.1 - 密码加密支持

```sql
ALTER TABLE global_connections RENAME COLUMN password TO password_encrypted;
```

#### 版本 v1.2 - 标签支持

```sql
ALTER TABLE global_connections ADD COLUMN tags TEXT;
```

#### 版本 v1.3 - 激活状态控制

```sql
ALTER TABLE global_connections ADD COLUMN is_active BOOLEAN DEFAULT 1;
```

#### 当前版本（v1.3 - 最终）

```sql
CREATE TABLE IF NOT EXISTS global_connections (
    id                 TEXT PRIMARY KEY,
    name               TEXT NOT NULL,
    driver             TEXT NOT NULL,           -- 数据库驱动类型 (mysql, postgres, sqlite, duckdb)
    host               TEXT,                    -- 主机地址
    port               INTEGER,                 -- 端口号
    database           TEXT,                    -- 数据库名
    username           TEXT,                    -- 用户名
    password_encrypted TEXT,                    -- 加密后的密码
    options            TEXT,                    -- JSON 格式的额外配置
    tags               TEXT,                    -- JSON 格式的标签数组
    is_active          BOOLEAN DEFAULT 1,       -- 是否激活
    created_at         TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at         TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_global_connections_driver ON global_connections(driver);
CREATE INDEX IF NOT EXISTS idx_global_connections_active ON global_connections(is_active);
CREATE INDEX IF NOT EXISTS idx_global_connections_updated ON global_connections(updated_at DESC);
```

#### 版本 v1.4 - 恢复 Schema 名

```sql
ALTER TABLE global_connections ADD COLUMN schema_name TEXT;
```

**变更说明**:

- 恢复 `schema_name` 字段
- 用于记录 PostgreSQL/Oracle 等多 Schema 数据库的默认 Schema

#### 版本 v1.5 - 恢复 DuckDB 联邦分析开关

```sql
ALTER TABLE global_connections ADD COLUMN use_duckdb_fed BOOLEAN DEFAULT 0;
```

**变更说明**:

- 恢复 `use_duckdb_fed` 字段
- **核心功能**：标记该连接是否启用 DuckDB 联邦查询
- 全局连接同样需要支持 DuckDB 联邦分析

#### 版本 v1.6 - 恢复元数据缓存路径

```sql
ALTER TABLE global_connections ADD COLUMN metadata_path TEXT;
```

**变更说明**:

- 恢复 `metadata_path` 字段
- 记录全局连接的元数据缓存文件路径
- **相对路径**：相对于全局数据目录（如 `metadata/{conn_id}.db`）
- 项目连接的 `metadata_path` 相对于项目根目录（如 `project_metadata/{conn_id}.db`）

#### 当前版本（v1.6 - 最终）

```sql
CREATE TABLE IF NOT EXISTS global_connections (
    id                 TEXT PRIMARY KEY,
    name               TEXT NOT NULL,
    driver             TEXT NOT NULL,           -- 数据库驱动类型 (mysql, postgres, sqlite, duckdb)
    host               TEXT,                    -- 主机地址
    port               INTEGER,                 -- 端口号
    database           TEXT,                    -- 数据库名
    schema_name        TEXT,                    -- 默认 Schema 名（PostgreSQL/Oracle 等）
    username           TEXT,                    -- 用户名
    password_encrypted TEXT,                    -- 加密后的密码
    options            TEXT,                    -- JSON 格式的额外配置
    tags               TEXT,                    -- JSON 格式的标签数组
    use_duckdb_fed     BOOLEAN DEFAULT 0,       -- 是否启用 DuckDB 联邦分析
    metadata_path      TEXT,                    -- 元数据缓存文件路径（相对于全局数据目录）
    is_active          BOOLEAN DEFAULT 1,       -- 是否激活
    created_at         TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at         TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_global_connections_driver ON global_connections(driver);
CREATE INDEX IF NOT EXISTS idx_global_connections_active ON global_connections(is_active);
CREATE INDEX IF NOT EXISTS idx_global_connections_updated ON global_connections(updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_global_connections_duckdb_fed ON global_connections(use_duckdb_fed);
```

---

### 2.3 query_history 表（SQL 查询历史 - 企业级）

**数据库**: 项目 SQLite (`{project_path}/.RSMETA/project.db`)

**变更次数**: 3 次

#### 版本 v1.0 - 初始版本（sql_history）

```sql
CREATE TABLE sql_history (
    id                TEXT PRIMARY KEY,
    connection_id     TEXT,
    sql_text          TEXT NOT NULL,
    execution_time_ms INTEGER,
    rows_affected     INTEGER,
    error_message     TEXT,
    created_at        TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (connection_id) REFERENCES connections(id)
);
```

#### 版本 v1.1 - 收藏支持

```sql
ALTER TABLE sql_history ADD COLUMN is_favorite BOOLEAN DEFAULT 0;
```

**变更说明**:

- 新增 `is_favorite` 字段
- 支持用户收藏常用的 SQL 语句

#### 版本 v2.0 - 重构为 query_history（企业级）

```sql
-- 重命名旧表
ALTER TABLE sql_history RENAME TO sql_history_old;

-- 创建新的 query_history 表
CREATE TABLE query_history (
    id              TEXT PRIMARY KEY,                     -- 历史ID
    connection_id   TEXT,                                 -- 关联连接ID
    database_name   TEXT,                                 -- 执行时数据库
    schema_name     TEXT,                                 -- 执行时Schema
    sql             TEXT NOT NULL,                        -- 执行SQL
    sql_hash        TEXT NOT NULL,                        -- SQL哈希值（去重）
    exec_mode       TEXT NOT NULL DEFAULT 'native',       -- 执行模式：native/duckdb_fed
    category        TEXT NOT NULL DEFAULT 'query',        -- 类型：query/ddl/dml
    success         BOOLEAN DEFAULT 1,                    -- 是否成功
    error_message   TEXT,                                 -- 错误信息
    duration_ms     INTEGER,                              -- 耗时（毫秒）
    rows_returned   INTEGER,                              -- 返回行数
    rows_affected   INTEGER,                              -- 影响行数
    is_pinned       BOOLEAN DEFAULT 0,                    -- 是否固定置顶
    created_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP,  -- 执行时间
    FOREIGN KEY (connection_id) REFERENCES connections(id)
);

-- 创建索引
CREATE INDEX IF NOT EXISTS idx_qh_conn ON query_history(connection_id);
CREATE INDEX IF NOT EXISTS idx_qh_time ON query_history(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_qh_hash ON query_history(sql_hash);
CREATE INDEX IF NOT EXISTS idx_qh_category ON query_history(category);
CREATE INDEX IF NOT EXISTS idx_qh_pinned ON query_history(is_pinned);

-- 创建兼容视图（保持旧代码兼容）
CREATE VIEW IF NOT EXISTS sql_history AS
SELECT
    id,
    connection_id,
    sql AS sql_text,
    duration_ms AS execution_time_ms,
    rows_affected,
    error_message,
    is_pinned AS is_favorite,
    created_at
FROM query_history;
```

**变更说明**:

- 重构为完整的查询历史表，支持企业级功能
- 新增字段：
  - `database_name`: 执行时使用的数据库
  - `schema_name`: 执行时使用的 Schema
  - `sql_hash`: SQL 哈希值，用于去重和慢查询分析
  - `exec_mode`: 执行模式（native/duckdb_fed），区分是否使用 DuckDB 联邦分析
  - `category`: SQL 类型（query/ddl/dml），用于分类统计
  - `success`: 是否执行成功
  - `duration_ms`: 执行耗时
  - `rows_returned`: 返回行数（SELECT 查询）
  - `is_pinned`: 是否置顶（替代 is_favorite）
- 创建 `sql_history` 兼容视图，确保旧代码无需修改

#### 当前版本（v2.0 - 最终）

```sql
CREATE TABLE IF NOT EXISTS query_history (
    id              TEXT PRIMARY KEY,
    connection_id   TEXT,
    database_name   TEXT,
    schema_name     TEXT,
    sql             TEXT NOT NULL,
    sql_hash        TEXT NOT NULL,
    exec_mode       TEXT NOT NULL DEFAULT 'native',
    category        TEXT NOT NULL DEFAULT 'query',
    success         BOOLEAN DEFAULT 1,
    error_message   TEXT,
    duration_ms     INTEGER,
    rows_returned   INTEGER,
    rows_affected   INTEGER,
    is_pinned       BOOLEAN DEFAULT 0,
    created_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (connection_id) REFERENCES connections(id)
);

CREATE INDEX IF NOT EXISTS idx_qh_conn ON query_history(connection_id);
CREATE INDEX IF NOT EXISTS idx_qh_time ON query_history(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_qh_hash ON query_history(sql_hash);
CREATE INDEX IF NOT EXISTS idx_qh_category ON query_history(category);
CREATE INDEX IF NOT EXISTS idx_qh_pinned ON query_history(is_pinned);

-- 兼容视图
CREATE VIEW IF NOT EXISTS sql_history AS
SELECT
    id,
    connection_id,
    sql AS sql_text,
    duration_ms AS execution_time_ms,
    rows_affected,
    error_message,
    is_pinned AS is_favorite,
    created_at
FROM query_history;
```

---

### 2.4 project_info 表（项目索引）

**数据库**: 全局 SQLite (`{data_dir}/RdataStation/system/global.db`)

**变更次数**: 2 次

#### 版本 v1.0 - 初始版本

```sql
CREATE TABLE project_info (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    description TEXT,
    path        TEXT NOT NULL,
    status      TEXT DEFAULT 'active',
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

#### 版本 v1.1 - 最后打开时间

```sql
ALTER TABLE project_info ADD COLUMN last_opened_at TIMESTAMP;
```

**变更说明**:

- 新增 `last_opened_at` 字段
- 用于排序最近使用的项目

#### 当前版本（v1.1 - 最终）

```sql
CREATE TABLE IF NOT EXISTS project_info (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    description     TEXT,
    path            TEXT NOT NULL,
    status          TEXT DEFAULT 'active',
    created_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    last_opened_at  TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_project_info_last_opened ON project_info(last_opened_at DESC);
CREATE INDEX IF NOT EXISTS idx_project_info_status ON project_info(status);
```

---

### 2.5 其他表（无变更）

以下表自创建以来未发生过结构变更：

#### project_settings（项目设置）

```sql
CREATE TABLE IF NOT EXISTS project_settings (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

#### workbench_state（工作台状态）

```sql
CREATE TABLE IF NOT EXISTS workbench_state (
    id              TEXT PRIMARY KEY DEFAULT 'default',
    layout          TEXT,
    open_panels     TEXT,
    active_panel_id TEXT,
    updated_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

#### navigator_states（导航器状态）

```sql
CREATE TABLE IF NOT EXISTS navigator_states (
    id              TEXT PRIMARY KEY,
    connection_id   TEXT NOT NULL,
    expanded_keys   TEXT,
    selected_keys   TEXT,
    filter_config   TEXT,
    updated_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_navigator_states_connection ON navigator_states(connection_id);
```

#### favorite_objects（收藏对象）

```sql
CREATE TABLE IF NOT EXISTS favorite_objects (
    id              TEXT PRIMARY KEY,
    connection_id   TEXT NOT NULL,
    database_name   TEXT,
    schema_name     TEXT,
    object_type     TEXT,
    object_name     TEXT,
    note            TEXT,
    added_at        TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_favorite_objects_connection ON favorite_objects(connection_id);
CREATE INDEX IF NOT EXISTS idx_favorite_objects_type ON favorite_objects(object_type);
```

#### plugins（插件注册）

```sql
CREATE TABLE IF NOT EXISTS plugins (
    id              TEXT PRIMARY KEY,
    code            TEXT NOT NULL,
    name            TEXT NOT NULL,
    version         TEXT NOT NULL,
    author          TEXT,
    description     TEXT,
    repo_url        TEXT,
    plugin_type     TEXT NOT NULL,
    manifest_json   TEXT,
    install_path    TEXT NOT NULL,
    is_enabled      INTEGER DEFAULT 1,
    is_builtin      INTEGER DEFAULT 0,
    installed_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at      TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(code, version)
);
```

#### query_results（查询结果缓存 - DuckDB）

```sql
CREATE TABLE IF NOT EXISTS query_results (
    id                TEXT PRIMARY KEY,
    query_id          TEXT,
    sql_hash          TEXT,
    connection_id     TEXT,
    result_json       TEXT,
    row_count         INTEGER,
    execution_time_ms INTEGER,
    created_at        TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_query_results_connection ON query_results(connection_id);
CREATE INDEX IF NOT EXISTS idx_query_results_created ON query_results(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_query_results_hash ON query_results(sql_hash);
```

#### analytics（数据分析 - DuckDB）

```sql
CREATE TABLE IF NOT EXISTS analytics (
    id                TEXT PRIMARY KEY,
    analysis_type     TEXT,
    source_connection TEXT,
    source_table      TEXT,
    analysis_json     TEXT,
    created_at        TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_analytics_type ON analytics(analysis_type);
CREATE INDEX IF NOT EXISTS idx_analytics_source ON analytics(source_connection);
```

#### metadata（连接元数据）

```sql
CREATE TABLE IF NOT EXISTS metadata (
    id              TEXT PRIMARY KEY,
    obj_type        TEXT NOT NULL,              -- 对象类型: table/view/column/index/function/trigger/procedure
    database_name   TEXT NOT NULL,              -- 数据库名
    schema_name     TEXT NOT NULL,              -- 模式名
    table_name      TEXT NOT NULL,              -- 表名
    name            TEXT,                       -- 对象名称
    data_type       TEXT,                       -- 数据类型（仅列）
    is_nullable     INTEGER,                    -- 是否可空
    is_primary      INTEGER DEFAULT 0,          -- 是否主键
    is_unique       INTEGER DEFAULT 0,          -- 是否唯一
    comment         TEXT,                       -- 注释
    definition      TEXT,                       -- 定义（视图/函数/过程）
    extra           TEXT,                       -- JSON 格式的额外信息
    last_sync       INTEGER NOT NULL            -- 最后同步时间戳
);

CREATE INDEX IF NOT EXISTS idx_meta_obj ON metadata(obj_type);
CREATE INDEX IF NOT EXISTS idx_meta_search ON metadata(database_name, schema_name, table_name, name);
CREATE INDEX IF NOT EXISTS idx_meta_schema ON metadata(database_name, schema_name);
CREATE INDEX IF NOT EXISTS idx_meta_table ON metadata(database_name, schema_name, table_name);
```

#### sync_log（元数据同步日志）

```sql
CREATE TABLE IF NOT EXISTS sync_log (
    id              TEXT PRIMARY KEY,
    start_at        INTEGER NOT NULL,           -- 开始时间戳
    end_at          INTEGER NOT NULL,           -- 结束时间戳
    success         INTEGER DEFAULT 1,          -- 是否成功
    message         TEXT,                       -- 日志消息
    objects_fetched INTEGER DEFAULT 0           -- 获取的对象数量
);

CREATE INDEX IF NOT EXISTS idx_sync_log_time ON sync_log(start_at DESC);
CREATE INDEX IF NOT EXISTS idx_sync_log_success ON sync_log(success);
```

---

## 三、迁移文件对应关系

由于项目经历了多次调整，当前迁移文件与表结构的对应关系如下：

| 迁移文件                                      | 对应版本                  | 说明             |
| --------------------------------------------- | ------------------------- | ---------------- |
| `migrations/project_meta/001_init.sql`        | v1.4 (connections)        | 包含所有最新变更 |
| `migrations/global/001_init.sql`              | v1.3 (global_connections) | 包含所有最新变更 |
| `migrations/project_analysis/001_init.sql`    | v1.0                      | 初始版本         |
| `migrations/connection_metadata/001_init.sql` | v1.0                      | 初始版本         |

**注意**: 所有迁移文件当前都是 `001_init.sql`，表示这些是完整表结构的定义，而非增量变更。

---

## 四、未来迁移策略

当需要修改表结构时，应遵循以下策略：

### 4.1 添加新字段

创建新的迁移文件，如 `002_add_new_column.sql`:

```sql
ALTER TABLE connections ADD COLUMN new_column TEXT;
```

### 4.2 修改字段类型

创建新的迁移文件，如 `003_modify_column.sql`:

```sql
-- SQLite 不支持直接修改列类型，需要重建表
CREATE TABLE connections_new AS SELECT * FROM connections;
DROP TABLE connections;
ALTER TABLE connections_new RENAME TO connections;
```

### 4.3 删除字段

创建新的迁移文件，如 `004_drop_column.sql`:

```sql
-- SQLite 不支持直接删除列（3.35.0+ 支持），需要重建表
```

### 4.4 添加索引

创建新的迁移文件，如 `005_add_index.sql`:

```sql
CREATE INDEX IF NOT EXISTS idx_connections_name ON connections(name);
```

---

## 五、已废弃的字段

| 表名               | 字段名   | 废弃版本 | 替代方案           |
| ------------------ | -------- | -------- | ------------------ |
| connections        | password | v1.1     | password_encrypted |
| connections        | db_type  | v1.2     | driver             |
| global_connections | password | v1.1     | password_encrypted |

---

## 六、前端 TypeScript 类型对应关系

### ProjectConnection（前端）

```typescript
export interface ProjectConnection {
  id: string
  name: string
  driver: string // 对应 driver 字段
  host?: string // 对应 host 字段
  port?: number // 对应 port 字段
  database?: string // 对应 database 字段
  schema_name?: string // 对应 schema_name 字段（PostgreSQL/Oracle 等）
  username?: string // 对应 username 字段
  password?: string // 对应 password_encrypted 字段（传输时）
  options?: string // 对应 options 字段
  tags?: string // 对应 tags 字段
  use_duckdb_fed?: boolean // 对应 use_duckdb_fed 字段（DuckDB 联邦分析开关）
  metadata_path?: string // 对应 metadata_path 字段（元数据缓存路径）
  is_active?: boolean // 对应 is_active 字段
  status?: ConnectionStatus // 运行时状态（不存储）
  error_message?: string // 运行时错误（不存储）
  last_connected_at?: string // 运行时信息（不存储）
  created_at: string
  updated_at: string
}
```

### 字段映射说明

| 前端字段       | 后端字段           | 说明                       |
| -------------- | ------------------ | -------------------------- |
| driver         | driver             | 一致                       |
| password       | password_encrypted | 前端传输明文，后端加密存储 |
| schema_name    | schema_name        | 默认 Schema 名             |
| use_duckdb_fed | use_duckdb_fed     | DuckDB 联邦分析开关        |
| metadata_path  | metadata_path      | 元数据缓存文件路径         |
| status         | -                  | 运行时状态，不持久化       |
| error_message  | -                  | 运行时错误，不持久化       |
