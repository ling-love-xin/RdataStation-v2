//! Plugin Service - 插件管理服务
//!
//! 提供插件的安装、卸载、启用、禁用，以及项目级插件管理等功能。
//! 与 DriverService 风格一致，通过 GlobalDatabaseManager 处理全局存储，
//! 通过 ProjectDatabaseManager 处理项目级存储。

use shared::error::{CoreError, PluginError};
use engine::persistence::global_db::GlobalDatabaseManager;
use engine::persistence::plugin_store::{
    Plugin, PluginGlobalConfig, ProjectPluginConfig, ProjectUsedPlugin,
};
use engine::persistence::project_connection_store::ProjectConnectionStore;
use crate::events::*;
use crate::manager::get_plugin_manager;
use specta::Type;

/// 安装插件输入参数
#[derive(Debug, Clone)]
pub struct InstallPluginInput {
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

impl From<InstallPluginInput> for Plugin {
    fn from(input: InstallPluginInput) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Plugin {
            id: uuid::Uuid::new_v4().to_string(),
            code: input.code.clone(),
            name: input.name,
            version: input.version.clone(),
            author: input.author,
            description: input.description,
            repo_url: input.repo_url,
            plugin_type: input.plugin_type,
            manifest_json: input.manifest_json,
            install_path: input.install_path,
            is_enabled: true,
            is_builtin: input.is_builtin.unwrap_or(false),
            installed_at: now.clone(),
            updated_at: now,
        }
    }
}

/// 插件状态
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Type)]
pub enum PluginStatus {
    /// 未安装
    NotInstalled,
    /// 已安装但未启用
    Installed,
    /// 已启用
    Enabled,
    /// 在项目中已启用
    ProjectEnabled,
}

/// 插件信息（包含状态）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Type)]
pub struct PluginWithStatus {
    pub plugin: Plugin,
    pub status: PluginStatus,
}

/// 插件服务
pub struct PluginService {
    global_db: &'static GlobalDatabaseManager,
}

impl PluginService {
    /// 创建插件服务实例
    pub fn new(global_db: &'static GlobalDatabaseManager) -> Self {
        Self { global_db }
    }

    /// 获取所有已安装的插件
    pub async fn get_installed_plugins(&self) -> Result<Vec<Plugin>, CoreError> {
        self.global_db.get_all_plugins().await
    }

    /// 获取带有状态的所有插件
    pub async fn get_plugins_with_status(
        &self,
        project_store: Option<&ProjectConnectionStore>,
    ) -> Result<Vec<PluginWithStatus>, CoreError> {
        let plugins = self.get_installed_plugins().await?;

        let mut result = Vec::with_capacity(plugins.len());

        for plugin in plugins {
            let status = if let Some(store) = project_store {
                let project_plugins = store.project_get_plugins().await?;
                let found = project_plugins
                    .iter()
                    .find(|p| p.plugin_code == plugin.code && p.plugin_version == plugin.version);

                if let Some(p) = found {
                    if p.enabled {
                        PluginStatus::ProjectEnabled
                    } else {
                        PluginStatus::Installed
                    }
                } else if plugin.is_enabled {
                    PluginStatus::Enabled
                } else {
                    PluginStatus::Installed
                }
            } else if plugin.is_enabled {
                PluginStatus::Enabled
            } else {
                PluginStatus::Installed
            };

            result.push(PluginWithStatus { plugin, status });
        }

        Ok(result)
    }

    /// 获取单个插件
    pub async fn get_plugin(&self, plugin_id: &str) -> Result<Option<Plugin>, CoreError> {
        self.global_db.get_plugin(plugin_id).await
    }

    /// 通过 code 和 version 获取插件
    pub async fn get_plugin_by_code_version(
        &self,
        code: &str,
        version: &str,
    ) -> Result<Option<Plugin>, CoreError> {
        self.global_db
            .get_plugin_by_code_version(code, version)
            .await
    }

    /// 安装新插件
    pub async fn install_plugin(&self, input: InstallPluginInput) -> Result<Plugin, CoreError> {
        // 检查是否已存在
        let existing = self
            .get_plugin_by_code_version(&input.code, &input.version)
            .await?;
        if existing.is_some() {
            return Err(CoreError::plugin(PluginError::already_exists(
                input.code.clone(),
                input.version.clone(),
            )));
        }

        let plugin = Plugin::from(input);
        self.global_db.register_plugin(&plugin).await?;

        // 发布插件已安装事件
        emit_plugin_installed(&plugin.id, &plugin.code, &plugin.version);

        Ok(plugin)
    }

    /// 全局启用插件
    pub async fn enable_plugin(&self, plugin_id: &str) -> Result<(), CoreError> {
        // 检查插件存在
        let plugin = self.get_plugin(plugin_id).await?;
        if plugin.is_none() {
            return Err(CoreError::plugin(PluginError::not_found(
                plugin_id.to_string(),
            )));
        }

        self.global_db
            .update_plugin_enabled(plugin_id, true)
            .await?;
        emit_plugin_enabled(plugin_id);

        // 尝试激活插件（管理器未初始化时跳过 —— 宿主尚未接入，见 README D7）
        if let Some(manager) = get_plugin_manager() {
            let _ = manager.activate_plugin(plugin_id).await;
        }

        Ok(())
    }

    /// 全局禁用插件
    pub async fn disable_plugin(&self, plugin_id: &str) -> Result<(), CoreError> {
        let plugin = self.get_plugin(plugin_id).await?;
        if plugin.is_none() {
            return Err(CoreError::plugin(PluginError::not_found(
                plugin_id.to_string(),
            )));
        }

        // 先停用插件
        if let Some(manager) = get_plugin_manager() {
            let _ = manager.deactivate_plugin(plugin_id).await;
        }

        self.global_db
            .update_plugin_enabled(plugin_id, false)
            .await?;
        emit_plugin_disabled(plugin_id);

        Ok(())
    }

    /// 卸载插件
    pub async fn uninstall_plugin(&self, plugin_id: &str) -> Result<(), CoreError> {
        let plugin = self.get_plugin(plugin_id).await?;
        if plugin.is_none() {
            return Err(CoreError::plugin(PluginError::not_found(
                plugin_id.to_string(),
            )));
        }

        // 先停用和卸载
        if let Some(manager) = get_plugin_manager() {
            let _ = manager.deactivate_plugin(plugin_id).await;
            let _ = manager.unload_plugin(plugin_id).await;
        }

        self.global_db.delete_plugin(plugin_id).await?;
        emit_plugin_uninstalled(plugin_id);

        Ok(())
    }

    // ==================== 项目级插件管理 ====================

    /// 在项目中启用插件
    pub async fn enable_plugin_in_project(
        &self,
        project_store: &ProjectConnectionStore,
        plugin_code: String,
        plugin_version: String,
        required: Option<bool>,
    ) -> Result<(), CoreError> {
        // 检查全局是否有这个插件
        let global_plugin = self
            .get_plugin_by_code_version(&plugin_code, &plugin_version)
            .await?;

        if global_plugin.is_none() {
            return Err(CoreError::plugin(PluginError::not_found_by_code(
                plugin_code,
                plugin_version,
            )));
        }

        let used_plugin = ProjectUsedPlugin {
            plugin_code,
            plugin_version,
            enabled: true,
            required: required.unwrap_or(false),
        };

        project_store.project_add_plugin(&used_plugin).await?;
        Ok(())
    }

    /// 在项目中禁用插件
    pub async fn disable_plugin_in_project(
        &self,
        project_store: &ProjectConnectionStore,
        plugin_code: String,
        plugin_version: String,
    ) -> Result<(), CoreError> {
        project_store
            .project_update_plugin_enabled(&plugin_code, &plugin_version, false)
            .await?;
        Ok(())
    }

    /// 从项目中移除插件
    pub async fn remove_plugin_from_project(
        &self,
        project_store: &ProjectConnectionStore,
        plugin_code: String,
        plugin_version: String,
    ) -> Result<(), CoreError> {
        project_store
            .project_remove_plugin(&plugin_code, &plugin_version)
            .await?;
        Ok(())
    }

    /// 获取项目使用的所有插件
    pub async fn get_project_plugins(
        &self,
        project_store: &ProjectConnectionStore,
    ) -> Result<Vec<ProjectUsedPlugin>, CoreError> {
        project_store.project_get_plugins().await
    }

    /// 设置项目插件配置
    pub async fn set_project_plugin_config(
        &self,
        project_store: &ProjectConnectionStore,
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
        project_store.project_set_plugin_config(&config).await?;
        Ok(())
    }

    /// 获取项目插件配置
    pub async fn get_project_plugin_configs(
        &self,
        project_store: &ProjectConnectionStore,
        plugin_code: String,
        plugin_version: String,
    ) -> Result<Vec<ProjectPluginConfig>, CoreError> {
        project_store
            .project_get_plugin_configs(&plugin_code, &plugin_version)
            .await
    }

    // ==================== 全局配置 ====================

    /// 设置插件全局配置
    pub async fn set_global_config(
        &self,
        plugin_id: String,
        key: String,
        value: Option<String>,
    ) -> Result<(), CoreError> {
        let config = PluginGlobalConfig {
            plugin_id,
            key,
            value,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        self.global_db.set_plugin_global_config(&config).await?;
        Ok(())
    }

    /// 获取插件全局配置
    pub async fn get_global_configs(
        &self,
        plugin_id: &str,
    ) -> Result<Vec<PluginGlobalConfig>, CoreError> {
        self.global_db.get_plugin_global_configs(plugin_id).await
    }

    // ==================== 启动流程 ====================

    /// 应用启动时加载所有启用的插件
    pub async fn load_enabled_plugins_on_startup(&self) -> Result<Vec<Plugin>, CoreError> {
        let all_plugins = self.get_installed_plugins().await?;
        let enabled_plugins = all_plugins
            .into_iter()
            .filter(|p| p.is_enabled)
            .collect::<Vec<_>>();

        let Some(manager) = get_plugin_manager() else {
            tracing::info!("PluginManager 未初始化，跳过启动加载（宿主尚未接入，见 README D7）");
            return Ok(enabled_plugins);
        };

        // 扫描并加载安装目录（安装态）
        let loaded = manager.scan_and_load_plugins().await?;

        // 只激活 DB 里标记为启用的插件（按 code+version 匹配）。
        // 此前 enabled_plugins 只用于计数日志，实际激活了目录里全部插件，与 DB 启用状态无关 —— 已修正。
        let mut activated = 0;
        for loaded_plugin in &loaded {
            let matches_enabled = enabled_plugins.iter().any(|p| {
                p.code == loaded_plugin.manifest.plugin.id
                    && p.version == loaded_plugin.manifest.plugin.version
            });
            if matches_enabled && manager.activate_plugin(&loaded_plugin.id).await.is_ok() {
                activated += 1;
            }
        }

        tracing::info!(
            "Loaded {} enabled plugins on startup（实际激活 {}）",
            enabled_plugins.len(),
            activated
        );

        Ok(enabled_plugins)
    }

    /// 项目打开时加载项目指定的插件
    pub async fn load_project_plugins_on_open(
        &self,
        project_store: &ProjectConnectionStore,
    ) -> Result<Vec<Plugin>, CoreError> {
        let project_plugins = self.get_project_plugins(project_store).await?;
        let mut result = Vec::with_capacity(project_plugins.len());

        let Some(manager) = get_plugin_manager() else {
            tracing::info!("PluginManager 未初始化，跳过项目插件加载（宿主尚未接入）");
            return Ok(result);
        };

        for used_plugin in project_plugins {
            if used_plugin.enabled {
                if let Some(plugin) = self
                    .get_plugin_by_code_version(
                        &used_plugin.plugin_code,
                        &used_plugin.plugin_version,
                    )
                    .await?
                {
                    result.push(plugin.clone());

                    // 尝试加载并激活插件（install_path 来自 DB；load_plugin_from_dir
                    // 内部会过 validate_plugin_id）
                    let install_path = std::path::PathBuf::from(&plugin.install_path);
                    if install_path.exists() {
                        if let Ok(loaded) = manager.load_plugin_from_dir(&install_path).await {
                            let _ = manager.activate_plugin(&loaded.id).await;
                        }
                    }
                }
            }
        }

        tracing::info!("Loaded {} project plugins on open", result.len());

        Ok(result)
    }
}
