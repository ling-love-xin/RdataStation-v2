//! Core 业务层
//!
//! 纯业务逻辑，无外部框架依赖。
//! 负责：
//! - 连接生命周期管理
//! - SQL 执行与事务
//! - 数据库驱动抽象
//! - 数据持久化
//! - 底层连接管理（SSL/SSH/Proxy）
//! - 多级缓存管理
//! - DBI 统一数据访问入口
//!
//! 注意：Core 不直接暴露给外部，需通过 Adapter 层（Tauri/CLI/HTTP）访问
//!
//! # 模块依赖规则
//!
//! ## 允许依赖：
//! - `models` → 无（基础层）
//! - `error`, `macros` → 无（基础层）
//! - `driver` → `error`, `macros`, `models`（含 connection 子模块）
//! - `datasource` → 已合并到 `driver`（router 迁移至 driver/router.rs）
//! - `connection` → 已迁移至 `driver/connection/`
//! - `project` → `error`, `models`, `persistence`
//! - `services` → `driver`, `persistence`, `connection`, `error`, `models`, `project`, `cache`
//! - `cache` → `error`, `models`
//! - `dbi` → `driver`, `error`, `models`, `stream`
//!
//! ## 禁止依赖：
//! - 任何 core 内部模块 → `api`（api 应依赖 core，而非相反）
//! - `driver` → `connection`（应通过 trait 解耦）
//! - `datasource` → `api`

pub mod api_version;
pub mod arrow;
pub mod cache;
pub mod crypto;
pub mod dbi;
pub mod driver;
pub mod duckdb;
pub mod error;
pub mod insight;
pub mod logging;
pub mod macros;
pub mod migration;
pub mod models;
pub mod performance;
pub mod persistence;
pub mod plugin;
pub mod port_negotiation;
pub mod project;
pub mod scratchpad;
pub mod services;
pub mod sql;
pub mod stream;
pub mod types;
pub mod utils;

// 重新导出常用错误类型
pub use error::{
    common_err, conn_err, invalid_arg, not_supported, query_err, storage_err, timeout, CommonError,
    ConnectionError, CoreError, CoreResult, DatabaseError, ErrorCategory, StorageError,
    TransactionState,
};

// 重新导出 Service 层（供 Adapters 使用）
pub use services::{
    get_connection_manager, ConnectionInfo, ConnectionManager, ConnectionService, SqlService,
};

// 重新导出驱动层
pub use driver::{
    // 驱动配置和自动注册
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

// 重新导出数据源路由层（已迁移至 driver/router.rs）
pub use driver::router::DataSourceRouter;

// 重新导出连接层（已迁移至 driver/connection/）
pub use driver::connection::{
    config::{ConnectionConfig, ConnectionMethod, ProxyConfig, SshConfig, SslConfig, TlsVersion},
    connector::{Connection, Connector},
    factory::ConnectionFactory,
    stream::ConnectionStream,
};

// 重新导出模型层
pub use models::{QueryResult, Row, Value};

// 重新导出项目层
pub use project::{
    ConnectionRef, Project, ProjectConfig, ProjectInfo, ProjectManager, ProjectPath, ProjectStatus,
    ProjectStore, QueryRef, Version, VersionInfo, Versioned,
};

// 重新导出端口协商层
pub use port_negotiation::{
    AdvancedPortNegotiator, PortNegotiationResult, PortNegotiator, PortRange, COMMON_DB_PORTS,
    DEFAULT_PORT_RANGE,
};

// 重新导出缓存层
pub use cache::{
    CacheConfig, CacheEntry, CacheLevel, CacheManager, CacheManagerStats, CachePolicy, CacheStats,
    LruCache, MetadataCache, MetadataCacheConfig, MetadataCacheKey, MetadataCacheValue,
};

// 重新导出 Arrow 相关
pub use arrow::{ArrowBatch, ArrowBatchStream, ArrowHandler};

// 重新导出流相关
pub use stream::{ArrowBatchStream as CoreArrowBatchStream, Stream, StreamQueryResult};

// 重新导出工具模块
pub use utils::{hash, string, time};

// 重新导出 DuckDB 分析引擎模块
pub use duckdb::{
    DataFormat, DataSourceConfig, DataSourceType, DuckDBExecutor, DuckDBManager, DuckDBResult,
    ExplainAnalyzer, ExportConfig, ExtensionInfo, ExtensionManager, ExtensionStatus, FTSManager,
    FederationManager, ImportConfig, ImportExportManager, PlanNode, PlanNodeType, PluginConnection,
    PluginManager, PluginPermissionLevel, TempTableConfig, TempTableManager, TempTableSource,
};

// 重新导出草稿箱模块
pub use scratchpad::{
    ExternalReference, ScratchpadConfig, ScratchpadEntry, ScratchpadEntryKind, ScratchpadResponse,
    ScratchpadStore,
};

// 重新导出日志模块
pub use logging::{
    config::LogConfig,
    record::{LogLevel, LogLevelCounts, LogPage, LogQuery, LogRecord, LogStats, TargetStat},
};

// 重新导出插件模块
pub use plugin::{
    CapabilitiesFrontend, CapabilitiesWasm, ContributesCommand, ContributesDriver,
    ContributesPanel, ContributesSetting, ManifestParser, PluginCapabilities, PluginContributes,
    PluginDependency, PluginEngines, PluginManifest, PluginMeta, PluginPermissions,
};
