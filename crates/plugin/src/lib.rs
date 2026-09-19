//! RdataStation v2 插件 crate（plugin，M9）
//!
//! 插件系统核心 + 运行时适配器：
//! - 核心：dependency（依赖解析）/ events（事件总线）/ installer / loader（热加载）
//!   / manager（生命周期）/ manifest（清单）/ permission（权限，四轨）
//! - 服务：plugin_bridge（桥接）/ plugin_service（插件管理服务）
//! - Sidecar 适配器：进程生命周期 + JSON-RPC 客户端（**当前是 HTTP/端口，与 D5 相反**，P1 换 stdio 分帧）
//! - WASM 适配器：Extism 运行时（分析/驱动/工具插件）
//!
//! 两处**待建而不是待补**（不要因为找不到就以为漏了）：
//! - 驱动适配层（把 sidecar 会话接到 `engine::driver::Database`）：P1 按三层对象模型新建；
//!   旧的 HTTP/端口版已删（P0 决策，见 `docs/architecture/plugin/plugin-dev-plan.md` §1.2）。
//! - wasm host function 面：P3 按 Q4 收敛后重建；现在**一个都不提供**（等于 wasm 插件
//!   拿不到任何宿主能力，与“默认拒绝”一致）。
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
