//! Plugin Service - 插件管理服务
//!
//! 提供插件的安装、卸载、启用、禁用，以及项目级插件管理等功能。
//! 与 DriverService 风格一致，通过 GlobalDatabaseManager 处理全局存储，
//! 通过 ProjectDatabaseManager 处理项目级存储。

use crate::events::*;
use crate::manager::get_plugin_manager;
use crate::manifest::PluginManifest;
use engine::persistence::global_db::GlobalDatabaseManager;
use engine::persistence::plugin_store::{
    Plugin, PluginDependency, PluginGlobalConfig, ProjectPluginConfig, ProjectUsedPlugin,
};
use engine::persistence::project_connection_store::ProjectConnectionStore;
use shared::error::{CoreError, PluginError};
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

    /// 登记一个已复制到 `plugins/<code>/` 的插件（只写 global.db，不碰磁盘）。
    ///
    /// 真实安装入口是 `installer::install_package`（解包 → 校验 → 复制 → 登记 → 激活）；
    /// 这里只保留给宿主未来的批量登记 / 内置插件导入用。同一 code 的再次登记按升级处理
    /// （`register_plugin` 事务内替换旧行并沿用停用状态），不再直接报 `already_exists`。
    pub async fn install_plugin(&self, input: InstallPluginInput) -> Result<Plugin, CoreError> {
        let plugin = Plugin::from(input);
        // 清单里声明的依赖跟插件行一起登记：只写 plugins 不写 plugin_dependencies，
        // 「被依赖不让卸载」的反向检查就查不到引用者。清单解析失败不在这里报——
        // 本入口只做登记，清单合法性由真实安装器 / 加载器负责。
        let deps: Vec<PluginDependency> = plugin
            .manifest_json
            .as_deref()
            .and_then(|json| serde_json::from_str::<crate::manifest::PluginManifest>(json).ok())
            .map(|manifest| {
                manifest
                    .dependencies
                    .into_iter()
                    .map(|d| PluginDependency {
                        plugin_id: plugin.id.clone(),
                        dep_code: d.id,
                        dep_version_range: d.version,
                        is_optional: false,
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.global_db
            .register_plugin_with_dependencies(&plugin, &deps)
            .await?;

        // 发布插件已安装事件
        emit_plugin_installed(&plugin.id, &plugin.code, &plugin.version);

        Ok(plugin)
    }

    /// 全局启用插件（`plugin_code` = 清单 id，运行时身份；不是安装实例 uuid）
    pub async fn enable_plugin(&self, plugin_code: &str) -> Result<(), CoreError> {
        let plugin = self.get_plugin_by_code(plugin_code).await?;
        if plugin.is_none() {
            return Err(CoreError::plugin(PluginError::not_found(
                plugin_code.to_string(),
            )));
        }

        // 与 manager 同口径：依赖未安装 / 未启用时不要把 DB 翻成启用，
        // 否则界面显示「已启用」但插件实际上缺底座。
        if let Some(json) = plugin.as_ref().and_then(|p| p.manifest_json.as_deref()) {
            let manifest: PluginManifest = serde_json::from_str(json).map_err(|e| {
                CoreError::plugin(PluginError::InvalidManifest {
                    reason: format!("插件 {plugin_code} 的清单无法解析: {e}"),
                })
            })?;
            let resolution = crate::dependency::DependencyManager::new(self.global_db)
                .resolve_dependencies(&manifest)
                .await?;
            let mut blocked: Vec<String> = resolution
                .to_install
                .iter()
                .chain(resolution.missing.iter())
                .map(|d| format!("{}@{}", d.id, d.version))
                .collect();
            blocked.sort();
            blocked.dedup();
            if !blocked.is_empty() {
                return Err(CoreError::plugin(PluginError::DependencyMissing {
                    plugin_id: plugin_code.to_string(),
                    dep_code: String::new(),
                    dep_version: format!(
                        "以下依赖未安装或未启用，请先处理: {}",
                        blocked.join(", ")
                    ),
                }));
            }
        }

        // 管理器已初始化时交给它统一处理：坏包（failed_loads）、依赖复核、写库、
        // 激活与事件都在 manager 一处，避免 service 先写库、再把激活失败吞掉。
        if let Some(manager) = get_plugin_manager() {
            return manager.enable_plugin(plugin_code).await;
        }

        // 管理器未初始化（宿主尚未接入，见 README D7）：只写全局登记。
        self.global_db
            .update_plugin_enabled_by_code(plugin_code, true)
            .await?;
        emit_plugin_enabled(plugin_code);

        Ok(())
    }

    /// 全局禁用插件（同上，按 code）
    pub async fn disable_plugin(&self, plugin_code: &str) -> Result<(), CoreError> {
        let plugin = self.get_plugin_by_code(plugin_code).await?;
        if plugin.is_none() {
            return Err(CoreError::plugin(PluginError::not_found(
                plugin_code.to_string(),
            )));
        }

        // 与 `PluginManager::disable_plugin` 同口径：service 也是权威写库入口，
        // 不能因为调用方不是面板就跳过反向依赖检查，否则依赖方会静默失效。
        let dependents = crate::dependency::DependencyManager::new(self.global_db)
            .check_dependents(plugin_code)
            .await?;
        if !dependents.is_empty() {
            return Err(CoreError::plugin(PluginError::DeactivationFailed {
                plugin_id: plugin_code.to_string(),
                reason: format!(
                    "被以下插件依赖，请先停用或卸载它们: {}",
                    dependents.join(", ")
                ),
            }));
        }

        // 先停用插件，再写库：停不下来时不要把 is_enabled 改成 0。
        // 只有登记行、没有运行实例时 deactivate 会返回 not_found，故先问 is_loaded。
        if let Some(manager) = get_plugin_manager() {
            if manager.is_loaded(plugin_code) {
                manager.deactivate_plugin(plugin_code).await?;
            }
        }

        self.global_db
            .update_plugin_enabled_by_code(plugin_code, false)
            .await?;
        emit_plugin_disabled(plugin_code);

        Ok(())
    }

    /// 按 code 查询登记
    pub async fn get_plugin_by_code(&self, plugin_code: &str) -> Result<Option<Plugin>, CoreError> {
        self.global_db.get_plugin_by_code(plugin_code).await
    }

    /// 卸载插件的登记（按 code；磁盘卸载走 `manager.uninstall_plugin`）
    pub async fn uninstall_plugin(&self, plugin_code: &str) -> Result<(), CoreError> {
        let plugin = self.get_plugin_by_code(plugin_code).await?;
        if plugin.is_none() {
            return Err(CoreError::plugin(PluginError::not_found(
                plugin_code.to_string(),
            )));
        }

        // 与磁盘卸载同口径：删登记行会让依赖方的清单引用变成孤儿，
        // 被依赖时先拒绝，别等服务层把 plugins 表里的行删掉才暴露。
        let dependents = crate::dependency::DependencyManager::new(self.global_db)
            .check_dependents(plugin_code)
            .await?;
        if !dependents.is_empty() {
            return Err(CoreError::plugin(PluginError::UninstallFailed {
                plugin_id: plugin_code.to_string(),
                reason: format!("被以下插件依赖，请先卸载它们: {}", dependents.join(", ")),
            }));
        }

        // 先卸运行时，再删登记：卸载失败时不要把登记行删掉，留下“进程还在、
        // 管理页却看不见”的孤儿插件。unload 内部已经会 deactivate。
        if let Some(manager) = get_plugin_manager() {
            if manager.is_loaded(plugin_code) {
                manager.unload_plugin(plugin_code).await?;
            }
        }

        self.global_db.delete_plugin_by_code(plugin_code).await?;
        emit_plugin_uninstalled(plugin_code);

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
