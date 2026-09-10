//! 错误传播与快照命令 单元测试
//!
//! 覆盖：CoreError 所有变体序列化、Display trait、
//!       ErrorCategory、From 实现、SnapshotResult 序列化。
//!
//! 注意：CoreError 和 SnapshotResult 仅实现 Serialize（非 Deserialize），
//!       因此反序列化测试不做，仅验证序列化 JSON 结构完整性。
//!
//! 本文件位于 src-tauri/tests/（集成测试），
//! 遵循 RdataStation 测试代码组织铁律。

use rdata_station_lib::commands::SnapshotResult;
use rdata_station_lib::core::error::{
    CacheError, CommonError, ConnectionError, CoreError, DatabaseError, ErrorCategory, PluginError,
    StorageError, TransactionState,
};

// ==================== CoreError 序列化测试 ====================

#[test]
fn test_core_error_serialization_common() {
    let err = CoreError::common(CommonError::general("测试错误"));
    let json = serde_json::to_string(&err).expect("序列化失败");
    assert!(json.contains("Common"));
    assert!(json.contains("测试错误"));
    assert_eq!(err.category(), ErrorCategory::Common);
}

#[test]
fn test_core_error_serialization_connection() {
    let err = CoreError::connection(ConnectionError::not_found("conn-1"));
    let json = serde_json::to_string(&err).expect("序列化失败");
    assert!(json.contains("Connection"));
    assert!(json.contains("conn-1"));
    assert_eq!(err.category(), ErrorCategory::Connection);
}

#[test]
fn test_core_error_serialization_database() {
    let err = CoreError::database(DatabaseError::query("SELECT * FROM x", "语法错误"));
    let json = serde_json::to_string(&err).expect("序列化失败");
    assert!(json.contains("Database"));
    assert_eq!(err.category(), ErrorCategory::Database);
}

#[test]
fn test_core_error_serialization_storage() {
    let err = CoreError::storage(StorageError::persistence("sqlite", "write", "磁盘满"));
    let json = serde_json::to_string(&err).expect("序列化失败");
    assert!(json.contains("Storage"));
    assert_eq!(err.category(), ErrorCategory::Storage);
}

#[test]
fn test_core_error_serialization_cache() {
    let err = CoreError::cache(CacheError::miss("key-123"));
    let json = serde_json::to_string(&err).expect("序列化失败");
    assert!(json.contains("Cache"));
    assert_eq!(err.category(), ErrorCategory::Cache);
}

#[test]
fn test_core_error_serialization_plugin() {
    let err = CoreError::plugin(PluginError::not_found("plugin-1"));
    let json = serde_json::to_string(&err).expect("序列化失败");
    assert!(json.contains("Plugin"));
    assert_eq!(err.category(), ErrorCategory::Plugin);
}

#[test]
fn test_core_error_serialization_all_domains() {
    let errors = vec![
        CoreError::common(CommonError::general("test")),
        CoreError::connection(ConnectionError::not_found("c1")),
        CoreError::database(DatabaseError::query("SQL", "reason")),
        CoreError::storage(StorageError::persistence("store", "op", "reason")),
        CoreError::cache(CacheError::miss("key")),
        CoreError::plugin(PluginError::not_found("p1")),
    ];

    for err in &errors {
        let json = serde_json::to_string(err).expect("序列化失败");
        // 验证 JSON 是有效格式（包含花括号）
        assert!(json.starts_with('{'));
        assert!(json.ends_with('}'));
        // 验证 category 和 code 方法可用
        let _cat = err.category();
        let _code = err.code();
    }
}

// ==================== ConnectionError 所有变体测试 ====================

#[test]
fn test_connection_error_refused() {
    let err = ConnectionError::refused("conn-1", "连接被拒绝");
    let msg = err.to_string();
    assert!(msg.contains("conn-1"));
    assert!(msg.contains("refused"));
    assert!(msg.contains("连接被拒绝"));
}

#[test]
fn test_connection_error_timeout() {
    let err = ConnectionError::timeout("conn-2", 5000);
    let msg = err.to_string();
    assert!(msg.contains("conn-2"));
    assert!(msg.contains("timeout"));
    assert!(msg.contains("5000"));
}

#[test]
fn test_connection_error_auth_failed() {
    let err = ConnectionError::auth_failed("conn-3", "admin");
    let msg = err.to_string();
    assert!(msg.contains("conn-3"));
    assert!(msg.contains("admin"));
    assert!(msg.contains("authentication"));
}

#[test]
fn test_connection_error_network() {
    let core_err = CoreError::connection(ConnectionError::Network {
        conn_id: "conn-net".to_string(),
        reason: "DNS 解析失败".to_string(),
    });
    assert_eq!(core_err.code(), "CONN_NETWORK");
    let msg = core_err.to_string();
    assert!(msg.contains("DNS"));
}

#[test]
fn test_connection_error_tls() {
    let core_err = CoreError::connection(ConnectionError::Tls {
        conn_id: "conn-tls".to_string(),
        reason: "证书无效".to_string(),
    });
    assert_eq!(core_err.code(), "CONN_TLS");
    let msg = core_err.to_string();
    assert!(msg.contains("TLS"));
    assert!(msg.contains("证书无效"));
}

#[test]
fn test_connection_error_invalid_config() {
    let core_err = CoreError::connection(ConnectionError::InvalidConfig {
        conn_id: "conn-cfg".to_string(),
        reason: "端口号超出范围".to_string(),
    });
    assert_eq!(core_err.code(), "CONN_INVALID_CONFIG");
    let msg = core_err.to_string();
    assert!(msg.contains("invalid config"));
    assert!(msg.contains("端口号"));
}

#[test]
fn test_connection_error_not_supported() {
    let core_err = CoreError::connection(ConnectionError::NotSupported("特性X".to_string()));
    assert_eq!(core_err.code(), "CONN_NOT_SUPPORTED");
    let msg = core_err.to_string();
    assert!(msg.contains("Not supported"));
}

#[test]
fn test_connection_error_driver_not_found() {
    let core_err = CoreError::connection(ConnectionError::DriverNotFound {
        driver: "oracle".to_string(),
    });
    assert_eq!(core_err.code(), "CONN_DRIVER_NOT_FOUND");
    let msg = core_err.to_string();
    assert!(msg.contains("oracle"));
}

#[test]
fn test_connection_error_pool_exhausted() {
    let core_err = CoreError::connection(ConnectionError::PoolExhausted {
        conn_id: "pool-1".to_string(),
        reason: "连接数已达上限".to_string(),
    });
    assert_eq!(core_err.code(), "CONN_POOL_EXHAUSTED");
    let msg = core_err.to_string();
    assert!(msg.contains("pool exhausted"));
}

#[test]
fn test_connection_error_no_active_connection() {
    let core_err = CoreError::connection(ConnectionError::NoActiveConnection);
    assert_eq!(core_err.code(), "CONN_NO_ACTIVE");
    let msg = core_err.to_string();
    assert!(msg.contains("No active connection"));
}

// ==================== CommonError 所有变体测试 ====================

#[test]
fn test_common_error_general() {
    let err = CommonError::general("通用错误信息");
    let msg = err.to_string();
    assert_eq!(msg, "通用错误信息");
}

#[test]
fn test_common_error_invalid_argument() {
    let err = CommonError::invalid_argument("host", "不能为空");
    let msg = err.to_string();
    assert!(msg.contains("host"));
    assert!(msg.contains("不能为空"));
}

#[test]
fn test_common_error_not_supported() {
    let err = CommonError::not_supported("联邦查询");
    let msg = err.to_string();
    assert!(msg.contains("联邦查询"));
    assert!(msg.contains("not supported"));
}

#[test]
fn test_common_error_timeout() {
    let err = CommonError::timeout("查询", 10000);
    let msg = err.to_string();
    assert!(msg.contains("查询"));
    assert!(msg.contains("10000"));
    assert!(msg.contains("timed out"));
}

#[test]
fn test_common_error_internal() {
    let core_err = CoreError::common(CommonError::Internal("内部错误".to_string()));
    assert_eq!(core_err.code(), "COMMON_INTERNAL");
    let msg = core_err.to_string();
    assert!(msg.contains("Internal"));
    assert!(msg.contains("内部错误"));
}

// ==================== DatabaseError 所有变体测试 ====================

#[test]
fn test_database_error_query() {
    let err = DatabaseError::query("SELECT * FROM nonexistent", "表不存在");
    let msg = err.to_string();
    assert!(msg.contains("Query failed"));
    assert!(msg.contains("nonexistent"));
    assert!(msg.contains("表不存在"));
}

#[test]
fn test_database_error_query_with_position() {
    let err = DatabaseError::query("SELECT * FROM users WHERE", "语法错误").with_position(15);
    let msg = err.to_string();
    assert!(msg.contains("position 15"));
}

#[test]
fn test_database_error_syntax() {
    let core_err = CoreError::database(DatabaseError::Syntax {
        sql: "SLECT * FROM users".to_string(),
        message: "unknown keyword SLECT".to_string(),
        line: Some(1),
        column: Some(1),
    });
    assert_eq!(core_err.code(), "DB_SYNTAX");
    let msg = core_err.to_string();
    assert!(msg.contains("line 1"));
    assert!(msg.contains("column 1"));
}

#[test]
fn test_database_error_transaction() {
    let core_err = CoreError::database(DatabaseError::Transaction {
        operation: "commit".to_string(),
        state: TransactionState::Failed("deadlock".to_string()),
        reason: "事务冲突".to_string(),
    });
    assert_eq!(core_err.code(), "DB_TRANSACTION");
    let msg = core_err.to_string();
    assert!(msg.contains("commit"));
    assert!(msg.contains("deadlock"));
}

#[test]
fn test_database_error_driver_version_mismatch() {
    let core_err = CoreError::database(DatabaseError::DriverVersionMismatch {
        driver: "mysql".to_string(),
        expected: "8.0".to_string(),
        found: "5.7".to_string(),
    });
    assert_eq!(core_err.code(), "DB_DRIVER_VERSION");
    let msg = core_err.to_string();
    assert!(msg.contains("5.7"));
    assert!(msg.contains("8.0"));
}

#[test]
fn test_database_error_constraint_violation() {
    let core_err = CoreError::database(DatabaseError::ConstraintViolation {
        constraint: "fk_user_order".to_string(),
        table: "orders".to_string(),
        reason: "引用的用户不存在".to_string(),
    });
    assert_eq!(core_err.code(), "DB_CONSTRAINT");
    let msg = core_err.to_string();
    assert!(msg.contains("fk_user_order"));
    assert!(msg.contains("orders"));
}

#[test]
fn test_database_error_table_not_found() {
    let core_err = CoreError::database(DatabaseError::TableNotFound {
        table: "missing_table".to_string(),
    });
    assert_eq!(core_err.code(), "DB_TABLE_NOT_FOUND");
    let msg = core_err.to_string();
    assert!(msg.contains("missing_table"));
}

#[test]
fn test_database_error_column_not_found() {
    let core_err = CoreError::database(DatabaseError::ColumnNotFound {
        column: "unknown_col".to_string(),
        table: "users".to_string(),
    });
    assert_eq!(core_err.code(), "DB_COLUMN_NOT_FOUND");
    let msg = core_err.to_string();
    assert!(msg.contains("unknown_col"));
    assert!(msg.contains("users"));
}

#[test]
fn test_database_error_driver_not_found() {
    let core_err = CoreError::database(DatabaseError::DriverNotFound {
        db_type: "unknown_db".to_string(),
    });
    assert_eq!(core_err.code(), "DB_DRIVER_NOT_FOUND");
    let msg = core_err.to_string();
    assert!(msg.contains("unknown_db"));
}

#[test]
fn test_database_error_driver() {
    let core_err = CoreError::database(DatabaseError::Driver {
        db_type: "PostgreSQL".to_string(),
        operation: "connect".to_string(),
        source: "connection refused".to_string(),
    });
    assert_eq!(core_err.code(), "DB_DRIVER");
    let msg = core_err.to_string();
    assert!(msg.contains("PostgreSQL"));
    assert!(msg.contains("connect"));
}

// ==================== StorageError 所有变体测试 ====================

#[test]
fn test_storage_error_persistence_via_constructor() {
    let err = StorageError::persistence("sqlite", "read", "文件不存在");
    let msg = err.to_string();
    assert!(msg.contains("sqlite"));
    assert!(msg.contains("read"));
    assert!(msg.contains("文件不存在"));
}

#[test]
fn test_storage_error_write() {
    let err = StorageError::write("history_store", "写入失败");
    let msg = err.to_string();
    assert!(msg.contains("history_store"));
    assert!(msg.contains("write"));
    assert!(msg.contains("写入失败"));
}

#[test]
fn test_storage_error_read() {
    let err = StorageError::read("connection_store", "读取失败");
    let msg = err.to_string();
    assert!(msg.contains("connection_store"));
    assert!(msg.contains("read"));
    assert!(msg.contains("读取失败"));
}

#[test]
fn test_storage_error_serialization() {
    let core_err = CoreError::storage(StorageError::Serialization {
        format: "json".to_string(),
        reason: "无效的 UTF-8".to_string(),
    });
    assert_eq!(core_err.code(), "STORE_SERIALIZE");
    let msg = core_err.to_string();
    assert!(msg.contains("json"));
    assert!(msg.contains("无效的 UTF-8"));
}

#[test]
fn test_storage_error_deserialization() {
    let core_err = CoreError::storage(StorageError::Deserialization {
        format: "json".to_string(),
        data: "{invalid}".to_string(),
        reason: "缺少字段".to_string(),
    });
    assert_eq!(core_err.code(), "STORE_DESERIALIZE");
    let msg = core_err.to_string();
    assert!(msg.contains("缺少字段"));
}

#[test]
fn test_storage_error_io_via_constructor() {
    let err = StorageError::io("/path/to/file", "open", "权限不足");
    let msg = err.to_string();
    assert!(msg.contains("/path/to/file"));
    assert!(msg.contains("open"));
    assert!(msg.contains("权限不足"));
}

// ==================== ErrorCategory 测试 ====================

#[test]
fn test_error_category_display() {
    assert_eq!(ErrorCategory::Common.to_string(), "Common");
    assert_eq!(ErrorCategory::Connection.to_string(), "Connection");
    assert_eq!(ErrorCategory::Database.to_string(), "Database");
    assert_eq!(ErrorCategory::Storage.to_string(), "Storage");
    assert_eq!(ErrorCategory::Cache.to_string(), "Cache");
    assert_eq!(ErrorCategory::Plugin.to_string(), "Plugin");
}

#[test]
fn test_error_category_from_core_error() {
    assert_eq!(
        CoreError::common(CommonError::general("x")).category(),
        ErrorCategory::Common
    );
    assert_eq!(
        CoreError::connection(ConnectionError::not_found("x")).category(),
        ErrorCategory::Connection
    );
    assert_eq!(
        CoreError::database(DatabaseError::query("x", "x")).category(),
        ErrorCategory::Database
    );
    assert_eq!(
        CoreError::storage(StorageError::persistence("x", "x", "x")).category(),
        ErrorCategory::Storage
    );
    assert_eq!(
        CoreError::cache(CacheError::miss("x")).category(),
        ErrorCategory::Cache
    );
    assert_eq!(
        CoreError::plugin(PluginError::not_found("x")).category(),
        ErrorCategory::Plugin
    );
}

// ==================== 错误重试判定测试 ====================

#[test]
fn test_error_retryable_timeout() {
    let err = CoreError::common(CommonError::timeout("query", 5000));
    assert!(err.is_retryable());

    let err = CoreError::connection(ConnectionError::timeout("c1", 3000));
    assert!(err.is_retryable());
}

#[test]
fn test_error_retryable_network() {
    let err = CoreError::connection(ConnectionError::Network {
        conn_id: "c1".to_string(),
        reason: "断开".to_string(),
    });
    assert!(err.is_retryable());
}

#[test]
fn test_error_retryable_pool() {
    let err = CoreError::connection(ConnectionError::PoolError {
        pool_id: "p1".to_string(),
        reason: "busy".to_string(),
    });
    assert!(err.is_retryable());
}

#[test]
fn test_error_not_retryable() {
    let err = CoreError::connection(ConnectionError::not_found("c1"));
    assert!(!err.is_retryable());

    let err = CoreError::common(CommonError::general("error"));
    assert!(!err.is_retryable());

    let err = CoreError::database(DatabaseError::query("x", "x"));
    assert!(!err.is_retryable());
}

// ==================== Error Display 测试 ====================

#[test]
fn test_error_display_contains_code() {
    let err = CoreError::common(CommonError::invalid_argument("host", "empty"));
    let msg = err.to_string();
    assert!(msg.contains("COMMON_INVALID_ARG"));

    let err = CoreError::connection(ConnectionError::timeout("c1", 5000));
    let msg = err.to_string();
    assert!(msg.contains("CONN_TIMEOUT"));

    let err = CoreError::database(DatabaseError::query("SQL", "fail"));
    let msg = err.to_string();
    assert!(msg.contains("DB_QUERY"));
}

#[test]
fn test_error_display_cache_error() {
    let err = CacheError::miss("my-key");
    assert!(err.to_string().contains("my-key"));

    let err = CacheError::full(100);
    assert!(err.to_string().contains("100"));

    let err = CacheError::invalid_key("bad", "too long");
    assert!(err.to_string().contains("bad"));
    assert!(err.to_string().contains("too long"));

    let err = CacheError::internal("corrupted");
    assert!(err.to_string().contains("corrupted"));
}

#[test]
fn test_error_display_plugin_error() {
    let err = PluginError::load_failed("p1", "missing dep");
    assert!(err.to_string().contains("p1"));
    assert!(err.to_string().contains("missing dep"));

    let err = PluginError::execution_failed("p2", "run", "timeout");
    assert!(err.to_string().contains("p2"));
    assert!(err.to_string().contains("run"));
    assert!(err.to_string().contains("timeout"));

    let err = PluginError::dependency_missing("p3", "dep-a", "1.0");
    assert!(err.to_string().contains("p3"));
    assert!(err.to_string().contains("dep-a"));
    assert!(err.to_string().contains("1.0"));
}

// ==================== Error From 实现测试 ====================

#[test]
fn test_from_string_to_core_error() {
    let err: CoreError = "错误消息".to_string().into();
    assert_eq!(err.category(), ErrorCategory::Common);
    let msg = err.to_string();
    assert!(msg.contains("错误消息"));
}

#[test]
fn test_from_str_to_core_error() {
    let err: CoreError = "错误消息".into();
    assert_eq!(err.category(), ErrorCategory::Common);
    assert_eq!(err.code(), "COMMON_GENERAL");
}

#[test]
fn test_from_io_error_to_core_error() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "文件未找到");
    let core_err: CoreError = io_err.into();
    assert_eq!(core_err.category(), ErrorCategory::Storage);
    let msg = core_err.to_string();
    assert!(msg.contains("文件未找到"));
}

#[test]
fn test_from_serde_json_error_to_core_error() {
    let json_err = serde_json::from_str::<serde_json::Value>("invalid json").unwrap_err();
    let core_err: CoreError = json_err.into();
    assert_eq!(core_err.category(), ErrorCategory::Storage);
    let msg = core_err.to_string();
    assert!(msg.contains("json"));
}

// ==================== TransactionState 测试 ====================

#[test]
fn test_transaction_state_variants() {
    let not_started = TransactionState::NotStarted;
    let in_progress = TransactionState::InProgress;
    let committed = TransactionState::Committed;
    let rolled_back = TransactionState::RolledBack;
    let failed = TransactionState::Failed("deadlock".to_string());

    let _ = not_started;
    let _ = in_progress;
    let _ = committed;
    let _ = rolled_back;
    let _ = failed;
}

// ==================== SnapshotResult 序列化测试 ====================

#[test]
fn test_snapshot_result_serialization() {
    let result = SnapshotResult {
        snapshot_id: "GP_env_mysql_20240101".to_string(),
        origin: "global_snapshot".to_string(),
        source_id: "G_env_mysql".to_string(),
    };
    let json = serde_json::to_string(&result).expect("序列化失败");
    assert!(json.contains("GP_env_mysql_20240101"));
    assert!(json.contains("global_snapshot"));
    assert!(json.contains("G_env_mysql"));

    // 验证 JSON 可以解析为 serde_json::Value（验证有效的 JSON 结构）
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("JSON 解析失败");
    assert_eq!(parsed["snapshot_id"], "GP_env_mysql_20240101");
    assert_eq!(parsed["origin"], "global_snapshot");
    assert_eq!(parsed["source_id"], "G_env_mysql");
}

#[test]
fn test_snapshot_result_env_snapshot() {
    // 模拟 snapshot_global_env 返回的快照结果
    let result = SnapshotResult {
        snapshot_id: "GP_env_prod_20240315".to_string(),
        origin: "global_snapshot".to_string(),
        source_id: "G_env_prod".to_string(),
    };
    assert_eq!(result.origin, "global_snapshot");
    assert!(result.snapshot_id.starts_with("GP_env_"));
}

#[test]
fn test_snapshot_result_auth_snapshot() {
    // 模拟 snapshot_global_auth 返回的快照结果
    let result = SnapshotResult {
        snapshot_id: "GP_auth_mysql_20240315".to_string(),
        origin: "global_snapshot".to_string(),
        source_id: "G_auth_mysql".to_string(),
    };
    assert_eq!(result.origin, "global_snapshot");
    assert!(result.source_id.starts_with("G_auth_"));
}

#[test]
fn test_snapshot_result_network_snapshot() {
    // 模拟 snapshot_global_network 返回的快照结果
    let result = SnapshotResult {
        snapshot_id: "GP_net_ssh_20240315".to_string(),
        origin: "global_snapshot".to_string(),
        source_id: "G_net_ssh".to_string(),
    };
    assert_eq!(result.origin, "global_snapshot");
    assert!(result.source_id.starts_with("G_net_"));
}

#[test]
fn test_snapshot_result_multiple_snapshots() {
    // 同一个环境可以快照多次，每次生成不同的 snapshot_id
    let snap1 = SnapshotResult {
        snapshot_id: "GP_env_prod_20240101".to_string(),
        origin: "global_snapshot".to_string(),
        source_id: "G_env_prod".to_string(),
    };
    let snap2 = SnapshotResult {
        snapshot_id: "GP_env_prod_20240601".to_string(),
        origin: "global_snapshot".to_string(),
        source_id: "G_env_prod".to_string(),
    };

    assert_eq!(snap1.source_id, snap2.source_id);
    assert_ne!(snap1.snapshot_id, snap2.snapshot_id);
}

// ==================== 错误传播：包含有用消息测试 ====================

#[test]
fn test_error_propagation_contains_useful_message() {
    let err = CoreError::common(CommonError::invalid_argument("port", "must be 1-65535"));
    let msg = err.to_string();
    assert!(msg.contains("port"));
    assert!(msg.contains("1-65535"));
    assert!(msg.contains("COMMON_INVALID_ARG"));
}

#[test]
fn test_error_propagation_connection_chain() {
    let err = CoreError::connection(ConnectionError::HostNotFound {
        conn_id: "prod-db".to_string(),
        host: "db.internal.example.com".to_string(),
    });
    let msg = err.to_string();
    assert!(msg.contains("prod-db"));
    assert!(msg.contains("db.internal.example.com"));
    assert!(msg.contains("CONN_HOST_NOT_FOUND"));
}

#[test]
fn test_error_propagation_port_unreachable() {
    let err = CoreError::connection(ConnectionError::PortUnreachable {
        conn_id: "test-conn".to_string(),
        host: "localhost".to_string(),
        port: 5432,
    });
    let msg = err.to_string();
    assert!(msg.contains("test-conn"));
    assert!(msg.contains("5432"));
    assert!(msg.contains("localhost"));
    assert!(msg.contains("CONN_PORT_UNREACHABLE"));
}

// ==================== 缓存错误完整性测试 ====================

#[test]
fn test_cache_error_via_constructors() {
    let err = CacheError::miss("my-key");
    assert_eq!(err.to_string(), "Cache miss for key: my-key");

    let err = CacheError::full(100);
    assert_eq!(err.to_string(), "Cache is full (capacity: 100)");

    let err = CacheError::invalid_key("bad", "too long");
    assert!(err.to_string().contains("bad"));

    let err = CacheError::invalid_value("corrupted data");
    assert!(err.to_string().contains("corrupted data"));

    let err = CacheError::timeout("evict", 5000);
    assert!(err.to_string().contains("evict"));
    assert!(err.to_string().contains("5000"));

    let err = CacheError::internal("oom");
    assert!(err.to_string().contains("oom"));

    let err = CacheError::serialization("invalid format");
    assert!(err.to_string().contains("invalid format"));
}

// ==================== 错误域分离测试 ====================

#[test]
fn test_error_domain_separation() {
    let common = CoreError::common(CommonError::general("test"));
    let conn = CoreError::connection(ConnectionError::not_found("conn1"));
    let db = CoreError::database(DatabaseError::query("SELECT", "syntax error"));
    let storage = CoreError::storage(StorageError::persistence("store", "op", "reason"));
    let cache = CoreError::cache(CacheError::miss("key"));
    let plugin = CoreError::plugin(PluginError::not_found("p1"));

    // 确保每个域的错误码不同
    assert_eq!(common.code(), "COMMON_GENERAL");
    assert_eq!(conn.code(), "CONN_NOT_FOUND");
    assert_eq!(db.code(), "DB_QUERY");
    assert_eq!(storage.code(), "STORE_PERSISTENCE");
    assert_eq!(cache.code(), "CACHE_MISS");
    assert_eq!(plugin.code(), "PLUGIN_NOT_FOUND");
}

// ==================== 错误域边界测试 ====================

#[test]
fn test_error_messages_not_empty() {
    // 确保所有错误类型都有有意义的 Display 输出
    let errors: Vec<CoreError> = vec![
        CoreError::common(CommonError::general("test")),
        CoreError::common(CommonError::invalid_argument("x", "y")),
        CoreError::common(CommonError::not_supported("f")),
        CoreError::common(CommonError::timeout("t", 1000)),
        CoreError::common(CommonError::Internal("e".to_string())),
        CoreError::connection(ConnectionError::refused("c", "r")),
        CoreError::connection(ConnectionError::timeout("c", 1000)),
        CoreError::connection(ConnectionError::auth_failed("c", "u")),
        CoreError::connection(ConnectionError::Network {
            conn_id: "c".to_string(),
            reason: "r".to_string(),
        }),
        CoreError::connection(ConnectionError::HostNotFound {
            conn_id: "c".to_string(),
            host: "h".to_string(),
        }),
        CoreError::connection(ConnectionError::PortUnreachable {
            conn_id: "c".to_string(),
            host: "h".to_string(),
            port: 80,
        }),
        CoreError::connection(ConnectionError::Tls {
            conn_id: "c".to_string(),
            reason: "r".to_string(),
        }),
        CoreError::connection(ConnectionError::NotFound("c".to_string())),
        CoreError::connection(ConnectionError::DriverNotFound {
            driver: "d".to_string(),
        }),
        CoreError::connection(ConnectionError::NoActiveConnection),
        CoreError::connection(ConnectionError::PoolError {
            pool_id: "p".to_string(),
            reason: "r".to_string(),
        }),
        CoreError::connection(ConnectionError::PoolExhausted {
            conn_id: "c".to_string(),
            reason: "r".to_string(),
        }),
        CoreError::connection(ConnectionError::PoolClosed),
        CoreError::connection(ConnectionError::Io {
            conn_id: "c".to_string(),
            reason: "r".to_string(),
        }),
        CoreError::connection(ConnectionError::InvalidConfig {
            conn_id: "c".to_string(),
            reason: "r".to_string(),
        }),
        CoreError::connection(ConnectionError::NotSupported("s".to_string())),
        CoreError::connection(ConnectionError::Other {
            conn_id: "c".to_string(),
            reason: "r".to_string(),
        }),
        CoreError::database(DatabaseError::query("sql", "reason")),
        CoreError::database(DatabaseError::Syntax {
            sql: "sql".to_string(),
            message: "msg".to_string(),
            line: None,
            column: None,
        }),
        CoreError::database(DatabaseError::Transaction {
            operation: "op".to_string(),
            state: TransactionState::Failed("f".to_string()),
            reason: "r".to_string(),
        }),
        CoreError::database(DatabaseError::Driver {
            db_type: "db".to_string(),
            operation: "op".to_string(),
            source: "src".to_string(),
        }),
        CoreError::database(DatabaseError::DriverNotFound {
            db_type: "db".to_string(),
        }),
        CoreError::database(DatabaseError::DriverVersionMismatch {
            driver: "d".to_string(),
            expected: "1.0".to_string(),
            found: "0.9".to_string(),
        }),
        CoreError::database(DatabaseError::ConstraintViolation {
            constraint: "c".to_string(),
            table: "t".to_string(),
            reason: "r".to_string(),
        }),
        CoreError::database(DatabaseError::TableNotFound {
            table: "t".to_string(),
        }),
        CoreError::database(DatabaseError::ColumnNotFound {
            column: "c".to_string(),
            table: "t".to_string(),
        }),
        CoreError::storage(StorageError::Persistence {
            store: "s".to_string(),
            operation: "op".to_string(),
            reason: "r".to_string(),
        }),
        CoreError::storage(StorageError::Serialization {
            format: "json".to_string(),
            reason: "r".to_string(),
        }),
        CoreError::storage(StorageError::Deserialization {
            format: "json".to_string(),
            data: "data".to_string(),
            reason: "r".to_string(),
        }),
        CoreError::storage(StorageError::Io {
            path: "p".to_string(),
            operation: "op".to_string(),
            reason: "r".to_string(),
        }),
        CoreError::cache(CacheError::miss("key")),
        CoreError::cache(CacheError::full(10)),
        CoreError::cache(CacheError::invalid_key("key", "reason")),
        CoreError::cache(CacheError::invalid_value("reason")),
        CoreError::cache(CacheError::timeout("op", 1000)),
        CoreError::cache(CacheError::internal("reason")),
        CoreError::cache(CacheError::serialization("reason")),
        CoreError::plugin(PluginError::not_found("p")),
        CoreError::plugin(PluginError::not_found_by_code("c", "v")),
        CoreError::plugin(PluginError::already_exists("c", "v")),
        CoreError::plugin(PluginError::load_failed("p", "r")),
        CoreError::plugin(PluginError::activation_failed("p", "r")),
        CoreError::plugin(PluginError::DeactivationFailed {
            plugin_id: "p".to_string(),
            reason: "r".to_string(),
        }),
        CoreError::plugin(PluginError::UninstallFailed {
            plugin_id: "p".to_string(),
            reason: "r".to_string(),
        }),
        CoreError::plugin(PluginError::execution_failed("p", "f", "r")),
        CoreError::plugin(PluginError::dependency_missing("p", "d", "v")),
        CoreError::plugin(PluginError::VersionIncompatible {
            plugin_id: "p".to_string(),
            version: "1.0".to_string(),
            expected: "2.0".to_string(),
        }),
        CoreError::plugin(PluginError::InvalidConfig {
            plugin_id: "p".to_string(),
            key: "k".to_string(),
            reason: "r".to_string(),
        }),
        CoreError::plugin(PluginError::InvalidManifest {
            reason: "r".to_string(),
        }),
        CoreError::plugin(PluginError::FileMissing {
            path: "p".to_string(),
            reason: "r".to_string(),
        }),
        CoreError::plugin(PluginError::Disabled {
            plugin_id: "p".to_string(),
        }),
        CoreError::plugin(PluginError::Internal {
            plugin_id: "p".to_string(),
            reason: "r".to_string(),
        }),
        CoreError::plugin(PluginError::UnsupportedType {
            plugin_type: "t".to_string(),
        }),
    ];

    for err in &errors {
        let msg = err.to_string();
        assert!(!msg.is_empty(), "错误消息不应为空: {:?}", err);
        // 所有错误消息应包含错误码
        assert!(msg.contains('['), "消息应包含错误码: {:?}", err);
    }
}
