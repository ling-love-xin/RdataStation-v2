//! Adapters 层
//!
//! 负责将外部请求（Tauri/CLI/HTTP/WASM）适配到 Core 业务层。
//! 每个适配器都是 Core 的独立入口，保持 Core 的纯净性。
//!
//! 架构层级：
//! ```
//! External Request → Adapter → Core Service → Datasource
//!     (Tauri/CLI)     (转换)     (业务逻辑)    (数据访问)
//! ```
//!
//! # 支持的适配器
//!
//! - **Tauri**: 桌面应用前端通信
//! - **WASM**: 插件系统（分析、驱动、工具）
//! - CLI: 命令行接口（未来）
//! - HTTP: REST API（未来）

pub mod tauri;
pub mod wasm;

// 未来扩展：
// #[cfg(feature = "cli")]
// pub mod cli;
//
// #[cfg(feature = "http")]
// pub mod http;

/// 适配器 trait
///
/// 所有适配器都需要实现此 trait
pub trait Adapter {
    /// 适配器名称
    fn name(&self) -> &str;

    /// 初始化适配器
    fn init(&mut self) -> Result<(), AdapterError>;

    /// 关闭适配器
    fn shutdown(&mut self) -> Result<(), AdapterError>;
}

/// 适配器错误
#[derive(Debug)]
pub enum AdapterError {
    /// 初始化错误
    InitError(String),
    /// 运行时错误
    RuntimeError(String),
    /// 通信错误
    CommunicationError(String),
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InitError(msg) => write!(f, "Init error: {}", msg),
            Self::RuntimeError(msg) => write!(f, "Runtime error: {}", msg),
            Self::CommunicationError(msg) => write!(f, "Communication error: {}", msg),
        }
    }
}

impl std::error::Error for AdapterError {}
