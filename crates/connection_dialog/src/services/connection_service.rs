//! 连接服务层：连接生命周期（`runtime.rs`）+ 网络方式解析（`network.rs`）。
//!
//! 对外契约不变：本模块继续直接导出 `ConnectionService` / `ConnectRequest` /
//! `ProbeConfigInput` / `SaveGlobalConnectionInput` / `resolve_network_method_with_project` /
//! `parse_network_config_json`（workbench 经 `services::connection_service` 引用）。

mod network;
mod runtime;

pub use network::{parse_network_config_json, resolve_network_method_with_project};
pub use runtime::{ConnectRequest, ConnectionService, ProbeConfigInput, SaveGlobalConnectionInput};
