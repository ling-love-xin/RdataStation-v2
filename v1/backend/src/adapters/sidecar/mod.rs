//! Sidecar 进程管理模块
//!
//! 负责 Go Sidecar 进程的生命周期管理、
//! JSON-RPC 通信和驱动适配。

pub mod client;
pub mod driver;
pub mod health_checker;
pub mod hot_reload_manager;
pub mod manager;

use serde::{Deserialize, Serialize};

/// Sidecar 错误类型
#[derive(Debug, Clone)]
pub enum SidecarError {
    /// 进程启动错误
    ProcessStartError(String),
    /// 通信错误
    CommunicationError(String),
    /// 内部错误（如锁中毒）
    Internal(String),
}

impl std::fmt::Display for SidecarError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProcessStartError(msg) => write!(f, "Process start error: {}", msg),
            Self::CommunicationError(msg) => write!(f, "Communication error: {}", msg),
            Self::Internal(msg) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl std::error::Error for SidecarError {}

/// Sidecar 配置
#[derive(Debug, Clone)]
pub struct SidecarConfig {
    /// 二进制文件路径
    pub binary_path: String,
    /// 调试模式
    pub debug: bool,
    /// 启动超时（毫秒）
    pub startup_timeout_ms: u64,
    /// 健康检查间隔（毫秒）
    pub health_check_interval_ms: u64,
}

impl Default for SidecarConfig {
    fn default() -> Self {
        Self {
            binary_path: "./rdata-sidecar".to_string(),
            debug: false,
            startup_timeout_ms: 10000,
            health_check_interval_ms: 5000,
        }
    }
}

/// Sidecar 运行状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SidecarStatus {
    /// 已停止
    Stopped,
    /// 启动中
    Starting,
    /// 运行中
    Running,
    /// 停止中
    Stopping,
    /// 错误
    Error,
}