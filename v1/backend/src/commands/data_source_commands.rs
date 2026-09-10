use std::sync::Arc;

use crate::commands::project_commands::ProjectState;
use crate::core::error::CoreError;
use crate::core::migration::get_global_db_manager;
use crate::core::persistence::auth_store;
use crate::core::persistence::driver_store::{DataSourceType, Driver, DriverFile};
use crate::core::persistence::env_store;
use crate::core::persistence::network_store;
use crate::core::persistence::project_connection_store::ProjectConnectionStore;
use crate::core::persistence::project_db::ProjectDatabaseManager;
use crate::core::services::driver_service::{self, DriverService};
use uuid::Uuid;

use chrono::Utc;

fn get_driver_service() -> Result<DriverService, CoreError> {
    let db = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database not initialized".to_string()))?;
    Ok(DriverService::new(db))
}

/// 驱动列表响应（含缺失驱动自检）
#[derive(serde::Serialize, Debug, specta::Type)]
pub struct DriverListResponse {
    pub drivers: Vec<Driver>,
    pub missing: Vec<driver_service::MissingDriver>,
}

/// 获取数据源类型目录（供前端数据源树渲染）
#[tauri::command]
#[specta::specta]
pub async fn get_data_source_types(
    category: Option<String>,
) -> Result<Vec<DataSourceType>, CoreError> {
    let service = get_driver_service()?;
    let types = service.get_data_source_types().await?;
    let filtered: Vec<DataSourceType> = types
        .into_iter()
        .filter(|t| category.as_ref().is_none_or(|c| t.category == *c))
        .collect();
    Ok(filtered)
}

/// 获取驱动列表，传入 project_path 时自动检测缺失驱动
#[tauri::command]
#[specta::specta]
pub async fn get_available_drivers(
    project_path: Option<String>,
    state: tauri::State<'_, ProjectState>,
) -> Result<DriverListResponse, CoreError> {
    let db = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database not initialized".to_string()))?;
    let drivers = db.get_all_drivers().await?;
    let mut missing = Vec::new();

    if let Some(path) = project_path {
        let proj_db = get_project_db_manager(&path, &state).await?;
        let store = ProjectConnectionStore::new(proj_db);
        let enabled_drivers = store.list_enabled_drivers().await?;

        for driver in &drivers {
            if !enabled_drivers.contains(&driver.id) {
                continue;
            }
            if driver.driver_kind != "native" {
                let version = driver.version.as_deref().unwrap_or("0.0.0");
                let installed = db
                    .is_driver_installed(&driver.id, version)
                    .await
                    .unwrap_or(false);
                if !installed {
                    missing.push(driver_service::MissingDriver {
                        driver_id: driver.id.clone(),
                        driver_name: driver.name.clone(),
                        download_url: driver.download_url.clone().unwrap_or_default(),
                    });
                }
            }
        }
    }

    Ok(DriverListResponse { drivers, missing })
}

/// 驱动详情响应（含可用性状态）
#[derive(serde::Serialize, Debug, specta::Type)]
pub struct DriverDetailResponse {
    pub driver: Driver,
    pub availability: String,
}

/// 获取驱动详情（含 config_schema + 可用性状态）
#[tauri::command]
#[specta::specta]
pub async fn get_driver_detail(
    driver_id: String,
    project_path: Option<String>,
    state: tauri::State<'_, ProjectState>,
) -> Result<DriverDetailResponse, CoreError> {
    let db = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database not initialized".to_string()))?;
    let driver = db
        .get_driver(&driver_id)
        .await?
        .ok_or_else(|| CoreError::from(format!("驱动 {} 不存在", driver_id)))?;

    let mut availability = "ready".to_string();
    if let Some(path) = project_path {
        let proj_db = get_project_db_manager(&path, &state).await?;
        let store = ProjectConnectionStore::new(proj_db);
        if !store.is_driver_enabled(&driver_id).await.unwrap_or(false) {
            availability = "not_enabled".to_string();
        } else if driver.driver_kind != "native" {
            let version = driver.version.as_deref().unwrap_or("0.0.0");
            if !db
                .is_driver_installed(&driver_id, version)
                .await
                .unwrap_or(false)
            {
                availability = "not_installed".to_string();
            }
        }
    }

    Ok(DriverDetailResponse {
        driver,
        availability,
    })
}

/// 安装外部驱动文件（下载到本机并注册）
#[tauri::command]
#[specta::specta]
pub async fn install_driver(driver_id: String) -> Result<(), CoreError> {
    let service = get_driver_service()?;
    service.install_driver(&driver_id).await
}

/// 列出某驱动在本机已安装的所有文件版本
#[tauri::command]
#[specta::specta]
pub async fn list_driver_files(driver_id: String) -> Result<Vec<DriverFile>, CoreError> {
    let service = get_driver_service()?;
    service.list_driver_files(&driver_id).await
}

/// 列出所有环境
#[tauri::command]
#[specta::specta]
pub async fn list_environments() -> Result<Vec<env_store::Environment>, CoreError> {
    let db = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database not initialized".to_string()))?;
    db.list_environments().await
}

/// 创建环境
#[tauri::command]
#[specta::specta]
pub async fn create_environment(mut env: env_store::Environment) -> Result<(), CoreError> {
    if env.id.is_empty() {
        env.id = format!("G_env_{}", Uuid::new_v4().to_string().replace('-', "_"));
    }
    if env.created_at.is_empty() {
        env.created_at = Utc::now().to_rfc3339();
    }
    if env.origin.is_none() {
        env.origin = Some("project".to_string());
    }
    let db = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database not initialized".to_string()))?;
    db.create_environment(&env).await
}

/// 更新环境
#[tauri::command]
#[specta::specta]
pub async fn update_environment(env: env_store::Environment) -> Result<(), CoreError> {
    let db = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database not initialized".to_string()))?;
    db.update_environment(&env).await
}

/// 删除环境
#[tauri::command]
#[specta::specta]
pub async fn delete_environment(id: String) -> Result<(), CoreError> {
    let db = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database not initialized".to_string()))?;
    db.delete_environment(&id).await
}

/// 列出环境策略
#[tauri::command]
#[specta::specta]
pub async fn list_environment_policies(
    environment_id: String,
) -> Result<Vec<env_store::EnvironmentPolicy>, CoreError> {
    let db = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database not initialized".to_string()))?;
    db.list_environment_policies(&environment_id).await
}

/// 创建环境策略
#[tauri::command]
#[specta::specta]
pub async fn create_environment_policy(
    mut policy: env_store::EnvironmentPolicy,
) -> Result<(), CoreError> {
    if policy.id.is_empty() {
        policy.id = format!("G_ep_{}", Uuid::new_v4().to_string().replace('-', "_"));
    }
    if policy.created_at.is_empty() {
        policy.created_at = Utc::now().to_rfc3339();
    }
    let db = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database not initialized".to_string()))?;
    db.create_environment_policy(&policy).await
}

/// 更新环境策略
#[tauri::command]
#[specta::specta]
pub async fn update_environment_policy(
    policy: env_store::EnvironmentPolicy,
) -> Result<(), CoreError> {
    let db = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database not initialized".to_string()))?;
    db.update_environment_policy(&policy).await
}

/// 删除环境策略
#[tauri::command]
#[specta::specta]
pub async fn delete_environment_policy(id: String) -> Result<(), CoreError> {
    let db = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database not initialized".to_string()))?;
    db.delete_environment_policy(&id).await
}

/// 列出认证配置
#[tauri::command]
#[specta::specta]
pub async fn list_auth_configs(
    auth_type: Option<String>,
) -> Result<Vec<auth_store::AuthConfig>, CoreError> {
    let db = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database not initialized".to_string()))?;
    db.list_auth_configs(auth_type.as_deref()).await
}

/// 创建认证配置（返回创建的 AuthConfig 含生成的 id）
#[tauri::command]
#[specta::specta]
pub async fn create_auth_config(
    mut ac: auth_store::AuthConfig,
) -> Result<auth_store::AuthConfig, CoreError> {
    let now = Utc::now().to_rfc3339();
    if ac.id.is_empty() {
        ac.id = format!("G_auth_{}", Uuid::new_v4().to_string().replace('-', "_"));
    }
    if ac.created_at.is_empty() {
        ac.created_at = now.clone();
    }
    if ac.updated_at.is_empty() {
        ac.updated_at = now;
    }
    let db = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database not initialized".to_string()))?;
    db.create_auth_config(&ac).await?;
    Ok(ac)
}

/// 删除认证配置
#[tauri::command]
#[specta::specta]
pub async fn delete_auth_config(id: String) -> Result<(), CoreError> {
    let db = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database not initialized".to_string()))?;
    db.delete_auth_config(&id).await
}

/// 更新认证配置
#[tauri::command]
#[specta::specta]
pub async fn update_auth_config(ac: auth_store::AuthConfig) -> Result<(), CoreError> {
    let db = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database not initialized".to_string()))?;
    db.update_auth_config(&ac).await
}

/// 列出网络配置
#[tauri::command]
#[specta::specta]
pub async fn list_network_configs(
    network_type: Option<String>,
) -> Result<Vec<network_store::NetworkConfig>, CoreError> {
    let db = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database not initialized".to_string()))?;
    db.list_network_configs(network_type.as_deref()).await
}

/// 创建网络配置（返回创建的 NetworkConfig 含生成的 id）
#[tauri::command]
#[specta::specta]
pub async fn create_network_config(
    mut nc: network_store::NetworkConfig,
) -> Result<network_store::NetworkConfig, CoreError> {
    let now = Utc::now().to_rfc3339();
    if nc.id.is_empty() {
        nc.id = format!("G_net_{}", Uuid::new_v4().to_string().replace('-', "_"));
    }
    if nc.created_at.is_empty() {
        nc.created_at = now.clone();
    }
    if nc.updated_at.is_empty() {
        nc.updated_at = now;
    }
    let db = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database not initialized".to_string()))?;
    db.create_network_config(&nc).await?;
    Ok(nc)
}

/// 更新网络配置
#[tauri::command]
#[specta::specta]
pub async fn update_network_config(nc: network_store::NetworkConfig) -> Result<(), CoreError> {
    let db = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database not initialized".to_string()))?;
    db.update_network_config(&nc).await
}

/// 删除网络配置
#[tauri::command]
#[specta::specta]
pub async fn delete_network_config(id: String) -> Result<(), CoreError> {
    let db = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database not initialized".to_string()))?;
    db.delete_network_config(&id).await
}

/// 获取全局驱动目录（按 category / driver_kind 过滤，供"驱动市场"展示）
#[tauri::command]
#[specta::specta]
pub async fn get_all_drivers_catalog(
    category: Option<String>,
    driver_kind: Option<String>,
) -> Result<Vec<Driver>, CoreError> {
    let db = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database not initialized".to_string()))?;

    let drivers = if let Some(cat) = category {
        let types = db.list_data_source_types().await?;
        let mut result = Vec::new();
        for dt in types {
            if dt.category == cat && dt.enabled {
                let mut d = db.get_drivers_by_type(&dt.id).await?;
                result.append(&mut d);
            }
        }
        result
    } else {
        db.get_all_drivers().await?
    };

    let filtered: Vec<Driver> = drivers
        .into_iter()
        .filter(|d| driver_kind.as_ref().is_none_or(|k| d.driver_kind == *k))
        .collect();

    Ok(filtered)
}

async fn get_project_db_manager(
    project_path: &str,
    state: &tauri::State<'_, ProjectState>,
) -> Result<Arc<ProjectDatabaseManager>, CoreError> {
    let guard = state.store.lock().await;
    let store = guard
        .as_ref()
        .ok_or_else(|| CoreError::from("没有打开的项目".to_string()))?;
    if store.db_manager.project_path().to_string_lossy() != project_path {
        return Err(CoreError::from("项目路径不匹配".to_string()));
    }
    Ok(store.db_manager.clone())
}

/// 为项目启用一个驱动
#[tauri::command]
#[specta::specta]
pub async fn enable_driver_for_project(
    driver_id: String,
    project_path: String,
    state: tauri::State<'_, ProjectState>,
) -> Result<(), CoreError> {
    let db_manager = get_project_db_manager(&project_path, &state).await?;
    let store = ProjectConnectionStore::new(db_manager);
    store.enable_driver(&driver_id).await
}

/// 为项目禁用一个驱动
#[tauri::command]
#[specta::specta]
pub async fn disable_driver_for_project(
    driver_id: String,
    project_path: String,
    state: tauri::State<'_, ProjectState>,
) -> Result<(), CoreError> {
    let db_manager = get_project_db_manager(&project_path, &state).await?;
    let store = ProjectConnectionStore::new(db_manager);
    store.disable_driver(&driver_id).await
}

/// 列出项目中所有已启用的驱动
#[tauri::command]
#[specta::specta]
pub async fn list_enabled_project_drivers(
    project_path: String,
    state: tauri::State<'_, ProjectState>,
) -> Result<Vec<String>, CoreError> {
    let db_manager = get_project_db_manager(&project_path, &state).await?;
    let store = ProjectConnectionStore::new(db_manager);
    store.list_enabled_drivers().await
}

// ========== 项目级环境命令 ==========

/// 在指定项目中创建环境
#[tauri::command]
#[specta::specta]
pub async fn project_create_environment(
    name: String,
    description: Option<String>,
    color: Option<String>,
    sort_order: Option<i32>,
    project_path: String,
    state: tauri::State<'_, ProjectState>,
) -> Result<env_store::Environment, CoreError> {
    let db_manager = get_project_db_manager(&project_path, &state).await?;
    db_manager
        .create_project_environment(&name, description.as_deref(), color.as_deref(), sort_order)
        .await
}

/// 列出指定项目中的所有环境
#[tauri::command]
#[specta::specta]
pub async fn project_list_environments(
    project_path: String,
    state: tauri::State<'_, ProjectState>,
) -> Result<Vec<env_store::Environment>, CoreError> {
    let db_manager = get_project_db_manager(&project_path, &state).await?;
    db_manager.list_project_environments().await
}

/// 更新指定项目中的环境
#[tauri::command]
#[specta::specta]
pub async fn project_update_environment(
    id: String,
    name: Option<String>,
    description: Option<String>,
    color: Option<String>,
    sort_order: Option<i32>,
    project_path: String,
    state: tauri::State<'_, ProjectState>,
) -> Result<bool, CoreError> {
    let db_manager = get_project_db_manager(&project_path, &state).await?;
    db_manager
        .update_project_environment(
            &id,
            name.as_deref(),
            description.as_deref(),
            color.as_deref(),
            sort_order,
        )
        .await?;
    Ok(true)
}

/// 从指定项目中删除环境
#[tauri::command]
#[specta::specta]
pub async fn project_delete_environment(
    id: String,
    project_path: String,
    state: tauri::State<'_, ProjectState>,
) -> Result<bool, CoreError> {
    let db_manager = get_project_db_manager(&project_path, &state).await?;
    db_manager.delete_project_environment(&id).await?;
    Ok(true)
}

// ========== 项目级环境策略命令 ==========

/// 在指定项目中创建环境策略
#[tauri::command]
#[specta::specta]
pub async fn project_create_environment_policy(
    environment_id: String,
    policy_type: String,
    policy_config: Option<String>,
    project_path: String,
    state: tauri::State<'_, ProjectState>,
) -> Result<env_store::EnvironmentPolicy, CoreError> {
    let db_manager = get_project_db_manager(&project_path, &state).await?;
    db_manager
        .create_project_environment_policy(&environment_id, &policy_type, policy_config.as_deref())
        .await
}

/// 列出指定项目中某环境的所有策略
#[tauri::command]
#[specta::specta]
pub async fn project_list_environment_policies(
    environment_id: String,
    project_path: String,
    state: tauri::State<'_, ProjectState>,
) -> Result<Vec<env_store::EnvironmentPolicy>, CoreError> {
    let db_manager = get_project_db_manager(&project_path, &state).await?;
    db_manager
        .list_project_environment_policies(&environment_id)
        .await
}

/// 更新指定项目中的环境策略
#[tauri::command]
#[specta::specta]
pub async fn project_update_environment_policy(
    id: String,
    policy_config: Option<String>,
    enabled: Option<bool>,
    project_path: String,
    state: tauri::State<'_, ProjectState>,
) -> Result<bool, CoreError> {
    let db_manager = get_project_db_manager(&project_path, &state).await?;
    db_manager
        .update_project_environment_policy(&id, policy_config.as_deref(), enabled)
        .await?;
    Ok(true)
}

/// 从指定项目中删除环境策略
#[tauri::command]
#[specta::specta]
pub async fn project_delete_environment_policy(
    id: String,
    project_path: String,
    state: tauri::State<'_, ProjectState>,
) -> Result<bool, CoreError> {
    let db_manager = get_project_db_manager(&project_path, &state).await?;
    db_manager.delete_project_environment_policy(&id).await?;
    Ok(true)
}

// ========== 项目级认证配置命令 ==========

/// 在指定项目中创建认证配置
#[tauri::command]
#[specta::specta]
pub async fn project_create_auth_config(
    name: Option<String>,
    auth_type: String,
    auth_data: String,
    project_path: String,
    state: tauri::State<'_, ProjectState>,
) -> Result<auth_store::AuthConfig, CoreError> {
    let db_manager = get_project_db_manager(&project_path, &state).await?;
    db_manager
        .create_project_auth_config(name.as_deref(), &auth_type, &auth_data)
        .await
}

/// 列出指定项目中的所有认证配置
#[tauri::command]
#[specta::specta]
pub async fn project_list_auth_configs(
    project_path: String,
    state: tauri::State<'_, ProjectState>,
) -> Result<Vec<auth_store::AuthConfig>, CoreError> {
    let db_manager = get_project_db_manager(&project_path, &state).await?;
    db_manager.list_project_auth_configs().await
}

/// 从指定项目中删除认证配置
#[tauri::command]
#[specta::specta]
pub async fn project_delete_auth_config(
    id: String,
    project_path: String,
    state: tauri::State<'_, ProjectState>,
) -> Result<bool, CoreError> {
    let db_manager = get_project_db_manager(&project_path, &state).await?;
    db_manager.delete_project_auth_config(&id).await?;
    Ok(true)
}

/// 更新指定项目中的认证配置
#[tauri::command]
#[specta::specta]
pub async fn project_update_auth_config(
    id: String,
    name: Option<String>,
    auth_type: String,
    auth_data: String,
    project_path: String,
    state: tauri::State<'_, ProjectState>,
) -> Result<(), CoreError> {
    let db_manager = get_project_db_manager(&project_path, &state).await?;
    db_manager
        .update_project_auth_config(&id, name.as_deref(), &auth_type, &auth_data)
        .await
}

// ========== 项目级网络配置命令 ==========

/// 在指定项目中创建网络配置
#[tauri::command]
#[specta::specta]
pub async fn project_create_network_config(
    name: Option<String>,
    network_type: String,
    config: String,
    project_path: String,
    state: tauri::State<'_, ProjectState>,
) -> Result<network_store::NetworkConfig, CoreError> {
    let db_manager = get_project_db_manager(&project_path, &state).await?;
    db_manager
        .create_project_network_config(name.as_deref(), &network_type, &config)
        .await
}

/// 列出指定项目中的所有网络配置
#[tauri::command]
#[specta::specta]
pub async fn project_list_network_configs(
    project_path: String,
    state: tauri::State<'_, ProjectState>,
) -> Result<Vec<network_store::NetworkConfig>, CoreError> {
    let db_manager = get_project_db_manager(&project_path, &state).await?;
    db_manager.list_project_network_configs().await
}

/// 更新指定项目中的网络配置
#[tauri::command]
#[specta::specta]
pub async fn project_update_network_config(
    id: String,
    name: Option<String>,
    config: Option<String>,
    project_path: String,
    state: tauri::State<'_, ProjectState>,
) -> Result<bool, CoreError> {
    let db_manager = get_project_db_manager(&project_path, &state).await?;
    db_manager
        .update_project_network_config(&id, name.as_deref(), config.as_deref())
        .await?;
    Ok(true)
}

/// 从指定项目中删除网络配置
#[tauri::command]
#[specta::specta]
pub async fn project_delete_network_config(
    id: String,
    project_path: String,
    state: tauri::State<'_, ProjectState>,
) -> Result<bool, CoreError> {
    let db_manager = get_project_db_manager(&project_path, &state).await?;
    db_manager.delete_project_network_config(&id).await?;
    Ok(true)
}

// ========== 网络配置测试命令 ==========

/// 测试网络配置响应
#[derive(serde::Serialize, Debug, specta::Type)]
pub struct TestNetworkConfigResponse {
    pub success: bool,
    pub message: String,
    pub response_time_ms: u32,
    pub detail: Option<String>,
}

/// 测试网络配置连通性
///
/// 在不创建数据库连接的情况下，独立测试 SSH 隧道 / SSL 证书 / 代理的连通性
#[tauri::command]
#[specta::specta]
pub async fn test_network_config(
    network_config_id: String,
    project_path: Option<String>,
) -> Result<TestNetworkConfigResponse, CoreError> {
    use crate::core::driver::connection::config::{ConnectionConfig, ProxyConfig, SshConfig};
    use crate::core::driver::connection::connector;
    use std::path::Path;
    use std::time::Instant;

    // 支持项目级网络配置（P_/GP_ 前缀）
    let net = if network_config_id.starts_with("P_") || network_config_id.starts_with("GP_") {
        if let Some(ref pp) = project_path {
            let db_path = Path::new(pp).join(".RSmeta").join("project.db");
            if db_path.exists() {
                let conn = rusqlite::Connection::open(&db_path)
                    .map_err(|e| CoreError::from(format!("打开项目数据库失败: {}", e)))?;
                let row = conn.query_row(
                    "SELECT network_type, config FROM network_configs WHERE id = ?1",
                    rusqlite::params![network_config_id],
                    |row| {
                        let network_type: String = row.get(0)?;
                        let config: String = row.get(1)?;
                        Ok((network_type, config))
                    },
                );
                match row {
                    Ok((network_type, config)) => network_store::NetworkConfig {
                        id: network_config_id.clone(),
                        name: None,
                        network_type,
                        config,
                        auth_config_id: None,
                        origin: Some("project".to_string()),
                        source_id: None,
                        snapshot_at: None,
                        created_at: String::new(),
                        updated_at: String::new(),
                    },
                    Err(_) => {
                        return Err(CoreError::from(format!(
                            "项目网络配置 {} 不存在",
                            network_config_id
                        )));
                    }
                }
            } else {
                return Err(CoreError::from(format!(
                    "项目路径不存在，无法加载网络配置 {}",
                    network_config_id
                )));
            }
        } else {
            return Err(CoreError::from(format!(
                "需要 project_path 来加载项目级网络配置 {}",
                network_config_id
            )));
        }
    } else {
        let db = get_global_db_manager()
            .ok_or_else(|| CoreError::from("Global database not initialized".to_string()))?;

        db.get_network_config(&network_config_id)
            .await?
            .ok_or_else(|| CoreError::from(format!("网络配置 {} 不存在", network_config_id)))?
    };

    let start = Instant::now();

    match net.network_type.as_str() {
        "ssh" => {
            let ssh_config: SshConfig = serde_json::from_str(&net.config)
                .map_err(|e| CoreError::from(format!("解析 SSH 配置 JSON 失败: {}", e)))?;

            let dummy = ConnectionConfig::direct(&ssh_config.remote_host, ssh_config.remote_port);
            match connector::establish_ssh_tunnel(&dummy, &ssh_config).await {
                Ok(guard) => {
                    let local_port = guard.port();
                    Ok(TestNetworkConfigResponse {
                        success: true,
                        message: format!("SSH 隧道测试成功，本地端口: {}", local_port),
                        response_time_ms: start.elapsed().as_millis() as u32,
                        detail: Some(format!(
                            "SSH {}:{} → {}:{}",
                            ssh_config.host,
                            ssh_config.port,
                            ssh_config.remote_host,
                            ssh_config.remote_port
                        )),
                    })
                }
                Err(e) => Ok(TestNetworkConfigResponse {
                    success: false,
                    message: format!("SSH 隧道测试失败: {}", e),
                    response_time_ms: start.elapsed().as_millis() as u32,
                    detail: None,
                }),
            }
        }
        "ssl" => {
            let cfg: crate::core::driver::connection::config::SslConfig =
                serde_json::from_str(&net.config)
                    .map_err(|e| CoreError::from(format!("解析 SSL 配置 JSON 失败: {}", e)))?;

            let mut checks = Vec::new();
            if let Some(ref ca) = cfg.ca_cert_path {
                match std::fs::read(ca) {
                    Ok(_) => {
                        checks.push(format!("CA 证书文件存在: {}", ca));
                        match connector::check_cert_expiry(ca) {
                            Ok(info) => {
                                checks.push(format!(
                                    "CA 证书: subject={}, 过期日期={}, 剩余{}天{}",
                                    info.subject,
                                    info.not_after,
                                    info.days_until_expiry,
                                    if info.is_expired {
                                        " ⚠️ 已过期"
                                    } else {
                                        ""
                                    }
                                ));
                            }
                            Err(e) => checks.push(format!("CA 证书过期检查失败: {}", e)),
                        }
                    }
                    Err(e) => checks.push(format!("CA 证书文件读取失败 '{}': {}", ca, e)),
                }
            }
            if let Some(ref cert) = cfg.client_cert_path {
                match std::fs::read(cert) {
                    Ok(_) => {
                        checks.push(format!("客户端证书文件存在: {}", cert));
                        match connector::check_cert_expiry(cert) {
                            Ok(info) => {
                                checks.push(format!(
                                    "客户端证书: subject={}, 过期日期={}, 剩余{}天{}",
                                    info.subject,
                                    info.not_after,
                                    info.days_until_expiry,
                                    if info.is_expired {
                                        " ⚠️ 已过期"
                                    } else {
                                        ""
                                    }
                                ));
                            }
                            Err(e) => checks.push(format!("客户端证书过期检查失败: {}", e)),
                        }
                    }
                    Err(e) => checks.push(format!("客户端证书文件读取失败 '{}': {}", cert, e)),
                }
            }
            if let Some(ref key) = cfg.client_key_path {
                match std::fs::read(key) {
                    Ok(_) => checks.push(format!("客户端私钥文件存在: {}", key)),
                    Err(e) => checks.push(format!("客户端私钥文件读取失败 '{}': {}", key, e)),
                }
            }

            let all_ok = !checks.iter().any(|c| c.contains("失败"));
            Ok(TestNetworkConfigResponse {
                success: all_ok,
                message: if all_ok {
                    "SSL 证书文件验证通过".to_string()
                } else {
                    "SSL 证书文件验证存在失败项".to_string()
                },
                response_time_ms: start.elapsed().as_millis() as u32,
                detail: Some(checks.join("\n")),
            })
        }
        "proxy" | "http_proxy" | "socks" | "socks5" => {
            let proxy_config: ProxyConfig = serde_json::from_str(&net.config)
                .map_err(|e| CoreError::from(format!("解析代理配置 JSON 失败: {}", e)))?;

            let is_socks = net.network_type == "socks" || net.network_type == "socks5";
            let test_host = "127.0.0.1";
            let test_port = 1u16; // 仅测试代理连通性，不实际连接目标

            let dummy = ConnectionConfig::direct(test_host, test_port);
            let result = if is_socks {
                connector::establish_socks_proxy(&dummy, &proxy_config).await
            } else {
                connector::establish_http_proxy(&dummy, &proxy_config).await
            };

            match result {
                Ok(_) => Ok(TestNetworkConfigResponse {
                    success: true,
                    message: "代理连接测试成功".to_string(),
                    response_time_ms: start.elapsed().as_millis() as u32,
                    detail: Some(format!("{}:{}", proxy_config.host, proxy_config.port)),
                }),
                Err(e) => Ok(TestNetworkConfigResponse {
                    success: false,
                    message: format!("代理连接测试失败: {}", e),
                    response_time_ms: start.elapsed().as_millis() as u32,
                    detail: Some(format!(
                        "代理 {}:{} → {}",
                        proxy_config.host, proxy_config.port, e
                    )),
                }),
            }
        }
        other => Ok(TestNetworkConfigResponse {
            success: false,
            message: format!("不支持的网络配置类型: {}", other),
            response_time_ms: start.elapsed().as_millis() as u32,
            detail: None,
        }),
    }
}

// ========== 快照命令（全局 → 项目） ==========

/// 快照结果
#[derive(serde::Serialize, Debug, specta::Type)]
pub struct SnapshotResult {
    pub snapshot_id: String,
    pub origin: String,
    pub source_id: String,
}

/// 从全局环境快照到项目
///
/// 将全局环境 G_env_xxx 复制到项目表，生成 GP_env_xxx_namedate ID。
/// 策略（environment_policies）同步复制到项目。
#[tauri::command]
#[specta::specta]
pub async fn snapshot_global_env(
    global_env_id: String,
    project_path: String,
    state: tauri::State<'_, ProjectState>,
) -> Result<SnapshotResult, CoreError> {
    let global_db = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database not initialized".to_string()))?;

    let source = global_db
        .get_environment(&global_env_id)
        .await?
        .ok_or_else(|| CoreError::from(format!("全局环境 {} 不存在", global_env_id)))?;

    let db_manager = get_project_db_manager(&project_path, &state).await?;
    let snapshot_id = db_manager
        .snapshot_environment(&source, &global_env_id)
        .await?;

    // 复制策略
    let policies = global_db.list_environment_policies(&global_env_id).await?;
    for policy in policies {
        db_manager
            .create_project_environment_policy(
                &snapshot_id,
                &policy.policy_type,
                policy.policy_config.as_deref(),
            )
            .await?;
    }

    Ok(SnapshotResult {
        snapshot_id,
        origin: "global_snapshot".to_string(),
        source_id: global_env_id,
    })
}

/// 从全局认证配置快照到项目
#[tauri::command]
#[specta::specta]
pub async fn snapshot_global_auth(
    global_auth_id: String,
    project_path: String,
    state: tauri::State<'_, ProjectState>,
) -> Result<SnapshotResult, CoreError> {
    tracing::info!(
        "snapshot_global_auth: global_auth_id={}, project_path={}",
        global_auth_id,
        project_path
    );

    let global_db = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database not initialized".to_string()))?;

    let source = global_db
        .get_auth_config(&global_auth_id)
        .await
        .map_err(|e| {
            tracing::error!("snapshot_global_auth: 读取全局认证配置失败: {}", e);
            e
        })?
        .ok_or_else(|| {
            let err = format!("全局认证配置 {} 不存在", global_auth_id);
            tracing::error!("snapshot_global_auth: {}", err);
            CoreError::from(err)
        })?;

    let db_manager = get_project_db_manager(&project_path, &state)
        .await
        .map_err(|e| {
            tracing::error!("snapshot_global_auth: 获取项目DB管理器失败: {}", e);
            e
        })?;
    let snapshot_id = db_manager
        .snapshot_auth_config(&source, &global_auth_id)
        .await
        .map_err(|e| {
            tracing::error!("snapshot_global_auth: 写入快照失败: {}", e);
            e
        })?;

    tracing::info!("snapshot_global_auth: 快照成功 snapshot_id={}", snapshot_id);

    Ok(SnapshotResult {
        snapshot_id,
        origin: "global_snapshot".to_string(),
        source_id: global_auth_id,
    })
}

/// 从全局网络配置快照到项目
#[tauri::command]
#[specta::specta]
pub async fn snapshot_global_network(
    global_net_id: String,
    project_path: String,
    state: tauri::State<'_, ProjectState>,
) -> Result<SnapshotResult, CoreError> {
    let global_db = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database not initialized".to_string()))?;

    let source = global_db
        .get_network_config(&global_net_id)
        .await?
        .ok_or_else(|| CoreError::from(format!("全局网络配置 {} 不存在", global_net_id)))?;

    let db_manager = get_project_db_manager(&project_path, &state).await?;
    let snapshot_id = db_manager
        .snapshot_network_config(&source, &global_net_id)
        .await?;

    Ok(SnapshotResult {
        snapshot_id,
        origin: "global_snapshot".to_string(),
        source_id: global_net_id,
    })
}
