//! 全文检索（责任域：DuckDB FTS 扩展封装）
//!
//! 目录化自 `duckdb/fts.rs`（engine 瘦身第 6 刀，2026-10-29）：
//! 只移动文件不重写逻辑；`pub mod fts` 与顶层项经 `pub use fts::*` 传导。

pub mod fts;

pub use fts::*;
