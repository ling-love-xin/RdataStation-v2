use std::sync::Arc;

use connection::config::ConnectionMethod;
use connection::{ConnectionConfig, ConnectionFactory};
use super::registry::DriverConnectionConfig;
use crate::driver::native::{
    duckdb::DuckDbDatabase, mysql::MySqlDatabase, mysql_native::MySqlNativeDatabase,
    postgres::PostgresDatabase, postgres_native::PostgresNativeDatabase, sqlite::SqliteDatabase,
};
use crate::driver::{DriverDescriptor, DriverFactory, DynDatabase};
use shared::error::{ConnectionError, CoreError};

/// MySQL 驱动工厂
///
/// 实现 DriverFactory trait，用于创建 MySQL 数据库连接
/// 支持多种连接方式：直接连接、SSL、SSH 隧道、代理
pub struct MySqlDriverFactory;

impl DriverFactory for MySqlDriverFactory {
    fn descriptor(&self) -> DriverDescriptor {
        crate::driver::registry::mysql_driver()
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
        crate::driver::registry::postgres_driver()
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
        crate::driver::registry::sqlite_driver()
    }

    fn create(
        &self,
        config: DriverConnectionConfig,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<DynDatabase, CoreError>> + Send>>
    {
        Box::pin(async move {
            // 地址来源与网络型驱动保持一致：优先 `url_override`（服务层 / 对话框传入的完整地址，
            // 如 `C:/data/app.db` 或 `sqlite://C:/data/app.db`），否则回退 `database` / `file_path`。
            // 旧版只读 `config.database`，于是“传了 url_override 也会报 Database path is required”。
            let path = sqlite_path_from_config(&config);
            if path.trim().is_empty() {
                return Err(CoreError::connection(ConnectionError::InvalidConfig {
                    conn_id: config.name.clone().unwrap_or_else(|| "sqlite".to_string()),
                    reason: "Database path is required for SQLite".to_string(),
                }));
            }

            // 创建 SQLite 数据库连接
            let db = SqliteDatabase::new(&path).map_err(|e| {
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

/// 从驱动配置取 SQLite 文件路径（去 `sqlite://` 前缀与查询串）。
///
/// 查询串来自 `to_url()` 追加的 driver_properties / options（如 `?journalMode=WAL`）；
/// 本驱动当前不解析这些参数，去掉而不是拼进文件名（否则会造出怪文件名）。
fn sqlite_path_from_config(config: &DriverConnectionConfig) -> String {
    let raw = config
        .to_url()
        .ok()
        .or_else(|| config.database.clone())
        .or_else(|| config.file_path.clone())
        .unwrap_or_default();
    let path = raw.trim();
    let path = path.strip_prefix("sqlite://").unwrap_or(path);
    path.split('?').next().unwrap_or(path).to_string()
}

/// DuckDB 驱动工厂
///
/// 实现 DriverFactory trait，用于创建 DuckDB 数据库连接
pub struct DuckDbDriverFactory;

impl DriverFactory for DuckDbDriverFactory {
    fn descriptor(&self) -> DriverDescriptor {
        crate::driver::registry::duckdb_driver()
    }

    fn create(
        &self,
        config: DriverConnectionConfig,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<DynDatabase, CoreError>> + Send>>
    {
        Box::pin(async move {
            // 同 SQLite：优先 `url_override`（完整地址），未给路径时才用 `:memory:`。
            let raw = config
                .to_url()
                .ok()
                .or_else(|| config.database.clone())
                .or_else(|| config.file_path.clone())
                .unwrap_or_default();
            let trimmed = raw.trim();
            let without_scheme = trimmed.strip_prefix("duckdb://").unwrap_or(trimmed);
            let path = without_scheme.split('?').next().unwrap_or(without_scheme);
            let path = if path.trim().is_empty() { ":memory:" } else { path };

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

/// 构造 mysql_async 认的连接串（纯函数，可测）。
///
/// 两件事：
/// 1. **scheme 归一**——连接记录里的 `db_type` 是驱动 id（`mysql_native`），
///    而 `mysql_async` 只认 `mysql://`（其余 scheme 报 `UnsupportedScheme`）；
/// 2. 补 `prefer_socket`（mysql_async 认的参数名，未给时显式关掉，避免走 localhost 的 unix socket）。
pub(crate) fn mysql_native_url(config: &DriverConnectionConfig) -> Result<String, CoreError> {
    let raw = config
        .to_url()
        .map_err(|e| invalid_config(config, "mysql_native", e.to_string()))?;
    let mut url = connection::url_params::normalize_url_scheme(&raw, "mysql");

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
    Ok(url)
}

impl DriverFactory for MySqlNativeDriverFactory {
    fn descriptor(&self) -> DriverDescriptor {
        crate::driver::registry::mysql_native_driver()
    }

    fn create(
        &self,
        config: DriverConnectionConfig,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<DynDatabase, CoreError>> + Send>>
    {
        Box::pin(async move {
            let url = mysql_native_url(&config)?;
            // 结构化 TLS 请求（证书路径 / 校验意图）：URL 表达不了的部分走这里
            let db = MySqlNativeDatabase::new_with_tls(&url, config.tls.as_ref()).await?;
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

/// 构造 tokio-postgres 认的连接串（纯函数，可测）。
///
/// tokio-postgres 只剥离 `postgres://` / `postgresql://` 两个前缀；
/// 驱动 id `postgres_native` 直接当 scheme 时两个前缀都不命中，整串会被当成
/// `key=value` 连接串去解析而报错（真机形态见 `data-layer-wiring-matrix.md` §7 第 10 条）。
pub(crate) fn postgres_native_url(config: &DriverConnectionConfig) -> Result<String, CoreError> {
    let raw = config
        .to_url()
        .map_err(|e| invalid_config(config, "postgres_native", e.to_string()))?;
    Ok(connection::url_params::normalize_url_scheme(&raw, "postgres"))
}

/// 统一的「驱动配置不合法」错误（`conn_id` 取显示名，无名字时回退驱动 id）。
fn invalid_config(
    config: &DriverConnectionConfig,
    fallback_id: &str,
    reason: String,
) -> CoreError {
    CoreError::connection(ConnectionError::InvalidConfig {
        conn_id: config
            .name
            .clone()
            .unwrap_or_else(|| fallback_id.to_string()),
        reason,
    })
}

impl DriverFactory for PostgresNativeDriverFactory {
    fn descriptor(&self) -> DriverDescriptor {
        crate::driver::registry::postgres_native_driver()
    }

    fn create(
        &self,
        config: DriverConnectionConfig,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<DynDatabase, CoreError>> + Send>>
    {
        Box::pin(async move {
            let url = postgres_native_url(&config)?;
            // 结构化 TLS 请求：tokio-postgres 的连接串里没有证书参数与 verify 档，
            // 校验与证书都在驱动侧连接器上落实（见 `pg_tls_connector`）
            let db = PostgresNativeDatabase::new_with_tls(&url, config.tls.as_ref()).await?;
            let db: DynDatabase = Arc::new(db);
            Ok(db)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真机踩到的形态：连接记录里存的是驱动 id，`build_connection_url` 把它当 scheme，
    /// 而 mysql_async / tokio-postgres 都不认——两个工厂必须在建连前归一。
    #[test]
    fn native_urls_carry_the_client_scheme_not_the_driver_id() {
        let mysql = DriverConnectionConfig::new("mysql_native")
            .with_url_override("mysql_native://root:pw@h:3306/db");
        assert_eq!(
            mysql_native_url(&mysql).unwrap(),
            "mysql://root:pw@h:3306/db?prefer_socket=false"
        );

        // 已是 mysql:// 时幂等；driver_properties 声明 prefer_socket 时按其值写
        let mut mysql = DriverConnectionConfig::new("mysql_native")
            .with_url_override("mysql://root:pw@h:3306/db");
        mysql
            .driver_properties
            .insert("prefer_socket".to_string(), "true".to_string());
        assert_eq!(
            mysql_native_url(&mysql).unwrap(),
            "mysql://root:pw@h:3306/db?prefer_socket=true"
        );

        let pg = DriverConnectionConfig::new("postgres_native")
            .with_url_override("postgres_native://u:p@h:5432/db?sslmode=require");
        assert_eq!(
            postgres_native_url(&pg).unwrap(),
            "postgres://u:p@h:5432/db?sslmode=require"
        );
    }
}
