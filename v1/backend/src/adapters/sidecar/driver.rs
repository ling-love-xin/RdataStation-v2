//! Sidecar 驱动工厂和实现
//!
//! 实现 Database trait，将数据库操作转发到 Go Sidecar

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::Mutex;

use super::client::{ConnectionParams, QueryParams, SidecarClient};
use super::manager::SidecarManager;
use super::{SidecarError, SidecarStatus};
use crate::core::driver::{
    ColumnDetail, DataSourceMeta, Database, DbPool, PoolStatus, QueryResult, SchemaObject,
    SchemaObjectKind, Transaction,
};
use crate::core::driver::{DriverFactory, DriverKind};
use crate::core::error::CoreError;

/// Sidecar 数据库连接
struct SidecarDatabase {
    client: Mutex<SidecarClient>,
    connection_params: ConnectionParams,
    metadata: DataSourceMeta,
}

impl SidecarDatabase {
    /// 创建新的 Sidecar 数据库连接
    pub fn new(port: u16, connection_params: ConnectionParams) -> Self {
        Self {
            client: Mutex::new(SidecarClient::new(port)),
            connection_params,
            metadata: DataSourceMeta {
                server_version: None,
                supports_transaction: true,
                supports_streaming: false,
                supports_arrow: false,
                supports_federated: false,
                supports_concurrent_write: true,
                is_in_memory: false,
            },
        }
    }
}

#[async_trait]
impl Database for SidecarDatabase {
    async fn query(&self, sql: &str) -> Result<QueryResult, CoreError> {
        let query_params = QueryParams {
            connection: self.connection_params.clone(),
            sql: sql.to_string(),
            params: None,
        };

        let mut client = self.client.lock().await;
        let result = client
            .execute_query(query_params)
            .await
            .map_err(|e| CoreError::database(crate::core::error::DatabaseError::Driver {
                db_type: "sidecar".to_string(),
                operation: "query".to_string(),
                source: e.to_string(),
            }))?;

        Ok(result)
    }

    async fn query_with_params(
        &self,
        sql: &str,
        params: Vec<crate::core::models::Value>,
    ) -> Result<QueryResult, CoreError> {
        let json_params: Vec<Value> = params
            .into_iter()
            .map(|v| match v {
                crate::core::models::Value::Null => Value::Null,
                crate::core::models::Value::Bool(b) => Value::Bool(b),
                crate::core::models::Value::Int(i) => Value::Number(i.into()),
                crate::core::models::Value::Int64(i) => Value::Number(i.into()),
                crate::core::models::Value::Float(f) => Value::Number(
                    serde_json::Number::from_f64(f).unwrap_or_else(|| 0.into()),
                ),
                crate::core::models::Value::String(s) => Value::String(s),
                crate::core::models::Value::DateTime(dt) => Value::String(dt.to_string()),
                crate::core::models::Value::Blob(b) => Value::Array(
                    b.into_iter()
                        .map(|byte| Value::Number(byte.into()))
                        .collect(),
                ),
            })
            .collect();

        let query_params = QueryParams {
            connection: self.connection_params.clone(),
            sql: sql.to_string(),
            params: Some(json_params),
        };

        let mut client = self.client.lock().await;
        let result = client
            .execute_query(query_params)
            .await
            .map_err(|e| CoreError::database(crate::core::error::DatabaseError::Driver {
                db_type: "sidecar".to_string(),
                operation: "query_with_params".to_string(),
                source: e.to_string(),
            }))?;

        Ok(result)
    }

    async fn query_with_cancel(
        &self,
        sql: &str,
        _cancel_token: tokio_util::sync::CancellationToken,
    ) -> Result<QueryResult, CoreError> {
        // TODO: 实现真正的取消
        self.query(sql).await
    }

    async fn begin_transaction(&self) -> Result<Box<dyn Transaction>, CoreError> {
        Err(CoreError::database(crate::core::error::DatabaseError::Driver {
            db_type: "sidecar".to_string(),
            operation: "begin_transaction".to_string(),
            source: "Transaction not supported yet".to_string(),
        }))
    }

    fn meta(&self) -> DataSourceMeta {
        self.metadata.clone()
    }

    async fn ping(&self) -> Result<(), CoreError> {
        let mut client = self.client.lock().await;
        let result = client.test_connection(self.connection_params.clone()).await;

        match result {
            Ok(mut resp) => {
                if let Some(success) = resp.get("success") {
                    if success.as_bool().unwrap_or(false) {
                        Ok(())
                    } else {
                        Err(CoreError::database(crate::core::error::DatabaseError::Driver {
                            db_type: "sidecar".to_string(),
                            operation: "ping".to_string(),
                            source: "Connection test failed".to_string(),
                        }))
                    }
                } else {
                    Err(CoreError::database(crate::core::error::DatabaseError::Driver {
                        db_type: "sidecar".to_string(),
                        operation: "ping".to_string(),
                        source: "Invalid response".to_string(),
                    }))
                }
            }
            Err(e) => Err(CoreError::database(crate::core::error::DatabaseError::Driver {
                db_type: "sidecar".to_string(),
                operation: "ping".to_string(),
                source: e.to_string(),
            })),
        }
    }

    async fn list_catalogs(&self) -> Result<Vec<String>, CoreError> {
        Ok(vec![])
    }

    async fn list_schemas(&self, _catalog: &str) -> Result<Vec<String>, CoreError> {
        Ok(vec![])
    }

    async fn list_tables(
        &self,
        _catalog: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<SchemaObject>, CoreError> {
        let mut client = self.client.lock().await;
        let result = client.list_tables(self.connection_params.clone()).await;

        match result {
            Ok(resp) => {
                // TODO: 正确解析响应
                Ok(vec![])
            }
            Err(e) => Err(CoreError::database(crate::core::error::DatabaseError::Driver {
                db_type: "sidecar".to_string(),
                operation: "list_tables".to_string(),
                source: e.to_string(),
            })),
        }
    }

    async fn list_columns(
        &self,
        _catalog: &str,
        _schema: Option<&str>,
        _table: &str,
    ) -> Result<Vec<ColumnDetail>, CoreError> {
        Ok(vec![])
    }
}

/// Sidecar 连接池
struct SidecarPool {
    manager: Arc<SidecarManager>,
    driver_id: String,
}

impl SidecarPool {
    /// 创建新的连接池
    pub fn new(manager: Arc<SidecarManager>, driver_id: String) -> Self {
        Self { manager, driver_id }
    }
}

#[async_trait]
impl DbPool for SidecarPool {
    async fn acquire(&self) -> Result<Box<dyn Database + Send + Sync>, CoreError> {
        let port = self.manager.port().ok_or_else(|| {
            CoreError::database(crate::core::error::DatabaseError::Driver {
                db_type: "sidecar".to_string(),
                operation: "acquire".to_string(),
                source: "Sidecar not running".to_string(),
            })
        })?;

        let connection_params = ConnectionParams {
            driver_id: self.driver_id.clone(),
            dsn: None,
            username: None,
            password: None,
            database: None,
            host: None,
            port: None,
        };

        Ok(Box::new(SidecarDatabase::new(port, connection_params)))
    }

    async fn close(&self) -> Result<(), CoreError> {
        Ok(())
    }

    fn is_closed(&self) -> bool {
        self.manager.status() != SidecarStatus::Running
    }

    fn status(&self) -> PoolStatus {
        PoolStatus::unknown()
    }
}

/// Sidecar 驱动工厂
pub struct SidecarDriverFactory {
    manager: Arc<SidecarManager>,
    driver_id: String,
    driver_name: String,
    db_type: String,
}

impl SidecarDriverFactory {
    /// 创建新的驱动工厂
    pub fn new(
        manager: Arc<SidecarManager>,
        driver_id: String,
        driver_name: String,
        db_type: String,
    ) -> Self {
        Self {
            manager,
            driver_id,
            driver_name,
            db_type,
        }
    }
}

#[async_trait::async_trait]
impl DriverFactory for SidecarDriverFactory {
    fn id(&self) -> &str {
        &self.driver_id
    }

    fn name(&self) -> &str {
        &self.driver_name
    }

    fn kind(&self) -> DriverKind {
        DriverKind::Native
    }

    fn default_port(&self) -> Option<u16> {
        // TODO: 根据 db_type 返回默认端口
        None
    }

    async fn create_pool(
        &self,
        _config: crate::core::driver::DriverConnectionConfig,
    ) -> Result<Box<dyn DbPool + Send + Sync>, CoreError> {
        Ok(Box::new(SidecarPool::new(
            Arc::clone(&self.manager),
            self.driver_id.clone(),
        )))
    }
}
