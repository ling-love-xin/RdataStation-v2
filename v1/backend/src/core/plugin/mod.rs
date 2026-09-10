//! 插件系统核心模块
//!
//! 提供完整的插件管理、加载、生命周期控制功能

pub mod dependency;
pub mod events;
pub mod installer;
pub mod loader;
pub mod manager;
pub mod manifest;
pub mod permission;

pub use dependency::*;
pub use events::*;
pub use loader::*;
pub use manager::*;
pub use manifest::*;
pub use permission::*;
