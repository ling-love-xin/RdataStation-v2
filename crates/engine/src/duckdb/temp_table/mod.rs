//! 临时表域（责任域：临时表的建 / 登记 / 清理，以及分析用临时表的生命周期）
//!
//! 目录化自 `duckdb/{temp_table,analysis}.rs`（engine 瘦身第 6 刀，2026-10-29）：
//! 只移动文件不重写逻辑；`pub mod temp_table` / `pub mod analysis` 经
//! `duckdb/mod.rs` 传导，外部 `engine::duckdb::analysis::*` 路径零改动。

pub mod temp_table;
pub mod analysis;

pub use temp_table::*;
pub use analysis::*;

// 标识符加引号的**唯一规则**在临时表模块（B7 切片二的导出 COPY 也要用）；
// 原 `duckdb/mod.rs` 以 `pub(crate)` 导出，目录内保持同一可见性（不能经 `pub use …::*` 提升）。
pub(crate) use temp_table::quote_ident;
