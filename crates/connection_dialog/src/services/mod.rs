//! 连接服务层（自 rds-workbench `services/` 下沉）。
//!
//! 依赖只向下：engine / connection / shared / workbench_shell，不引用任何特性 crate。
//! workbench 侧经 `services/mod.rs` 的 `pub use` 重导保持 `crate::services::*` 路径不变。

pub mod connection_service;
pub mod data_source_service;
pub mod driver_service;
pub mod secret_integration;
pub mod workspace_loader;
