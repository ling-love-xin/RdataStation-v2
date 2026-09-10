//! RdataStation v2 插件 crate（plugin，M9）
//!
//! 插件系统核心 + 运行时适配器：
//! - 核心：dependency（依赖解析）/ events（事件总线）/ installer / loader（热加载）
//!   / manager（生命周期）/ manifest（清单）/ permission（权限）/ storage
//! - 服务：plugin_bridge（桥接）/ plugin_service（插件管理服务）
//! - Sidecar 适配器：Go Sidecar 进程管理（JSON-RPC 通信 + 驱动适配）
//! - WASM 适配器：Extism 运行时（分析/驱动/工具插件）
//!
//! 依赖方向：plugin → engine → shared。

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


pub mod plugin_bridge;
pub mod plugin_service;
pub mod sidecar;
pub mod wasm;

pub use plugin_service::PluginService;
