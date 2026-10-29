//! DuckDB 扩展插件接口（责任域：插件连接 / 权限 / 管理器）
//!
//! 目录化自 `duckdb/plugin.rs`（engine 瘦身第 6 刀，2026-10-29）：
//! 只移动文件不重写逻辑；`pub mod plugin` 与顶层项经 `pub use plugin::*` 传导。

pub mod plugin;

pub use plugin::*;
