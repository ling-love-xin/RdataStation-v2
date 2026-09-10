//! WASM Adapter 模块
//!
//! 负责 WASM 插件系统的适配，支持：
//! - 插件加载与管理
//! - 插件与 Core 的通信
//! - 数据分析插件支持
//!
//! # 架构设计
//!
//! ```
//! WASM Plugin (Rust/C/Go compiled to WASM)
//!     │
//!     ▼
//! WASM Runtime (Extism)
//!     │
//!     ▼
//! WASM Adapter (adapters/wasm/)
//!     │
//!     ▼
//! Core Services (core/services/)
//! ```
//!
//! # 插件类型
//!
//! 1. **分析插件**: 数据可视化、统计分析
//! 2. **驱动插件**: 自定义数据库驱动
//! 3. **工具插件**: SQL 格式化、代码生成

use crate::core::CoreError;
use serde::Serialize;

// 导出子模块
pub mod api;
pub mod extism;
pub mod host_functions;
pub mod plugin_manager;

// 重新导出核心类型
pub use self::extism::ExtismPluginManager;
pub use self::plugin_manager::{
    AdvancedPluginManager, PluginSandbox, PluginSandboxConfig, ResourceLimits, ResourceUsage,
};

/// WASM 适配错误
#[derive(Debug)]
pub enum WasmAdapterError {
    /// 插件加载错误
    LoadError(String),
    /// 运行时错误
    RuntimeError(String),
    /// 通信错误
    CommunicationError(String),
    /// 权限错误
    PermissionError(String),
    /// 资源限制错误
    ResourceLimitError(String),
    /// Core 错误
    CoreError(CoreError),
}

impl std::fmt::Display for WasmAdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LoadError(msg) => write!(f, "Plugin load error: {}", msg),
            Self::RuntimeError(msg) => write!(f, "Runtime error: {}", msg),
            Self::CommunicationError(msg) => write!(f, "Communication error: {}", msg),
            Self::PermissionError(msg) => write!(f, "Permission error: {}", msg),
            Self::ResourceLimitError(msg) => write!(f, "Resource limit error: {}", msg),
            Self::CoreError(e) => write!(f, "Core error: {}", e),
        }
    }
}

impl std::error::Error for WasmAdapterError {}

impl From<CoreError> for WasmAdapterError {
    fn from(err: CoreError) -> Self {
        Self::CoreError(err)
    }
}

/// 插件类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PluginType {
    /// 分析插件
    Analytics,
    /// 驱动插件
    Driver,
    /// 工具插件
    Tool,
}

/// 插件元数据
#[derive(Debug, Clone, Serialize)]
pub struct PluginMetadata {
    /// 插件 ID
    pub id: String,
    /// 插件名称
    pub name: String,
    /// 插件版本
    pub version: String,
    /// 插件类型
    pub plugin_type: PluginType,
    /// 插件描述
    pub description: String,
    /// 作者
    pub author: String,
    /// 入口函数
    pub entry_point: String,
}

/// 插件管理器 trait
pub trait PluginManager {
    /// 加载插件
    fn load_plugin(&mut self, path: &str) -> Result<PluginMetadata, WasmAdapterError>;

    /// 卸载插件
    fn unload_plugin(&mut self, id: &str) -> Result<(), WasmAdapterError>;

    /// 获取已加载插件列表
    fn list_plugins(&self) -> Vec<PluginMetadata>;

    /// 调用插件函数
    fn call_plugin(&self, id: &str, func: &str, args: &[u8]) -> Result<Vec<u8>, WasmAdapterError>;
}

/// WASM 运行时配置
#[derive(Debug, Clone)]
pub struct WasmRuntimeConfig {
    /// 最大内存限制 (MB)
    pub max_memory_mb: usize,
    /// 最大执行时间 (ms)
    pub max_execution_time_ms: u64,
    /// 是否允许文件系统访问
    pub allow_fs: bool,
    /// 是否允许网络访问
    pub allow_network: bool,
}

impl Default for WasmRuntimeConfig {
    fn default() -> Self {
        Self {
            max_memory_mb: 512,
            max_execution_time_ms: 30000,
            allow_fs: false,
            allow_network: false,
        }
    }
}

/// 创建默认的插件管理器
pub fn create_default_plugin_manager() -> AdvancedPluginManager {
    AdvancedPluginManager::new(None)
}

/// 创建自定义配置的插件管理器
pub fn create_plugin_manager(config: WasmRuntimeConfig) -> AdvancedPluginManager {
    AdvancedPluginManager::new(Some(config))
}
