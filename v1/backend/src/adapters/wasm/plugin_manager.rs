use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::{
    ExtismPluginManager, PluginManager, PluginMetadata, WasmAdapterError, WasmRuntimeConfig,
};

/// 插件沙箱配置
#[derive(Debug, Clone, Default)]
pub struct PluginSandboxConfig {
    /// 权限列表
    pub permissions: Vec<String>,
    /// 资源限制
    pub resource_limits: ResourceLimits,
}

/// 资源限制
#[derive(Debug, Clone)]
pub struct ResourceLimits {
    /// 最大内存 (MB)
    pub max_memory_mb: usize,
    /// 最大CPU时间 (ms)
    pub max_cpu_time_ms: u64,
    /// 最大文件系统大小 (MB)
    pub max_fs_size_mb: usize,
    /// 最大网络请求数
    pub max_network_requests: u32,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_memory_mb: 512,
            max_cpu_time_ms: 30000,
            max_fs_size_mb: 1024,
            max_network_requests: 100,
        }
    }
}

/// 插件沙箱实现
#[derive(Clone)]
pub struct PluginSandbox {
    /// 沙箱配置
    config: PluginSandboxConfig,
    /// 资源使用情况
    resource_usage: Arc<Mutex<ResourceUsage>>,
}

/// 资源使用情况
#[derive(Debug, Clone, Default)]
pub struct ResourceUsage {
    /// 当前内存使用 (MB)
    pub memory_used_mb: usize,
    /// 当前CPU时间使用 (ms)
    pub cpu_time_used_ms: u64,
    /// 当前文件系统使用 (MB)
    pub fs_used_mb: usize,
    /// 当前网络请求数
    pub network_requests_made: u32,
}

impl PluginSandbox {
    /// 创建新的插件沙箱
    pub fn new(config: Option<PluginSandboxConfig>) -> Self {
        let config = config.unwrap_or_default();

        Self {
            config,
            resource_usage: Arc::new(Mutex::new(ResourceUsage::default())),
        }
    }

    /// 检查权限
    pub fn check_permission(&self, permission: &str) -> bool {
        self.config.permissions.contains(&permission.to_string())
    }

    /// 监控资源使用
    pub fn monitor_resources(&self) -> Result<ResourceUsage, WasmAdapterError> {
        let usage = self.resource_usage.lock().map_err(|e| {
            WasmAdapterError::RuntimeError(format!("Failed to lock resource usage: {}", e))
        })?;

        Ok(usage.clone())
    }

    /// 更新资源使用
    pub fn update_resource_usage(&self, usage: ResourceUsage) -> Result<(), WasmAdapterError> {
        let mut current_usage = self.resource_usage.lock().map_err(|e| {
            WasmAdapterError::RuntimeError(format!("Failed to lock resource usage: {}", e))
        })?;

        *current_usage = usage;
        Ok(())
    }

    /// 检查资源限制
    pub fn check_resource_limits(&self) -> Result<(), WasmAdapterError> {
        let usage = self.resource_usage.lock().map_err(|e| {
            WasmAdapterError::RuntimeError(format!("Failed to lock resource usage: {}", e))
        })?;

        let limits = &self.config.resource_limits;

        if usage.memory_used_mb > limits.max_memory_mb {
            return Err(WasmAdapterError::RuntimeError(
                "Memory limit exceeded".to_string(),
            ));
        }

        if usage.cpu_time_used_ms > limits.max_cpu_time_ms {
            return Err(WasmAdapterError::RuntimeError(
                "CPU time limit exceeded".to_string(),
            ));
        }

        if usage.fs_used_mb > limits.max_fs_size_mb {
            return Err(WasmAdapterError::RuntimeError(
                "File system size limit exceeded".to_string(),
            ));
        }

        if usage.network_requests_made > limits.max_network_requests {
            return Err(WasmAdapterError::RuntimeError(
                "Network requests limit exceeded".to_string(),
            ));
        }

        Ok(())
    }
}

/// 高级插件管理器，集成沙箱功能
pub struct AdvancedPluginManager {
    /// 底层插件管理器
    inner: Arc<Mutex<ExtismPluginManager>>,
    /// 插件沙箱映射
    sandboxes: Arc<Mutex<HashMap<String, PluginSandbox>>>,
}

impl AdvancedPluginManager {
    /// 创建新的高级插件管理器
    pub fn new(_config: Option<WasmRuntimeConfig>) -> Self {
        let inner = ExtismPluginManager::new();

        Self {
            inner: Arc::new(Mutex::new(inner)),
            sandboxes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 加载插件并配置沙箱
    pub fn load_plugin_with_sandbox(
        &mut self,
        path: &str,
        sandbox_config: Option<PluginSandboxConfig>,
    ) -> Result<PluginMetadata, WasmAdapterError> {
        // 读取 wasm 文件
        let wasm_bytes = std::fs::read(path).map_err(|e| {
            WasmAdapterError::LoadError(format!("Failed to read plugin file {}: {}", path, e))
        })?;

        // 生成插件 ID
        let id = std::path::Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        // 加载插件
        let metadata = {
            let inner = self.inner.lock().map_err(|_| {
                WasmAdapterError::RuntimeError("Failed to lock plugin manager".to_string())
            })?;
            inner
                .load_plugin(&id, &wasm_bytes, None, None)
                .map_err(|e| WasmAdapterError::LoadError(e.to_string()))?
        };

        // 创建沙箱
        let sandbox = PluginSandbox::new(sandbox_config);

        // 存储沙箱
        let mut sandboxes = self.sandboxes.lock().map_err(|_| {
            WasmAdapterError::RuntimeError("Failed to lock sandboxes map".to_string())
        })?;

        sandboxes.insert(metadata.id.clone(), sandbox);

        Ok(metadata)
    }

    /// 获取插件沙箱
    pub fn get_sandbox(&self, plugin_id: &str) -> Result<Option<PluginSandbox>, WasmAdapterError> {
        let sandboxes = self.sandboxes.lock().map_err(|e| {
            WasmAdapterError::RuntimeError(format!("Failed to lock sandboxes map: {}", e))
        })?;

        Ok(sandboxes.get(plugin_id).cloned())
    }

    /// 更新插件沙箱配置
    pub fn update_sandbox_config(
        &mut self,
        plugin_id: &str,
        config: PluginSandboxConfig,
    ) -> Result<(), WasmAdapterError> {
        let mut sandboxes = self.sandboxes.lock().map_err(|_| {
            WasmAdapterError::RuntimeError("Failed to lock sandboxes map".to_string())
        })?;

        if let Some(sandbox) = sandboxes.get_mut(plugin_id) {
            // 更新沙箱配置
            *sandbox = PluginSandbox::new(Some(config));
            Ok(())
        } else {
            Err(WasmAdapterError::RuntimeError(format!(
                "Plugin {} not found",
                plugin_id
            )))
        }
    }
}

impl PluginManager for AdvancedPluginManager {
    /// 加载插件（使用默认沙箱配置）
    fn load_plugin(&mut self, path: &str) -> Result<PluginMetadata, WasmAdapterError> {
        self.load_plugin_with_sandbox(path, None)
    }

    /// 卸载插件
    fn unload_plugin(&mut self, id: &str) -> Result<(), WasmAdapterError> {
        // 卸载插件
        {
            let inner = self.inner.lock().map_err(|_| {
                WasmAdapterError::RuntimeError("Failed to lock plugin manager".to_string())
            })?;
            inner
                .unload_plugin(id)
                .map_err(|e| WasmAdapterError::RuntimeError(e.to_string()))?;
        }

        // 移除沙箱
        let mut sandboxes = self.sandboxes.lock().map_err(|_| {
            WasmAdapterError::RuntimeError("Failed to lock sandboxes map".to_string())
        })?;

        sandboxes.remove(id);

        Ok(())
    }

    /// 获取已加载插件列表
    fn list_plugins(&self) -> Vec<PluginMetadata> {
        match self.inner.lock() {
            Ok(inner) => match inner.list_plugins() {
                Ok(plugins) => plugins.into_iter().map(|(meta, _state)| meta).collect(),
                Err(_) => Vec::new(),
            },
            Err(_) => {
                // 如果获取锁失败，返回空列表而不是 panic
                Vec::new()
            }
        }
    }

    /// 调用插件函数（带沙箱检查）
    fn call_plugin(&self, id: &str, func: &str, args: &[u8]) -> Result<Vec<u8>, WasmAdapterError> {
        // 检查沙箱
        let sandboxes = self.sandboxes.lock().map_err(|_| {
            WasmAdapterError::RuntimeError("Failed to lock sandboxes map".to_string())
        })?;

        if let Some(sandbox) = sandboxes.get(id) {
            // 检查资源限制
            sandbox.check_resource_limits()?;
        }

        // 调用插件函数
        let result = {
            let inner = self.inner.lock().map_err(|_| {
                WasmAdapterError::RuntimeError("Failed to lock plugin manager".to_string())
            })?;
            inner
                .call_plugin(id, func, args)
                .map_err(|e| WasmAdapterError::RuntimeError(e.to_string()))?
        };

        // 更新资源使用（当前为占位实现，后续对接 wasmtime 资源计量 API）
        if let Some(sandbox) = sandboxes.get(id) {
            let mut usage = sandbox.monitor_resources()?;
            usage.cpu_time_used_ms += 1; // 示例：增加 CPU 时间
            sandbox.update_resource_usage(usage)?;
        }

        Ok(result)
    }
}
