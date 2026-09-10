//! 连接相关命令
//!
//! 处理数据库连接的创建、管理、关闭等操作

use crate::core::driver::connection::config::ConnectionMethod;
use crate::core::driver::DriverConnectionConfig;
use crate::core::error::{CommonError, CoreError};
use crate::core::services::connection_service::{
    parse_network_config_json, resolve_network_method, resolve_network_method_with_project,
    ConnectRequest,
};
use crate::core::services::{ConnectionService, ConnectionType};
use crate::core::{get_connection_manager, DataSourceMeta};

// ==================== Connection Commands ====================

/// 创建数据库连接请求参数
#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct ConnectDatabaseInput {
    pub conn_id: Option<String>,
    pub db_type: String,
    pub url: String,
    pub name: Option<String>,
    pub connection_type: Option<String>, // "global" 或 "project"
    pub project_id: Option<String>,      // 仅项目连接时需要
    pub description: Option<String>,
    pub driver_id: Option<String>,
    pub environment_id: Option<String>,
    pub auth_config_id: Option<String>,
    pub auth_method: Option<String>,
    pub network_config_id: Option<String>,
    pub driver_properties: Option<String>,
    pub advanced_options: Option<String>,
    pub options: Option<String>,
    pub tags: Option<String>,
    pub metadata_path: Option<String>,
    pub schema_name: Option<String>,
    pub use_duckdb_fed: Option<bool>,
    pub password: Option<String>,
}

impl ConnectDatabaseInput {
    /// 转换为 `ConnectRequest`，统一 `connect_database` 和 `test_connection` 的构造逻辑
    fn into_connect_request(
        self,
        connection_type: ConnectionType,
        network_method: Option<ConnectionMethod>,
        skip_persistence: Option<bool>,
    ) -> ConnectRequest {
        ConnectRequest {
            conn_id: self.conn_id,
            db_type: self.db_type,
            url: self.url,
            name: self.name,
            connection_type,
            project_path: self.project_id,
            description: self.description,
            driver_id: self.driver_id,
            environment_id: self.environment_id,
            auth_config_id: self.auth_config_id,
            auth_method: self.auth_method,
            network_config_id: self.network_config_id,
            driver_properties: self.driver_properties,
            advanced_options: self.advanced_options,
            options: self.options,
            tags: self.tags,
            metadata_path: self.metadata_path,
            schema_name: self.schema_name,
            use_duckdb_fed: self.use_duckdb_fed,
            password: self.password,
            skip_persistence,
            network_method,
        }
    }
}

/// 连接响应
#[derive(serde::Serialize, Debug, specta::Type)]
pub struct ConnectDatabaseResponse {
    pub conn_id: String,
    pub name: String,
    pub db_type: String,
    pub url: String,
    pub status: String,
    pub meta: DataSourceMetaResponse,
}

/// 数据源元数据响应
#[derive(serde::Serialize, Debug, specta::Type)]
pub struct DataSourceMetaResponse {
    pub supports_transaction: bool,
    pub supports_streaming: bool,
    pub supports_arrow: bool,
    pub supports_federated: bool,
    pub supports_concurrent_write: bool,
    pub is_in_memory: bool,
    pub server_version: Option<String>,
}

impl From<DataSourceMeta> for DataSourceMetaResponse {
    fn from(meta: DataSourceMeta) -> Self {
        Self {
            supports_transaction: meta.supports_transaction,
            supports_streaming: meta.supports_streaming,
            supports_arrow: meta.supports_arrow,
            supports_federated: meta.supports_federated,
            supports_concurrent_write: meta.supports_concurrent_write,
            is_in_memory: meta.is_in_memory,
            server_version: meta.server_version,
        }
    }
}

/// 创建数据库连接
#[tauri::command]
#[specta::specta]
pub async fn connect_database(
    input: ConnectDatabaseInput,
) -> Result<ConnectDatabaseResponse, CoreError> {
    if input.url.is_empty() {
        return Err("Database URL cannot be empty".into());
    }
    if input.db_type.trim().is_empty() {
        return Err(CoreError::common(CommonError::InvalidArgument {
            param: "db_type".to_string(),
            reason: "数据库类型不能为空".to_string(),
        }));
    }
    if let Some(ref name) = input.name {
        if name.trim().is_empty() {
            return Err(CoreError::common(CommonError::InvalidArgument {
                param: "name".to_string(),
                reason: "连接名称不能为空".to_string(),
            }));
        }
    }

    let manager = get_connection_manager().clone();
    let service = ConnectionService::new(manager);

    // 解析连接类型
    let connection_type = match input.connection_type.as_deref() {
        Some("global") | None => ConnectionType::Global,
        Some("project") => ConnectionType::Project,
        Some(other) => return Err(format!("Invalid connection type: {}", other).into()),
    };

    // 项目连接必须有 project_id
    if connection_type == ConnectionType::Project && input.project_id.is_none() {
        return Err("project_id is required for project connections".into());
    }

    // ===== 数据源模块：驱动校验 =====
    if let Some(ref driver_id) = input.driver_id {
        let global_db = crate::core::migration::get_global_db_manager()
            .ok_or_else(|| CoreError::from("Global database not initialized".to_string()))?;

        let driver = global_db
            .get_driver(driver_id)
            .await?
            .ok_or_else(|| CoreError::from(format!("驱动 {} 不存在于全局目录中", driver_id)))?;

        if connection_type == ConnectionType::Project {
            if let Some(ref proj_path) = input.project_id {
                let meta_dir = std::path::Path::new(proj_path).join(".RSmeta");
                let db_path = meta_dir.join("project.db");
                if db_path.exists() {
                    let conn = rusqlite::Connection::open(&db_path)
                        .map_err(|e| CoreError::from(format!("打开项目数据库失败: {}", e)))?;
                    let enabled: bool = match conn.query_row(
                        "SELECT enabled FROM project_drivers WHERE driver_id = ?1",
                        rusqlite::params![driver_id],
                        |row| row.get(0),
                    ) {
                        Ok(v) => v,
                        Err(rusqlite::Error::QueryReturnedNoRows) => false,
                        Err(e) => {
                            tracing::warn!(
                                driver_id = %driver_id,
                                error = %e,
                                "Failed to query project_drivers, assuming disabled"
                            );
                            false
                        }
                    };
                    if !enabled {
                        return Err(CoreError::from(format!(
                            "驱动 {} 未在当前项目中启用，请先在驱动管理中启用",
                            driver_id
                        )));
                    }
                }
            }
        }

        if driver.driver_kind != "native" {
            let version = driver.version.as_deref().unwrap_or("0.0.0");
            let installed = global_db
                .is_driver_installed(driver_id, version)
                .await
                .map_err(|e| CoreError::from(format!("检查驱动安装状态失败: {}", e)))?;
            if !installed {
                return Err(CoreError::from(format!(
                    "驱动 {} 的文件未在本机安装，请先下载安装",
                    driver_id
                )));
            }
        }
    }

    // ===== 数据源模块：环境/认证/网络校验 =====
    // 辅助闭包：打开项目DB连接（复用于项目级校验）
    let open_project_db = |proj_path: &str| -> Option<rusqlite::Connection> {
        let db_path = std::path::Path::new(proj_path)
            .join(".RSmeta")
            .join("project.db");
        if db_path.exists() {
            rusqlite::Connection::open(&db_path).ok()
        } else {
            None
        }
    };

    let is_project = connection_type == ConnectionType::Project;
    let proj_db = input.project_id.as_deref().and_then(open_project_db);

    if let Some(ref env_id) = input.environment_id {
        let mut found = false;
        // 先查全局环境
        if let Some(gdb) = crate::core::migration::get_global_db_manager() {
            if let Ok(envs) = gdb.list_environments().await {
                found = envs.iter().any(|e| e.id == *env_id);
            }
        }
        // 项目连接时也查项目级环境
        if !found && is_project {
            if let Some(ref conn) = proj_db {
                found = conn
                    .query_row::<i64, _, _>(
                        "SELECT COUNT(*) FROM environments WHERE id = ?1",
                        rusqlite::params![env_id],
                        |row| row.get(0),
                    )
                    .map(|c| c > 0)
                    .unwrap_or(false);
            }
        }
        if !found {
            return Err(CoreError::from(format!("环境 {} 不存在", env_id)));
        }
    }

    if let Some(ref auth_id) = input.auth_config_id {
        let mut found = false;
        if let Some(gdb) = crate::core::migration::get_global_db_manager() {
            if let Ok(auths) = gdb.list_auth_configs(None).await {
                found = auths.iter().any(|a| a.id == *auth_id);
            }
        }
        if !found && is_project {
            if let Some(ref conn) = proj_db {
                found = conn
                    .query_row::<i64, _, _>(
                        "SELECT COUNT(*) FROM auth_configs WHERE id = ?1",
                        rusqlite::params![auth_id],
                        |row| row.get(0),
                    )
                    .map(|c| c > 0)
                    .unwrap_or(false);
            }
        }
        if !found {
            return Err(CoreError::from(format!("认证配置 {} 不存在", auth_id)));
        }
    }

    if let Some(ref net_id) = input.network_config_id {
        let mut found = false;
        if let Some(gdb) = crate::core::migration::get_global_db_manager() {
            if let Ok(nets) = gdb.list_network_configs(None).await {
                found = nets.iter().any(|n| n.id == *net_id);
            }
        }
        if !found && is_project {
            if let Some(ref conn) = proj_db {
                found = conn
                    .query_row::<i64, _, _>(
                        "SELECT COUNT(*) FROM network_configs WHERE id = ?1",
                        rusqlite::params![net_id],
                        |row| row.get(0),
                    )
                    .map(|c| c > 0)
                    .unwrap_or(false);
            }
        }
        if !found {
            return Err(CoreError::from(format!("网络配置 {} 不存在", net_id)));
        }
    }

    // ===== 数据源模块：解析网络配置为 ConnectionMethod =====
    let network_method = parse_network_method(&input).await?;

    // 提取响应所需字段（input 将被 move 进 into_connect_request）
    let safe_url = ConnectionService::mask_password_in_url(&input.url);
    let db_type = input.db_type.clone();
    let conn_name = input.name.clone();

    let (conn_id, db) = service
        .connect_with_type(input.into_connect_request(connection_type, network_method, None))
        .await?;

    let meta = db.meta();

    // ===== 项目连接持久化：由前端 handleApply → create_project_connection 统一管理 =====
    // 此处不再重复写入，避免 conn_id 不一致和数据覆盖

    Ok(ConnectDatabaseResponse {
        conn_id,
        name: conn_name.unwrap_or_else(|| safe_url.clone()),
        db_type,
        url: safe_url,
        status: "connected".to_string(),
        meta: meta.into(),
    })
}

/// 解析 network_config_id → ConnectionMethod
///
/// 从 global/project 数据库中加载网络配置，
/// 根据 network_type 将 config JSON 反序列化为对应的连接方式
///
/// ## ID 前缀解析优先级
///
/// | 前缀  | 查找顺序                                          |
/// |-------|--------------------------------------------------|
/// | `G_`  | 1. global.db.network_configs                       |
/// | `P_`  | 1. project.db.network_configs (GP_ 优先)           |
/// | `GP_` | 1. project.db.network_configs（origin='global_snapshot'）|
/// | 无前缀 | 向后兼容：先查 global，再查 project（历史数据）    |
async fn parse_network_method(
    input: &ConnectDatabaseInput,
) -> Result<Option<ConnectionMethod>, CoreError> {
    let Some(ref net_id) = input.network_config_id else {
        return Ok(None);
    };

    let is_project = input.connection_type.as_deref() == Some("project");

    if net_id.starts_with("GP_") {
        if let Some(ref proj_path) = input.project_id {
            let db_path = std::path::Path::new(proj_path)
                .join(".RSmeta")
                .join("project.db");
            if db_path.exists() {
                if let Ok((network_type, config_str, auth_config_id)) =
                    project_query_network_config_with_auth(&db_path, net_id)
                {
                    return parse_network_config_json(
                        &network_type,
                        &config_str,
                        auth_config_id.as_deref(),
                        input.project_id.as_deref(),
                    )
                    .await;
                }
            }
        }
        return Ok(None);
    }

    if net_id.starts_with("P_") && is_project {
        if let Some(ref proj_path) = input.project_id {
            let db_path = std::path::Path::new(proj_path)
                .join(".RSmeta")
                .join("project.db");
            if db_path.exists() {
                if let Ok((network_type, config_str, auth_config_id)) =
                    project_query_network_config_with_auth(&db_path, net_id)
                {
                    return parse_network_config_json(
                        &network_type,
                        &config_str,
                        auth_config_id.as_deref(),
                        input.project_id.as_deref(),
                    )
                    .await;
                }
            }
        }
        return Ok(None);
    }

    if let Some(gdb) = crate::core::migration::get_global_db_manager() {
        if let Ok(nets) = gdb.list_network_configs(None).await {
            if let Some(net) = nets.iter().find(|n| n.id == *net_id) {
                return parse_network_config_json(
                    &net.network_type,
                    &net.config,
                    net.auth_config_id.as_deref(),
                    None,
                )
                .await;
            }
        }
    }

    if is_project {
        if let Some(ref proj_path) = input.project_id {
            let db_path = std::path::Path::new(proj_path)
                .join(".RSmeta")
                .join("project.db");
            if db_path.exists() {
                if let Ok((network_type, config_str, auth_config_id)) =
                    project_query_network_config_with_auth(&db_path, net_id)
                {
                    return parse_network_config_json(
                        &network_type,
                        &config_str,
                        auth_config_id.as_deref(),
                        input.project_id.as_deref(),
                    )
                    .await;
                }
            }
        }
    }

    Ok(None)
}

/// 查询项目网络配置，同时返回 network_type、config 和 auth_config_id
fn project_query_network_config_with_auth(
    db_path: &std::path::Path,
    net_id: &str,
) -> Result<(String, String, Option<String>), String> {
    let conn = rusqlite::Connection::open(db_path).map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT network_type, config, auth_config_id FROM network_configs WHERE id = ?1",
        rusqlite::params![net_id],
        |row| {
            let network_type: String = row.get(0)?;
            let config: String = row.get(1)?;
            let auth_config_id: Option<String> = row.get(2)?;
            Ok((network_type, config, auth_config_id))
        },
    )
    .map_err(|e| e.to_string())
}

/// 连接信息响应
#[derive(serde::Serialize, Debug, specta::Type)]
pub struct ConnectionInfoResponse {
    pub id: String,
    pub name: String,
    pub db_type: String,
    pub url: String,
    pub connection_type: String,
    pub project_id: Option<String>,
    pub status: String,
    pub is_active: bool,
    pub created_at_ms: f64,
    pub server_version: Option<String>,
    pub driver_id: Option<String>,
    pub description: Option<String>,
    pub environment_id: Option<String>,
    pub auth_config_id: Option<String>,
    pub auth_method: Option<String>,
    pub network_config_id: Option<String>,
    pub driver_properties: Option<String>,
    pub advanced_options: Option<String>,
}

impl ConnectionInfoResponse {
    fn from_info(
        info: crate::core::services::connection_manager::ConnectionInfo,
        is_active: bool,
    ) -> Self {
        let masked_url = ConnectionService::mask_password_in_url(&info.url);
        Self {
            id: info.id,
            name: info.name,
            db_type: info.db_type,
            url: masked_url,
            connection_type: info.connection_type.to_string(),
            project_id: info.project_id,
            status: "connected".to_string(),
            is_active,
            created_at_ms: info.created_at.elapsed().as_millis() as f64,
            server_version: info.server_version,
            driver_id: info.driver_id,
            environment_id: info.environment_id,
            description: info.description,
            auth_config_id: info.auth_config_id,
            auth_method: info.auth_method,
            network_config_id: info.network_config_id,
            driver_properties: info.driver_properties,
            advanced_options: info.advanced_options,
        }
    }
}

/// 获取所有连接
#[tauri::command]
#[specta::specta]
pub async fn get_connections() -> Result<Vec<ConnectionInfoResponse>, CoreError> {
    let manager = get_connection_manager().clone();
    let service = ConnectionService::new(manager);

    let connections = service.list_connections().await;
    let active_id = service.get_active_conn_id().await;

    Ok(connections
        .into_iter()
        .map(|info| {
            let is_active = active_id.as_ref() == Some(&info.id);
            ConnectionInfoResponse::from_info(info, is_active)
        })
        .collect())
}

/// 切换活动连接
#[tauri::command]
#[specta::specta]
pub async fn switch_connection(conn_id: String) -> Result<(), CoreError> {
    let manager = get_connection_manager().clone();
    let service = ConnectionService::new(manager);

    service.switch_connection(&conn_id).await
}

/// 关闭指定连接
#[tauri::command]
#[specta::specta]
pub async fn close_connection(conn_id: String) -> Result<(), CoreError> {
    let manager = get_connection_manager().clone();
    let service = ConnectionService::new(manager);

    service.close_connection(&conn_id).await
}

/// 关闭所有连接
#[tauri::command]
#[specta::specta]
pub async fn close_all_connections() -> Result<(), CoreError> {
    let manager = get_connection_manager().clone();
    let service = ConnectionService::new(manager);

    service.close_all_connections().await
}

/// 获取当前活动连接
#[tauri::command]
#[specta::specta]
pub async fn get_active_connection() -> Result<Option<ConnectionInfoResponse>, CoreError> {
    let manager = get_connection_manager().clone();
    let service = ConnectionService::new(manager);

    let connections = service.list_connections().await;
    let active_id = service.get_active_conn_id().await;

    Ok(connections
        .into_iter()
        .find(|info| active_id.as_ref() == Some(&info.id))
        .map(|info| ConnectionInfoResponse::from_info(info, true)))
}

/// 最近连接记录响应
#[derive(serde::Serialize, Debug, specta::Type)]
pub struct RecentConnectionResponse {
    pub name: String,
    pub db_type: String,
    pub url: String,
    pub last_used_at: String,
    pub description: Option<String>,
    pub driver_id: Option<String>,
    pub environment_id: Option<String>,
    pub auth_config_id: Option<String>,
    pub auth_method: Option<String>,
    pub network_config_id: Option<String>,
    pub driver_properties: Option<String>,
    pub advanced_options: Option<String>,
}

/// 获取最近连接列表
#[tauri::command]
#[specta::specta]
pub async fn get_recent_connections() -> Result<Vec<RecentConnectionResponse>, CoreError> {
    let manager = get_connection_manager().clone();
    let service = ConnectionService::new(manager);

    let connections = service.get_recent_connections()?;

    Ok(connections
        .into_iter()
        .map(|c| {
            let masked_url = ConnectionService::mask_password_in_url(&c.url);
            RecentConnectionResponse {
                name: c.name,
                db_type: c.db_type,
                url: masked_url,
                last_used_at: c.last_used_at.to_rfc3339(),
                description: c.description,
                driver_id: c.driver_id,
                environment_id: c.environment_id,
                auth_config_id: c.auth_config_id,
                auth_method: c.auth_method,
                network_config_id: c.network_config_id,
                driver_properties: c.driver_properties,
                advanced_options: c.advanced_options,
            }
        })
        .collect())
}

/// 删除最近连接记录
#[tauri::command]
#[specta::specta]
pub async fn remove_recent_connection(name: String) -> Result<(), CoreError> {
    let manager = get_connection_manager().clone();
    let service = ConnectionService::new(manager);

    service
        .remove_recent_connection(&name)
        .map_err(|e| CoreError::from(e.to_string()))
}

/// 连接类型转换请求参数
#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct ConvertConnectionInput {
    pub conn_id: String,
    pub target_type: String,        // "global" 或 "project"
    pub project_id: Option<String>, // 转为项目连接时需要
}

/// 连接类型转换响应
#[derive(serde::Serialize, Debug, specta::Type)]
pub struct ConvertConnectionResponse {
    pub conn_id: String,
    pub connection_type: String,
    pub project_id: Option<String>,
    pub message: String,
}

/// 转换连接类型（全局↔项目）
#[tauri::command]
#[specta::specta]
pub async fn convert_connection_type(
    input: ConvertConnectionInput,
) -> Result<ConvertConnectionResponse, CoreError> {
    let manager = get_connection_manager().clone();
    let service = ConnectionService::new(manager);

    let new_info = match input.target_type.as_str() {
        "project" => {
            let project_id = input.project_id.ok_or_else(|| {
                "project_id is required when converting to project connection".to_string()
            })?;
            service
                .convert_to_project_connection(&input.conn_id, &project_id)
                .await
                .map_err(|e| e.to_string())?
        }
        "global" => service
            .convert_to_global_connection(&input.conn_id)
            .await
            .map_err(|e| e.to_string())?,
        other => return Err(format!("Invalid target type: {}", other).into()),
    };

    let message = format!(
        "Connection {} converted to {} connection",
        input.conn_id, new_info.connection_type
    );

    Ok(ConvertConnectionResponse {
        conn_id: new_info.id,
        connection_type: new_info.connection_type.to_string(),
        project_id: new_info.project_id,
        message,
    })
}

/// 检测项目中的全局连接
#[tauri::command]
#[specta::specta]
pub async fn detect_global_connections_in_project(
    project_id: String,
) -> Result<Vec<ConnectionInfoResponse>, CoreError> {
    let manager = get_connection_manager().clone();
    let service = ConnectionService::new(manager);

    let connections = service
        .detect_global_connections_in_project(&project_id)
        .await?;

    let active_id = service.get_active_conn_id().await;

    Ok(connections
        .into_iter()
        .map(|info| {
            let is_active = active_id.as_ref() == Some(&info.id);
            ConnectionInfoResponse::from_info(info, is_active)
        })
        .collect())
}

/// 测试连接响应
#[derive(serde::Serialize, Debug, specta::Type)]
pub struct TestConnectionResponse {
    pub success: bool,
    pub message: String,
    pub server_version: String,
    pub response_time_ms: u32,
}

/// 测试数据库连接
#[tauri::command]
#[specta::specta]
pub async fn test_connection(
    db_type: String,
    url: String,
    network_config_id: Option<String>,
    auth_config_id: Option<String>,
    auth_method: Option<String>,
    project_path: Option<String>,
    password: Option<String>,
) -> Result<TestConnectionResponse, CoreError> {
    use std::time::{Duration, Instant};

    if url.is_empty() {
        return Err("Database URL cannot be empty".into());
    }

    let start = Instant::now();
    let manager = get_connection_manager().clone();
    let service = ConnectionService::new(manager.clone());

    // 解析网络配置（支持全局和项目级），跳过已有连接检查
    let network_method =
        resolve_network_method_with_project(network_config_id.as_deref(), project_path.as_deref())
            .await?;

    // 检查是否已有相同 URL 的正式连接
    let all_connections = service.list_connections().await;
    let existing_conn = all_connections.iter().find(|info| info.url == url);

    // 如果已有正式连接，直接返回成功信息，不创建新连接
    if let Some(info) = existing_conn {
        tracing::info!("测试连接：发现已有正式连接（ID={}），直接返回成功", info.id);

        let server_version = manager
            .get_connection(&info.id)
            .await
            .and_then(|db| db.meta().server_version)
            .unwrap_or_else(|| format!("{} (未知版本)", db_type));

        let response_time_ms = start.elapsed().as_millis() as u32;

        return Ok(TestConnectionResponse {
            success: true,
            message: format!("连接成功（已有连接：{}）", info.name),
            server_version,
            response_time_ms,
        });
    }

    // 没有已有连接，创建临时测试连接（30秒超时保护）
    let masked_url = ConnectionService::mask_password_in_url(&url);
    let has_embedded_creds = url.contains('@') && url.contains("://");
    let has_password_param = password.is_some();
    tracing::info!(
        "测试连接：创建临时连接进行测试（URL={}，has_creds={}，has_password_param={}，network_config={:?}，auth_config={:?}，auth_method={:?}）",
        masked_url, has_embedded_creds, has_password_param, network_config_id, auth_config_id, auth_method
    );

    let test_input = ConnectDatabaseInput {
        conn_id: None,
        db_type: db_type.clone(),
        url: url.clone(),
        name: Some("test_connection".to_string()),
        connection_type: Some("global".to_string()),
        project_id: project_path.clone(),
        description: None,
        driver_id: None,
        environment_id: None,
        auth_config_id: auth_config_id.clone(),
        auth_method: auth_method.clone(),
        network_config_id: network_config_id.clone(),
        driver_properties: None,
        advanced_options: None,
        options: None,
        tags: None,
        metadata_path: None,
        schema_name: None,
        use_duckdb_fed: None,
        password,
    };

    let connect_future = service.connect_with_type(test_input.into_connect_request(
        ConnectionType::Global,
        network_method,
        Some(true),
    ));

    let (conn_id, db) = match tokio::time::timeout(Duration::from_secs(30), connect_future).await {
        Ok(Ok(result)) => result,
        Ok(Err(e)) => {
            tracing::error!("测试连接失败：{}", e);
            return Err(format!("连接失败: {}", e).into());
        }
        Err(_elapsed) => {
            tracing::error!("测试连接超时（30秒），URL={}", masked_url);
            return Err(format!("连接超时（30秒）: 无法在 30 秒内连接到 {}", masked_url).into());
        }
    };

    let server_version = db
        .meta()
        .server_version
        .unwrap_or_else(|| format!("{} (未知版本)", db_type));

    let response_time_ms = start.elapsed().as_millis() as u32;

    // 关键：测试成功后立即返回响应，清理操作放入后台任务
    // 避免 close_connection 阻塞前端 UI（隧道释放、元数据缓存删除等可能耗时）
    let service_clone = service.clone();
    let conn_id_clone = conn_id.clone();
    tokio::spawn(async move {
        drop(db);
        tracing::info!("测试连接：已释放数据库连接引用（ID={}）", conn_id_clone);

        if let Err(e) = service_clone.close_connection(&conn_id_clone).await {
            tracing::error!("测试连接：关闭临时连接失败（ID={}）：{}", conn_id_clone, e);
        } else {
            tracing::info!("测试连接：临时连接已彻底关闭并清理（ID={}）", conn_id_clone);
        }
    });

    tracing::info!(
        "测试连接成功 {} ({}ms)，后台清理中...",
        masked_url,
        response_time_ms
    );

    Ok(TestConnectionResponse {
        success: true,
        message: "连接成功".to_string(),
        server_version,
        response_time_ms,
    })
}

/// 创建数据库文件请求参数
#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct CreateDatabaseFileInput {
    pub db_type: String, // "sqlite" 或 "duckdb"
    pub file_path: String,
}

/// 创建数据库文件响应
#[derive(serde::Serialize, Debug, specta::Type)]
pub struct CreateDatabaseFileResponse {
    pub file_path: String,
    pub success: bool,
    pub message: String,
}

/// 创建数据库文件（SQLite/DuckDB）
#[tauri::command]
#[specta::specta]
pub async fn create_database_file(
    input: CreateDatabaseFileInput,
) -> Result<CreateDatabaseFileResponse, CoreError> {
    use std::path::Path;

    // 验证数据库类型
    if input.db_type != "sqlite" && input.db_type != "duckdb" {
        return Err(format!(
            "不支持的数据库类型: {}. 仅支持 sqlite 和 duckdb",
            input.db_type
        )
        .into());
    }

    let path = Path::new(&input.file_path);

    // 检查文件是否已存在
    if path.exists() {
        return Err("文件已存在".to_string().into());
    }

    // 确保父目录存在
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
        }
    }

    // 根据数据库类型创建文件
    match input.db_type.as_str() {
        "sqlite" => {
            // SQLite: 创建空文件，实际连接时会自动初始化
            std::fs::File::create(path).map_err(|e| format!("创建 SQLite 文件失败: {}", e))?;
        }
        "duckdb" => {
            // DuckDB: 需要初始化数据库文件
            // 先创建临时连接来初始化文件，然后立即关闭
            let url = format!("duckdb://{}", input.file_path);
            let manager = get_connection_manager().clone();
            let service = ConnectionService::new(manager);

            tracing::info!("创建 DuckDB 文件：初始化数据库文件（{}）", input.file_path);
            let (conn_id, db) = service
                .connect(None, "duckdb", &url, Some("init_duckdb".to_string()))
                .await
                .map_err(|e| format!("初始化 DuckDB 失败: {}", e))?;

            // 立即关闭连接，释放文件锁
            drop(db);
            service
                .close_connection(&conn_id)
                .await
                .map_err(|e| format!("关闭初始化连接失败: {}", e))?;

            tracing::info!(
                "创建 DuckDB 文件：初始化完成并关闭连接（{}）",
                input.file_path
            );
        }
        _ => unreachable!(),
    }

    Ok(CreateDatabaseFileResponse {
        file_path: input.file_path,
        success: true,
        message: format!("{} 数据库文件创建成功", input.db_type),
    })
}

/// 测试连接配置（不保存）
#[tauri::command]
#[specta::specta]
pub async fn test_connection_config(config: DriverConnectionConfig) -> Result<(), CoreError> {
    let url = config.to_url()?;

    let manager = get_connection_manager().clone();
    let service = ConnectionService::new(manager);

    // 创建临时测试连接
    let masked_url = ConnectionService::mask_password_in_url(&url);
    tracing::info!(
        "测试连接配置：创建临时连接（driver={}, url={}）",
        config.driver,
        masked_url
    );
    let (conn_id, db) = service
        .connect(None, &config.driver, &url, Some("test".to_string()))
        .await
        .map_err(|e| {
            tracing::error!("测试连接配置失败：{}", e);
            format!("连接失败: {}", e)
        })?;

    // 彻底关闭临时连接
    // 1. 释放 db 的 Arc 引用
    drop(db);
    tracing::info!("测试连接配置：已释放数据库连接引用（ID={}）", conn_id);

    // 2. 从连接管理器中关闭并移除连接
    service.close_connection(&conn_id).await.map_err(|e| {
        tracing::error!("测试连接配置：关闭临时连接失败（ID={}）：{}", conn_id, e);
        format!("关闭测试连接失败: {}", e)
    })?;

    tracing::info!("测试连接配置：临时连接已彻底关闭并清理（ID={}）", conn_id);

    Ok(())
}

/// 全局连接信息响应
#[derive(serde::Serialize, Debug, specta::Type)]
pub struct GlobalConnectionInfoResponse {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub host: Option<String>,
    pub port: Option<i32>,
    pub database: Option<String>,
    pub schema_name: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub options: Option<String>,
    pub tags: Vec<String>,
    pub use_duckdb_fed: bool,
    pub metadata_path: Option<String>,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
    pub server_version: Option<String>,
    pub driver_id: Option<String>,
    pub environment_id: Option<String>,
    pub auth_config_id: Option<String>,
    pub auth_method: Option<String>,
    pub network_config_id: Option<String>,
    pub driver_properties: Option<String>,
    pub advanced_options: Option<String>,
    pub description: Option<String>,
}

/// 连接池状态响应
#[derive(serde::Serialize, Debug, specta::Type)]
pub struct ConnectionPoolStatusResponse {
    pub conn_id: String,
    pub active_connections: u32,
    pub idle_connections: u32,
    pub max_connections: u32,
    pub min_connections: u32,
    pub connection_timeout_ms: u32,
    pub idle_timeout_ms: u32,
    pub total_connections: u32,
    pub wait_queue_size: u32,
}

/// 获取连接池状态
#[tauri::command]
#[specta::specta]
pub async fn get_connection_pool_status(
    conn_id: String,
) -> Result<ConnectionPoolStatusResponse, CoreError> {
    let manager = get_connection_manager().clone();

    let _connection_info = manager
        .get_connection_info(&conn_id)
        .await
        .ok_or_else(|| format!("Connection not found: {}", conn_id))?;

    let pool_status = match manager.get_connection(&conn_id).await {
        Some(db) => db.pool_status().await,
        None => None,
    };

    match pool_status {
        Some(ps) => Ok(ConnectionPoolStatusResponse {
            conn_id: conn_id.clone(),
            active_connections: ps.active as u32,
            idle_connections: ps.idle as u32,
            max_connections: ps.max_connections as u32,
            min_connections: ps.min_connections as u32,
            connection_timeout_ms: 30000u32,
            idle_timeout_ms: 300000u32,
            total_connections: ps.size as u32,
            wait_queue_size: ps.waiting as u32,
        }),
        None => Ok(ConnectionPoolStatusResponse {
            conn_id: conn_id.clone(),
            active_connections: 1,
            idle_connections: 0,
            max_connections: 1,
            min_connections: 1,
            connection_timeout_ms: 30000u32,
            idle_timeout_ms: 300000u32,
            total_connections: 1,
            wait_queue_size: 0,
        }),
    }
}

/// 获取所有全局连接
#[tauri::command]
#[specta::specta]
pub async fn get_global_connections() -> Result<Vec<GlobalConnectionInfoResponse>, CoreError> {
    use crate::core::migration::global_init;

    let global_db = global_init::get_global_db_manager()
        .ok_or_else(|| "Global database manager not initialized".to_string())?;

    let connections = global_db
        .get_global_connections(None, None)
        .await
        .map_err(|e| format!("获取全局连接失败: {}", e))?;

    Ok(connections
        .into_iter()
        .map(|conn| {
            let tags: Vec<String> = conn.tags.as_ref().map_or(Vec::new(), |t| {
                serde_json::from_str(t).unwrap_or_else(|_| {
                    if t.is_empty() {
                        vec![]
                    } else {
                        vec![t.clone()]
                    }
                })
            });

            let password = conn
                .password_encrypted
                .and_then(|p| crate::core::crypto::decrypt_password(&p).ok().or(Some(p)));

            GlobalConnectionInfoResponse {
                id: conn.id,
                name: conn.name,
                driver: conn.driver,
                host: conn.host,
                port: conn.port,
                database: conn.database,
                schema_name: conn.schema_name,
                username: conn.username,
                password,
                options: conn.options,
                tags,
                use_duckdb_fed: conn.use_duckdb_fed,
                metadata_path: conn.metadata_path,
                is_active: conn.is_active,
                created_at: conn.created_at,
                updated_at: conn.updated_at,
                server_version: conn.server_version,
                driver_id: conn.driver_id,
                environment_id: conn.environment_id,
                auth_config_id: conn.auth_config_id,
                auth_method: conn.auth_method,
                network_config_id: conn.network_config_id,
                driver_properties: conn.driver_properties,
                advanced_options: conn.advanced_options,
                description: conn.description,
            }
        })
        .collect())
}

/// 校验连接配置（不实际连接）
/// 验证 URL、连接类型、驱动/环境/认证/网络配置的完整性和存在性
#[derive(serde::Serialize, Debug, specta::Type)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// 更新全局连接请求参数
#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct UpdateGlobalConnectionInput {
    pub conn_id: String,
    pub name: Option<String>,
    pub url: Option<String>,
    pub driver: Option<String>,
    pub host: Option<String>,
    pub port: Option<i32>,
    pub database: Option<String>,
    pub schema_name: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub options: Option<String>,
    pub tags: Option<Vec<String>>,
    pub use_duckdb_fed: Option<bool>,
    pub metadata_path: Option<String>,
    pub driver_id: Option<String>,
    pub environment_id: Option<String>,
    pub auth_config_id: Option<String>,
    pub auth_method: Option<String>,
    pub network_config_id: Option<String>,
    pub driver_properties: Option<String>,
    pub advanced_options: Option<String>,
    pub description: Option<String>,
    pub server_version: Option<String>,
}

/// 更新全局连接
#[tauri::command]
#[specta::specta]
pub async fn update_global_connection(input: UpdateGlobalConnectionInput) -> Result<(), CoreError> {
    use crate::core::persistence::global_db::GlobalConnectionUpdateInput;

    let global_db = crate::core::migration::global_init::get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database manager not initialized".to_string()))?;

    let name = input
        .name
        .clone()
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| format!("unknown-{}", &input.conn_id[..8.min(input.conn_id.len())]));

    let tags_json = input
        .tags
        .map(|t| serde_json::to_string(&t).unwrap_or_default());

    let conn_id = input.conn_id.clone();
    let host_clone = input.host.clone();
    let driver_clone = input.driver.clone();
    let port_clone = input.port;
    let database_clone = input.database.clone();
    let username_clone = input.username.clone();

    let db_input = GlobalConnectionUpdateInput {
        conn_id: input.conn_id,
        name,
        driver: input.driver,
        host: input.host,
        port: input.port,
        database: input.database,
        schema_name: input.schema_name,
        username: input.username,
        password: input.password,
        tags: tags_json,
        server_version: input.server_version,
        description: input.description,
        driver_id: input.driver_id,
        environment_id: input.environment_id,
        auth_config_id: input.auth_config_id,
        auth_method: input.auth_method,
        network_config_id: input.network_config_id,
        options: input.options,
        driver_properties: input.driver_properties,
        advanced_options: input.advanced_options,
        use_duckdb_fed: input.use_duckdb_fed,
        metadata_path: input.metadata_path,
    };

    global_db
        .update_global_connection(db_input)
        .await
        .map_err(|e| CoreError::from(format!("Failed to update global connection: {}", e)))?;

    let manager = get_connection_manager().clone();
    if let Some(mut conn_info) = manager.get_connection_info(&conn_id).await {
        if let Some(ref host) = host_clone {
            let driver = driver_clone.unwrap_or_else(|| conn_info.db_type.clone());
            let port = port_clone.unwrap_or(0);
            let database = database_clone.unwrap_or_default();
            let username = username_clone.unwrap_or_default();
            let password = "******";
            let url = format!(
                "{}://{}:{}@{}:{}/{}",
                driver, username, password, host, port, database
            );
            conn_info.url = url;
            if let Err(e) = manager.update_connection_info(&conn_id, conn_info).await {
                tracing::warn!("更新运行时连接信息失败: {}", e);
            }
        }
    }

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn validate_connection_config(
    input: ConnectDatabaseInput,
) -> Result<ValidationResult, CoreError> {
    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // 1. URL 非空
    if input.url.trim().is_empty() {
        errors.push("Database URL cannot be empty".to_string());
    }

    // 2. connection_type 枚举校验
    match input.connection_type.as_deref() {
        Some("global") | None => { /* OK, global is default */ }
        Some("project") => {
            if input.project_id.as_deref().unwrap_or("").is_empty() {
                errors.push("project_id is required for project connections".to_string());
            }
        }
        Some(other) => {
            errors.push(format!(
                "Invalid connection_type '{}', must be 'global' or 'project'",
                other
            ));
        }
    }

    // 3. driver_id 存在性
    if let Some(ref driver_id) = input.driver_id {
        if !driver_id.trim().is_empty() {
            if let Some(gdb) = crate::core::migration::global_init::get_global_db_manager() {
                let driver = gdb.get_driver(driver_id).await;
                match driver {
                    Ok(None) | Err(_) => {
                        errors.push(format!("Driver '{}' not registered", driver_id));
                    }
                    Ok(Some(_)) => { /* OK */ }
                }
            }
        }
    }

    // 4. environment_id 按前缀路由查找
    if let Some(ref env_id) = input.environment_id {
        if !env_id.trim().is_empty() {
            let mut found = false;
            if let Some(gdb) = crate::core::migration::global_init::get_global_db_manager() {
                if let Ok(envs) = gdb.list_environments().await {
                    found = envs.iter().any(|e| e.id == *env_id);
                }
            }
            if !found {
                errors.push(format!("Environment '{}' not found", env_id));
            }
        }
    }

    // 5. auth_config_id 按前缀路由查找
    if let Some(ref auth_id) = input.auth_config_id {
        if !auth_id.trim().is_empty() {
            let mut found = false;
            if let Some(gdb) = crate::core::migration::global_init::get_global_db_manager() {
                if let Ok(auths) = gdb.list_auth_configs(None).await {
                    found = auths.iter().any(|a| a.id == *auth_id);
                }
            }
            if !found {
                errors.push(format!("Auth config '{}' not found", auth_id));
            }
        }
    }

    // 6. network_config_id 按前缀路由查找 + parse_network_method
    if let Some(ref net_id) = input.network_config_id {
        if !net_id.trim().is_empty() {
            let network_method = resolve_network_method(Some(net_id)).await;
            match network_method {
                Err(e) => {
                    errors.push(format!("Network config '{}' invalid: {}", net_id, e));
                }
                Ok(_) => { /* OK */ }
            }
        }
    }

    // 7. 数据库类型检查
    let known_dbs = [
        "mysql",
        "postgresql",
        "postgres",
        "sqlite",
        "duckdb",
        "mssql",
        "sqlserver",
    ];
    if !known_dbs.contains(&input.db_type.to_lowercase().as_str()) {
        warnings.push(format!(
            "Unrecognized database type '{}', supported: MySQL, PostgreSQL, SQLite, DuckDB, SQL Server",
            input.db_type
        ));
    }

    Ok(ValidationResult {
        valid: errors.is_empty(),
        errors,
        warnings,
    })
}
