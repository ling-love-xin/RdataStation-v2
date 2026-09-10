# 模块依赖规则

> 版本：v1.1
> 最后更新：2026-05-09
> 状态：✅ 路径已更正

## 核心原则

**依赖方向**：`adapters` → `api` → `core` → `external`

```
┌─────────────────────────────────────┐
│  Adapters Layer                     │
│  (Tauri/CLI/HTTP)                   │
└──────────────┬──────────────────────┘
               │ 依赖
┌──────────────▼──────────────────────┐
│  API Layer                          │
│  (DTO / Error Types)                │
└──────────────┬──────────────────────┘
               │ 依赖
┌──────────────▼──────────────────────┐
│  Core Layer                         │
│  (Business Logic)                   │
│  ┌──────────────────────────────┐  │
│  │  dbi (统一数据访问) 🔥        │  │
│  │  - engine/driver_engine      │  │
│  │  - engine/duckdb_engine      │  │
│  │  - engine/stream_engine      │  │
│  └──────────────┬───────────────┘  │
│                 │                  │
│  ┌──────────────▼──────────────┐  │
│  │  services                   │  │
│  └────┬───┘ ┌───┬────┘ ┌───┬──┘  │
│       │     │     │     │        │
│  ┌───▼──┐ ┌▼────┐ ┌───▼─────┐  │
│  │driver│ │persistence│ │project│  │
│  │+conn │ │          │ │       │  │
│  │+route│ │          │ │       │  │
│  └──────┘ └─────────┘ └───────┘  │
└──────────────┬──────────────────────┘
               │ 依赖
┌──────────────▼──────────────────────┐
│  External Resources                 │
│  (Database / File System)           │
└─────────────────────────────────────┘
```

## 依赖规则矩阵

### Core 层内部依赖（实际）

| 模块                | 允许依赖                                                | 禁止依赖                    |
| ------------------- | ------------------------------------------------------- | --------------------------- |
| `models`            | 无（基础层）                                            | 所有其他模块                |
| `error`             | 无（基础层）                                            | 所有其他模块                |
| `driver/traits`     | `error`, `models`                                       | `services`, `adapters`      |
| `driver/registry`   | `error`, `models`, `driver/traits`, `driver/connection` | `services`, `adapters`      |
| `driver/native`     | `driver/traits`, `driver/registry`, `error`, `models`   | `services`, `adapters`      |
| `driver/router`     | `driver/registry`, `error`, `models`                    | `api`, `adapters`           |
| `driver/connection` | `error`, `models`                                       | `driver/native`, `services` |
| `persistence`       | `error`, `models`                                       | `driver/native`, `services` |
| `project`           | `error`, `models`, `persistence`                        | `driver/native`, `services` |
| `services`          | `driver`, `persistence`, `error`, `models`, `project`   | `api`, `adapters`           |
| `dbi`               | `driver`, `error`, `models`, `stream`                   | `api`, `adapters`           |

> ⚠️ 注意：`driver/traits.rs` 是宪法文件，禁止修改已有 trait 签名。`driver/router.rs` 做数据源路由（原 `datasource/`），不存放具体驱动实现。具体实现在 `driver/native/`。`connection/` 已迁移至 `driver/connection/`（配置/连接器/工厂/流）。

### 层间依赖

| 层         | 允许依赖            | 禁止依赖           |
| ---------- | ------------------- | ------------------ |
| `api`      | 无（被依赖）        | `core`, `adapters` |
| `core`     | `api`（仅错误类型） | `adapters`         |
| `adapters` | `api`, `core`       | 无                 |

## 依赖规则详解

### 1. 基础层（models, error, macros）

**规则**：不依赖任何其他模块

```rust
// ✅ 正确：models 不依赖其他模块
// core/models.rs
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
}

// ❌ 错误：models 不能依赖 services
// use crate::core::services::ConnectionService; // 禁止！
```

### 2. Driver 层

**规则**：只依赖基础层（error/models），不依赖 driver/native

```rust
// ✅ 正确：driver 只依赖基础层
// core/driver/traits.rs
use crate::core::error::CoreError;
use crate::core::models::QueryResult;

#[async_trait]
pub trait Database: Send + Sync {
    async fn query(&self, sql: &str) -> Result<QueryResult, CoreError>;
}

// ❌ 错误：driver 不能依赖 connection
// use crate::core::driver::connection::ConnectionConfig; // 禁止！
```

### 3. Connection 层（已内聚到 driver/connection/）

**规则**：不依赖 driver/native，通过 trait 解耦

```rust
// ✅ 正确：connection 只依赖基础层
// core/driver/connection/config.rs
use crate::core::error::CoreError;

pub struct ConnectionConfig {
    pub host: String,
    pub port: u16,
}

// ❌ 错误：connection 不能依赖 driver
// use crate::core::driver::Database; // 禁止！
```

### 4. 驱动路由层（原 Datasource 层，已合并到 driver/）

**规则**：依赖 driver::registry，只做路由，不存放具体驱动实现

```rust
// ✅ 正确：router 只依赖 registry
// core/driver/router.rs
use crate::core::driver::{DriverRegistry, DriverFactory};
use crate::core::error::CoreError;

pub struct DataSourceRouter;

impl DataSourceRouter {
    pub async fn route(config: ConnectionConfig) -> Result<DynDatabase, CoreError> {
        let factory = DriverRegistry::get(&config.driver)?;
        factory.create(config).await
    }
}

// ❌ 错误：router 不能直接实例化驱动
// use crate::core::driver::native::mysql::MySqlDatabase; // 禁止！
```

> ⚠️ 具体驱动实现位于 `driver/native/{mysql,postgres,sqlite,duckdb}.rs`。路由层在 `driver/router.rs`（原 `datasource/router.rs`）。

### 5. Services 层

**规则**：可以依赖所有 core 内部模块，但不能依赖 adapters

```rust
// ✅ 正确：services 依赖 core 内部模块
// core/services/connection_service.rs
use crate::core::driver::{Database, DriverRegistry};
use crate::core::driver::connection::{ConnectionConfig, ConnectionFactory};
use crate::core::persistence::ConnectionStore;
use crate::core::error::CoreError;
use crate::core::models::QueryResult;

pub struct ConnectionService;

impl ConnectionService {
    pub async fn connect(&self, config: ConnectionConfig) -> Result<String, CoreError> {
        // 实现
    }
}

// ❌ 错误：services 不能依赖 adapters
// use crate::adapters::tauri::TauriWindow; // 禁止！
```

### 7. Commands 层

**规则**：可以依赖 api 和 core services，处理 Tauri 命令

```rust
// ✅ 正确：commands 依赖 api 和 core
// commands/sql_commands.rs
use crate::core::services::{ConnectionManager, SqlService};
use crate::core::error::CoreError;

#[tauri::command]
pub async fn execute_sql(conn_id: String, sql: String) -> Result<QueryResult, String> {
    let manager = get_connection_manager();
    let db = manager.get_connection(&conn_id).await
        .ok_or("Connection not found")?;
    db.query(&sql).await.map_err(|e| e.to_string())
}

// ❌ 错误：commands 不应该被 core 依赖
```

> ⚠️ 命令已从 `adapters/tauri/command.rs` 迁移到 `commands/` 目录。旧路径不再有效。

## 循环依赖检测

### 什么是循环依赖？

```rust
// A.rs
use crate::B::BType;

// B.rs
use crate::A::AType; // 循环依赖！
```

### 如何避免

1. **提取公共类型到基础层**

```rust
// ❌ 错误：循环依赖
// A.rs
use crate::B::SharedType;

// B.rs
use crate::A::SharedType;

// ✅ 正确：提取到基础层
// core/models.rs
pub struct SharedType;

// A.rs
use crate::core::models::SharedType;

// B.rs
use crate::core::models::SharedType;
```

2. **使用 trait 抽象**

```rust
// ❌ 错误：直接依赖
// service.rs
use crate::core::driver::native::mysql::MySqlDriver;

// ✅ 正确：通过 trait 解耦
// service.rs
use crate::core::driver::Database;

pub struct Service<T: Database> {
    db: T,
}
```

3. **依赖注入**

```rust
// ✅ 正确：通过参数注入
pub async fn process(db: &dyn Database) -> Result<()> {
    db.query("SELECT 1").await
}
```

## 依赖注入实践

### 构造函数注入

```rust
pub struct ConnectionService {
    manager: Arc<ConnectionManager>,
    store: Arc<dyn ConnectionStore>,
}

impl ConnectionService {
    pub fn new(
        manager: Arc<ConnectionManager>,
        store: Arc<dyn ConnectionStore>,
    ) -> Self {
        Self { manager, store }
    }
}
```

### 方法参数注入

```rust
pub async fn execute_sql(
    &self,
    db: &dyn Database,  // 运行时注入
    sql: &str,
) -> Result<QueryResult, CoreError> {
    db.query(sql).await
}
```

## 测试中的依赖

### Mock 实现

```rust
#[cfg(test)]
use mockall::mock;

mock! {
    pub Database {}

    #[async_trait]
    impl Database for Database {
        async fn query(&self, sql: &str) -> Result<QueryResult, CoreError>;
    }
}

#[tokio::test]
async fn test_service() {
    let mut mock_db = MockDatabase::new();
    mock_db.expect_query()
        .returning(|_| Ok(QueryResult::default()));

    let service = SqlService::new();
    let result = service.execute_with_db(&mock_db, "SELECT 1").await;
    assert!(result.is_ok());
}
```

## 依赖可视化

### 生成依赖图

```bash
# 使用 cargo-depgraph
cargo install cargo-depgraph
cargo depgraph --package rdata-station-core > deps.dot
dot -Tpng deps.dot -o deps.png
```

### 手动检查

```bash
# 检查模块依赖
grep -r "use crate::" src/core/driver/ | grep -v "error\|models\|macros"
# 应该没有输出（driver 只依赖基础层）
```

## 常见错误

### 错误 1：Core 依赖 Adapters

```rust
// ❌ 错误
// core/services/mod.rs
use crate::adapters::tauri::TauriWindow;

// ✅ 正确：通过事件或回调解耦
// core/services/mod.rs
pub trait EventPublisher {
    fn publish(&self, event: Event);
}

// adapters/tauri/mod.rs
impl EventPublisher for TauriAdapter {
    fn publish(&self, event: Event) {
        // 发送给前端
    }
}
```

### 错误 2：Driver 依赖 Datasource

```rust
// ❌ 错误
// core/driver/traits.rs
use crate::core::driver::native::mysql::MySqlDriver;

// ✅ 正确：trait 不依赖具体实现
// core/driver/traits.rs
pub trait Database: Send + Sync {
    // 方法定义
}

// core/driver/native/mysql.rs
use crate::core::driver::Database;
impl Database for MySqlDriver { }
```

### 错误 3：循环依赖 Services

```rust
// ❌ 错误
// services/connection_service.rs
use crate::core::services::sql_service::SqlService;

// services/sql_service.rs
use crate::core::services::connection_service::ConnectionService;

// ✅ 正确：提取公共逻辑或合并服务
// services/query_service.rs
pub struct QueryService {
    connection: Arc<ConnectionService>,
    sql: Arc<SqlExecutor>,
}
```

## 最佳实践

1. **依赖方向**：始终从上到下（adapters → core → external）
2. **接口隔离**：依赖 trait 而非具体类型
3. **构造函数注入**：使用依赖注入而非全局状态
4. **最小依赖**：每个模块只依赖必需的模块
5. **测试友好**：设计时考虑 mock 测试
