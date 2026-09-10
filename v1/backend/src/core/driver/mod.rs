//! Driver 层 — 数据库驱动 trait 抽象 + 注册 + 连接管理
//!
//! ═══════════ 架构边界 ═══════════
//! Driver 层定位：数据库连接抽象层，位于 dbi 之下、native 实现之上。
//!
//!    commands ──► services ──► dbi ──► driver ──► native
//!                  (业务逻辑)   (引擎)   (本层)    (实现)
//!
//! ## 职责
//! - 定义 Database / Transaction / Stream 核心 trait（[traits]）
//! - 驱动注册与发现（[registry]、[loader]）
//! - 连接池抽象与管理（[smart_pool]、[standard_pool]、[connection]）
//! - SQL 安全工具函数（[utils::escape_sql_string]、[utils::quote_identifier]）
//!
//! ## 连接池双层架构
//!
//! ```
//! SmartPool（守护系统内置库）                 StandardPool（用户数据源）
//! ├── 应用级 SQLite（global.db）              ├── 用户连接的 MySQL
//! ├── 项目级 SQLite（project.db）             ├── 用户连接的 PostgreSQL
//! ├── 连接元数据 SQLite（conn_{id}.sqlite）   ├── 用户连接的 SQLite（外部 .db）
//! ├── 应用级 DuckDB（analytics.duckdb）       └── 用户连接的 DuckDB（外部 .duckdb）
//! └── 项目级 DuckDB（analytics.duckdb）
//! ```
//!
//! | 维度         | SmartPool                        | StandardPool                    |
//! |-------------|----------------------------------|---------------------------------|
//! | 管理对象     | 系统内置库（RdataStation 自身依赖） | 用户数据源（用户外部数据库）      |
//! | 生命周期     | 应用/项目级别（启动→关闭）          | 连接级别（连接→断开）            |
//! | 配置方式     | 应用开发者硬编码                    | 用户在连接页面手动设置            |
//! | 失败处理     | 系统级故障（需要立即告警）           | 用户级故障（提示并允许重试）       |
//! | 动态扩缩容   | ✅ 延迟感知 + 内存压力感知           | ❌ 固定大小（用户配置）            |
//!
//! ## 非职责（禁止事项）
//! - ❌ 不处理业务逻辑（属于 services）
//! - ❌ 不路由执行引擎（属于 dbi）
//! - ❌ 不直接返回给前端（属于 commands）
//! - ❌ 不在 trait 中定义与数据访问无关的方法
//!
//! ## 与 dbi 层的边界
//! - driver 提供 `Database` trait，dbi 通过 `DriverEngine` 调用 trait 方法
//! - driver 不感知 dbi 的执行模式和路由策略
//! - dbi 不直接调用 driver/native 的实现，始终通过 trait 接口
pub mod auto_register;
pub mod connection;
pub mod factory;
pub mod introspection;
pub mod jdbc;
pub mod loader;
pub mod manager;
pub mod metadata;
pub mod native;
pub mod registry;
pub mod router;
pub mod smart_pool;
pub mod standard_pool;
pub mod traits;
pub mod utils;
pub mod wasm;
pub use auto_register::AutoDriverRegistrar;
pub use factory::{
    DuckDbDriverFactory, MySqlDriverFactory, MySqlNativeDriverFactory, PostgresDriverFactory,
    PostgresNativeDriverFactory, SqliteDriverFactory,
};
pub use introspection::{get_level, remove_level, set_level, IntrospectionLevel};
pub use loader::{BuiltinDriverDiscovery, DriverLoader, JdbcDriverDiscovery, WasmDriverDiscovery};
pub use manager::{
    get_driver_manager, init_driver_manager, DriverInfo, DriverManager, DriverStatus,
    DRIVER_MANAGER,
};
pub use metadata::{DriverFormField, DriverIcon, DriverMetadata, DriverType};
pub use registry::{
    DriverConnectionConfig, DriverDescriptor, DriverFactory, DriverKind, DriverRegistry,
};
pub use router::DataSourceRouter;
pub use smart_pool::{PoolStats, SmartPool, SmartPoolBuilder, SmartPoolConfig};
pub use standard_pool::{StandardPool, StandardPoolBuilder, StandardPoolConfig, StandardPoolStats};
pub use traits::{
    ColumnDetail, ConstraintDetail, DataSourceMeta, Database, DbPool, DynDatabase, IndexDetail,
    MetadataBrowser, NodeDetail, NodeInfo, PoolStatus, SchemaObject, SchemaObjectKind, Transaction,
};
pub use utils::{build_connection_url, parse_driver_id, validate_driver_config};
