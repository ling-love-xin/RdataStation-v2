//! 数据导入导出（责任域：import / export 的配置与执行入口）
//!
//! 目录化自 `duckdb/import_export.rs`（engine 瘦身第 6 刀，2026-10-29）：
//! 只移动文件不重写逻辑；`pub mod import_export` 与全部顶层项经
//! `duckdb/mod.rs` 的 `pub use transfer::*` 传导，外部路径零改动。

pub mod import_export;

pub use import_export::*;
