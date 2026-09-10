use std::collections::HashMap;

use extism::{Manifest, Plugin, Wasm};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

use super::{PluginMetadata, PluginType};
use crate::core::error::{CommonError, CoreError};

/// 插件状态枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PluginState {
    /// 已加载但未激活
    Loaded,
    /// 已激活，正在运行
    Active,
    /// 已停用
    Inactive,
    /// 发生错误
    Error,
}

/// 插件实例包装
struct ManagedPlugin {
    plugin: Option<Plugin>,
    metadata: PluginMetadata,
    state: PluginState,
    config: HashMap<String, String>,
    env_vars: HashMap<String, String>,
}

impl ManagedPlugin {
    fn new(plugin: Plugin, metadata: PluginMetadata) -> Self {
        Self {
            plugin: Some(plugin),
            metadata,
            state: PluginState::Loaded,
            config: HashMap::new(),
            env_vars: HashMap::new(),
        }
    }
}

/// Extism 插件管理器增强版
pub struct ExtismPluginManager {
    plugins: Arc<RwLock<HashMap<String, ManagedPlugin>>>,
}

impl Default for ExtismPluginManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ExtismPluginManager {
    /// 创建新的插件管理器
    pub fn new() -> Self {
        Self {
            plugins: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 加载插件（增强版）
    pub fn load_plugin(
        &self,
        id: &str,
        wasm_bytes: &[u8],
        config: Option<HashMap<String, String>>,
        env_vars: Option<HashMap<String, String>>,
    ) -> Result<PluginMetadata, CoreError> {
        let mut manifest = Manifest::new([Wasm::data(wasm_bytes.to_vec())]);

        // 应用配置（如果有）
        if let Some(config) = &config {
            for (key, value) in config {
                manifest = manifest.with_config([(key.clone(), value.clone())].into_iter());
            }
        }

        let plugin = Plugin::new(&manifest, [], true).map_err(|e| {
            CoreError::common(CommonError::Internal(format!(
                "Failed to load plugin {}: {}",
                id, e
            )))
        })?;

        let metadata = PluginMetadata {
            id: id.to_string(),
            name: id.to_string(),
            version: "0.1.0".to_string(),
            plugin_type: PluginType::Tool,
            description: String::new(),
            author: String::new(),
            entry_point: "main".to_string(),
        };

        let mut managed = ManagedPlugin::new(plugin, metadata.clone());
        managed.config = config.unwrap_or_default();
        managed.env_vars = env_vars.unwrap_or_default();

        self.plugins
            .write()
            .map_err(|_| {
                CoreError::common(CommonError::Internal("Failed to acquire lock".to_string()))
            })?
            .insert(id.to_string(), managed);

        Ok(metadata)
    }

    /// 激活插件
    pub fn activate_plugin(&self, id: &str) -> Result<(), CoreError> {
        let mut plugins = self.plugins.write().map_err(|_| {
            CoreError::common(CommonError::Internal("Failed to acquire lock".to_string()))
        })?;

        let plugin = plugins.get_mut(id).ok_or_else(|| {
            CoreError::common(CommonError::Internal(format!("Plugin {} not found", id)))
        })?;

        if plugin.state == PluginState::Active {
            return Ok(());
        }

        // 尝试调用插件的 activate 钩子
        if let Some(plugin_instance) = plugin.plugin.as_mut() {
            let _: Result<Vec<u8>, _> = plugin_instance.call("activate", &b""[..]);
        }

        plugin.state = PluginState::Active;
        Ok(())
    }

    /// 停用插件
    pub fn deactivate_plugin(&self, id: &str) -> Result<(), CoreError> {
        let mut plugins = self.plugins.write().map_err(|_| {
            CoreError::common(CommonError::Internal("Failed to acquire lock".to_string()))
        })?;

        let plugin = plugins.get_mut(id).ok_or_else(|| {
            CoreError::common(CommonError::Internal(format!("Plugin {} not found", id)))
        })?;

        if plugin.state != PluginState::Active && plugin.state != PluginState::Error {
            return Ok(());
        }

        // 尝试调用插件的 deactivate 钩子
        if let Some(plugin_instance) = plugin.plugin.as_mut() {
            let _: Result<Vec<u8>, _> = plugin_instance.call("deactivate", &b""[..]);
        }

        plugin.state = PluginState::Inactive;
        Ok(())
    }

    /// 热更新插件
    pub fn hot_reload_plugin(
        &self,
        id: &str,
        new_wasm: &[u8],
    ) -> Result<PluginMetadata, CoreError> {
        let mut plugins = self.plugins.write().map_err(|_| {
            CoreError::common(CommonError::Internal("Failed to acquire lock".to_string()))
        })?;

        let old_plugin = plugins.remove(id).ok_or_else(|| {
            CoreError::common(CommonError::Internal(format!("Plugin {} not found", id)))
        })?;

        let was_activated = old_plugin.state == PluginState::Active;

        let new_metadata = self.load_plugin(
            id,
            new_wasm,
            Some(old_plugin.config.clone()),
            Some(old_plugin.env_vars.clone()),
        )?;

        if was_activated {
            self.activate_plugin(id)?;
        }

        Ok(new_metadata)
    }

    /// 卸载插件
    pub fn unload_plugin(&self, id: &str) -> Result<(), CoreError> {
        self.deactivate_plugin(id)?;

        self.plugins
            .write()
            .map_err(|_| {
                CoreError::common(CommonError::Internal("Failed to acquire lock".to_string()))
            })?
            .remove(id);

        Ok(())
    }

    /// 列出所有插件
    pub fn list_plugins(&self) -> Result<Vec<(PluginMetadata, PluginState)>, CoreError> {
        let plugins = self.plugins.read().map_err(|_| {
            CoreError::common(CommonError::Internal("Failed to acquire lock".to_string()))
        })?;
        Ok(plugins
            .values()
            .map(|p| (p.metadata.clone(), p.state))
            .collect())
    }

    /// 获取插件状态
    pub fn get_plugin_state(&self, id: &str) -> Result<PluginState, CoreError> {
        let plugins = self.plugins.read().map_err(|_| {
            CoreError::common(CommonError::Internal("Failed to acquire lock".to_string()))
        })?;

        let plugin = plugins.get(id).ok_or_else(|| {
            CoreError::common(CommonError::Internal(format!("Plugin {} not found", id)))
        })?;

        Ok(plugin.state)
    }

    /// 调用插件函数
    pub fn call_plugin(&self, id: &str, func: &str, input: &[u8]) -> Result<Vec<u8>, CoreError> {
        let mut plugins = self.plugins.write().map_err(|_| {
            CoreError::common(CommonError::Internal("Failed to acquire lock".to_string()))
        })?;

        let plugin = plugins.get_mut(id).ok_or_else(|| {
            CoreError::common(CommonError::Internal(format!("Plugin {} not found", id)))
        })?;

        if plugin.state != PluginState::Active {
            return Err(CoreError::common(CommonError::Internal(format!(
                "Plugin {} is not active",
                id
            ))));
        }

        let plugin_instance = plugin.plugin.as_mut().ok_or_else(|| {
            CoreError::common(CommonError::Internal("Plugin not initialized".to_string()))
        })?;

        plugin_instance.call(func, input).map_err(|e| {
            plugin.state = PluginState::Error;
            CoreError::common(CommonError::Internal(format!("Plugin call failed: {}", e)))
        })
    }

    /// 更新插件配置
    pub fn update_plugin_config(
        &self,
        id: &str,
        config: HashMap<String, String>,
    ) -> Result<(), CoreError> {
        let mut plugins = self.plugins.write().map_err(|_| {
            CoreError::common(CommonError::Internal("Failed to acquire lock".to_string()))
        })?;

        let plugin = plugins.get_mut(id).ok_or_else(|| {
            CoreError::common(CommonError::Internal(format!("Plugin {} not found", id)))
        })?;

        plugin.config = config;
        Ok(())
    }
}
