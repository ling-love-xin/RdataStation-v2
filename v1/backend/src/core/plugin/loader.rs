//! 插件加载器
//!
//! 负责从文件系统加载插件包，解析清单，注册到系统

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

use crate::core::error::{CommonError, CoreError};
use crate::core::plugin::manager::get_plugin_manager;
use crate::core::plugin::manifest::{ManifestParser, PluginManifest};

/// 插件加载器
pub struct PluginLoader {
    /// 插件安装目录
    install_dir: PathBuf,
    /// 加载的插件缓存
    loaded_plugins: RwLock<std::collections::HashMap<String, LoadedPlugin>>,
}

/// 已加载的插件
#[derive(Debug, Clone)]
pub struct LoadedPlugin {
    /// 插件 ID
    pub id: String,
    /// 插件清单
    pub manifest: PluginManifest,
    /// 插件安装路径
    pub install_path: PathBuf,
    /// 加载状态
    pub status: LoadStatus,
}

/// 加载状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LoadStatus {
    /// 未加载
    #[default]
    Unloaded,
    /// 加载中
    Loading,
    /// 已加载
    Loaded,
    /// 激活中
    Activating,
    /// 已激活
    Active,
    /// 加载失败
    LoadFailed(String),
    /// 激活失败
    ActivationFailed(String),
}

impl PluginLoader {
    /// 创建新的插件加载器
    pub fn new(install_dir: PathBuf) -> Self {
        Self {
            install_dir,
            loaded_plugins: RwLock::new(std::collections::HashMap::new()),
        }
    }

    /// 初始化插件目录
    pub async fn init(&self) -> Result<(), CoreError> {
        if !self.install_dir.exists() {
            std::fs::create_dir_all(&self.install_dir).map_err(|e| {
                CoreError::common(CommonError::general(format!(
                    "Failed to create plugin directory: {}",
                    e
                )))
            })?;
        }
        Ok(())
    }

    /// 从目录扫描并加载插件
    pub async fn scan_and_load_plugins(&self) -> Result<Vec<LoadedPlugin>, CoreError> {
        let mut loaded = Vec::new();

        if !self.install_dir.exists() {
            return Ok(loaded);
        }

        let entries = std::fs::read_dir(&self.install_dir).map_err(|e| {
            CoreError::common(CommonError::general(format!(
                "Failed to read plugin directory: {}",
                e
            )))
        })?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Ok(plugin) = self.load_plugin_from_dir(&path).await {
                    loaded.push(plugin);
                }
            }
        }

        Ok(loaded)
    }

    /// 从目录加载单个插件
    pub async fn load_plugin_from_dir(&self, plugin_dir: &Path) -> Result<LoadedPlugin, CoreError> {
        let manifest_path = plugin_dir.join("plugin.toml");
        if !manifest_path.exists() {
            return Err(CoreError::common(CommonError::general(format!(
                "Plugin manifest not found at {}",
                manifest_path.display()
            ))));
        }

        let manifest = ManifestParser::parse(&manifest_path)?;

        let plugin_id = manifest.plugin.id.clone();
        let loaded_plugin = LoadedPlugin {
            id: plugin_id.clone(),
            manifest,
            install_path: plugin_dir.to_path_buf(),
            status: LoadStatus::Loaded,
        };

        self.loaded_plugins
            .write()
            .map_err(|e| {
                CoreError::common(CommonError::general(format!(
                    "Failed to lock plugin map: {}",
                    e
                )))
            })?
            .insert(plugin_id, loaded_plugin.clone());

        Ok(loaded_plugin)
    }

    /// 获取已加载的插件
    pub fn get_loaded_plugin(&self, plugin_id: &str) -> Option<LoadedPlugin> {
        self.loaded_plugins.read().ok()?.get(plugin_id).cloned()
    }

    /// 获取所有已加载的插件
    pub fn list_loaded_plugins(&self) -> Vec<LoadedPlugin> {
        self.loaded_plugins
            .read()
            .map(|map| map.values().cloned().collect())
            .unwrap_or_default()
    }

    /// 激活插件
    pub async fn activate_plugin(&self, plugin_id: &str) -> Result<(), CoreError> {
        let mut plugin_map = self.loaded_plugins.write().map_err(|e| {
            CoreError::common(CommonError::general(format!(
                "Failed to lock plugin map: {}",
                e
            )))
        })?;

        let plugin = plugin_map.get_mut(plugin_id).ok_or_else(|| {
            CoreError::common(CommonError::general(format!(
                "Plugin {} not found",
                plugin_id
            )))
        })?;

        plugin.status = LoadStatus::Activating;

        let manager = get_plugin_manager().ok_or_else(|| {
            CoreError::common(CommonError::general("Plugin manager not initialized"))
        })?;

        // 根据插件类型加载
        if let Some(wasm) = &plugin.manifest.capabilities.wasm {
            // 加载 WASM 插件
            let wasm_path = plugin.install_path.join(&wasm.entry);
            if let Err(e) = manager.load_plugin(plugin_id, &wasm_path) {
                plugin.status = LoadStatus::ActivationFailed(e.to_string());
                return Err(CoreError::common(CommonError::general(format!(
                    "Failed to activate WASM plugin: {}",
                    e
                ))));
            }
        }

        if plugin.manifest.capabilities.frontend.is_some() {
            // 前端插件加载 - 前端处理
            tracing::info!("Frontend plugin {} detected", plugin_id);
        }

        plugin.status = LoadStatus::Active;

        // 触发激活事件
        crate::core::plugin::events::emit_plugin_activated(plugin_id);

        Ok(())
    }

    /// 停用插件
    pub async fn deactivate_plugin(&self, plugin_id: &str) -> Result<(), CoreError> {
        let mut plugin_map = self.loaded_plugins.write().map_err(|e| {
            CoreError::common(CommonError::general(format!(
                "Failed to lock plugin map: {}",
                e
            )))
        })?;

        let plugin = plugin_map.get_mut(plugin_id).ok_or_else(|| {
            CoreError::common(CommonError::general(format!(
                "Plugin {} not found",
                plugin_id
            )))
        })?;

        plugin.status = LoadStatus::Loaded;

        let manager = get_plugin_manager().ok_or_else(|| {
            CoreError::common(CommonError::general("Plugin manager not initialized"))
        })?;

        manager.deactivate_plugin(plugin_id)?;

        crate::core::plugin::events::emit_plugin_deactivated(plugin_id);

        Ok(())
    }

    /// 卸载插件
    pub async fn unload_plugin(&self, plugin_id: &str) -> Result<(), CoreError> {
        let mut plugin_map = self.loaded_plugins.write().map_err(|e| {
            CoreError::common(CommonError::general(format!(
                "Failed to lock plugin map: {}",
                e
            )))
        })?;

        if plugin_map.remove(plugin_id).is_some() {
            let manager = get_plugin_manager().ok_or_else(|| {
                CoreError::common(CommonError::general("Plugin manager not initialized"))
            })?;

            let _ = manager.unload_plugin(plugin_id);

            crate::core::plugin::events::emit_plugin_unloaded(plugin_id);
        }

        Ok(())
    }
}

/// 全局插件加载器实例
static PLUGIN_LOADER: std::sync::OnceLock<Arc<PluginLoader>> = std::sync::OnceLock::new();

/// 获取全局插件加载器
pub fn get_plugin_loader() -> Result<Arc<PluginLoader>, CoreError> {
    PLUGIN_LOADER.get().cloned().ok_or_else(|| {
        CoreError::common(CommonError::Internal(
            "Plugin loader not initialized".to_string(),
        ))
    })
}

/// 初始化全局插件加载器
pub async fn init_plugin_loader(install_dir: PathBuf) -> Result<(), CoreError> {
    let loader = Arc::new(PluginLoader::new(install_dir));
    loader.init().await?;

    PLUGIN_LOADER.set(loader).map_err(|_| {
        CoreError::common(CommonError::general("Plugin loader already initialized"))
    })?;

    Ok(())
}
