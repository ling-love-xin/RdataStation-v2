//! 命令模块
//!
//! 包含所有 Tauri 命令的实现。
//!
//! ═══════════ 架构边界 ═══════════
//! ❌ 禁止 import core::dbi::*（commands 只能访问 services）
//! ✅ 允许 import core::services::*（业务逻辑入口）
//! ✅ 允许 import core::error / core::models（基础类型）

pub mod cache_warming_commands;
pub mod connection_commands;
pub mod data_source_commands;
pub mod driver_commands;
pub mod memory_commands;
pub mod metadata_cache_commands;
pub mod metadata_commands;
pub mod navigator_commands;
pub use metadata_commands::*;
pub mod analytics_resource_commands;
pub mod logging_commands;
pub mod mock_commands;
pub mod mock_persistence_commands;
pub mod performance_commands;
pub mod plugin_commands;
pub mod port_commands;
pub mod project_commands;
pub mod project_store_commands;
pub mod result_commands;
pub mod sql_commands;
pub mod sql_parser_commands;
pub mod sql_template_commands;

pub mod scratchpad_commands;
pub mod system_commands;

// 重新导出所有命令，方便 lib.rs 统一导入
pub use analytics_resource_commands::*;
pub use cache_warming_commands::*;
pub use connection_commands::*;
pub use data_source_commands::*;
pub use driver_commands::*;
pub use logging_commands::*;
pub use memory_commands::*;
pub use metadata_cache_commands::*;
pub use mock_commands::*;
pub use mock_persistence_commands::*;
pub use navigator_commands::*;
pub use performance_commands::*;
pub use plugin_commands::*;
pub use port_commands::*;
pub use project_commands::*;
pub use project_store_commands::*;
pub use result_commands::*;
pub use scratchpad_commands::*;
pub use sql_commands::*;
pub use sql_parser_commands::*;
pub use sql_template_commands::*;
pub use system_commands::*;
