use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::core::error::{CommonError, CoreError};
use crate::core::get_connection_manager;
use crate::core::models::QueryResult;

pub struct PluginBridge {
    permissions: RwLock<HashMap<String, PluginPermissions>>,
}

#[derive(Debug, Clone)]
pub struct PluginPermissions {
    pub plugin_id: String,
    pub allowed_connections: Vec<String>,
    pub can_query: bool,
    pub can_read_metadata: bool,
    pub can_use_duckdb: bool,
}

impl Default for PluginBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginBridge {
    pub fn new() -> Self {
        Self {
            permissions: RwLock::new(HashMap::new()),
        }
    }

    pub async fn register_permissions(&self, perms: PluginPermissions) {
        let mut p = self.permissions.write().await;
        p.insert(perms.plugin_id.clone(), perms);
    }

    pub async fn check_query_permission(&self, plugin_id: &str) -> Result<(), CoreError> {
        let p = self.permissions.read().await;
        match p.get(plugin_id) {
            Some(perms) if perms.can_query => Ok(()),
            _ => Err(CoreError::common(CommonError::general(format!(
                "Plugin '{}' does not have db_query permission",
                plugin_id
            )))),
        }
    }

    pub async fn check_metadata_permission(&self, plugin_id: &str) -> Result<(), CoreError> {
        let p = self.permissions.read().await;
        match p.get(plugin_id) {
            Some(perms) if perms.can_read_metadata => Ok(()),
            _ => Err(CoreError::common(CommonError::general(format!(
                "Plugin '{}' does not have db_metadata permission",
                plugin_id
            )))),
        }
    }

    pub async fn query(
        &self,
        plugin_id: &str,
        conn_id: &str,
        sql: &str,
    ) -> Result<QueryResult, CoreError> {
        self.check_query_permission(plugin_id).await?;

        let manager = get_connection_manager();
        let conn_id_owned = conn_id.to_string();
        let conn = manager
            .get_connection(&conn_id_owned)
            .await
            .ok_or_else(|| {
                CoreError::common(CommonError::general(format!(
                    "Connection '{}' not found",
                    conn_id
                )))
            })?;

        conn.query(sql).await
    }

    pub async fn metadata(
        &self,
        plugin_id: &str,
        conn_id: &str,
        catalog: &str,
        schema: &str,
        kind: &str,
    ) -> Result<serde_json::Value, CoreError> {
        self.check_metadata_permission(plugin_id).await?;

        let manager = get_connection_manager();
        let conn_id_owned = conn_id.to_string();
        let conn = manager
            .get_connection(&conn_id_owned)
            .await
            .ok_or_else(|| {
                CoreError::common(CommonError::general(format!(
                    "Connection '{}' not found",
                    conn_id
                )))
            })?;

        match kind {
            "tables" => {
                let result = conn.list_tables(catalog, Some(schema)).await?;
                serde_json::to_value(result).map_err(|e| {
                    CoreError::common(CommonError::general(format!(
                        "Failed to serialize tables: {}",
                        e
                    )))
                })
            }
            "columns" => {
                let result = conn
                    .list_columns(catalog, Some(schema), "placeholder")
                    .await?;
                serde_json::to_value(result).map_err(|e| {
                    CoreError::common(CommonError::general(format!(
                        "Failed to serialize columns: {}",
                        e
                    )))
                })
            }
            _ => Err(CoreError::common(CommonError::general(
                "Unknown metadata kind",
            ))),
        }
    }
}

static PLUGIN_BRIDGE: std::sync::OnceLock<Arc<PluginBridge>> = std::sync::OnceLock::new();

pub fn get_plugin_bridge() -> Arc<PluginBridge> {
    PLUGIN_BRIDGE
        .get_or_init(|| Arc::new(PluginBridge::new()))
        .clone()
}
