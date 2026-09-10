//! 插件相关命令
//!
//! 处理插件的安装、卸载、项目级管理等操作

use crate::core::error::CoreError;
use crate::core::migration::get_global_db_manager;
use crate::core::persistence::plugin_store::{Plugin, ProjectPluginConfig, ProjectUsedPlugin};
use crate::core::persistence::project_connection_store::ProjectConnectionStore;
use crate::core::plugin::manager::{PluginInfo, PluginManager};
use crate::core::services::plugin_bridge::get_plugin_bridge;
use crate::core::services::plugin_service::{InstallPluginInput, PluginService, PluginWithStatus};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Arc;
use tauri::State;

#[derive(Debug, Serialize, Deserialize, Type)]
pub struct PluginStatus {
    pub plugin_id: String,
    pub name: String,
    pub version: String,
    pub plugin_type: String,
    pub status: String,
    #[specta(skip)]
    pub config: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Type)]
pub struct InstallPluginRequest {
    pub code: String,
    pub name: String,
    pub version: String,
    pub author: Option<String>,
    pub description: Option<String>,
    pub repo_url: Option<String>,
    pub plugin_type: String,
    pub manifest_json: Option<String>,
    pub install_path: String,
    pub is_builtin: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Type)]
pub struct EnablePluginInProjectRequest {
    pub plugin_code: String,
    pub plugin_version: String,
    pub required: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Type)]
pub struct LoadPluginRequest {
    pub plugin_id: String,
    pub plugin_path: String,
}

// 项目状态包装，便于命令访问
pub(crate) use crate::commands::project_commands::ProjectState;

// ==================== 通用插件命令 ====================

#[tauri::command]
pub async fn plugin_db_query(
    plugin_id: String,
    conn_id: String,
    sql: String,
    timeout: Option<u64>,
) -> Result<crate::api::dto::QueryResult, CoreError> {
    let _ = timeout;
    let bridge = get_plugin_bridge();
    let result = bridge.query(&plugin_id, &conn_id, &sql).await?;
    Ok(result)
}

#[tauri::command]
pub async fn plugin_db_metadata(
    plugin_id: String,
    conn_id: String,
    catalog: String,
    schema: String,
    kind: String,
) -> Result<serde_json::Value, CoreError> {
    let bridge = get_plugin_bridge();
    let result = bridge
        .metadata(&plugin_id, &conn_id, &catalog, &schema, &kind)
        .await?;
    Ok(result)
}

#[tauri::command]
pub fn plugin_list(
    plugin_manager: State<'_, Arc<PluginManager>>,
) -> Result<Vec<PluginInfo>, CoreError> {
    plugin_manager.list_plugins()
}

#[tauri::command]
pub fn plugin_load(
    plugin_manager: State<'_, Arc<PluginManager>>,
    plugin_id: String,
    plugin_path: String,
) -> Result<PluginInfo, CoreError> {
    use std::path::PathBuf;
    let path = PathBuf::from(plugin_path);
    let info = plugin_manager.load_plugin(&plugin_id, &path)?;
    Ok(info)
}

#[tauri::command]
pub fn plugin_activate(
    plugin_manager: State<'_, Arc<PluginManager>>,
    plugin_id: String,
) -> Result<(), CoreError> {
    plugin_manager.activate_plugin(&plugin_id)?;
    Ok(())
}

#[tauri::command]
pub fn plugin_deactivate(
    plugin_manager: State<'_, Arc<PluginManager>>,
    plugin_id: String,
) -> Result<(), CoreError> {
    plugin_manager.deactivate_plugin(&plugin_id)?;
    Ok(())
}

#[tauri::command]
pub fn plugin_unload(
    plugin_manager: State<'_, Arc<PluginManager>>,
    plugin_id: String,
) -> Result<(), CoreError> {
    plugin_manager.unload_plugin(&plugin_id)?;
    Ok(())
}

#[tauri::command]
pub fn plugin_get_status(
    plugin_manager: State<'_, Arc<PluginManager>>,
    plugin_id: String,
) -> Result<PluginStatus, CoreError> {
    let plugins = plugin_manager.list_plugins()?;
    let plugin = plugins
        .into_iter()
        .find(|p| p.id == plugin_id)
        .ok_or_else(|| CoreError::from("Plugin not found"))?;

    Ok(PluginStatus {
        plugin_id: plugin.id,
        name: plugin.name,
        version: plugin.version,
        plugin_type: format!("{:?}", plugin.kind),
        status: format!("{:?}", plugin.state),
        config: None,
    })
}

#[tauri::command]
pub fn plugin_add_directory(
    plugin_manager: State<'_, Arc<PluginManager>>,
    directory: String,
) -> Result<Vec<PluginInfo>, CoreError> {
    use std::path::PathBuf;
    let path = PathBuf::from(&directory);
    plugin_manager.add_plugin_dir(path);
    let discovered = plugin_manager.scan_plugins()?;
    Ok(discovered)
}

// ==================== 插件安装管理 ====================

#[tauri::command]
pub async fn plugin_get_all_installed() -> Result<Vec<Plugin>, CoreError> {
    let db_manager = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database manager not available"))?;
    let service = PluginService::new(db_manager);
    service.get_installed_plugins().await
}

#[tauri::command]
pub async fn plugin_get_with_status(
    project_state: State<'_, ProjectState>,
) -> Result<Vec<PluginWithStatus>, CoreError> {
    let db_manager = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database manager not available"))?;
    let service = PluginService::new(db_manager);

    let store = project_state.store.lock().await;
    let project_db = store.as_ref().map(|s| s.project_db());

    match project_db {
        Some(db) => {
            let conn_store = ProjectConnectionStore::new(db);
            service.get_plugins_with_status(Some(&conn_store)).await
        }
        None => service.get_plugins_with_status(None).await,
    }
}

#[tauri::command]
pub async fn plugin_get(plugin_id: String) -> Result<Option<Plugin>, CoreError> {
    let db_manager = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database manager not available"))?;
    let service = PluginService::new(db_manager);
    service.get_plugin(&plugin_id).await
}

#[tauri::command]
pub async fn plugin_install(request: InstallPluginRequest) -> Result<Plugin, CoreError> {
    let db_manager = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database manager not available"))?;
    let service = PluginService::new(db_manager);
    service
        .install_plugin(InstallPluginInput {
            code: request.code,
            name: request.name,
            version: request.version,
            author: request.author,
            description: request.description,
            repo_url: request.repo_url,
            plugin_type: request.plugin_type,
            manifest_json: request.manifest_json,
            install_path: request.install_path,
            is_builtin: request.is_builtin,
        })
        .await
}

#[tauri::command]
pub async fn plugin_enable(plugin_id: String) -> Result<(), CoreError> {
    let db_manager = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database manager not available"))?;
    let service = PluginService::new(db_manager);
    service.enable_plugin(&plugin_id).await
}

#[tauri::command]
pub async fn plugin_disable(plugin_id: String) -> Result<(), CoreError> {
    let db_manager = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database manager not available"))?;
    let service = PluginService::new(db_manager);
    service.disable_plugin(&plugin_id).await
}

#[tauri::command]
pub async fn plugin_uninstall(plugin_id: String) -> Result<(), CoreError> {
    let db_manager = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database manager not available"))?;
    let service = PluginService::new(db_manager);
    service.uninstall_plugin(&plugin_id).await
}

// ==================== 项目插件管理 ====================

#[tauri::command]
pub async fn project_plugin_enable(
    project_state: State<'_, ProjectState>,
    request: EnablePluginInProjectRequest,
) -> Result<(), CoreError> {
    let db_manager = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database manager not available"))?;
    let service = PluginService::new(db_manager);

    let store = project_state.store.lock().await;
    let project_db = store
        .as_ref()
        .map(|s| s.project_db())
        .ok_or_else(|| CoreError::from("Project not open"))?;
    let conn_store = ProjectConnectionStore::new(project_db);

    service
        .enable_plugin_in_project(
            &conn_store,
            request.plugin_code,
            request.plugin_version,
            request.required,
        )
        .await
}

#[tauri::command]
pub async fn project_plugin_disable(
    project_state: State<'_, ProjectState>,
    plugin_code: String,
    plugin_version: String,
) -> Result<(), CoreError> {
    let db_manager = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database manager not available"))?;
    let service = PluginService::new(db_manager);

    let store = project_state.store.lock().await;
    let project_db = store
        .as_ref()
        .map(|s| s.project_db())
        .ok_or_else(|| CoreError::from("Project not open"))?;
    let conn_store = ProjectConnectionStore::new(project_db);

    service
        .disable_plugin_in_project(&conn_store, plugin_code, plugin_version)
        .await
}

#[tauri::command]
pub async fn project_plugin_remove(
    project_state: State<'_, ProjectState>,
    plugin_code: String,
    plugin_version: String,
) -> Result<(), CoreError> {
    let store = project_state.store.lock().await;
    let project_db = store
        .as_ref()
        .map(|s| s.project_db())
        .ok_or_else(|| CoreError::from("Project not open"))?;
    let conn_store = ProjectConnectionStore::new(project_db);

    conn_store
        .project_remove_plugin(&plugin_code, &plugin_version)
        .await?;
    Ok(())
}

#[tauri::command]
pub async fn project_plugin_list(
    project_state: State<'_, ProjectState>,
) -> Result<Vec<ProjectUsedPlugin>, CoreError> {
    let store = project_state.store.lock().await;
    let project_db = store
        .as_ref()
        .map(|s| s.project_db())
        .ok_or_else(|| CoreError::from("Project not open"))?;
    let conn_store = ProjectConnectionStore::new(project_db);

    conn_store.project_get_plugins().await
}

#[tauri::command]
pub async fn project_plugin_set_config(
    project_state: State<'_, ProjectState>,
    plugin_code: String,
    plugin_version: String,
    key: String,
    value: Option<String>,
) -> Result<(), CoreError> {
    let config = ProjectPluginConfig {
        plugin_code,
        plugin_version,
        key,
        value,
        updated_at: chrono::Utc::now().to_rfc3339(),
    };

    let store = project_state.store.lock().await;
    let project_db = store
        .as_ref()
        .map(|s| s.project_db())
        .ok_or_else(|| CoreError::from("Project not open"))?;
    let conn_store = ProjectConnectionStore::new(project_db);

    conn_store.project_set_plugin_config(&config).await?;
    Ok(())
}

#[tauri::command]
pub async fn project_plugin_get_configs(
    project_state: State<'_, ProjectState>,
    plugin_code: String,
    plugin_version: String,
) -> Result<Vec<ProjectPluginConfig>, CoreError> {
    let store = project_state.store.lock().await;
    let project_db = store
        .as_ref()
        .map(|s| s.project_db())
        .ok_or_else(|| CoreError::from("Project not open"))?;
    let conn_store = ProjectConnectionStore::new(project_db);

    conn_store
        .project_get_plugin_configs(&plugin_code, &plugin_version)
        .await
}

// ==================== 启动相关 ====================

#[tauri::command]
pub async fn plugin_load_enabled_on_startup() -> Result<Vec<Plugin>, CoreError> {
    let db_manager = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database manager not available"))?;
    let service = PluginService::new(db_manager);
    service.load_enabled_plugins_on_startup().await
}
