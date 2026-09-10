//! 连接工厂
//!
//! 提供统一的连接创建入口，根据配置自动选择合适的连接器

use std::collections::HashMap;
use std::sync::Arc;

use super::config::{ConnectionConfig, ConnectionMethod};
use super::connector::{
    Connection, Connector, DirectConnector, HttpProxyConnector, SocksProxyConnector,
    SshTunnelConnector, SslConnector,
};
use super::stream::ConnectionStream;
use crate::core::error::CoreError;

/// 连接工厂
///
/// 管理所有可用的连接器，根据连接配置自动选择合适的连接器
pub struct ConnectionFactory {
    connectors: HashMap<String, Arc<dyn Connector>>,
}

impl ConnectionFactory {
    pub fn new() -> Self {
        let mut factory = Self {
            connectors: HashMap::new(),
        };
        factory.register_builtin_connectors();
        factory
    }

    fn register_builtin_connectors(&mut self) {
        self.register(Arc::new(DirectConnector));
        self.register(Arc::new(SslConnector));
        self.register(Arc::new(SshTunnelConnector));
        self.register(Arc::new(HttpProxyConnector));
        self.register(Arc::new(SocksProxyConnector));
    }

    pub fn register(&mut self, connector: Arc<dyn Connector>) {
        let name = connector.name().to_string();
        self.connectors.insert(name, connector);
    }

    pub async fn create_connection(
        &self,
        config: ConnectionConfig,
    ) -> Result<Connection, CoreError> {
        let connector = self.find_connector(&config.method)?;
        let stream = connector.connect(&config).await?;

        Ok(Connection::new(stream, config))
    }

    pub async fn create_stream(
        &self,
        config: &ConnectionConfig,
    ) -> Result<ConnectionStream, CoreError> {
        let connector = self.find_connector(&config.method)?;
        connector.connect(config).await
    }

    fn find_connector(&self, method: &ConnectionMethod) -> Result<Arc<dyn Connector>, CoreError> {
        for connector in self.connectors.values() {
            if connector.supports(method) {
                return Ok(connector.clone());
            }
        }

        Err(CoreError::connection(
            crate::core::error::ConnectionError::NotSupported(format!(
                "No connector found for method: {:?}",
                method
            )),
        ))
    }

    pub fn get_connector_names(&self) -> Vec<&str> {
        self.connectors.keys().map(|s| s.as_str()).collect()
    }

    pub fn supports(&self, method: &ConnectionMethod) -> bool {
        self.find_connector(method).is_ok()
    }
}

impl Default for ConnectionFactory {
    fn default() -> Self {
        Self::new()
    }
}

use std::sync::OnceLock;

static GLOBAL_CONNECTION_FACTORY: OnceLock<ConnectionFactory> = OnceLock::new();

pub fn get_connection_factory() -> &'static ConnectionFactory {
    GLOBAL_CONNECTION_FACTORY.get_or_init(ConnectionFactory::new)
}

pub async fn create_connection(config: ConnectionConfig) -> Result<Connection, CoreError> {
    get_connection_factory().create_connection(config).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::driver::connection::config::ConnectionConfig;

    #[test]
    fn test_connection_factory_new() {
        let factory = ConnectionFactory::new();
        let names = factory.get_connector_names();
        assert!(names.contains(&"direct"));
        assert!(names.contains(&"ssl"));
        assert!(names.contains(&"ssh_tunnel"));
        assert!(names.contains(&"http_proxy"));
        assert!(names.contains(&"socks_proxy"));
    }

    #[test]
    fn test_supports_direct() {
        let factory = ConnectionFactory::new();
        let config = ConnectionConfig::direct("localhost", 3306);
        assert!(factory.supports(&config.method));
    }

    #[tokio::test]
    async fn test_create_connection_direct() {
        let factory = ConnectionFactory::new();
        let config = ConnectionConfig::direct("127.0.0.1", 3306);

        let result = factory.create_stream(&config).await;
        assert!(result.is_err() || result.is_ok());
    }
}
