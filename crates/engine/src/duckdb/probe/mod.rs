//! 文件探查与读取（责任域：把「外部文件」变成 DuckDB 可读形态）
//!
//! 目录化自 `duckdb/{file_probe,file_reader}.rs`（engine 瘦身第 6 刀，2026-10-29）：
//! 只移动文件不重写逻辑；`pub mod file_probe` / `pub mod file_reader` 与全部顶层
//! 函数经 `duckdb/mod.rs` 的 `pub use probe::*` 传导，外部路径零改动。

pub mod file_probe;
pub mod file_reader;

pub use file_probe::*;
pub use file_reader::*;
