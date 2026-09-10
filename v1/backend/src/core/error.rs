//! Core 错误处理模块
//!
//! 采用"错误域（Domain Error）"设计，避免 God Enum，支持插件扩展。
//!
//! ## 架构设计
//!
//! ```
//! CoreError (核心错误容器)
//! ├── Common (通用错误域)
//! ├── Connection (连接错误域)
//! ├── Database (数据库错误域)
//! └── Plugin (插件错误域 - 预留扩展)
//! ```
//!
//! ## 设计原则
//!
//! 1. **错误域隔离**: 每个域独立定义，互不影响
//! 2. **插件友好**: Wasm 插件可以定义自己的错误类型
//! 3. **显式转换**: 不实现 blanket From，避免上下文丢失
//! 4. **向前兼容**: 新增错误域不会破坏现有代码

use serde::Serialize;
use specta::Type;
use std::fmt;

/// 核心错误容器
///
/// 这是 Core 层统一的错误类型，采用"容器模式"包装各个错误域。
/// 每个错误域都是独立的枚举，避免 God Enum 问题。
#[derive(Debug, Clone, Serialize, Type)]
pub enum CoreError {
    /// 通用错误域
    Common(CommonError),

    /// 连接错误域
    Connection(ConnectionError),

    /// 数据库错误域
    Database(DatabaseError),

    /// 存储错误域
    Storage(StorageError),

    /// 缓存错误域
    Cache(CacheError),

    /// 插件错误域
    ///
    /// 插件管理、加载、执行相关的所有错误
    Plugin(PluginError),
}

// ==================== 通用错误域 ====================

/// 通用错误域
///
/// 适用于所有模块的基础错误类型
#[derive(Debug, Clone, Serialize, Type)]
pub enum CommonError {
    /// 通用错误 - 兜底类型
    General(String),

    /// 无效参数
    InvalidArgument { param: String, reason: String },

    /// 不支持的操作
    NotSupported(String),

    /// 超时错误
    Timeout { operation: String, duration_ms: u64 },

    /// 内部错误
    Internal(String),
}

impl CommonError {
    pub fn general(msg: impl Into<String>) -> Self {
        CommonError::General(msg.into())
    }

    pub fn invalid_argument(param: impl Into<String>, reason: impl Into<String>) -> Self {
        CommonError::InvalidArgument {
            param: param.into(),
            reason: reason.into(),
        }
    }

    pub fn not_supported(feature: impl Into<String>) -> Self {
        CommonError::NotSupported(feature.into())
    }

    pub fn timeout(operation: impl Into<String>, duration_ms: u64) -> Self {
        CommonError::Timeout {
            operation: operation.into(),
            duration_ms,
        }
    }
}

// ==================== 连接错误域 ====================

/// 连接错误域
///
/// 数据库连接相关的所有错误
#[derive(Debug, Clone, Serialize, Type)]
pub enum ConnectionError {
    /// 连接被拒绝
    Refused { conn_id: String, reason: String },

    /// 连接超时
    Timeout { conn_id: String, duration_ms: u64 },

    /// 认证失败
    AuthenticationFailed { conn_id: String, username: String },

    /// 网络错误
    Network { conn_id: String, reason: String },

    /// 主机未找到
    HostNotFound { conn_id: String, host: String },

    /// 端口不可达
    PortUnreachable {
        conn_id: String,
        host: String,
        port: u16,
    },

    /// SSL/TLS 错误
    Tls { conn_id: String, reason: String },

    /// 连接不存在
    NotFound(String),

    /// 驱动未找到
    DriverNotFound { driver: String },

    /// 没有活动连接
    NoActiveConnection,

    /// 连接池错误
    PoolError { pool_id: String, reason: String },

    /// 连接池耗尽
    PoolExhausted { conn_id: String, reason: String },

    /// 连接池已关闭
    PoolClosed,

    /// IO 错误
    Io { conn_id: String, reason: String },

    /// 无效配置
    InvalidConfig { conn_id: String, reason: String },

    /// 不支持的操作
    NotSupported(String),

    /// 其他连接错误
    Other { conn_id: String, reason: String },
}

impl ConnectionError {
    pub fn refused(conn_id: impl Into<String>, reason: impl Into<String>) -> Self {
        ConnectionError::Refused {
            conn_id: conn_id.into(),
            reason: reason.into(),
        }
    }

    pub fn timeout(conn_id: impl Into<String>, duration_ms: u64) -> Self {
        ConnectionError::Timeout {
            conn_id: conn_id.into(),
            duration_ms,
        }
    }

    pub fn auth_failed(conn_id: impl Into<String>, username: impl Into<String>) -> Self {
        ConnectionError::AuthenticationFailed {
            conn_id: conn_id.into(),
            username: username.into(),
        }
    }

    pub fn not_found(conn_id: impl Into<String>) -> Self {
        ConnectionError::NotFound(conn_id.into())
    }
}

// ==================== 数据库错误域 ====================

/// 数据库错误域
///
/// SQL 执行、事务、查询相关的错误
#[derive(Debug, Clone, Serialize, Type)]
pub enum DatabaseError {
    /// 查询执行错误
    Query {
        sql: String,
        reason: String,
        position: Option<usize>,
    },

    /// 语法错误
    Syntax {
        sql: String,
        message: String,
        line: Option<u32>,
        column: Option<u32>,
    },

    /// 事务错误
    Transaction {
        operation: String,
        state: TransactionState,
        reason: String,
    },

    /// 数据库特定错误
    Driver {
        db_type: String,
        operation: String,
        source: String,
    },

    /// 驱动未找到
    DriverNotFound { db_type: String },

    /// 驱动版本不兼容
    DriverVersionMismatch {
        driver: String,
        expected: String,
        found: String,
    },

    /// 约束冲突
    ConstraintViolation {
        constraint: String,
        table: String,
        reason: String,
    },

    /// 表不存在
    TableNotFound { table: String },

    /// 列不存在
    ColumnNotFound { column: String, table: String },
}

impl DatabaseError {
    pub fn query(sql: impl Into<String>, reason: impl Into<String>) -> Self {
        DatabaseError::Query {
            sql: sql.into(),
            reason: reason.into(),
            position: None,
        }
    }

    pub fn with_position(mut self, pos: usize) -> Self {
        if let DatabaseError::Query { position, .. } = &mut self {
            *position = Some(pos);
        }
        self
    }
}

/// 事务状态
#[derive(Debug, Clone, Serialize, Type)]
pub enum TransactionState {
    NotStarted,
    InProgress,
    Committed,
    RolledBack,
    Failed(String),
}

// ==================== 存储错误域 ====================

/// 存储错误域
///
/// 持久化、序列化、IO 相关的错误
#[derive(Debug, Clone, Serialize, Type)]
pub enum StorageError {
    /// 持久化存储错误
    Persistence {
        store: String,
        operation: String,
        reason: String,
    },

    /// 序列化错误
    Serialization { format: String, reason: String },

    /// 反序列化错误
    Deserialization {
        format: String,
        data: String,
        reason: String,
    },

    /// IO 错误（包含完整上下文）
    Io {
        path: String,
        operation: String,
        reason: String,
    },
}

// ==================== 缓存错误域 ====================

/// 缓存错误域
///
/// 缓存操作相关的错误
#[derive(Debug, Clone, Serialize, Type)]
pub enum CacheError {
    /// 缓存未命中
    Miss { key: String },

    /// 缓存已满
    Full { capacity: usize },

    /// 缓存键无效
    InvalidKey { key: String, reason: String },

    /// 缓存值无效
    InvalidValue { reason: String },

    /// 缓存操作超时
    Timeout { operation: String, duration_ms: u64 },

    /// 缓存内部错误
    Internal { reason: String },

    /// 缓存序列化错误
    Serialization { reason: String },
}

impl CacheError {
    pub fn miss(key: impl Into<String>) -> Self {
        CacheError::Miss { key: key.into() }
    }

    pub fn full(capacity: usize) -> Self {
        CacheError::Full { capacity }
    }

    pub fn invalid_key(key: impl Into<String>, reason: impl Into<String>) -> Self {
        CacheError::InvalidKey {
            key: key.into(),
            reason: reason.into(),
        }
    }

    pub fn invalid_value(reason: impl Into<String>) -> Self {
        CacheError::InvalidValue {
            reason: reason.into(),
        }
    }

    pub fn timeout(operation: impl Into<String>, duration_ms: u64) -> Self {
        CacheError::Timeout {
            operation: operation.into(),
            duration_ms,
        }
    }

    pub fn internal(reason: impl Into<String>) -> Self {
        CacheError::Internal {
            reason: reason.into(),
        }
    }

    pub fn serialization(reason: impl Into<String>) -> Self {
        CacheError::Serialization {
            reason: reason.into(),
        }
    }
}

impl fmt::Display for CacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CacheError::Miss { key } => {
                write!(f, "Cache miss for key: {}", key)
            }
            CacheError::Full { capacity } => {
                write!(f, "Cache is full (capacity: {})", capacity)
            }
            CacheError::InvalidKey { key, reason } => {
                write!(f, "Invalid cache key '{}': {}", key, reason)
            }
            CacheError::InvalidValue { reason } => {
                write!(f, "Invalid cache value: {}", reason)
            }
            CacheError::Timeout {
                operation,
                duration_ms,
            } => {
                write!(
                    f,
                    "Cache operation '{}' timed out after {}ms",
                    operation, duration_ms
                )
            }
            CacheError::Internal { reason } => {
                write!(f, "Cache internal error: {}", reason)
            }
            CacheError::Serialization { reason } => {
                write!(f, "Cache serialization error: {}", reason)
            }
        }
    }
}

// ==================== 插件错误域 ====================

/// 插件错误域
///
/// 插件管理、加载、执行相关的所有错误
#[derive(Debug, Clone, Serialize, Type)]
pub enum PluginError {
    /// 插件未找到
    NotFound { plugin_id: String },

    /// 插件代码与版本未找到
    NotFoundByCode { code: String, version: String },

    /// 插件已存在
    AlreadyExists { code: String, version: String },

    /// 插件加载失败
    LoadFailed { plugin_id: String, reason: String },

    /// 插件激活失败
    ActivationFailed { plugin_id: String, reason: String },

    /// 插件停用失败
    DeactivationFailed { plugin_id: String, reason: String },

    /// 插件卸载失败
    UninstallFailed { plugin_id: String, reason: String },

    /// 插件执行失败
    ExecutionFailed {
        plugin_id: String,
        function: String,
        reason: String,
    },

    /// 插件依赖缺失
    DependencyMissing {
        plugin_id: String,
        dep_code: String,
        dep_version: String,
    },

    /// 插件版本不兼容
    VersionIncompatible {
        plugin_id: String,
        version: String,
        expected: String,
    },

    /// 插件无效配置
    InvalidConfig {
        plugin_id: String,
        key: String,
        reason: String,
    },

    /// 插件清单无效
    InvalidManifest { reason: String },

    /// 插件文件缺失
    FileMissing { path: String, reason: String },

    /// 插件未启用
    Disabled { plugin_id: String },

    /// 插件内部错误
    Internal { plugin_id: String, reason: String },

    /// 不支持的插件类型
    UnsupportedType { plugin_type: String },
}

impl PluginError {
    pub fn not_found(plugin_id: impl Into<String>) -> Self {
        PluginError::NotFound {
            plugin_id: plugin_id.into(),
        }
    }

    pub fn not_found_by_code(code: impl Into<String>, version: impl Into<String>) -> Self {
        PluginError::NotFoundByCode {
            code: code.into(),
            version: version.into(),
        }
    }

    pub fn already_exists(code: impl Into<String>, version: impl Into<String>) -> Self {
        PluginError::AlreadyExists {
            code: code.into(),
            version: version.into(),
        }
    }

    pub fn load_failed(plugin_id: impl Into<String>, reason: impl Into<String>) -> Self {
        PluginError::LoadFailed {
            plugin_id: plugin_id.into(),
            reason: reason.into(),
        }
    }

    pub fn activation_failed(plugin_id: impl Into<String>, reason: impl Into<String>) -> Self {
        PluginError::ActivationFailed {
            plugin_id: plugin_id.into(),
            reason: reason.into(),
        }
    }

    pub fn execution_failed(
        plugin_id: impl Into<String>,
        function: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        PluginError::ExecutionFailed {
            plugin_id: plugin_id.into(),
            function: function.into(),
            reason: reason.into(),
        }
    }

    pub fn dependency_missing(
        plugin_id: impl Into<String>,
        dep_code: impl Into<String>,
        dep_version: impl Into<String>,
    ) -> Self {
        PluginError::DependencyMissing {
            plugin_id: plugin_id.into(),
            dep_code: dep_code.into(),
            dep_version: dep_version.into(),
        }
    }
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PluginError::NotFound { plugin_id } => {
                write!(f, "Plugin '{}' not found", plugin_id)
            }
            PluginError::NotFoundByCode { code, version } => {
                write!(f, "Plugin '{}@{}' not found", code, version)
            }
            PluginError::AlreadyExists { code, version } => {
                write!(f, "Plugin '{}@{}' already exists", code, version)
            }
            PluginError::LoadFailed { plugin_id, reason } => {
                write!(f, "Failed to load plugin '{}': {}", plugin_id, reason)
            }
            PluginError::ActivationFailed { plugin_id, reason } => {
                write!(f, "Failed to activate plugin '{}': {}", plugin_id, reason)
            }
            PluginError::DeactivationFailed { plugin_id, reason } => {
                write!(f, "Failed to deactivate plugin '{}': {}", plugin_id, reason)
            }
            PluginError::UninstallFailed { plugin_id, reason } => {
                write!(f, "Failed to uninstall plugin '{}': {}", plugin_id, reason)
            }
            PluginError::ExecutionFailed {
                plugin_id,
                function,
                reason,
            } => {
                write!(
                    f,
                    "Plugin '{}' execution of '{}' failed: {}",
                    plugin_id, function, reason
                )
            }
            PluginError::DependencyMissing {
                plugin_id,
                dep_code,
                dep_version,
            } => {
                write!(
                    f,
                    "Plugin '{}' missing dependency '{}@{}'",
                    plugin_id, dep_code, dep_version
                )
            }
            PluginError::VersionIncompatible {
                plugin_id,
                version,
                expected,
            } => {
                write!(
                    f,
                    "Plugin '{}' version '{}' incompatible, expected '{}'",
                    plugin_id, version, expected
                )
            }
            PluginError::InvalidConfig {
                plugin_id,
                key,
                reason,
            } => {
                write!(
                    f,
                    "Invalid config '{}' for plugin '{}': {}",
                    key, plugin_id, reason
                )
            }
            PluginError::InvalidManifest { reason } => {
                write!(f, "Invalid plugin manifest: {}", reason)
            }
            PluginError::FileMissing { path, reason } => {
                write!(f, "Plugin file '{}' missing: {}", path, reason)
            }
            PluginError::Disabled { plugin_id } => {
                write!(f, "Plugin '{}' is disabled", plugin_id)
            }
            PluginError::Internal { plugin_id, reason } => {
                write!(f, "Plugin '{}' internal error: {}", plugin_id, reason)
            }
            PluginError::UnsupportedType { plugin_type } => {
                write!(f, "Unsupported plugin type: {}", plugin_type)
            }
        }
    }
}

impl StorageError {
    pub fn io(
        path: impl Into<String>,
        operation: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        StorageError::Io {
            path: path.into(),
            operation: operation.into(),
            reason: reason.into(),
        }
    }

    pub fn persistence(
        store: impl Into<String>,
        operation: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        StorageError::Persistence {
            store: store.into(),
            operation: operation.into(),
            reason: reason.into(),
        }
    }

    pub fn write(store: impl Into<String>, reason: impl Into<String>) -> Self {
        StorageError::Persistence {
            store: store.into(),
            operation: "write".to_string(),
            reason: reason.into(),
        }
    }

    pub fn read(store: impl Into<String>, reason: impl Into<String>) -> Self {
        StorageError::Persistence {
            store: store.into(),
            operation: "read".to_string(),
            reason: reason.into(),
        }
    }
}

// ==================== CoreError 实现 ====================

impl CoreError {
    /// 创建通用错误
    pub fn common(err: CommonError) -> Self {
        CoreError::Common(err)
    }

    /// 创建连接错误
    pub fn connection(err: ConnectionError) -> Self {
        CoreError::Connection(err)
    }

    /// 创建数据库错误
    pub fn database(err: DatabaseError) -> Self {
        CoreError::Database(err)
    }

    /// 创建存储错误
    pub fn storage(err: StorageError) -> Self {
        CoreError::Storage(err)
    }

    /// 创建缓存错误
    pub fn cache(err: CacheError) -> Self {
        CoreError::Cache(err)
    }

    /// 创建插件错误
    pub fn plugin(err: PluginError) -> Self {
        CoreError::Plugin(err)
    }

    /// 获取错误代码
    pub fn code(&self) -> &'static str {
        match self {
            CoreError::Common(e) => match e {
                CommonError::General(_) => "COMMON_GENERAL",
                CommonError::InvalidArgument { .. } => "COMMON_INVALID_ARG",
                CommonError::NotSupported(_) => "COMMON_NOT_SUPPORTED",
                CommonError::Timeout { .. } => "COMMON_TIMEOUT",
                CommonError::Internal(_) => "COMMON_INTERNAL",
            },
            CoreError::Connection(e) => match e {
                ConnectionError::Refused { .. } => "CONN_REFUSED",
                ConnectionError::Timeout { .. } => "CONN_TIMEOUT",
                ConnectionError::AuthenticationFailed { .. } => "CONN_AUTH_FAILED",
                ConnectionError::Network { .. } => "CONN_NETWORK",
                ConnectionError::HostNotFound { .. } => "CONN_HOST_NOT_FOUND",
                ConnectionError::PortUnreachable { .. } => "CONN_PORT_UNREACHABLE",
                ConnectionError::Tls { .. } => "CONN_TLS",
                ConnectionError::NotFound(_) => "CONN_NOT_FOUND",
                ConnectionError::DriverNotFound { .. } => "CONN_DRIVER_NOT_FOUND",
                ConnectionError::NoActiveConnection => "CONN_NO_ACTIVE",
                ConnectionError::PoolError { .. } => "CONN_POOL_ERROR",
                ConnectionError::PoolExhausted { .. } => "CONN_POOL_EXHAUSTED",
                ConnectionError::PoolClosed => "CONN_POOL_CLOSED",
                ConnectionError::Io { .. } => "CONN_IO",
                ConnectionError::InvalidConfig { .. } => "CONN_INVALID_CONFIG",
                ConnectionError::NotSupported(_) => "CONN_NOT_SUPPORTED",
                ConnectionError::Other { .. } => "CONN_ERROR",
            },
            CoreError::Database(e) => match e {
                DatabaseError::Query { .. } => "DB_QUERY",
                DatabaseError::Syntax { .. } => "DB_SYNTAX",
                DatabaseError::Transaction { .. } => "DB_TRANSACTION",
                DatabaseError::Driver { .. } => "DB_DRIVER",
                DatabaseError::DriverNotFound { .. } => "DB_DRIVER_NOT_FOUND",
                DatabaseError::DriverVersionMismatch { .. } => "DB_DRIVER_VERSION",
                DatabaseError::ConstraintViolation { .. } => "DB_CONSTRAINT",
                DatabaseError::TableNotFound { .. } => "DB_TABLE_NOT_FOUND",
                DatabaseError::ColumnNotFound { .. } => "DB_COLUMN_NOT_FOUND",
            },
            CoreError::Storage(e) => match e {
                StorageError::Persistence { .. } => "STORE_PERSISTENCE",
                StorageError::Serialization { .. } => "STORE_SERIALIZE",
                StorageError::Deserialization { .. } => "STORE_DESERIALIZE",
                StorageError::Io { .. } => "STORE_IO",
            },
            CoreError::Cache(e) => match e {
                CacheError::Miss { .. } => "CACHE_MISS",
                CacheError::Full { .. } => "CACHE_FULL",
                CacheError::InvalidKey { .. } => "CACHE_INVALID_KEY",
                CacheError::InvalidValue { .. } => "CACHE_INVALID_VALUE",
                CacheError::Timeout { .. } => "CACHE_TIMEOUT",
                CacheError::Internal { .. } => "CACHE_INTERNAL",
                CacheError::Serialization { .. } => "CACHE_SERIALIZE",
            },
            CoreError::Plugin(e) => match e {
                PluginError::NotFound { .. } => "PLUGIN_NOT_FOUND",
                PluginError::NotFoundByCode { .. } => "PLUGIN_NOT_FOUND_BY_CODE",
                PluginError::AlreadyExists { .. } => "PLUGIN_ALREADY_EXISTS",
                PluginError::LoadFailed { .. } => "PLUGIN_LOAD_FAILED",
                PluginError::ActivationFailed { .. } => "PLUGIN_ACTIVATION_FAILED",
                PluginError::DeactivationFailed { .. } => "PLUGIN_DEACTIVATION_FAILED",
                PluginError::UninstallFailed { .. } => "PLUGIN_UNINSTALL_FAILED",
                PluginError::ExecutionFailed { .. } => "PLUGIN_EXECUTION_FAILED",
                PluginError::DependencyMissing { .. } => "PLUGIN_DEPENDENCY_MISSING",
                PluginError::VersionIncompatible { .. } => "PLUGIN_VERSION_INCOMPATIBLE",
                PluginError::InvalidConfig { .. } => "PLUGIN_INVALID_CONFIG",
                PluginError::InvalidManifest { .. } => "PLUGIN_INVALID_MANIFEST",
                PluginError::FileMissing { .. } => "PLUGIN_FILE_MISSING",
                PluginError::Disabled { .. } => "PLUGIN_DISABLED",
                PluginError::Internal { .. } => "PLUGIN_INTERNAL",
                PluginError::UnsupportedType { .. } => "PLUGIN_UNSUPPORTED_TYPE",
            },
        }
    }

    /// 获取错误分类
    pub fn category(&self) -> ErrorCategory {
        match self {
            CoreError::Common(_) => ErrorCategory::Common,
            CoreError::Connection(_) => ErrorCategory::Connection,
            CoreError::Database(_) => ErrorCategory::Database,
            CoreError::Storage(_) => ErrorCategory::Storage,
            CoreError::Cache(_) => ErrorCategory::Cache,
            CoreError::Plugin(_) => ErrorCategory::Plugin,
        }
    }

    /// 是否可重试
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            CoreError::Common(CommonError::Timeout { .. })
                | CoreError::Connection(ConnectionError::Timeout { .. })
                | CoreError::Connection(ConnectionError::Network { .. })
                | CoreError::Connection(ConnectionError::PoolError { .. })
        )
    }
}

impl From<String> for CoreError {
    fn from(s: String) -> Self {
        CoreError::Common(CommonError::General(s))
    }
}

impl From<&str> for CoreError {
    fn from(s: &str) -> Self {
        CoreError::Common(CommonError::General(s.to_string()))
    }
}

impl From<std::io::Error> for CoreError {
    fn from(e: std::io::Error) -> Self {
        CoreError::Storage(StorageError::Io {
            path: String::new(),
            operation: "io".to_string(),
            reason: e.to_string(),
        })
    }
}

impl From<serde_json::Error> for CoreError {
    fn from(e: serde_json::Error) -> Self {
        CoreError::Storage(StorageError::Serialization {
            format: "json".to_string(),
            reason: e.to_string(),
        })
    }
}

impl From<duckdb::Error> for CoreError {
    fn from(e: duckdb::Error) -> Self {
        CoreError::Database(DatabaseError::Driver {
            db_type: "DuckDB".to_string(),
            operation: "query".to_string(),
            source: e.to_string(),
        })
    }
}

impl From<rusqlite::Error> for CoreError {
    fn from(e: rusqlite::Error) -> Self {
        CoreError::Storage(StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: "query".to_string(),
            reason: e.to_string(),
        })
    }
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoreError::Common(e) => write!(f, "[{}] {}", self.code(), e),
            CoreError::Connection(e) => write!(f, "[{}] {}", self.code(), e),
            CoreError::Database(e) => write!(f, "[{}] {}", self.code(), e),
            CoreError::Storage(e) => write!(f, "[{}] {}", self.code(), e),
            CoreError::Cache(e) => write!(f, "[{}] {}", self.code(), e),
            CoreError::Plugin(e) => {
                write!(f, "[{}] {}", self.code(), e)
            }
        }
    }
}

impl fmt::Display for CommonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommonError::General(msg) => write!(f, "{}", msg),
            CommonError::InvalidArgument { param, reason } => {
                write!(f, "Invalid argument '{}': {}", param, reason)
            }
            CommonError::NotSupported(feature) => {
                write!(f, "Feature not supported: {}", feature)
            }
            CommonError::Timeout {
                operation,
                duration_ms,
            } => {
                write!(
                    f,
                    "Operation '{}' timed out after {}ms",
                    operation, duration_ms
                )
            }
            CommonError::Internal(msg) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl fmt::Display for ConnectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnectionError::Refused { conn_id, reason } => {
                write!(f, "Connection '{}' refused: {}", conn_id, reason)
            }
            ConnectionError::Timeout {
                conn_id,
                duration_ms,
            } => {
                write!(
                    f,
                    "Connection '{}' timeout after {}ms",
                    conn_id, duration_ms
                )
            }
            ConnectionError::AuthenticationFailed { conn_id, username } => {
                write!(
                    f,
                    "Connection '{}' authentication failed for user '{}'",
                    conn_id, username
                )
            }
            ConnectionError::Network { conn_id, reason } => {
                write!(f, "Connection '{}' network error: {}", conn_id, reason)
            }
            ConnectionError::HostNotFound { conn_id, host } => {
                write!(f, "Connection '{}' host '{}' not found", conn_id, host)
            }
            ConnectionError::PortUnreachable {
                conn_id,
                host,
                port,
            } => {
                write!(
                    f,
                    "Connection '{}' port {} on '{}' unreachable",
                    conn_id, port, host
                )
            }
            ConnectionError::Tls { conn_id, reason } => {
                write!(f, "Connection '{}' TLS error: {}", conn_id, reason)
            }
            ConnectionError::NotFound(conn_id) => {
                write!(f, "Connection '{}' not found", conn_id)
            }
            ConnectionError::DriverNotFound { driver } => {
                write!(f, "Driver '{}' not found in registry", driver)
            }
            ConnectionError::NoActiveConnection => {
                write!(f, "No active connection")
            }
            ConnectionError::PoolError { pool_id, reason } => {
                write!(f, "Pool '{}' error: {}", pool_id, reason)
            }
            ConnectionError::PoolExhausted { conn_id, reason } => {
                write!(f, "Connection '{}' pool exhausted: {}", conn_id, reason)
            }
            ConnectionError::PoolClosed => {
                write!(f, "Connection pool is closed")
            }
            ConnectionError::Io { conn_id, reason } => {
                write!(f, "Connection '{}' IO error: {}", conn_id, reason)
            }
            ConnectionError::InvalidConfig { conn_id, reason } => {
                write!(f, "Connection '{}' invalid config: {}", conn_id, reason)
            }
            ConnectionError::NotSupported(msg) => {
                write!(f, "Not supported: {}", msg)
            }
            ConnectionError::Other { conn_id, reason } => {
                write!(f, "Connection '{}' error: {}", conn_id, reason)
            }
        }
    }
}

impl fmt::Display for DatabaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DatabaseError::Query {
                sql,
                reason,
                position,
            } => {
                if let Some(pos) = position {
                    write!(
                        f,
                        "Query failed at position {}: {} (SQL: {})",
                        pos, reason, sql
                    )
                } else {
                    write!(f, "Query failed: {} (SQL: {})", reason, sql)
                }
            }
            DatabaseError::Syntax {
                sql,
                message,
                line,
                column,
            } => {
                if let (Some(l), Some(c)) = (line, column) {
                    write!(
                        f,
                        "Syntax error at line {}, column {}: {} (SQL: {})",
                        l, c, message, sql
                    )
                } else {
                    write!(f, "Syntax error: {} (SQL: {})", message, sql)
                }
            }
            DatabaseError::Transaction {
                operation,
                state,
                reason,
            } => {
                write!(
                    f,
                    "Transaction '{}' failed in state {:?}: {}",
                    operation, state, reason
                )
            }
            DatabaseError::Driver {
                db_type,
                operation,
                source,
            } => {
                write!(
                    f,
                    "Database '{}' operation '{}' failed: {}",
                    db_type, operation, source
                )
            }
            DatabaseError::DriverNotFound { db_type } => {
                write!(f, "Driver for database type '{}' not found", db_type)
            }
            DatabaseError::DriverVersionMismatch {
                driver,
                expected,
                found,
            } => {
                write!(
                    f,
                    "Driver '{}' version mismatch: expected {}, found {}",
                    driver, expected, found
                )
            }
            DatabaseError::ConstraintViolation {
                constraint,
                table,
                reason,
            } => {
                write!(
                    f,
                    "Constraint '{}' violation on table '{}': {}",
                    constraint, table, reason
                )
            }
            DatabaseError::TableNotFound { table } => {
                write!(f, "Table '{}' not found", table)
            }
            DatabaseError::ColumnNotFound { column, table } => {
                write!(f, "Column '{}' not found in table '{}'", column, table)
            }
        }
    }
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageError::Persistence {
                store,
                operation,
                reason,
            } => {
                write!(
                    f,
                    "Persistence store '{}' operation '{}' failed: {}",
                    store, operation, reason
                )
            }
            StorageError::Serialization { format, reason } => {
                write!(f, "Serialization to '{}' failed: {}", format, reason)
            }
            StorageError::Deserialization {
                format,
                data,
                reason,
            } => {
                write!(
                    f,
                    "Deserialization from '{}' failed: {} (data: {})",
                    format, reason, data
                )
            }
            StorageError::Io {
                path,
                operation,
                reason,
            } => {
                write!(
                    f,
                    "IO operation '{}' on '{}' failed: {}",
                    operation, path, reason
                )
            }
        }
    }
}

impl std::error::Error for CoreError {}

/// 错误分类
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ErrorCategory {
    Common,
    Connection,
    Database,
    Storage,
    Cache,
    Plugin,
}

impl fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorCategory::Common => write!(f, "Common"),
            ErrorCategory::Connection => write!(f, "Connection"),
            ErrorCategory::Database => write!(f, "Database"),
            ErrorCategory::Storage => write!(f, "Storage"),
            ErrorCategory::Cache => write!(f, "Cache"),
            ErrorCategory::Plugin => write!(f, "Plugin"),
        }
    }
}

/// 结果类型别名
pub type CoreResult<T> = Result<T, CoreError>;

// ==================== 便捷构造函数 ====================

/// 创建通用错误
pub fn common_err(msg: impl Into<String>) -> CoreError {
    CoreError::common(CommonError::general(msg))
}

/// 创建无效参数错误
pub fn invalid_arg(param: impl Into<String>, reason: impl Into<String>) -> CoreError {
    CoreError::common(CommonError::invalid_argument(param, reason))
}

/// 创建不支持错误
pub fn not_supported(feature: impl Into<String>) -> CoreError {
    CoreError::common(CommonError::not_supported(feature))
}

/// 创建超时错误
pub fn timeout(operation: impl Into<String>, duration_ms: u64) -> CoreError {
    CoreError::common(CommonError::timeout(operation, duration_ms))
}

/// 创建连接错误
pub fn conn_err(_conn_id: impl Into<String>, kind: ConnectionError) -> CoreError {
    CoreError::connection(kind)
}

/// 创建数据库查询错误
pub fn query_err(sql: impl Into<String>, reason: impl Into<String>) -> CoreError {
    CoreError::database(DatabaseError::query(sql, reason))
}

/// 创建存储错误
pub fn storage_err(
    store: impl Into<String>,
    operation: impl Into<String>,
    reason: impl Into<String>,
) -> CoreError {
    CoreError::storage(StorageError::persistence(store, operation, reason))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_domain_separation() {
        let common = CoreError::common(CommonError::general("test"));
        let conn = CoreError::connection(ConnectionError::not_found("conn1"));
        let db = CoreError::database(DatabaseError::query("SELECT", "syntax error"));

        assert_eq!(common.category(), ErrorCategory::Common);
        assert_eq!(conn.category(), ErrorCategory::Connection);
        assert_eq!(db.category(), ErrorCategory::Database);
    }

    #[test]
    fn test_error_code() {
        let err = CoreError::common(CommonError::general("test"));
        assert_eq!(err.code(), "COMMON_GENERAL");

        let err = CoreError::connection(ConnectionError::timeout("conn1", 5000));
        assert_eq!(err.code(), "CONN_TIMEOUT");
    }

    #[test]
    fn test_retryable() {
        let err = timeout("query", 5000);
        assert!(err.is_retryable());

        let err = CoreError::connection(ConnectionError::not_found("conn1"));
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_display() {
        let err = invalid_arg("host", "cannot be empty");
        let msg = err.to_string();
        assert!(msg.contains("COMMON_INVALID_ARG"));
        assert!(msg.contains("host"));
    }
}
