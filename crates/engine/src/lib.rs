//! RdataStation v2 引擎 crate（engine）
//!
//! 承载 M2 双引擎基础设施与统一数据访问层。
//!
//! ## 迁移进度
//! - ✅ Round 1：DuckDB 分析引擎（`duckdb`，自 v1 `core/duckdb`）
//! - ✅ Round 2：驱动层（`driver`，自 v1 `core/driver` 全量）+ 统一数据访问（`dbi`）
//!   + 多级缓存（`cache`）+ 连接管理（`connection_manager`）
//! - ✅ Round 5：元数据持久化（`persistence`，自 v1 `core/persistence`，不含 analytics_resource_store）
//!   + 日志（`logging`）+ 迁移（`migration` + `migrations/` SQL 资源）
//! - ⏳ 后续轮次：SQL 服务等按 Feature 迁入
//!
//! 依赖方向：engine → rds-shared；Feature crates → engine；engine 不得依赖 Feature。

pub mod cache;
pub mod connection_manager;
pub mod dbi;
pub mod driver;
pub mod duckdb;
pub mod logging;
pub mod migration;
pub mod persistence;
pub mod services;
pub mod sql;

// 重新导出常用错误类型（与 v1 core/mod.rs 一致）
pub use shared::error::{
    common_err, conn_err, invalid_arg, not_supported, query_err, storage_err, timeout, CommonError,
    ConnectionError, CoreError, CoreResult, DatabaseError, ErrorCategory, StorageError,
    TransactionState,
};

// 重新导出驱动层
pub use driver::{
    AutoDriverRegistrar,
    DataSourceMeta,
    Database,
    DbPool,
    DriverConnectionConfig,
    DriverDescriptor,
    DriverFactory,
    DriverRegistry,
    DynDatabase,
    PoolStatus,
    SchemaObject,
    SchemaObjectKind,
    Transaction,
};

// 重新导出驱动注册表函数
pub use driver::registry::{get_all_drivers, get_driver};

// 重新导出数据源路由层
pub use driver::router::DataSourceRouter;

// 重新导出连接管理层
pub use connection_manager::{
    get_connection_manager, ConnId, ConnectionConfig, ConnectionInfo, ConnectionManager,
    ConnectionType,
};

// 重新导出缓存层
pub use cache::{
    CacheConfig, CacheEntry, CacheLevel, CacheManager, CacheManagerStats, CachePolicy, CacheStats,
    LruCache, MetadataCache, MetadataCacheConfig, MetadataCacheKey, MetadataCacheValue,
};

// 重新导出日志模块
pub use logging::{
    config::LogConfig,
    record::{LogLevel, LogLevelCounts, LogPage, LogQuery, LogRecord, LogStats, TargetStat},
};

// 重新导出 SQL 服务（查询执行链）
pub use services::sql_service::{SqlExecuteOptions, SqlService};
pub use sql::{AlterOperation, ColumnDefInfo, DdlInfo, SqlDialect, SqlEngine, SqlStatementType};

// 重新导出连接探测（测试连接）
pub use services::connection_probe::{test_connection, test_connection_result};

// 重新导出 DuckDB 分析引擎模块
pub use duckdb::{
    DataFormat, DataSourceConfig, DataSourceType, DuckDBExecutor, DuckDBManager, DuckDBResult,
    ExplainAnalyzer, ExportConfig, ExtensionInfo, ExtensionManager, ExtensionStatus, FTSManager,
    FederationManager, ImportConfig, ImportExportManager, PlanNode, PlanNodeType, PluginConnection,
    PluginManager, PluginPermissionLevel, TempTableConfig, TempTableManager, TempTableSource,
};
