use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

use super::manifest::*;
use crate::adapters::wasm::extism::PluginState;
use crate::adapters::wasm::{ExtismPluginManager, PluginMetadata};
use crate::core::error::{CommonError, CoreError};

/// 插件类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PluginKind {
    /// WASM 插件
    Wasm,
    /// Go Sidecar 插件
    Sidecar,
}

/// 插件信息
#[derive(Debug, Clone, Serialize)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: PluginKind,
    pub metadata: PluginMetadata,
    pub state: PluginState,
}

/// 插件管理器核心
pub struct PluginManager {
    wasm_manager: Arc<ExtismPluginManager>,
    plugin_index: Arc<RwLock<HashMap<String, PluginKind>>>,
    plugin_dirs: RwLock<Vec<PathBuf>>,
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginManager {
    /// 创建新的插件管理器
    pub fn new() -> Self {
        Self {
            wasm_manager: Arc::new(ExtismPluginManager::new()),
            plugin_index: Arc::new(RwLock::new(HashMap::new())),
            plugin_dirs: RwLock::new(Vec::new()),
        }
    }

    /// 添加插件目录
    pub fn add_plugin_dir(&self, path: PathBuf) {
        if let Ok(mut dirs) = self.plugin_dirs.write() {
            dirs.push(path);
        }
    }

    /// 扫描并发现插件
    pub fn scan_plugins(&self) -> Result<Vec<PluginInfo>, CoreError> {
        let mut discovered_plugins = Vec::new();

        let dirs = self.plugin_dirs.read().map_err(|_| {
            CoreError::common(CommonError::Internal(
                "Failed to acquire lock on plugin_dirs".to_string(),
            ))
        })?;

        for dir in dirs.iter() {
            if !dir.exists() {
                continue;
            }

            let entries = std::fs::read_dir(dir).map_err(|e| {
                CoreError::common(CommonError::Internal(format!(
                    "Failed to read plugin dir: {}",
                    e
                )))
            })?;

            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(info) = self.try_discover_plugin(&path)? {
                    discovered_plugins.push(info);
                }
            }
        }

        Ok(discovered_plugins)
    }

    /// 尝试发现单个插件
    fn try_discover_plugin(&self, path: &PathBuf) -> Result<Option<PluginInfo>, CoreError> {
        let manifest_path = path.join("plugin.toml");

        if !manifest_path.exists() {
            return Ok(None);
        }

        let manifest_str = std::fs::read_to_string(&manifest_path).map_err(|e| {
            CoreError::common(CommonError::Internal(format!(
                "Failed to read manifest: {}",
                e
            )))
        })?;

        let manifest: PluginManifest = toml::from_str(&manifest_str).map_err(|e| {
            CoreError::common(CommonError::Internal(format!(
                "Failed to parse manifest: {}",
                e
            )))
        })?;

        let kind = self.detect_plugin_kind(&manifest, path)?;
        let state = PluginState::Inactive;

        Ok(Some(PluginInfo {
            id: manifest.plugin.id.clone(),
            name: manifest.plugin.name.clone(),
            version: manifest.plugin.version.clone(),
            kind,
            metadata: PluginMetadata {
                id: manifest.plugin.id.clone(),
                name: manifest.plugin.name.clone(),
                version: manifest.plugin.version.clone(),
                plugin_type: if manifest.capabilities.wasm.is_some() {
                    crate::adapters::wasm::PluginType::Tool
                } else {
                    crate::adapters::wasm::PluginType::Analytics
                },
                description: manifest.plugin.description.clone(),
                author: manifest.plugin.publisher.clone(),
                entry_point: String::new(),
            },
            state,
        }))
    }

    /// 检测插件类型
    fn detect_plugin_kind(
        &self,
        manifest: &PluginManifest,
        path: &PathBuf,
    ) -> Result<PluginKind, CoreError> {
        // 根据 manifest capabilities 判断
        if manifest.capabilities.wasm.is_some() {
            return Ok(PluginKind::Wasm);
        }
        if manifest.capabilities.frontend.is_some() {
            return Ok(PluginKind::Sidecar);
        }

        // 自动检测：检查文件系统中的文件
        let _ = path;
        Err(CoreError::common(CommonError::Internal(format!(
            "Could not detect plugin type for {}",
            path.display()
        ))))
    }

    /// 加载插件
    pub fn load_plugin(&self, id: &str, path: &PathBuf) -> Result<PluginInfo, CoreError> {
        let info = self.try_discover_plugin(path)?.ok_or_else(|| {
            CoreError::common(CommonError::Internal(
                "Plugin not found or invalid".to_string(),
            ))
        })?;

        match info.kind {
            PluginKind::Wasm => self.load_wasm_plugin(id, path)?,
            PluginKind::Sidecar => self.load_sidecar_plugin(id, path)?,
        }

        self.plugin_index
            .write()
            .map_err(|_| {
                CoreError::common(CommonError::Internal("Failed to acquire lock".to_string()))
            })?
            .insert(id.to_string(), info.kind);

        Ok(info)
    }

    /// 加载 WASM 插件
    fn load_wasm_plugin(&self, id: &str, path: &Path) -> Result<(), CoreError> {
        let wasm_path = path.join("plugin.wasm");
        let wasm_bytes = std::fs::read(wasm_path).map_err(|e| {
            CoreError::common(CommonError::Internal(format!("Failed to read wasm: {}", e)))
        })?;

        let manifest_path = path.join("plugin.toml");
        let manifest_str = std::fs::read_to_string(&manifest_path).map_err(|e| {
            CoreError::common(CommonError::Internal(format!(
                "Failed to read manifest: {}",
                e
            )))
        })?;

        let _manifest: PluginManifest = toml::from_str(&manifest_str).map_err(|e| {
            CoreError::common(CommonError::Internal(format!(
                "Failed to parse manifest: {}",
                e
            )))
        })?;

        self.wasm_manager.load_plugin(
            id,
            &wasm_bytes,
            Some(std::collections::HashMap::new()),
            None,
        )?;
        Ok(())
    }

    /// 加载 Sidecar 插件（占位实现）
    fn load_sidecar_plugin(&self, _id: &str, _path: &PathBuf) -> Result<(), CoreError> {
        tracing::warn!("Sidecar plugin loading is not yet implemented");
        Ok(())
    }

    /// 激活插件
    pub fn activate_plugin(&self, id: &str) -> Result<(), CoreError> {
        let index = self.plugin_index.read().map_err(|_| {
            CoreError::common(CommonError::Internal("Failed to acquire lock".to_string()))
        })?;

        let kind = index.get(id).ok_or_else(|| {
            CoreError::common(CommonError::Internal(format!("Plugin {} not found", id)))
        })?;

        match kind {
            PluginKind::Wasm => self.wasm_manager.activate_plugin(id),
            PluginKind::Sidecar => Ok(()),
        }
    }

    /// 停用插件
    pub fn deactivate_plugin(&self, id: &str) -> Result<(), CoreError> {
        let index = self.plugin_index.read().map_err(|_| {
            CoreError::common(CommonError::Internal("Failed to acquire lock".to_string()))
        })?;

        let kind = index.get(id).ok_or_else(|| {
            CoreError::common(CommonError::Internal(format!("Plugin {} not found", id)))
        })?;

        match kind {
            PluginKind::Wasm => self.wasm_manager.deactivate_plugin(id),
            PluginKind::Sidecar => Ok(()),
        }
    }

    /// 卸载插件
    pub fn unload_plugin(&self, id: &str) -> Result<(), CoreError> {
        let index = self.plugin_index.read().map_err(|_| {
            CoreError::common(CommonError::Internal("Failed to acquire lock".to_string()))
        })?;

        let kind = index.get(id).ok_or_else(|| {
            CoreError::common(CommonError::Internal(format!("Plugin {} not found", id)))
        })?;

        match kind {
            PluginKind::Wasm => self.wasm_manager.unload_plugin(id)?,
            PluginKind::Sidecar => { /* no-op */ }
        }

        self.plugin_index
            .write()
            .map_err(|_| {
                CoreError::common(CommonError::Internal("Failed to acquire lock".to_string()))
            })?
            .remove(id);

        Ok(())
    }

    /// 列出所有已加载插件
    pub fn list_plugins(&self) -> Result<Vec<PluginInfo>, CoreError> {
        let wasm_plugins: Vec<_> = self
            .wasm_manager
            .list_plugins()
            .map_err(|e| {
                CoreError::common(CommonError::Internal(format!(
                    "Failed to list plugins: {}",
                    e
                )))
            })?
            .into_iter()
            .map(|(meta, state)| PluginInfo {
                id: meta.id.clone(),
                name: meta.name.clone(),
                version: meta.version.clone(),
                kind: PluginKind::Wasm,
                metadata: meta,
                state,
            })
            .collect();

        // TODO: Add Sidecar plugins
        Ok(wasm_plugins)
    }

    /// 获取 WASM 管理器引用
    pub fn wasm_manager(&self) -> Arc<ExtismPluginManager> {
        Arc::clone(&self.wasm_manager)
    }
}

/// 全局插件管理器实例
pub static PLUGIN_MANAGER: OnceLock<Arc<PluginManager>> = OnceLock::new();

/// 获取全局插件管理器实例
///
/// 如果尚未初始化，将返回 None。必须先调用 init_plugin_manager() 进行初始化。
pub fn get_plugin_manager() -> Option<&'static Arc<PluginManager>> {
    PLUGIN_MANAGER.get()
}

/// 初始化全局插件管理器
///
/// 必须在应用启动时调用此函数，且只能调用一次。
pub async fn init_plugin_manager() -> Result<(), CoreError> {
    let manager = Arc::new(PluginManager::new());

    PLUGIN_MANAGER.set(manager).map_err(|_| {
        CoreError::common(crate::core::error::CommonError::General(
            "Plugin manager already initialized".to_string(),
        ))
    })?;

    Ok(())
}
