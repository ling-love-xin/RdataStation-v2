use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use shared::error::{CommonError, CoreError};
use engine::get_connection_manager;
use shared::models::QueryResult;

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

    /// duckdb 能力门控（`can_use_duckdb` 此前只有字段、没有检查入口；P3-c 授权 UI 落地前
    /// 保持显式调用方可用的形态）
    pub async fn check_duckdb_permission(&self, plugin_id: &str) -> Result<(), CoreError> {
        let p = self.permissions.read().await;
        match p.get(plugin_id) {
            Some(perms) if perms.can_use_duckdb => Ok(()),
            _ => Err(CoreError::common(CommonError::general(format!(
                "Plugin '{}' does not have duckdb permission",
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

    /// 读取连接元数据。
    ///
    /// `table` 仅在 `kind == "columns"` 时必填（列出某张表的列）；此前表名被硬编码为
    /// `"placeholder"`，任何调用都会拿到一张不存在的表的空结果 —— D6 修复为显式入参。
    ///
    /// 注：本结构里的权限表与 `permission.rs` 的 `PermissionManager` 是两套体系（历史并存）；
    /// 统一到 `PermissionManager` 归 P3-c 授权 UI（README D7 口径），这里保留桥接侧的自检。
    pub async fn metadata(
        &self,
        plugin_id: &str,
        conn_id: &str,
        catalog: &str,
        schema: &str,
        kind: &str,
        table: Option<&str>,
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
                let table_name = table.ok_or_else(|| {
                    CoreError::common(CommonError::general(
                        "metadata(kind=\"columns\") 必须提供 table 参数".to_string(),
                    ))
                })?;
                let result = conn
                    .list_columns(catalog, Some(schema), table_name)
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
