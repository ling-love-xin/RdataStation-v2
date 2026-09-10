//! Tauri Adapter 模块
//!
//! 负责将 Tauri 前端的请求适配到 Core 业务层。
//! 这是多适配器架构中的一个实现，未来还会有 CLI、HTTP、WASM 等适配器。
//!
//! # 架构设计
//!
//! ```
//! Frontend (Vue3)
//!     │
//!     ▼
//! Tauri Commands (commands/)
//!     │
//!     ▼
//! Tauri Adapter (adapters/tauri/) - 输入校验、DTO转换、流处理
//!     │
//!     ▼
//! Core Services (core/services/)
//! ```

// 导出子模块
pub mod event;
pub mod state;
pub mod stream;

// 重新导出核心类型
pub use self::stream::{stream_utils, QueryResultChunk, QueryResultStream, StreamAdapter};

use crate::core::CoreError;

/// Tauri 适配错误
#[derive(Debug)]
pub enum TauriAdapterError {
    /// 输入验证错误
    ValidationError(String),
    /// DTO 转换错误
    ConversionError(String),
    /// 流处理错误
    StreamError(String),
    /// Core 错误
    CoreError(CoreError),
}

impl std::fmt::Display for TauriAdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ValidationError(msg) => write!(f, "Validation error: {}", msg),
            Self::ConversionError(msg) => write!(f, "Conversion error: {}", msg),
            Self::StreamError(msg) => write!(f, "Stream error: {}", msg),
            Self::CoreError(e) => write!(f, "Core error: {}", e),
        }
    }
}

impl std::error::Error for TauriAdapterError {}

impl From<CoreError> for TauriAdapterError {
    fn from(err: CoreError) -> Self {
        Self::CoreError(err)
    }
}

/// 将适配错误转换为 Tauri 错误字符串
pub fn to_tauri_error(err: TauriAdapterError) -> String {
    err.to_string()
}

/// 输入验证 trait
pub trait ValidateInput {
    fn validate(&self) -> Result<(), TauriAdapterError>;
}

/// DTO 转换 trait
pub trait IntoDto<T> {
    fn into_dto(self) -> Result<T, TauriAdapterError>;
}

/// DTO 转换 trait (反向)
pub trait FromDto<T> {
    fn from_dto(dto: T) -> Result<Self, TauriAdapterError>
    where
        Self: Sized;
}
