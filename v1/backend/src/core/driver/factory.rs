use std::sync::Arc;

use super::connection::config::ConnectionMethod;
use super::connection::{ConnectionConfig, ConnectionFactory};
use super::registry::DriverConnectionConfig;
use crate::core::driver::native::{
    duckdb::DuckDbDatabase, mysql::MySqlDatabase, mysql_native::MySqlNativeDatabase,
    postgres::PostgresDatabase, postgres_native::PostgresNativeDatabase, sqlite::SqliteDatabase,
};
use crate::core::driver::{DriverDescriptor, DriverFactory, DynDatabase};
use crate::core::error::{ConnectionError, CoreError};

/// MySQL 驱动工厂
///
/// 实现 DriverFactory trait，用于创建 MySQL 数据库连接
/// 支持多种连接方式：直接连接、SSL、SSH 隧道、代理
pub struct MySqlDriverFactory;

impl DriverFactory for MySqlDriverFactory {
    fn descriptor(&self) -> DriverDescriptor {
        crate::core::driver::registry::mysql_driver()
    }

    fn create(
        &self,
        config: DriverConnectionConfig,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<DynDatabase, CoreError>> + Send>>
    {
        Box::pin(async move {
            let url = config.to_url().map_err(|e| {
                CoreError::connection(ConnectionError::InvalidConfig {
                    conn_id: config.name.clone().unwrap_or_else(|| "mysql".to_string()),
                    reason: e.to_string(),
                })
            })?;

            let db = MySqlDatabase::new(&url).await?;
            let db: DynDatabase = Arc::new(db);
            Ok(db)
        })
    }
}

/// PostgreSQL 驱动工厂
///
/// 实现 DriverFactory trait，用于创建 PostgreSQL 数据库连接
/// 支持多种连接方式：直接连接、SSL、SSH 隧道、代理
pub struct PostgresDriverFactory;

impl DriverFactory for PostgresDriverFactory {
    fn descriptor(&self) -> DriverDescriptor {
        crate::core::driver::registry::postgres_driver()
    }

    fn create(
        &self,
        config: DriverConnectionConfig,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<DynDatabase, CoreError>> + Send>>
    {
        Box::pin(async move {
            // 根据连接方式决定如何建立连接
            let url = match &config.connection_method {
                ConnectionMethod::Direct
                | ConnectionMethod::Ssl(_)
                | ConnectionMethod::Chain(_) => {
                    // 直接连接或 SSL 连接：使用标准 URL
                    config.to_url().map_err(|e| {
                        CoreError::connection(ConnectionError::InvalidConfig {
                            conn_id: config
                                .name
                                .clone()
                                .unwrap_or_else(|| "postgres".to_string()),
                            reason: e.to_string(),
                        })
                    })?
                }
                ConnectionMethod::Ssh(ssh_config) => {
                    // SSH 隧道连接：建立隧道后连接到本地端口
                    let conn_config = ConnectionConfig::ssh(
                        config
                            .host
                            .clone()
                            .unwrap_or_else(|| "localhost".to_string()),
                        config.port.unwrap_or(5432),
                        ssh_config.clone(),
                    );

                    let factory = ConnectionFactory::new();
                    let connection = factory.create_connection(conn_config).await?;

                    // 获取本地端口并构建 URL
                    let local_addr = connection.stream.local_addr().map_err(|e| {
                        CoreError::connection(ConnectionError::Network {
                            conn_id: "postgres_ssh_tunnel".to_string(),
                            reason: format!("Failed to get local address: {}", e),
                        })
                    })?;

                    format!(
                        "postgres://{}:{}@{}:{}/{}",
                        config.username.as_deref().unwrap_or("postgres"),
                        config.password.as_deref().unwrap_or(""),
                        local_addr.ip(),
                        local_addr.port(),
                        config.database.as_deref().unwrap_or("postgres")
                    )
                }
                ConnectionMethod::HttpProxy(_proxy_config)
                | ConnectionMethod::SocksProxy(_proxy_config) => {
                    // 代理连接：通过代理建立连接
                    let conn_config = ConnectionConfig {
                        host: config
                            .host
                            .clone()
                            .unwrap_or_else(|| "localhost".to_string()),
                        port: config.port.unwrap_or(5432),
                        method: config.connection_method.clone(),
                        options: std::collections::HashMap::new(),
                    };

                    let factory = ConnectionFactory::new();
                    let _connection = factory.create_connection(conn_config).await?;

                    // 对于代理连接，sqlx 需要特殊处理
                    // 这里简化处理，实际实现需要更复杂的逻辑
                    config.to_url().map_err(|e| {
                        CoreError::connection(ConnectionError::InvalidConfig {
                            conn_id: config
                                .name
                                .clone()
                                .unwrap_or_else(|| "postgres".to_string()),
                            reason: e.to_string(),
                        })
                    })?
                }
            };

            // 创建 PostgreSQL 连接池
            let pool = sqlx::postgres::PgPool::connect(&url).await.map_err(|e| {
                CoreError::connection(ConnectionError::Refused {
                    conn_id: config
                        .name
                        .clone()
                        .unwrap_or_else(|| "postgres".to_string()),
                    reason: e.to_string(),
                })
            })?;
            let db: DynDatabase = Arc::new(PostgresDatabase::from_pool(pool));
            Ok(db)
        })
    }
}

/// SQLite 驱动工厂
///
/// 实现 DriverFactory trait，用于创建 SQLite 数据库连接
pub struct SqliteDriverFactory;

impl DriverFactory for SqliteDriverFactory {
    fn descriptor(&self) -> DriverDescriptor {
        crate::core::driver::registry::sqlite_driver()
    }

    fn create(
        &self,
        config: DriverConnectionConfig,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<DynDatabase, CoreError>> + Send>>
    {
        Box::pin(async move {
            // SQLite 使用文件路径作为连接字符串
            let path = config.database.as_deref().ok_or_else(|| {
                CoreError::connection(ConnectionError::InvalidConfig {
                    conn_id: config.name.clone().unwrap_or_else(|| "sqlite".to_string()),
                    reason: "Database path is required for SQLite".to_string(),
                })
            })?;

            // 创建 SQLite 数据库连接
            let db = SqliteDatabase::new(path).map_err(|e| {
                CoreError::connection(ConnectionError::InvalidConfig {
                    conn_id: config.name.clone().unwrap_or_else(|| "sqlite".to_string()),
                    reason: e.to_string(),
                })
            })?;

            let db: DynDatabase = Arc::new(db);
            Ok(db)
        })
    }
}

/// DuckDB 驱动工厂
///
/// 实现 DriverFactory trait，用于创建 DuckDB 数据库连接
pub struct DuckDbDriverFactory;

impl DriverFactory for DuckDbDriverFactory {
    fn descriptor(&self) -> DriverDescriptor {
        crate::core::driver::registry::duckdb_driver()
    }

    fn create(
        &self,
        config: DriverConnectionConfig,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<DynDatabase, CoreError>> + Send>>
    {
        Box::pin(async move {
            // DuckDB 使用文件路径作为连接字符串
            // 如果未指定路径，使用内存数据库
            let path = config.database.as_deref().unwrap_or(":memory:");

            // 创建 DuckDB 数据库连接
            let db = DuckDbDatabase::new(path).map_err(|e| {
                CoreError::connection(ConnectionError::InvalidConfig {
                    conn_id: config.name.clone().unwrap_or_else(|| "duckdb".to_string()),
                    reason: e.to_string(),
                })
            })?;

            let db: DynDatabase = Arc::new(db);
            Ok(db)
        })
    }
}

/// MySQL 官方原生驱动工厂（mysql_async）
///
/// 实现 DriverFactory trait，用于创建 MySQL 数据库连接
/// 使用 MySQL 官方维护的纯 Rust 异步驱动
pub struct MySqlNativeDriverFactory;

impl DriverFactory for MySqlNativeDriverFactory {
    fn descriptor(&self) -> DriverDescriptor {
        crate::core::driver::registry::mysql_native_driver()
    }

    fn create(
        &self,
        config: DriverConnectionConfig,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<DynDatabase, CoreError>> + Send>>
    {
        Box::pin(async move {
            let mut url = config.to_url().map_err(|e| {
                CoreError::connection(ConnectionError::InvalidConfig {
                    conn_id: config
                        .name
                        .clone()
                        .unwrap_or_else(|| "mysql_native".to_string()),
                    reason: e.to_string(),
                })
            })?;

            if !url.contains("prefer_socket") {
                let prefer_socket = config
                    .driver_properties
                    .get("prefer_socket")
                    .map(|v| v == "true")
                    .unwrap_or(false);
                let sep = if url.contains('?') { '&' } else { '?' };
                url.push(sep);
                url.push_str(&format!("prefer_socket={}", prefer_socket));
            }

            let db = MySqlNativeDatabase::new(&url).await?;
            let db: DynDatabase = Arc::new(db);
            Ok(db)
        })
    }
}

/// PostgreSQL 官方原生驱动工厂（tokio-postgres）
///
/// 实现 DriverFactory trait，用于创建 PostgreSQL 数据库连接
/// 使用 PostgreSQL 官方维护的异步驱动
pub struct PostgresNativeDriverFactory;

impl DriverFactory for PostgresNativeDriverFactory {
    fn descriptor(&self) -> DriverDescriptor {
        crate::core::driver::registry::postgres_native_driver()
    }

    fn create(
        &self,
        config: DriverConnectionConfig,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<DynDatabase, CoreError>> + Send>>
    {
        Box::pin(async move {
            let url = config.to_url().map_err(|e| {
                CoreError::connection(ConnectionError::InvalidConfig {
                    conn_id: config
                        .name
                        .clone()
                        .unwrap_or_else(|| "postgres_native".to_string()),
                    reason: e.to_string(),
                })
            })?;

            let db = PostgresNativeDatabase::new(&url).await?;
            let db: DynDatabase = Arc::new(db);
            Ok(db)
        })
    }
}
