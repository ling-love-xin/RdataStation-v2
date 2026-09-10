# 双层存储架构

> 版本：v1.0
> 最后更新：2026-05-09
> 状态：✅ 已实现

## 概述

RdataStation 采用 **SQLite + DuckDB 双层存储架构**。SQLite 负责事务性数据（连接信息、历史记录、项目元数据），DuckDB 负责分析性数据（联邦查询、洞察分析、大数据加速）。

## 架构总览

```
┌─────────────────────────────────────────────────────────────┐
│                    双层存储架构                              │
├──────────────────────────┬──────────────────────────────────┤
│        SQLite 事务层      │        DuckDB 分析层             │
│    (OLTP / 事务性操作)    │    (OLAP / 分析性操作)           │
├──────────────────────────┼──────────────────────────────────┤
│  • 连接信息存储           │  • 联邦查询引擎                  │
│  • SQL 历史记录           │  • 大数据集分析加速              │
│  • 项目元数据索引         │  • 洞察结果持久化                │
│  • 工作台状态             │  • 临时表 / 中间结果             │
│  • 元数据缓存             │  • CSV/Parquet 外部文件导入      │
│  • 分析资源管理           │  • 跨库 JOIN 查询               │
│  • 数据校验（checksum）   │  • 结果集导出                   │
├──────────────────────────┼──────────────────────────────────┤
│  连接池（WAL + 共享缓存）  │  单例长连接（serialized）        │
│  并发支持                 │  串行化执行                      │
└──────────────────────────┴──────────────────────────────────┘
         │                           │
         └───────────┬───────────────┘
                     │
          ┌──────────▼──────────┐
          │       全局层        │
          │  system/global.db   │
          │  system/analytics/  │
          └──────────┬──────────┘
                     │
          ┌──────────▼──────────┐
          │       项目层        │
          │  项目/meta/*.db     │
          │  项目/analytics/    │
          └─────────────────────┘
```

## 配置分层

| 层级     | SQLite 用途                         | DuckDB 用途              | 生命周期      |
| -------- | ----------------------------------- | ------------------------ | ------------- |
| **全局** | 最近连接列表、SQL 历史、最近项目    | 全局查询分析、跨项目缓存 | 应用启动→退出 |
| **项目** | 项目连接信息、项目 SQL 历史、元数据 | 联邦查询、分析洞察       | 项目打开→关闭 |

## 一、全局系统存储

### 初始化入口

**路径**: [lib.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/lib.rs#L48)

```rust
// 应用启动时初始化
rt.block_on(core::migration::initialize_global_system())?;
rt.block_on(core::driver::init_driver_manager())?;
```

### 全局 SQLite（系统数据库）

**路径**: [persistence/global_db.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/persistence/global_db.rs)

```
存储位置: system/global.db
```

**核心表**：

| 表名              | 用途         | 实现文件              |
| ----------------- | ------------ | --------------------- |
| `connections`     | 全局连接信息 | connection_store.rs   |
| `sql_history`     | SQL 执行历史 | history_store.rs      |
| `recent_projects` | 最近打开项目 | (project_commands.rs) |

**连接池特性**：

- **WAL 模式**：Write-Ahead Logging，支持并发读写
- **共享缓存**：多连接共享同一缓存，减少内存占用
- **信号量控制**：`Arc<Semaphore>` 控制最大并发连接数
- **RAII 归还**：`SqlitePoolConnection` drop 时自动归还连接

```rust
pub struct GlobalSqlitePool {
    pool: Arc<Mutex<Vec<SqliteConnection>>>,
    semaphore: Arc<Semaphore>,
    db_path: PathBuf,
}
```

### 全局 DuckDB（分析数据库）

```
存储位置: system/analytics/global.duckdb
```

**用途**：

- 全局查询分析结果缓存
- 跨项目数据共享
- DuckDB 扩展管理

**连接方式**：单例长连接（`Arc<Mutex<DuckConnection>>`），DuckDB 不支持并发写入，通过 Mutex 串行化。

### 全局数据库迁移

**路径**: [migration/](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/migration/)

```
MigrationManager
├── MigrationType::GlobalSqlite   → system/global.db
├── MigrationType::GlobalDuckDB   → system/analytics/global.duckdb
├── MigrationType::ProjectSqlite  → {project}/meta/project.db
└── MigrationType::ProjectDuckDB  → {project}/analytics/data.duckdb
```

## 二、项目存储

### 项目目录结构

```
{project_path}/
├── meta/                       # SQLite 事务层
│   ├── project.db              # 项目元数据索引
│   └── connection_metadata/    # 连接级元数据缓存
│       └── conn_{id}.sqlite    # 每个连接独立的缓存
├── analytics/                   # DuckDB 分析层
│   └── data.duckdb             # 分析数据 + 版本载体
├── config/                      # 项目配置
│   ├── connections.json        # 连接配置
│   └── sql/                    # SQL 文件
└── scratchpad/                  # 草稿箱（用户脚本/笔记）
```

### 项目 SQLite

**路径**: [persistence/project_db.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/persistence/project_db.rs)

```
文件: meta/project.db
```

**核心表**：

| 表名                  | 用途               | 实现文件                    |
| --------------------- | ------------------ | --------------------------- |
| `project_info`        | 项目基本信息       | project_store.rs            |
| `connections`         | 项目连接信息       | project_connection_store.rs |
| `sql_history`         | 项目 SQL 历史      | (project_store.rs)          |
| `workbench_state`     | 工作台布局状态     | workbench_context_store.rs  |
| `sql_templates`       | SQL 模板           | sql_template_store.rs       |
| `analytics_resources` | 分析资源（图表等） | analytics_resource_store.rs |
| `insight_records`     | 洞察记录           | insight_store.rs            |
| `insight_meta`        | 洞察元数据         | insight_meta_store.rs       |
| `cache_version`       | 缓存版本管理       | cache_version_migration.rs  |

### 项目 DuckDB

```
文件: analytics/data.duckdb
```

**用途**：

- **联邦查询引擎**：ATTACH 外部数据库，执行跨库 JOIN
- **分析加速**：复杂查询路由到 DuckDB 加速执行
- **版本载体**：分析数据的版本化管理
- **结果集持久化**：大结果集缓存

## 三、连接级元数据缓存

### MetadataCacheManager

**路径**: [persistence/metadata_cache.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/persistence/metadata_cache.rs)

每个数据库连接有独立的 SQLite 元数据缓存：

```
全局连接: system/global_metadata/conn_{id}.sqlite
项目连接: {project}/meta/connection_metadata/conn_{id}.sqlite
```

**缓存内容**：

- 表列表及其元数据
- 列列表及其元数据
- Schema 层级结构
- 缓存版本号（用于失效检测）

**缓存预热**：

通过 `cache_warming_commands` 模块支持：

- `start_cache_warming` - 开始预热
- `get_warming_progress` - 获取进度
- `cancel_cache_warming` - 取消预热
- `check_cache_version` - 版本检查
- `execute_cache_migration` - 缓存迁移

### 缓存表结构

```sql
-- 表元数据缓存
CREATE TABLE IF NOT EXISTS table_metadata (
    conn_id TEXT NOT NULL,
    db_name TEXT NOT NULL,
    schema_name TEXT NOT NULL DEFAULT '',
    table_name TEXT NOT NULL,
    table_type TEXT NOT NULL DEFAULT 'table',  -- table/view
    row_count_estimate INTEGER,
    comment TEXT,
    cached_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (conn_id, db_name, schema_name, table_name)
);

-- 列元数据缓存
CREATE TABLE IF NOT EXISTS column_metadata (
    conn_id TEXT NOT NULL,
    db_name TEXT NOT NULL,
    schema_name TEXT NOT NULL DEFAULT '',
    table_name TEXT NOT NULL,
    column_name TEXT NOT NULL,
    ordinal_position INTEGER,
    data_type TEXT,
    is_nullable BOOLEAN,
    is_primary_key BOOLEAN DEFAULT FALSE,
    default_value TEXT,
    comment TEXT,
    cached_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (conn_id, db_name, schema_name, table_name, column_name)
);
```

## 四、洞察存储

### InsightStore

**路径**: [persistence/insight_store.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/persistence/insight_store.rs)

存储列级别的数据分析洞察结果，支持：

- 列画像（profile）：min/max/avg/distinct_count/null_count
- 列质量（quality）：异常值检测、模式匹配
- 洞察版本管理
- 洞察历史查询

### InsightMetaStore

**路径**: [persistence/insight_meta_store.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/persistence/insight_meta_store.rs)

存储洞察规则的元数据配置。

## 五、草稿箱存储

### Scratchpad

**路径**: [core/scratchpad/](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/core/scratchpad/)

```
{project_path}/scratchpad/
├── index.sqlite             # 草稿箱索引
├── entries/                 # 草稿条目
└── trash/                   # 回收站
```

支持功能：

- 创建/删除/重命名草稿
- 导入外部文件
- 回收站恢复
- 提升为分析资源（promote to resource）

## 六、数据流示意

```
用户操作
    │
    ▼
┌─────────────────────────────────────────┐
│              Tauri Commands              │
└───────────────────┬─────────────────────┘
                    │
        ┌───────────┼───────────┐
        ▼           ▼           ▼
   ┌─────────┐ ┌─────────┐ ┌─────────┐
   │ 连接管理 │ │ SQL 执行 │ │ 分析洞察 │
   └────┬────┘ └────┬────┘ └────┬────┘
        │           │           │
        ▼           ▼           ▼
   ┌─────────────────────────────────────┐
   │            SQLite 事务层             │
   │  connection_store / history_store   │
   │  project_store / metadata_cache     │
   └─────────────────────────────────────┘
        │
        │  大数据量/跨库/分析查询
        ▼
   ┌─────────────────────────────────────┐
   │            DuckDB 分析层             │
   │  联邦查询 / 洞察引擎 / 临时表        │
   │  ATTACH mysql_pg ATTACH sqlite...   │
   └─────────────────────────────────────┘
```

## 七、关键设计决策

### 为什么用 SQLite 做主存储？

1. **零配置**：无需独立数据库服务，嵌入式运行
2. **WAL 模式**：支持并发读写，性能和可靠性有保障
3. **单文件**：项目可整体打包、迁移
4. **成熟稳定**：全球最广泛部署的嵌入式数据库

### 为什么用 DuckDB 做分析引擎？

1. **OLAP 原生**：列式存储，向量化执行，分析查询比 SQLite 快 10-100 倍
2. **联邦查询**：ATTACH 外部数据库，支持跨库 JOIN
3. **Arrow 原生**：与 WASM 插件零拷贝交互
4. **文件导入**：直接读取 CSV/Parquet/JSON/Excel

### 双层分离原则

| 操作类型      | SQLite | DuckDB | 判断依据           |
| ------------- | ------ | ------ | ------------------ |
| 连接信息 CRUD | ✅     | ❌     | 事务性、小数据量   |
| SQL 历史记录  | ✅     | ❌     | 高频写入、小数据量 |
| 元数据缓存    | ✅     | ❌     | 需要并发读         |
| 大结果集缓存  | ❌     | ✅     | 大数据量           |
| 联邦查询      | ❌     | ✅     | DuckDB 原生能力    |
| 洞察分析      | ❌     | ✅     | OLAP 场景          |
| 跨库 JOIN     | ❌     | ✅     | 需要 DuckDB ATTACH |

### 版本化支持

所有核心模型支持 `Versioned<T>` 包装器：

```rust
pub struct Versioned<T> {
    pub data: T,
    pub version_id: String,
    pub parent_version_id: Option<String>,
    pub created_by: Option<String>,    // DuckLake 预留
    pub checksum: String,              // SHA-256
    pub created_at: DateTime<Utc>,
}
```

## 八、存储路径汇总

| 存储内容       | 全局路径                                  | 项目路径                              |
| -------------- | ----------------------------------------- | ------------------------------------- |
| 系统数据库     | `system/global.db`                        | `{project}/meta/project.db`           |
| DuckDB 分析库  | `system/analytics/global.duckdb`          | `{project}/analytics/data.duckdb`     |
| 连接元数据缓存 | `system/global_metadata/conn_{id}.sqlite` | `{project}/meta/connection_metadata/` |
| 连接配置       | `system/connections.json`                 | `{project}/config/connections.json`   |
| SQL 文件       | -                                         | `{project}/config/sql/`               |
| 草稿箱         | -                                         | `{project}/scratchpad/`               |

## 九、后续演进

### DuckLake 网络存储（预留）

```
本地: /path/to/project/
网络: ducklake://project-id       # 远程协作存储
```

- `Versioned<T>` 的 `created_by` 字段预留多用户标识
- `checksum` 字段支持数据完整性校验
- DuckDB 作为版本载体

### 项目快照与分享

- 项目整体打包（SQLite + DuckDB + Config）
- 版本快照对比
- 项目导出/导入
