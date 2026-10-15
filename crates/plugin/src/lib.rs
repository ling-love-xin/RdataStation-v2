//! RdataStation v2 插件 crate（plugin，M9）
//!
//! 插件系统核心 + 运行时适配器：
//! - 核心：dependency（依赖解析）/ events（事件总线）/ installer / manager（生命周期：
//!   发现 → 加载 → 激活 → 停用 → 卸载，2026-10-15 起 loader 已并入）/ manifest（清单）/
//!   permission（权限，四轨）
//! - 服务：plugin_bridge（桥接）/ plugin_service（插件管理服务）
//! - Sidecar 适配器：进程生命周期 + JSON-RPC 客户端（**P1 已落地：stdio + 二进制分帧，D5**；HTTP/端口旧版已删）
//! - WASM 适配器：Extism 运行时（分析/驱动/工具插件）
//!
//! 一处**待建而不是待补**（不要因为找不到就以为漏了）：
//! - wasm host function 面：P3 按 Q4 收敛后重建；现在**一个都不提供**（等于 wasm 插件
//!   拿不到任何宿主能力，与“默认拒绝”一致）。
//!
//! 驱动适配层（把 sidecar 会话接到 `engine::driver::Database`）**P1 已落地**，不是待建：
//! `sidecar/driver.rs` 的 `SidecarDatabase` 实现 `Database` trait（结果只填 `batches`），
//! 注册走 `sidecar/factory.rs` 的 `register_sidecar_drivers()`；旧的 HTTP/端口版已删（P0 决策，
//! 见 `docs/architecture/plugin/plugin-dev-plan.md` §1.2）。进度口径见
//! `docs/architecture/plugin/README.md` §7 与 `plugin-dev-plan.md` §11。
//!
//! 依赖方向：plugin → engine → shared。

pub mod dependency;
pub mod events;
pub mod installer;
pub mod manager;
pub mod manifest;
pub mod permission;
pub mod render;

pub use dependency::*;
pub use events::*;
pub use manager::*;
pub use manifest::*;
pub use permission::*;

pub mod plugin_bridge;
pub mod plugin_service;
pub mod sidecar;
pub mod wasm;

pub use plugin_service::PluginService;
