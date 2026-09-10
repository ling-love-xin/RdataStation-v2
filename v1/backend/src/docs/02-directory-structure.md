# 目录结构

> 版本：v2.0
> 最后更新：2026-05-09
> 状态：✅ 实际代码对齐

## 整体结构

```
src-tauri/src/
├── lib.rs                              # 主入口：模块导出、70+ 命令注册
├── main.rs                             # 程序启动入口
│
├── api/                                # API 层（前后端共享 DTO）
│   ├── mod.rs                          # 模块导出
│   └── dto.rs                          # QueryResult / Row / Value / ErrorResponse
│
├── core/                               # 核心业务层（无框架依赖）
│   ├── mod.rs                          # 模块导出
│   ├── error.rs                        # CoreError 统一错误定义
│   ├── models.rs                       # 核心数据模型（QueryResult / Row / Value）
│   ├── port_negotiation.rs             # 端口协商
│   │
│   ├── driver/                         # 驱动层（驱动核心 + 连接管理 + 数据源路由）
│   │   ├── mod.rs                      # 模块导出 + re-export
│   │   ├── traits.rs                   # Database / Transaction / DbPool / SchemaObject
│   │   ├── registry/                   # DriverRegistry 子模块
│   │   │   ├── mod.rs                  # DriverRegistry + DriverFactory trait + 全局单例
│   │   │   ├── config.rs               # DriverConnectionConfig + to_url() + URL builder
│   │   │   └── descriptors.rs          # DriverDescriptor + DriverKind + 6 个内置描述符
│   │   ├── factory.rs                  # 6 个 DriverFactory 实现
│   │   ├── router.rs                   # DataSourceRouter（原 datasource/router.rs 迁移至此）
│   │   ├── auto_register.rs            # AutoDriverRegistrar（委托 BuiltinDriverDiscovery，唯一真相源 × 6）
│   │   ├── manager.rs                  # DriverManager（全局驱动状态，同源 BuiltinDriverDiscovery × 6）
│   │   ├── metadata.rs                 # DriverMetadata / DriverType / DriverIcon
│   │   ├── loader.rs                   # DriverLoader + BuiltinDriverDiscovery（唯一真相源）
│   │   ├── utils.rs                    # build_connection_url / validate_driver_config
│   │   ├── smart_pool.rs               # SmartPool 智能连接池
│   │   ├── driver_config.rs            # 驱动配置（待合并）
│   │   │
│   │   ├── connection/                 # 连接管理（原 core/connection/ 迁移至此）
│   │   │   ├── mod.rs
│   │   │   ├── config.rs               # ConnectionConfig / ConnectionMethod
│   │   │   ├── connector.rs            # Connector trait + 实现（Direct/SSL/SSH/Proxy）
│   │   │   ├── factory.rs              # ConnectionFactory
│   │   │   └── stream.rs               # ConnectionStream
│   │   │
│   │   ├── native/                     # 原生驱动实现
│   │   │   ├── mod.rs
│   │   │   ├── mysql.rs                # MySqlDatabase（sqlx）
│   │   │   ├── mysql_pool.rs           # MySQL 连接池
│   │   │   ├── postgres.rs             # PostgresDatabase（sqlx）
│   │   │   ├── postgres_pool.rs        # PostgreSQL 连接池
│   │   │   ├── sqlite.rs               # SqliteDatabase（rusqlite）
│   │   │   ├── sqlite_pool.rs          # SQLite 连接池
│   │   │   ├── duckdb.rs               # DuckDbDatabase（duckdb-rs）
│   │   │   └── duckdb_pool.rs          # DuckDB 连接池
│   │   │
│   │   ├── jdbc/                       # JDBC 桥接（骨架，待 Go Sidecar）
│   │   ├── wasm/                       # WASM 插件驱动（骨架）
│   │   └── tests/                      # 驱动测试
│   │
│   ├── services/                       # 业务服务
│   │   ├── mod.rs
│   │   ├── connection_manager.rs       # ConnectionManager（连接生命周期）
│   │   ├── connection_service.rs       # ConnectionService（连接创建、管理）
│   │   └── sql_service.rs              # SqlService（SQL 执行、事务）
│   │
│   ├── persistence/                    # 持久化层（双层存储）
│   │   ├── mod.rs
│   │   ├── global_db.rs                # 全局 SQLite 连接池 + 全局 DuckDB 连接
│   │   ├── project_db.rs               # 项目 SQLite 连接池 + 项目 DuckDB 连接
│   │   ├── connection_store.rs         # 全局连接信息存储
│   │   ├── history_store.rs            # SQL 历史存储
│   │   ├── project_store.rs            # 项目存储
│   │   ├── project_connection_store.rs # 项目连接存储
│   │   ├── metadata_cache.rs           # MetadataCacheManager（连接级元数据缓存）
│   │   ├── cache_version_migration.rs  # 缓存版本管理（自动迁移）
│   │   ├── insight_store.rs            # 洞察分析结果存储
│   │   ├── insight_meta_store.rs       # 洞察元数据配置
│   │   ├── analytics_resource_store.rs # 分析资源存储（图表/仪表盘）
│   │   ├── sql_template_store.rs       # SQL 模板存储
│   │   └── workbench_context_store.rs  # 工作台布局状态
│   │
│   ├── project/                        # 项目管理
│   │   ├── mod.rs
│   │   ├── models.rs                   # Project / ProjectConfig
│   │   └── store.rs                    # ProjectStore
│   │
│   ├── migration/                      # 数据库迁移
│   │   ├── mod.rs                      # MigrationManager（4 种迁移类型）
│   │   └── global_init.rs              # initialize_global_system()
│   │
│   ├── scratchpad/                     # 草稿箱
│   │   ├── mod.rs
│   │   └── store.rs                    # ScratchpadStore
│   │
│   ├── insight/                        # 洞察引擎
│   │   ├── mod.rs
│   │   ├── engine.rs                   # InsightEngine
│   │   ├── rules.rs                    # 洞察规则
│   │   └── executor.rs                 # 规则执行器
│   │
│   ├── mock/                           # 模拟数据生成
│   │   ├── mod.rs
│   │   ├── generator.rs
│   │   └── templates.rs
│   │
│   ├── sql/                            # SQL 解析与转译
│   │   ├── mod.rs
│   │   ├── parser.rs
│   │   ├── formatter.rs
│   │   └── transpiler.rs
│   │
│   └── export/                         # 数据导出
│       ├── mod.rs
│       └── exporter.rs
│
├── adapters/                           # 适配器层
│   ├── mod.rs
│   └── tauri/                          # Tauri 适配器
│       ├── mod.rs
│       ├── event.rs                    # 事件系统
│       ├── state.rs                    # 状态管理（⚠️ 命令已迁移到 commands/）
│       └── stream.rs                   # 流处理
│
└── commands/                           # 命令模块（按功能组织）
    ├── mod.rs                          # 模块导出
    ├── connection_commands.rs          # 连接管理命令（~15 个）
    ├── driver_commands.rs              # 驱动管理命令（~4 个）
    ├── sql_commands.rs                 # SQL 执行命令（~12 个）
    ├── project_commands.rs             # 项目管理命令（~12 个）
    ├── project_store_commands.rs       # 项目存储命令（~8 个）
    ├── port_commands.rs                # 端口协商命令
    ├── metadata_cache_commands.rs      # 元数据缓存命令（~8 个）
    ├── cache_warming_commands.rs       # 缓存预热命令（~9 个）
    ├── analytics_resource_commands.rs  # 分析资源命令（~25 个）
    ├── insight_commands.rs             # 洞察分析命令
    ├── federation_commands.rs          # 联邦查询命令
    ├── scratchpad_commands.rs          # 草稿箱命令（~20 个）
    ├── mock_commands.rs                # 模拟数据命令
    ├── sql_parse_commands.rs           # SQL 解析命令
    ├── export_commands.rs              # 数据导出命令
    └── duckdb_pool_commands.rs         # DuckDB 连接池命令
```

## 与旧版文档的关键差异

| 路径                           | 旧版文档                    | 实际代码                               |
| ------------------------------ | --------------------------- | -------------------------------------- |
| Tauri 命令入口                 | `adapters/tauri/command.rs` | `commands/*.rs`                        |
| 驱动注册                       | 单 DriverRegistry           | **双重注册**（Registry + Manager）     |
| DBI 层                         | `core/dbi/`                 | 未独立存在，在 driver/ / services/ 中  |
| 缓存层                         | `core/cache/`               | `core/persistence/metadata_cache.rs`   |
| metadata_cache / insight       | 未列出                      | ✅ 存在                                |
| scratchpad / mock / sql/export | 未列出                      | ✅ 存在                                |
| driver/native pool 文件        | 未列出                      | mysql_pool / sqlite_pool / duckdb_pool |
| migration                      | 未列出                      | ✅ 存在（4 种迁移类型）                |
| analytics_resource_store       | 未列出                      | ✅ 存在                                |

## 详细说明

### 1. lib.rs

**路径**: [lib.rs](file:///e:/myapps/tauirapps/RdataStation/rdata-station/src-tauri/src/lib.rs)

**关键代码**：

```rust
// 启动流程
fn register_drivers() {
    AutoDriverRegistrar::auto_register();   // 委托 BuiltinDriverDiscovery，注册 6 种驱动
}

pub fn run() {
    register_drivers();

    // 初始化全局系统数据库（SQLite + DuckDB）
    rt.block_on(core::migration::initialize_global_system())?;

    // 初始化全局驱动管理器（同源 BuiltinDriverDiscovery × 6）
    rt.block_on(core::driver::init_driver_manager())?;

    tauri::Builder::default()
        .manage(ProjectState::new())
        .manage(AnalyticsResourceState::new())
        .manage(ScratchpadState::new())
        .invoke_handler(tauri::generate_handler![
            // 70+ 个命令注册...
        ])
        .run(tauri::generate_context!())?;
}
```

### 2. core/driver/ 目录

驱动层的核心，详见 [05-驱动架构](./05-driver-architecture.md)。

关键文件：

| 文件                      | 行数 | 职责                                                   |
| ------------------------- | ---- | ------------------------------------------------------ |
| `registry/mod.rs`         | ~240 | DriverRegistry + DriverFactory trait + 全局单例        |
| `registry/descriptors.rs` | ~720 | DriverDescriptor + DriverKind + 6 种驱动描述符         |
| `factory.rs`              | ~290 | 6 个 DriverFactory 实现                                |
| `traits.rs`               | 253  | Database / Transaction / DbPool trait                  |
| `auto_register.rs`        | 80   | AutoDriverRegistrar（委托 BuiltinDriverDiscovery × 6） |
| `loader.rs`               | ~186 | DriverLoader + BuiltinDriverDiscovery（唯一真相源）    |
| `smart_pool.rs`           | -    | SmartPool 智能连接池                                   |

### 3. core/persistence/ 目录

双层存储的核心实现，详见 [06-存储架构](./06-storage-architecture.md)。

重要文件：

| 文件                          | 职责                                     |
| ----------------------------- | ---------------------------------------- |
| `global_db.rs`                | 全局 SQLite 连接池 + DuckDB 长连接       |
| `project_db.rs`               | 项目 SQLite 连接池 + DuckDB 长连接       |
| `metadata_cache.rs`           | 连接级元数据缓存（每个连接独立 .sqlite） |
| `cache_version_migration.rs`  | 缓存版本管理与自动迁移                   |
| `insight_store.rs`            | 洞察分析结果持久化                       |
| `analytics_resource_store.rs` | 分析资源（图表/仪表盘）管理              |

### 4. core/migration/ 目录

数据库 Schema 迁移管理：

```
MigrationType::GlobalSqlite   → system/global.db
MigrationType::GlobalDuckDB   → system/analytics/global.duckdb
MigrationType::ProjectSqlite  → {project}/meta/project.db
MigrationType::ProjectDuckDB  → {project}/analytics/data.duckdb
```

### 5. commands/ 目录

命令模块按功能组织，已从 `adapters/tauri/command.rs` 迁移。

| 模块                             | 功能域        |
| -------------------------------- | ------------- |
| `connection_commands.rs`         | 连接 CRUD     |
| `driver_commands.rs`             | 驱动发现      |
| `sql_commands.rs`                | SQL 执行      |
| `project_commands.rs`            | 项目管理      |
| `project_store_commands.rs`      | 项目持久化    |
| `metadata_cache_commands.rs`     | 元数据缓存    |
| `cache_warming_commands.rs`      | 缓存预热      |
| `analytics_resource_commands.rs` | 分析资源      |
| `insight_commands.rs`            | 洞察分析      |
| `federation_commands.rs`         | 联邦查询      |
| `scratchpad_commands.rs`         | 草稿箱        |
| `mock_commands.rs`               | 模拟数据      |
| `sql_parse_commands.rs`          | SQL 解析      |
| `export_commands.rs`             | 数据导出      |
| `duckdb_pool_commands.rs`        | DuckDB 连接池 |

## 命名规范

### 文件命名

| 类型     | 规范               | 示例                          |
| -------- | ------------------ | ----------------------------- |
| 模块文件 | snake_case.rs      | `connection_service.rs`       |
| 测试文件 | {module}\_tests.rs | `registry_tests.rs`           |
| 文档文件 | {number}-{name}.md | `01-architecture-overview.md` |

### 导入规范

```rust
// 1. 标准库
use std::sync::Arc;

// 2. 第三方库
use tokio::sync::RwLock;
use serde::{Serialize, Deserialize};

// 3. 本地模块（绝对路径）
use crate::core::error::CoreError;
use crate::core::driver::DriverRegistry;
```

## 新增模块指南

### 添加新数据库驱动

1. 在 `driver/native/` 创建 `{db}.rs`
2. 实现 `Database` + `DbPool` trait
3. 在 `driver/native/mod.rs` 导出
4. 在 `driver/factory.rs` 创建 `DriverFactory` 实现
5. **在 `driver/loader.rs` `BuiltinDriverDiscovery::builtin_factories()` 添加一行**（唯一修改点，auto_register 和 manager 自动同步）

### 添加新命令

1. 在 `commands/` 创建 `{name}_commands.rs`
2. 在 `commands/mod.rs` 导出
3. 在 `lib.rs` 的 `invoke_handler` 中注册
4. Command 只能调用 Service 层，禁止直接访问 driver 内部实现
