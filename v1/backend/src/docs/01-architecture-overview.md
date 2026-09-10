# 架构概述

> 版本：v2.0
> 最后更新：2026-05-09
> 状态：✅ 实际代码对齐

## 设计目标

RdataStation 后端架构设计目标：

1. **可维护性**：代码结构清晰，易于理解和维护
2. **可测试性**：业务逻辑可独立单元测试
3. **可扩展性**：轻松添加新的数据库支持（当前锁定 4 种，后续通过插件扩展）
4. **可移植性**：Core 层无框架依赖，可复用
5. **性能**：启动速度 < 1.5s，核心内存 < 150MB
6. **10 年兼容**：核心接口保持向前兼容

## 架构风格

采用**分层架构（Layered Architecture）** + **六边形架构（Hexagonal Architecture）** 混合风格：

### 分层架构（实际）

```
┌──────────────────────────────────────────────────────┐
│                Presentation Layer                     │
│  commands/ 目录（Tauri Command 实现）                  │
│  - connection_commands / driver_commands              │
│  - sql_commands / project_commands                    │
│  - metadata_cache_commands / analytics_resource       │
│  - 输入校验、DTO 转换、错误处理                        │
├──────────────────────────────────────────────────────┤
│                Service Layer                          │
│  core/services/                                       │
│  - ConnectionService（连接创建、管理）                  │
│  - ConnectionManager（连接生命周期）                   │
│  - SqlService（SQL 执行、事务）                        │
├──────────────────────────────────────────────────────┤
│                Domain Layer (Core)                    │
│  core/driver/traits.rs  - Database / Transaction / DbPool │
│  core/driver/registry.rs - DriverRegistry + DriverFactory │
│  core/driver/router.rs - DataSourceRouter             │
│  core/models.rs - QueryResult / Row / Value           │
│  core/error.rs  - CoreError 统一错误                   │
├──────────────────────────────────────────────────────┤
│                Infrastructure Layer                   │
│  ├── driver/native/    4 种原生驱动（mysql/pg/sqlite/duckdb）│
│  ├── driver/jdbc/      JDBC 桥接（骨架）              │
│  ├── driver/wasm/      WASM 插件（骨架）              │
│  ├── driver/smart_pool.rs - 智能连接池                │
│  └── persistence/      双层存储（SQLite + DuckDB）    │
│      ├── global_db.rs  - 全局系统数据库               │
│      ├── project_db.rs - 项目数据库                   │
│      ├── connection_store.rs / history_store.rs       │
│      ├── metadata_cache.rs / insight_store.rs         │
│      └── analytics_resource_store.rs                  │
└──────────────────────────────────────────────────────┘
```

## 核心模块

### 1. API 层 (`api/`)

**职责**：定义前后端共享的数据类型

```rust
// api/dto.rs - 实际存在的类型
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub execution_time_ms: u64,
    pub affected_rows: Option<u64>,
    pub is_read_only: bool,
}

pub struct ErrorResponse {
    pub code: String,
    pub message: String,
    pub details: Option<String>,
}
```

### 2. Core 层 (`core/`)

**职责**：纯业务逻辑，无框架依赖

#### 2.1 Driver 模块 (`driver/`)

数据库驱动抽象，详见 [05-驱动架构](./05-driver-architecture.md)。

核心 Trait：

- `Database`：SQL 执行 + 元数据 + 联邦查询
- `Transaction`：事务管理
- `DbPool`：连接池抽象
- `DriverFactory`：驱动工厂（创建连接）

当前实现：4 种内置驱动（MySQL/sqlx, PostgreSQL/sqlx, SQLite/rusqlite, DuckDB/duckdb-rs）

#### 2.2 驱动路由（原 Datasource 模块，已合并至 driver/router.rs）

数据源路由层，详见 [05-驱动架构](./05-driver-architecture.md#数据源路由层)：

- `DataSourceRouter::route(config)` - 根据配置从 DriverRegistry 获取工厂并创建连接

#### 2.3 Services 模块 (`services/`)

业务服务：

- `ConnectionManager`：连接生命周期管理（创建、复用、关闭、切换）
- `ConnectionService`：连接业务逻辑（连接、断开、测试、元数据缓存初始化）
- `SqlService`：SQL 执行业务（查询、事务、历史记录）

#### 2.4 Persistence 模块 (`persistence/`)

双层存储架构，详见 [06-存储架构](./06-storage-architecture.md)。

全局层：`system/global.db` (SQLite) + `system/analytics/global.duckdb` (DuckDB)
项目层：`{project}/meta/project.db` (SQLite) + `{project}/analytics/data.duckdb` (DuckDB)

### 3. Commands 层 (`commands/`)

**职责**：Tauri Command 实现，按功能组织

| 模块                             | 职责       | 命令数 |
| -------------------------------- | ---------- | ------ |
| `connection_commands.rs`         | 连接管理   | ~15    |
| `driver_commands.rs`             | 驱动管理   | ~4     |
| `sql_commands.rs`                | SQL 执行   | ~12    |
| `project_commands.rs`            | 项目管理   | ~12    |
| `project_store_commands.rs`      | 项目存储   | ~8     |
| `metadata_cache_commands.rs`     | 元数据缓存 | ~8     |
| `cache_warming_commands.rs`      | 缓存预热   | ~9     |
| `analytics_resource_commands.rs` | 分析资源   | ~25    |

> ⚠️ 命令已从 `adapters/tauri/command.rs` 迁移到 `commands/` 目录。旧路径不再有效。

## 数据流

### SQL 执行流程（实际）

```
Frontend (Vue3)
    │ invoke('execute_sql', { connId, sql })
    ▼
Tauri Runtime
    │
    ▼
commands/sql_commands.rs
    │ 1. 从 ConnectionManager 获取连接
    │ 2. 调用 db.query(sql)
    ▼
driver/native/{db}.rs
    │ Database::query()
    │ → QueryResult { columns, rows, ... }
    ▼
commands/sql_commands.rs
    │ 返回 QueryResult（自动序列化为 JSON）
    ▼
Frontend
```

### 连接创建流程（实际）

```
Frontend
    │ invoke('create_connection', { config })
    ▼
commands/connection_commands.rs
    │
    ▼
ConnectionService::connect(config)
    │ 1. create_database(db_type, url) [P0: 硬编码]
    │ 2. 注册到 ConnectionManager
    │ 3. 初始化元数据缓存
    ▼
ConnectionManager
    │ 管理连接生命周期
```

> 详见 [04-数据流设计](./04-data-flow.md)

## 双层存储架构

```
┌──────────────────────────────────────┐
│           SQLite 事务层               │
│  • 连接信息 / SQL 历史 / 项目元数据    │
│  • 元数据缓存 / 分析资源管理          │
│  • WAL 模式 + 共享缓存 + 并发池       │
├──────────────────────────────────────┤
│           DuckDB 分析层               │
│  • 联邦查询 / 洞察分析 / 大数据加速   │
│  • Arrow 原生 / CSV/Parquet 导入      │
│  • 单例长连接（串行化）              │
└──────────────────────────────────────┘
```

> 详见 [06-存储架构](./06-storage-architecture.md)

## 升级策略

分层升级，禁止主版本升级：

| 依赖     | 当前版本 | 策略                     |
| -------- | -------- | ------------------------ |
| Tokio    | 1.44.x   | ✅ minor/patch，❌ major |
| Tauri    | 2.10.x   | ✅ patch only            |
| sqlx     | 0.8.x    | ✅ patch only            |
| wasmtime | 43.0.x   | ✅ minor/patch，❌ major |

> 详见 [07-升级策略](./07-upgrade-strategy.md)

## 扩展点

### 1. 添加新数据库驱动（后续阶段）

当前锁定 4 种内置数据库。后续通过以下方式新增：

- **JVM Sidecar**：JDBC 驱动通过 Go Sidecar 管理 JVM，gRPC + Arrow Flight 通信
- **WASM 插件**：通过 wasmtime 加载 .wasm 插件
- **ADBC**：Arrow Database Connectivity 标准协议

### 2. 添加新 Tauri Command

步骤：

1. 在 `commands/` 目录创建或编辑命令文件
2. 在 `commands/mod.rs` 导出
3. 在 `lib.rs` 的 `invoke_handler` 中注册

## 当前 P0 问题

| 编号 | 问题                                           | 影响                   |
| ---- | ---------------------------------------------- | ---------------------- |
| P0-1 | `DRIVER_FACTORY_MANAGER` 重复注册（✅ 已移除） | 维护两套注册表         |
| P0-2 | `create_database()` 硬编码匹配                 | 新增数据库需改多处代码 |
| P0-3 | `to_url()` 硬编码匹配                          | 同上                   |
| P0-4 | `SchemaObject` 缺少列详情                      | 无法展示列注释/类型    |

> 详见 [05-驱动架构](./05-driver-architecture.md#p0-问题总结)

## 五阶段调整计划

| 阶段 | 目标                    | 内容                                  |
| ---- | ----------------------- | ------------------------------------- |
| 1    | 架构归一化（✅ 已完成） | 消除 DRIVER_FACTORY_MANAGER，统一创建 |
| 2    | Database trait 增强     | 引入 MetadataBrowser trait            |
| 3    | DriverDescriptor 增强   | 增加 driver_kind / url_template       |
| 4    | Command 层清理          | 合并重复命令，统一走 DataSourceRouter |
| 5    | 质量保证                | 单元测试、集成测试、clippy/fmt        |

## 性能目标

- ✅ 启动速度 < 1.5 秒
- ✅ Core 内存 < 150MB（MVP）
- ✅ 插件内存 ≤ 500MB（可配置）
- ✅ 插件崩溃不影响主程序

## 技术栈总览

| 层级       | 技术                              | 版本                 |
| ---------- | --------------------------------- | -------------------- |
| **Rust**   | Edition / Tokio / Tauri           | 2021 / 1.44 / 2.10   |
| **数据库** | sqlx / rusqlite / duckdb-rs       | 0.8 / 0.32 / 1.10502 |
| **插件**   | wasmtime / Arrow                  | 43.0 / 53.0          |
| **前端**   | Vue 3 / TypeScript / Vite         | 3.5 / 5.8 / 6        |
| **UI**     | naive-ui / dockview-vue / AG Grid | latest / 5.2 / 33    |
| **编辑器** | Monaco Editor                     | 0.52                 |

## 相关文档

| 文档                                        | 说明                     |
| ------------------------------------------- | ------------------------ |
| [02-目录结构](./02-directory-structure.md)  | 目录组织及职责           |
| [03-模块依赖](./03-module-dependencies.md)  | 依赖关系及约束           |
| [04-数据流](./04-data-flow.md)              | 请求处理流程             |
| [05-驱动架构](./05-driver-architecture.md)  | 数据库驱动设计           |
| [06-存储架构](./06-storage-architecture.md) | SQLite + DuckDB 双层存储 |
| [07-升级策略](./07-upgrade-strategy.md)     | 版本升级策略             |
| [09-开发指南](./09-development-guide.md)    | 开发规范及实践           |
| [10-API 参考](./10-api-reference.md)        | Tauri 命令参考           |
