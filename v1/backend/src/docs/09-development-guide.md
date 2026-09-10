# 开发指南

> 版本：v1.1
> 最后更新：2026-05-09
> 状态：✅ 路径已更正

## 环境准备

### 1. 安装 Rust

```bash
# 使用 rustup 安装
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 安装完成后，重启终端
source $HOME/.cargo/env

# 验证安装
rustc --version
cargo --version
```

### 2. 安装 Tauri 依赖

```bash
# Windows
# 安装 WebView2 运行时（通常已预装）

# 安装 Visual Studio Build Tools
# 下载地址: https://visualstudio.microsoft.com/visual-cpp-build-tools/
# 安装 "使用 C++ 的桌面开发" 工作负载
```

### 3. 安装 Node.js

```bash
# 使用 nvm 安装
nvm install 20
nvm use 20

# 验证
node --version
npm --version
```

### 4. 克隆项目

```bash
git clone https://github.com/your-org/rdata-station.git
cd rdata-station
```

### 5. 安装依赖

```bash
# 前端依赖
pnpm install

# Rust 依赖（会自动安装）
cd src-tauri
cargo build
```

## 项目结构

```
rdata-station/
├── src/                      # 前端代码 (Vue3 + TypeScript)
│   ├── components/
│   ├── views/
│   ├── stores/
│   └── ...
├── src-tauri/               # 后端代码 (Rust)
│   ├── src/
│   │   ├── api/
│   │   ├── core/
│   │   ├── adapters/
│   │   └── commands/
│   └── Cargo.toml
├── src-shared/              # 共享类型 (TypeScript)
│   └── config/
├── docs/                    # 文档
└── ...
```

## 开发工作流

### 1. 启动开发服务器

```bash
# 同时启动前端和后端
pnpm tauri dev

# 只启动前端
pnpm dev

# 只启动后端（调试）
cd src-tauri
cargo run
```

### 2. 代码规范

#### Rust 代码规范

```bash
# 格式化代码
cargo fmt

# 检查代码
cargo clippy -- -D warnings

# 运行测试
cargo test
```

#### TypeScript 代码规范

```bash
# 格式化代码
pnpm run format

# 检查代码
pnpm run lint

# 类型检查
pnpm run typecheck
```

### 3. 提交代码

```bash
# 1. 格式化代码
cargo fmt
pnpm run format

# 2. 检查代码
cargo clippy -- -D warnings
pnpm run lint

# 3. 运行测试
cargo test
pnpm run test

# 4. 提交
git add .
git commit -m "feat: 添加新功能"
```

## 添加新功能

### 1. 添加新数据库驱动

> ⚠️ **路径注意**：具体驱动实现位于 `core/driver/native/`，路由层在 `core/driver/router.rs`（已从 `datasource/` 迁移）。

#### 步骤 1: 创建驱动文件

```bash
touch src-tauri/src/core/driver/native/mongodb.rs
touch src-tauri/src/core/driver/native/mongodb_pool.rs
```

#### 步骤 2: 实现 Database trait

```rust
// core/driver/native/mongodb.rs
use crate::core::driver::traits::Database;
use crate::core::error::CoreError;
use crate::core::models::QueryResult;

pub struct MongoDatabase { /* ... */ }

#[async_trait::async_trait]
impl Database for MongoDatabase {
    async fn query(&self, sql: &str) -> Result<QueryResult, CoreError> { todo!() }
    async fn list_databases(&self) -> Result<Vec<String>, CoreError> { todo!() }
    fn meta(&self) -> DataSourceMeta { /* ... */ }
}
```

#### 步骤 3: 创建 DriverFactory

```rust
// core/driver/factory.rs
pub struct MongoDriverFactory;

impl DriverFactory for MongoDriverFactory {
    fn descriptor(&self) -> DriverDescriptor {
        DriverDescriptor::new("mongodb", "MongoDB")
    }

    fn create(&self, config: ConnectionConfig)
        -> Pin<Box<dyn Future<Output = Result<DynDatabase, CoreError>> + Send>>
    {
        Box::pin(async move { /* 创建连接 */ })
    }
}
```

#### 步骤 4: 注册驱动

```rust
// core/driver/loader.rs — BuiltinDriverDiscovery::builtin_factories()
// 这是驱动列表的唯一真相源，新增驱动只需在此添加一行：
impl BuiltinDriverDiscovery {
    pub fn builtin_factories() -> Vec<Arc<dyn DriverFactory>> {
        vec![
            // ... 现有 6 种（mysql/postgres/sqlite/duckdb + mysql_native/postgres_native）
            Arc::new(MongoDriverFactory),    // ← 在此添加一行即可
            // auto_register.rs 和 manager.rs 自动同步，无需修改
        ]
    }
}
```

> ✅ **v2.2 已统一**：`BuiltinDriverDiscovery::builtin_factories()` 作为唯一真相源，`auto_register.rs` 和 `manager.rs` 均从此获取驱动列表，不再各自硬编码。

```rust
// lib.rs — 保持不变，AutoDriverRegistrar 自动从 BuiltinDriverDiscovery 获取
fn register_drivers() {
    use core::driver::auto_register::AutoDriverRegistrar;
    AutoDriverRegistrar::auto_register();   // 自动包含所有 builtin_factories() 中的驱动
}
```

#### 步骤 6: 添加依赖

```toml
# src-tauri/Cargo.toml
[dependencies]
mongodb = "2.8"
```

#### 步骤 7: 测试

```rust
#[tokio::test]
async fn test_mongodb_driver() {
    let config = ConnectionConfig {
        host: "localhost".to_string(),
        port: 27017,
        // ...
    };

    let factory = MongoDbDriverFactory;
    let db = factory.create(config).await.unwrap();

    let result = db.list_databases().await.unwrap();
    assert!(!result.is_empty());
}
```

### 2. 添加新 Tauri 命令

#### 步骤 1: 创建命令函数

```rust
// commands/my_commands.rs (或在现有 commands 文件中添加)
use crate::core::services::ConnectionManager;
use crate::core::error::CoreError;

#[tauri::command]
pub async fn my_new_command(conn_id: String, param: String) -> Result<MyOutput, String> {
    let manager = get_connection_manager();
    let db = manager.get_connection(&conn_id).await
        .ok_or_else(|| "Connection not found".to_string())?;
    // 业务逻辑
    Ok(MyOutput { /* ... */ })
}
```

#### 步骤 2: 在 commands/mod.rs 导出

```rust
// commands/mod.rs
pub mod my_commands;
pub use my_commands::*;
```

#### 步骤 3: 在 lib.rs 注册

```rust
// lib.rs
.invoke_handler(tauri::generate_handler![
    // ... 现有命令
    my_new_command,
])
```

> ⚠️ **命令约束**：Command 只能调用 Service 层（ConnectionService / SqlService），禁止直接访问 driver/native。

### 3. 添加新服务

#### 步骤 1: 创建服务文件

```rust
// core/services/my_service.rs

use crate::core::error::CoreError;
use crate::core::models::QueryResult;

pub struct MyService {
    connection_manager: Arc<ConnectionManager>,
}

impl MyService {
    pub fn new(connection_manager: Arc<ConnectionManager>) -> Self {
        Self { connection_manager }
    }

    pub async fn do_something(
        &self,
        param: &str,
    ) -> Result<String, CoreError> {
        // 业务逻辑
        let conn = self.connection_manager
            .get_active_connection()
            .await
            .ok_or_else(|| CoreError::no_active_connection())?;

        let result = conn.query(&format!("SELECT '{}'", param)).await?;

        Ok(format!("Result: {:?}", result))
    }
}
```

#### 步骤 2: 导出服务

```rust
// core/services/mod.rs
pub mod my_service;
pub use my_service::MyService;
```

#### 步骤 3: 在 core/mod.rs 重新导出

```rust
// core/mod.rs
pub use services::MyService;
```

### 4. 添加 DBI 执行引擎 🔥

#### 步骤 1: 创建引擎文件

```rust
// core/dbi/engine/my_engine.rs

use crate::core::dbi::engine::ExecutionEngine;
use crate::core::dbi::context::QueryContext;
use crate::core::error::CoreError;
use crate::core::models::QueryResult;

pub struct MyEngine {
    // 引擎特定配置
}

#[async_trait]
impl ExecutionEngine for MyEngine {
    async fn execute(&self, sql: &str, context: &QueryContext)
        -> Result<QueryResult, CoreError>
    {
        // 1. 验证执行模式
        if !self.supports_mode(context.mode()) {
            return Err(CoreError::common(CommonError::NotSupported(
                "Execution mode not supported".to_string(),
            )));
        }

        // 2. 获取连接
        let db = self.get_connection(context.connection_id()).await?;

        // 3. 执行查询
        let result = db.query(sql).await?;

        // 4. 返回结果（Arrow 格式）
        Ok(result)
    }

    fn name(&self) -> &str {
        "MyEngine"
    }
}
```

#### 步骤 2: 注册引擎到 QueryRouter

```rust
// core/dbi/engine/mod.rs

pub struct QueryRouter {
    driver_engine: Arc<DriverEngine>,
    duckdb_engine: Arc<DuckDBEngine>,
    stream_engine: Arc<StreamEngine>,
    my_engine: Arc<MyEngine>, // 新增引擎
}

impl QueryRouter {
    pub fn new() -> Self {
        Self {
            driver_engine: Arc::new(DriverEngine::new()),
            duckdb_engine: Arc::new(DuckDBEngine::new()),
            stream_engine: Arc::new(StreamEngine::new()),
            my_engine: Arc::new(MyEngine::new()),
        }
    }

    pub async fn execute(&self, sql: &str, context: &QueryContext)
        -> Result<QueryResult, CoreError>
    {
        let engine = self.select_engine(context.mode());
        engine.execute(sql, context).await
    }

    fn select_engine(&self, mode: ExecutionMode) -> Arc<dyn ExecutionEngine> {
        match mode {
            ExecutionMode::Native => self.driver_engine.clone(),
            ExecutionMode::DuckDB => self.duckdb_engine.clone(),
            ExecutionMode::Stream => self.stream_engine.clone(),
            ExecutionMode::UserChoice => self.recommend_engine(),
        }
    }
}
```

#### 步骤 3: 更新 QueryRouter 推荐逻辑

```rust
impl QueryRouter {
    pub fn recommend_mode(&self, sql: &str) -> ExecutionMode {
        let sql_upper = sql.trim_start().to_uppercase();

        // 写操作必须走原生驱动
        if sql_upper.starts_with("INSERT")
            || sql_upper.starts_with("UPDATE")
            || sql_upper.starts_with("DELETE")
            || sql_upper.starts_with("CREATE")
            || sql_upper.starts_with("DROP")
            || sql_upper.starts_with("ALTER")
        {
            return ExecutionMode::Native;
        }

        // 复杂查询推荐 DuckDB
        if sql_upper.contains("GROUP BY")
            || sql_upper.contains("JOIN")
            || sql_upper.contains("ORDER BY")
            || sql_upper.contains("HAVING")
            || sql_upper.contains("UNION")
        {
            return ExecutionMode::DuckDB;
        }

        // 默认用户选择
        ExecutionMode::UserChoice
    }
}
```

### 5. 添加外部数据库到 DuckDB 🔥

#### 步骤 1: 注册外部数据库

```rust
// 在 Tauri Command 中
#[tauri::command]
pub async fn register_external_database(
    name: String,
    driver: String,
    connection_string: String,
) -> Result<(), String> {
    let duckdb_engine = get_duckdb_engine();

    duckdb_engine
        .register_external_database(&name, &driver, &connection_string)
        .await
        .map_err(|e| e.to_string())
}
```

#### 步骤 2: 加载文件数据源

```rust
#[tauri::command]
pub async fn load_file_source(
    path: String,
    table_name: String,
) -> Result<(), String> {
    let duckdb_engine = get_duckdb_engine();

    duckdb_engine
        .load_file_source(&path, &table_name)
        .await
        .map_err(|e| e.to_string())
}
```

#### 步骤 3: 执行联邦查询

```rust
#[tauri::command]
pub async fn execute_federated_query(
    sql: String,
) -> Result<QueryResult, String> {
    let dbi = get_dbi();

    dbi.query(&sql, ExecutionMode::DuckDB)
        .await
        .map_err(|e| e.to_string())
}
```

## 测试

### 单元测试

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_something() {
        let result = do_something();
        assert_eq!(result, expected);
    }

    #[tokio::test]
    async fn test_async_something() {
        let result = do_something_async().await;
        assert!(result.is_ok());
    }
}
```

### 集成测试

```rust
// tests/integration_test.rs

use rdata_station::core::services::ConnectionService;

#[tokio::test]
async fn test_database_connection() {
    let service = ConnectionService::new();

    let config = ConnectionConfig {
        host: "localhost".to_string(),
        port: 5432,
        database: "test".to_string(),
        username: "postgres".to_string(),
        password: "password".to_string(),
        ..Default::default()
    };

    let (conn_id, _) = service.connect(Some(config)).await.unwrap();

    // 测试查询
    let result = service.execute_sql(&conn_id, "SELECT 1").await.unwrap();
    assert_eq!(result.rows.len(), 1);

    // 清理
    service.close_connection(&conn_id).await.unwrap();
}
```

### Mock 测试

```rust
use mockall::mock;

mock! {
    pub Database {}

    #[async_trait]
    impl Database for Database {
        async fn query(&self, sql: &str) -> Result<QueryResult, CoreError>;
    }
}

#[tokio::test]
async fn test_with_mock() {
    let mut mock = MockDatabase::new();
    mock.expect_query()
        .with(eq("SELECT 1"))
        .returning(|_| Ok(QueryResult::default()));

    let service = SqlService::with_db(Box::new(mock));
    let result = service.execute("SELECT 1").await;

    assert!(result.is_ok());
}
```

## 调试技巧

### 1. 日志调试

```rust
use log::{info, debug, error, warn};

pub async fn do_something(&self) -> Result<(), CoreError> {
    info!("Starting operation");

    debug!("Connecting to database: {}", self.config.host);
    let conn = self.connect().await?;

    debug!("Executing query");
    let result = conn.query("SELECT * FROM users").await?;

    info!("Query returned {} rows", result.rows.len());

    Ok(())
}
```

### 2. 使用 dbg! 宏

```rust
let result = some_operation();
dbg!(&result); // 打印变量值和位置
```

### 3. IDE 调试

在 VS Code 中配置调试：

```json
// .vscode/launch.json
{
  "version": "0.2.0",
  "configurations": [
    {
      "type": "lldb",
      "request": "launch",
      "name": "Debug Tauri",
      "cargo": {
        "args": ["build", "--manifest-path=src-tauri/Cargo.toml"]
      },
      "args": []
    }
  ]
}
```

### 4. 性能分析

```bash
# 使用 cargo flamegraph
cargo install flamegraph

# 生成火焰图
cargo flamegraph --bin rdata-station

# 查看结果
open flamegraph.svg
```

## 常见问题

### 1. 编译错误：找不到模块

```bash
# 确保模块已导出
# 检查 mod.rs 是否包含 pub mod xxx;
```

### 2. 运行时错误：连接池耗尽

```rust
// 增加连接池大小
let pool = PgPoolOptions::new()
    .max_connections(20) // 增加连接数
    .connect(&url)
    .await?;
```

### 3. 异步错误：future 不能 Send

```rust
// 使用 Arc<Mutex<T>> 替代 Rc<RefCell<T>>
use std::sync::Arc;
use tokio::sync::Mutex;

let data = Arc::new(Mutex::new(Vec::new()));
```

### 4. 类型错误：trait bound 不满足

```rust
// 确保实现了必要的 trait
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MyStruct {
    // ...
}
```

## 性能优化

### 1. 使用连接池

```rust
// 不要每次创建新连接
let pool = PgPool::connect(&url).await?;

// 复用连接
let conn = pool.acquire().await?;
```

### 2. 批量操作

```rust
// 使用事务批量插入
let mut tx = db.begin_transaction().await?;

for row in rows {
    tx.execute(&format!("INSERT INTO ...")).await?;
}

tx.commit().await?;
```

### 3. 缓存

```rust
use cached::proc_macro::cached;

#[cached(size = 100, time = 300)]
async fn get_table_info(table: String) -> TableInfo {
    // 查询数据库
}
```

## 发布流程

### 1. 版本更新

```bash
# 更新 Cargo.toml 版本
# src-tauri/Cargo.toml
[package]
version = "1.1.0"

# 更新 package.json 版本
# package.json
"version": "1.1.0"
```

### 2. 构建发布版本

```bash
# 构建
pnpm tauri build

# 输出位置
# src-tauri/target/release/bundle/
```

### 3. 测试发布版本

```bash
# 运行发布版本
./src-tauri/target/release/rdata-station.exe
```

## 贡献指南

### 提交 PR

1. Fork 仓库
2. 创建功能分支：`git checkout -b feature/my-feature`
3. 提交更改：`git commit -m "feat: 添加新功能"`
4. 推送分支：`git push origin feature/my-feature`
5. 创建 Pull Request

### 代码审查清单

- [ ] 代码格式化（`cargo fmt`, `pnpm run format`）
- [ ] 代码检查通过（`cargo clippy`, `pnpm run lint`）
- [ ] 测试通过（`cargo test`, `pnpm run test`）
- [ ] 文档已更新
- [ ] 提交信息符合规范

### 提交信息规范

```
类型(范围): 简短描述

详细描述（可选）

Fixes #123
```

类型：

- `feat`: 新功能
- `fix`: 修复
- `docs`: 文档
- `style`: 格式
- `refactor`: 重构
- `test`: 测试
- `chore`: 构建/工具
