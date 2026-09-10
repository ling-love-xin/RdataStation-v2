# DBI - 统一数据访问入口 (Database Interface)

## 概述

DBI (Database Interface) 是 RdataStation 的核心数据访问层，作为**统一数据访问入口**，负责：

- 统一查询/执行接口
- 会话和事务管理
- 查询上下文传递
- 多引擎路由（原生驱动 / DuckDB 加速 / 流处理）

## 架构

```
dbi/
├── mod.rs                # 模块入口，重新导出公共接口
├── dbi.rs                # DBI 结构体，对外唯一接口
├── session.rs            # 会话管理（会话级/持久化结果集）
├── context.rs            # 查询上下文和执行上下文
└── engine/
    ├── mod.rs            # 引擎模块入口
    ├── driver_engine.rs  # 原生数据库驱动执行
    ├── duckdb_engine.rs  # DuckDB 本地加速/联邦查询
    └── stream_engine.rs  # 流式处理、合并、后处理
```

## 核心概念

### 1. DBI 结构体

```rust
let dbi = DBI::new(router, session);

// 查询（只读）
let result = dbi.query("SELECT * FROM users", ExecutionMode::DuckDB).await?;

// 执行（写操作）
let result = dbi.execute("INSERT INTO users ...").await?;
```

### 2. 执行模式

| 模式         | 说明               | 适用场景            |
| ------------ | ------------------ | ------------------- |
| `Native`     | 原生数据库驱动执行 | 写操作、简单查询    |
| `DuckDB`     | DuckDB 加速执行    | 复杂分析、跨库 JOIN |
| `Stream`     | 流式执行           | 大数据量、流式处理  |
| `UserChoice` | 智能推荐           | 由系统自动判断      |

### 3. 会话模式

| 模式         | 说明               | 存储位置                |
| ------------ | ------------------ | ----------------------- |
| `Session`    | 会话级，关闭后消失 | DuckDB 内存表           |
| `Persistent` | 持久化，保存到项目 | `analytics/data.duckdb` |

### 4. 结果集管理

```rust
// 注册结果集
session.register_result_set("my_analysis", Some(sql), SessionMode::Persistent).await?;

// 获取结果集
let meta = session.get_result_set("my_analysis").await;

// 列出所有结果集
let sets = session.list_result_sets().await;
```

## DuckDB 加速

### 注册外部数据库

```rust
duckdb_engine.register_external_database(
    "mysql_prod",
    "mysql",
    "mysql://user:pass@host:3306/db"
).await?;
```

### 加载文件数据源

```rust
// CSV 文件
duckdb_engine.load_file_source("/path/to/data.csv", "csv_data").await?;

// Parquet 文件
duckdb_engine.load_file_source("/path/to/data.parquet", "parquet_data").await?;
```

## 智能推荐

DBI 内置智能推荐引擎，根据 SQL 特征自动推荐执行模式：

| SQL 特征               | 推荐模式   | 原因             |
| ---------------------- | ---------- | ---------------- |
| `INSERT/UPDATE/DELETE` | Native     | 写操作必须走原生 |
| `GROUP BY`             | DuckDB     | 列式存储聚合快   |
| `JOIN`                 | DuckDB     | 向量化执行       |
| `ORDER BY + LIMIT`     | DuckDB     | 排序优化         |
| 简单 `SELECT`          | UserChoice | 由用户决定       |

## 数据流

```
SQL 编辑器
    │
    ▼
┌─────────┐
│   DBI   │
└────┬────┘
     │
     ▼
┌─────────────┐
│ QueryRouter │
└──┬───┬───┬──┘
   │   │   │
   ▼   ▼   ▼
┌────┐┌─────┐┌──────┐
│驱动││DuckDB││Stream│
└────┘└─────┘└──────┘
   │   │   │
   ▼   ▼   ▼
┌─────────────┐
│ Arrow 结果  │
└──────┬──────┘
       │
       ▼
   前端渲染
```

## 依赖关系

```
dbi → driver (原生驱动)
dbi → error (错误处理)
dbi → models (数据模型)
dbi → stream (流式处理)
```

## 注意事项

1. **只读保证**: DuckDB 加速模式下，外部数据库连接必须是只读的
2. **连接安全**: 外部数据库连接信息需要加密存储
3. **结果集命名**: 支持用户自定义名称，需要避免冲突
4. **文件路径**: 使用绝对路径，避免相对路径问题
5. **Arrow 传输**: 所有数据流转使用 Arrow 格式，零拷贝
